#!/usr/bin/env python3
"""Checks `tempent_fix.rs`'s six detours against a real DoD `client.dll`.

`tempent_fix.rs` writes a jump right after six `R_TempSprite` / `R_TempModel`
calls whose result DoD never checks, and hands the game a scratch buffer when
the engine returns NULL (issue #374). The unit tests prove the stub assembles
as documented; only the binary can prove the rest.

Per site:

  1. The signature matches the shipped `client.dll` exactly once.
  2. `detour_at` bytes past the match are the stolen bytes, and the
     instruction just before them is the call to the effects API: `call
     [reg+0xc0]` (`R_TempModel`) or `call [reg+0xc8]` (`R_TempSprite`), or a
     `call [reg]` whose register was loaded with `lea reg, [x+0xc0/0xc8]`.
  3. Nothing branches into the interior of the span, and no relocated dword
     points into it.
  4. The stolen instructions are safe to run from the stub: no relative
     operands, and none reads the flags the stub's `test` changes.
  5. **What makes the scratch buffer safe:** from the call to the function's
     `ret`, the result is only ever written through -- never read, never
     stored anywhere, never pushed as an argument, never branched around --
     and every write lands inside `SCRATCH_SIZE`.

And once: the six spans do not overlap.

Every constant comes out of `tempent_fix.rs` rather than being restated here,
for the reason in `verify_deathmsg_offsets.py`: a check with its own copy of the
thing it checks only proves the copy is self-consistent.

Usage:
    python goldsrc-hooks/tools/verify_tempent_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Needs `pip install pefile
capstone`.
"""

import re
import sys
from pathlib import Path

try:
    import pefile
    import capstone
    from capstone import x86
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\dod\cl_dlls\client.dll"
)
RUST = Path(__file__).resolve().parent.parent / "src" / "tempent_fix.rs"

# efx_api_t slots whose result the six sites use: R_TempModel, R_TempSprite.
EFX_SLOTS = {0xC0: "R_TempModel", 0xC8: "R_TempSprite"}

# Registers a `call` leaves undefined (cdecl/stdcall caller-saved).
CLOBBERED_BY_CALL = {x86.X86_REG_EAX, x86.X86_REG_ECX, x86.X86_REG_EDX}

# Instructions that store to their first operand without reading it.
STORES = {"mov", "fstp", "fst", "fistp", "fist"}


def rust_sites(src: str):
    """[(what, pattern, detour_at, stolen)] from the `SITES` array."""
    body = src.split("const SITES")[1].split("];\n")[0]
    sites = []
    for block in re.findall(r"Site \{(.*?)\n    \}", body, re.S):
        what = re.search(r'what: "(.*?)"', block).group(1)
        pattern = re.search(r'pattern: "(.*?)"', block, re.S).group(1)
        pattern = " ".join(pattern.replace("\\", " ").split())
        detour_at = int(re.search(r"detour_at: (0x[0-9a-f]+|\d+)", block).group(1), 0)
        stolen_text = block.split("stolen:")[1]
        stolen = bytes(int(b, 16) for b in re.findall(r"0x([0-9a-fA-F]{2})", stolen_text))
        sites.append((what, pattern, detour_at, stolen))
    return sites


def rust_scalar(src: str, name: str) -> int:
    match = re.search(rf"const {name}: \w+ = (0x[0-9a-f_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}` in tempent_fix.rs")
    return int(match.group(1).replace("_", ""), 0)


def branch_targets(pe, image, base, md):
    """{target rva: [source rvas]} for every direct branch in executable code."""
    found = {}
    for section in pe.sections:
        if not section.Characteristics & 0x20000000:  # IMAGE_SCN_MEM_EXECUTE
            continue
        lo = section.VirtualAddress
        hi = lo + max(section.Misc_VirtualSize, section.SizeOfRawData)
        for ins in md.disasm(image[lo:hi], base + lo):
            # skipdata's stand-in for a byte it could not decode has no groups.
            if ins.id == 0 or not ({x86.X86_GRP_JUMP, x86.X86_GRP_CALL} & set(ins.groups)):
                continue
            for op in ins.operands:
                if op.type == x86.X86_OP_IMM:
                    found.setdefault(op.imm - base, []).append(ins.address - base)
    return found


