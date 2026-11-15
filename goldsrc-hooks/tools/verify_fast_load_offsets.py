#!/usr/bin/env python3
"""Checks `fast_load.rs`'s detour against a real `Core.dll`.

`fast_load.rs` replaces the packet counter's `inc eax` .. `cmp eax, 0x20` in
`Server::RunFrame` with a jump to a stub that redoes the increment and the
store, asks the hook for a cap, and compares the count against it, leaving
the engine's own `jg` to decide (issue #798). The unit tests prove the stub
assembles as documented; only the binary can prove what the stub relies on:

  0. Exactly one of the builds in `BUILDS` (pre-Anniversary, 25th
     Anniversary) matches this `Core.dll`, exactly once, and the others match
     nothing.
  1. The stolen bytes are at the build's `site`: `inc eax`, the store of the
     count to the build's `count` operand, and `cmp eax, 0x20`, in the
     build's order. None is relative or reads a flag. The instruction before
     the span loads `eax` from the same operand, and the one before that is a
     `call` (so no XMM or x87 value is live, and the direction flag is clear,
     when the stub calls the hook). The instruction right after the span is
     the `jg` that reads the stub's flags.
  2. Nothing branches into the span's interior, and no relocated dword points
     into it.
  3. Both of the `jg`'s successors write `eax` before reading it, so the cap
     the stub leaves in `eax` is dead.
  4. The count is set to 0 before the loop: the build's init instruction is
     in the function ahead of the site.
  5. The loop reads the next packet after the `jg` (`[eax+0x60]`) and loops
     back to before the site while the reader returns one (`test esi, esi;
     jne`), and the `jg` leaves the loop for the same place the "no packet"
     test does -- so the count is the only per-frame limit.

Every build's facts come out of `fast_load.rs` rather than being restated
here, for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_fast_load_offsets.py [path-to-Core.dll]
    python goldsrc-hooks/tools/verify_fast_load_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the
25th Anniversary movies install. Needs `pip install pefile capstone`.
"""

import hashlib
import re
import sys
from pathlib import Path

try:
    import pefile
    import capstone
    from capstone import x86
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLL = STEAM / "Half-Life - PRE-Anniversary for Movies" / "Core.dll"
ANNIVERSARY_DLL = STEAM / "Half-Life - POST-Anniversary for Movies" / "Core.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "fast_load.rs"

# Where each build zeroes the count before the loop, as capstone prints it.
# The pre-Anniversary build does it with two arguments of the next call
# already pushed, so `[esp+0x14]` there is `[esp+0xc]` once the callee
# (thiscall, `ret 8`) has popped them.
INIT = {
    "pre-Anniversary": "mov dword ptr [esp + 0x14], 0",
    "25th Anniversary": "mov dword ptr [ebp - 4], 0",
}


def rust_builds(src: str):
    """[(name, pattern, site, stolen, count)] from `BUILDS` in fast_load.rs."""
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in fast_load.rs")
    builds = []
    for name, pattern, site, stolen, count in re.findall(
        r'name: "([^"]+)",\s*pattern: "(.*?)",\s*site: (\d+),.*?stolen: &\[([^\]]*)\],.*?count: &\[([^\]]*)\]',
        table.group(1),
        re.S,
    ):
        as_bytes = lambda text: bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", text))
        pattern = " ".join(pattern.replace(chr(92), " ").split())
        builds.append((name, pattern, int(site), as_bytes(stolen), as_bytes(count)))
    if not builds:
        raise SystemExit("`BUILDS` in fast_load.rs has no entries this script can read")
    return builds


def signature_matches(image, pattern):
    """Every offset `pattern` (hex bytes and ?? wildcards) matches in `image`."""
    rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()), re.S)
    return [m.start() for m in rx.finditer(image)]


def branch_targets(pe, image, base, md):
    """{target rva: [(source rva, mnemonic)]} for every direct branch in code."""
    found = {}
    for section in pe.sections:
        if not section.Characteristics & 0x20000000:  # IMAGE_SCN_MEM_EXECUTE
            continue
        lo = section.VirtualAddress
        hi = lo + max(section.Misc_VirtualSize, section.SizeOfRawData)
        for ins in md.disasm(image[lo:hi], base + lo):
            if ins.id == 0 or not ({x86.X86_GRP_JUMP, x86.X86_GRP_CALL} & set(ins.groups)):
                continue
            for op in ins.operands:
                if op.type == x86.X86_OP_IMM:
                    found.setdefault(op.imm - base, []).append((ins.address - base, ins.mnemonic))
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


def ins_at(image, base, md, rva):
    """The one instruction at `rva`."""
    return next(md.disasm(image[rva : rva + 16], base + rva))


def text(ins):
    return f"{ins.mnemonic} {ins.op_str}".strip()