def relocated_pointers(pe, image, base):
    """{pointed-at rva: [rva of the dword]} for every HIGHLOW relocation."""
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_BASERELOC"]]
    )
    found = {}
    for block in getattr(pe, "DIRECTORY_ENTRY_BASERELOC", []):
        for entry in block.entries:
            if entry.type == 3:
                value = int.from_bytes(image[entry.rva : entry.rva + 4], "little")
                found.setdefault(value - base, []).append(entry.rva)
    return found


def instruction_before(image, base, md, start, target):
    """The instruction ending exactly at `target`, decoding from `start` -- the
    signature's first byte, which every signature puts on an instruction."""
    last = None
    for ins in md.disasm(image[start:target], base + start):
        last = ins
    if last is not None and last.address + last.size == base + target:
        return last
    return None


def effects_call(image, base, md, call):
    """The efx slot `call` reaches, or None if it is not an effects-API call."""
    if call is None or call.mnemonic != "call" or len(call.operands) != 1:
        return None
    op = call.operands[0]
    if op.type != x86.X86_OP_MEM or op.mem.index != 0 or op.mem.base == 0:
        return None
    if op.mem.disp in EFX_SLOTS:
        return op.mem.disp
    if op.mem.disp != 0:
        return None
    # `call [esi]`: the pointer was cached with `lea esi, [x+slot]` earlier.
    start, slot = call.address - base - 0x60, None
    for ins in md.disasm(image[start : call.address - base], base + start):
        if (
            ins.mnemonic == "lea"
            and ins.operands[0].reg == op.mem.base
            and ins.operands[1].mem.disp in EFX_SLOTS
        ):
            slot = ins.operands[1].mem.disp
    return slot


def check_uses(image, base, md, target, scratch_size):
    """Walks from the return address to `ret`, following the result.

    Returns (problems, highest offset written)."""
    tracked = {x86.X86_REG_EAX}
    problems, highest = [], 0
    for ins in md.disasm(image[target : target + 0x400], base + target):
        rva = ins.address - base
        if ins.mnemonic == "ret":
            return problems, highest
        if {x86.X86_GRP_JUMP} & set(ins.groups):
            problems.append(f"+{rva:#x} {ins.mnemonic} -- a branch before the ret; check by hand")
            return problems, highest
        for index, op in enumerate(ins.operands):
            if op.type == x86.X86_OP_MEM and op.mem.base in tracked:
                if index == 0 and ins.mnemonic in STORES:
                    highest = max(highest, op.mem.disp + op.size)
                else:
                    problems.append(f"+{rva:#x} {ins.mnemonic} {ins.op_str} -- reads through the result")
            if op.type == x86.X86_OP_MEM and op.mem.index in tracked:
                problems.append(f"+{rva:#x} {ins.mnemonic} {ins.op_str} -- indexes by the result")
        if (
            len(ins.operands) == 2
            and ins.operands[0].type == x86.X86_OP_MEM
            and ins.operands[1].type == x86.X86_OP_REG
            and ins.operands[1].reg in tracked
        ):
            problems.append(f"+{rva:#x} {ins.mnemonic} {ins.op_str} -- keeps the result somewhere")
        if ins.mnemonic == "push" and ins.operands[0].type == x86.X86_OP_REG and ins.operands[0].reg in tracked:
            problems.append(f"+{rva:#x} push {ins.op_str} -- passes the result to a call")
        if ins.mnemonic == "call":
            tracked -= CLOBBERED_BY_CALL
            continue
        if not ins.operands or ins.operands[0].type != x86.X86_OP_REG:
            continue
        dest = ins.operands[0].reg
        if ins.mnemonic == "mov" and ins.operands[1].type == x86.X86_OP_REG and ins.operands[1].reg in tracked:
            tracked.add(dest)
        elif dest in tracked and ins.mnemonic not in ("push", "cmp", "test"):
            tracked.discard(dest)
        if not tracked:
            return problems, highest
    problems.append("no ret within 0x400 bytes")
    return problems, highest


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no client.dll at {dll}") or 2

    src = RUST.read_text(encoding="utf-8")
    sites = rust_sites(src)
    scratch_size = rust_scalar(src, "SCRATCH_SIZE")
    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = bytes(pe.get_memory_mapped_image())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    md.skipdata = True
    ok = len(sites) == 6
    if not ok:
        print(f"FAIL read {len(sites)} sites out of tempent_fix.rs, expected 6")

    targets = branch_targets(pe, image, base, md)
    pointers = relocated_pointers(pe, image, base)
    spans = []

    for what, pattern, detour_at, stolen in sites:
        print(f"\n== {what} ==")

        # -- 1. the signature identifies exactly one place --------------------
        rx = re.compile(
            b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()),
            re.S,
        )
        matches = [m.start() for m in rx.finditer(image)]
        if len(matches) != 1:
            print(f"FAIL the signature matches {len(matches)} times: {[hex(m) for m in matches]}")
            ok = False
            continue
        target = matches[0] + detour_at
        print(f"OK   the signature matches exactly once; the detour goes at +{target:#x}")
        spans.append(range(target, target + len(stolen)))

        # -- 2. the stolen bytes, right after an effects-API call -------------
        present = image[target : target + len(stolen)]
        if present != stolen:
            ok = False
            print(f"FAIL +{target:#x} holds {present.hex(' ')}, rust reproduces {stolen.hex(' ')}")
        call = instruction_before(image, base, md, matches[0], target)
        slot = effects_call(image, base, md, call)
        if slot is None:
            ok = False
            print(f"FAIL the instruction before +{target:#x} is not an R_TempModel/R_TempSprite call")
        else:
            print(f"OK   +{call.address - base:#x} `call {call.op_str}` is {EFX_SLOTS[slot]}")

        # -- 3. nothing lands inside the span ---------------------------------
        interior = range(target + 1, target + len(stolen))
        inbound = [(s, t) for t in interior for s in targets.get(t, ())]
        inbound += [(s, t) for t in interior for s in pointers.get(t, ())]
        if inbound:
            ok = False
            for src_rva, dst in inbound:
                print(f"FAIL +{src_rva:#x} points to +{dst:#x}, inside the span")
        else:
            print("OK   no branch or relocated dword points inside the span")
        if len(stolen) < 5:
            ok = False
            print(f"FAIL the span is {len(stolen)} bytes; a near jump needs 5")

        # -- 4. the stolen instructions run unchanged from the stub -----------
        for ins in md.disasm(stolen, 0):
            relative = {x86.X86_GRP_JUMP, x86.X86_GRP_CALL, x86.X86_GRP_BRANCH_RELATIVE} & set(ins.groups)
            reads_flags = ins.eflags & (
                x86.X86_EFLAGS_TEST_OF | x86.X86_EFLAGS_TEST_SF | x86.X86_EFLAGS_TEST_ZF
                | x86.X86_EFLAGS_TEST_PF | x86.X86_EFLAGS_TEST_CF | x86.X86_EFLAGS_TEST_AF
            )
            if relative or reads_flags:
                ok = False
                print(f"FAIL stolen `{ins.mnemonic} {ins.op_str}` is relative or reads flags")
        print(f"OK   stolen: {'; '.join(f'{i.mnemonic} {i.op_str}' for i in md.disasm(stolen, 0))}")

        # -- 5. the result is only written through, inside the scratch --------
        problems, highest = check_uses(image, base, md, target, scratch_size)
        if problems:
            ok = False
            for problem in problems:
                print(f"FAIL {problem}")
        elif highest > scratch_size:
            ok = False
            print(f"FAIL writes up to +{highest:#x}, past SCRATCH_SIZE ({scratch_size:#x})")
        else:
            print(f"OK   only written through, up to +{highest:#x} (scratch is {scratch_size:#x})")

    print("\n== together ==")
    overlaps = [(a, b) for i, a in enumerate(spans) for b in spans[i + 1 :] if set(a) & set(b)]
    if overlaps:
        ok = False
        print(f"FAIL overlapping spans: {overlaps}")
    else:
        print(f"OK   the {len(spans)} spans are disjoint")

    print("\nDETOURS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