def main() -> int:
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    dll = ANNIVERSARY_DLL if arg == "--anniversary" else Path(arg) if arg else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no Core.dll at {dll}") or 2

    builds = rust_builds(RUST.read_text(encoding="utf-8"))
    raw = dll.read_bytes()
    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = bytes(pe.get_memory_mapped_image())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    md.skipdata = True
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    # -- 0. which build ------------------------------------------------------
    print(f"== {dll} ==")
    print(f"     {len(raw):,} bytes, md5 {hashlib.md5(raw).hexdigest()}, "
          f"timestamp {pe.FILE_HEADER.TimeDateStamp:#x}, image size {pe.OPTIONAL_HEADER.SizeOfImage:#x}")
    hits = {b[0]: signature_matches(image, b[1]) for b in builds}
    for name, found in hits.items():
        print(f"     {name} signature: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [b for b in builds if hits[b[0]]]
    if len(matching) != 1 or len(hits[matching[0][0]]) != 1:
        print("FAIL exactly one build's signature should match, exactly once")
        print("\nMISMATCH -- do not ship")
        return 1
    name, pattern, site, stolen, count = matching[0]
    found = hits[name][0]
    target = found + site
    end = target + len(stolen)
    check(True, f"this is the {name} Core.dll; the signature matches once, at +{found:#x}")

    # -- 1. the span ---------------------------------------------------------
    print("\n== the span ==")
    check(image[target:end] == stolen, f"+{target:#x}: the stolen bytes {stolen.hex(' ')}")
    check(len(stolen) >= 5, f"the span is {len(stolen)} bytes; a near jump needs 5")
    store = bytes([0x89]) + count
    load = bytes([0x8B]) + count
    check(store in stolen, f"the span stores eax to the count operand ({store.hex(' ')})")
    spelled = [text(i) for i in md.disasm(stolen, 0)]
    check(spelled[0] == "inc eax" and "cmp eax, 0x20" in spelled and len(spelled) == 3,
          f"the span is inc, store and compare: {spelled}")
    for ins in md.disasm(stolen, 0):
        relative = {x86.X86_GRP_JUMP, x86.X86_GRP_CALL, x86.X86_GRP_BRANCH_RELATIVE} & set(ins.groups)
        reads_flags = ins.eflags & (
            x86.X86_EFLAGS_TEST_OF | x86.X86_EFLAGS_TEST_SF | x86.X86_EFLAGS_TEST_ZF
            | x86.X86_EFLAGS_TEST_PF | x86.X86_EFLAGS_TEST_CF | x86.X86_EFLAGS_TEST_AF
        )
        check(not relative and not reads_flags, f"stolen `{text(ins)}` is position-independent and reads no flags")
    before = image[target - len(load) : target]
    check(before == load, f"the instruction before loads eax from the count ({load.hex(' ')})")
    call_at = target - len(load) - 5
    check(image[call_at] == 0xE8, f"+{call_at:#x}, before that, is a `call`")
    jg = ins_at(image, base, md, end)
    check(jg.mnemonic == "jg", f"+{end:#x} right after the span is `{text(jg)}`")
    exit_rva = jg.operands[0].imm - base if jg.mnemonic == "jg" else None

    # -- 2. nothing lands inside it --------------------------------------------
    targets = branch_targets(pe, image, base, md)
    pointers = relocated_pointers(pe, image, base)
    interior = range(target + 1, end)
    inbound = [(s, t) for t in interior for s, _ in targets.get(t, ())]
    inbound += [(s, t) for t in interior for s in pointers.get(t, ())]
    check(not inbound, "no branch or relocated dword points inside the span"
          + "".join(f"\n       +{s:#x} -> +{t:#x}" for s, t in inbound))

    # -- 3. eax is dead after the jg ---------------------------------------
    print("\n== after the jg ==")
    for label, rva in (("falls through to", end + jg.size), ("jumps to", exit_rva)):
        if rva is None:
            check(False, f"the jg {label}: no target")
            continue
        first = ins_at(image, base, md, rva)
        writes = re.match(r"mov eax, (dword ptr )?\[(?!.*eax)", text(first))
        check(writes, f"the jg {label} +{rva:#x} `{text(first)}`, which writes eax without reading it")

    # -- 4. the count starts at 0 ----------------------------------------------
    print("\n== the count ==")
    window = 0x200
    inits = [i for i in md.disasm(image[target - window : target], base + target - window) if text(i) == INIT[name]]
    check(len(inits) == 1, f"`{INIT[name]}` once in the {window:#x} bytes before the site"
          + "".join(f" (+{i.address - base:#x})" for i in inits))

    # -- 5. the loop -----------------------------------------------------------
    print("\n== the loop ==")
    after = list(md.disasm(image[end : end + 0x60], base + end))
    reads = [i for i in after[:6] if "+ 0x60]" in i.op_str]
    check(reads, "the next packet's read (`[eax + 0x60]`) follows the jg"
          + "".join(f" (+{i.address - base:#x} `{text(i)}`)" for i in reads))
    back = next((k for k, i in enumerate(after) if i.mnemonic == "jne"), None)
    if back is None:
        check(False, "no `jne` back to the loop head after the site")
    else:
        head = after[back].operands[0].imm - base
        check(text(after[back - 1]) == "test esi, esi" and head < target,
              f"+{after[back].address - base:#x} `{text(after[back - 1])}; jne +{head:#x}` loops back while there is a packet")
        # The first packet's "none" test ends right at the loop head:
        # `test esi, esi; je rel32`.
        lead = image[head - 8 : head - 4]
        none = head + int.from_bytes(image[head - 4 : head], "little", signed=True)
        check(lead == bytes([0x85, 0xF6, 0x0F, 0x84]) and none == exit_rva,
              f"the jg leaves for +{exit_rva:#x}, where the first packet's `test esi, esi; je` goes too (+{none:#x})")

    print("\nDETOUR VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
