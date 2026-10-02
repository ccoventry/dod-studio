#!/usr/bin/env python3
"""Checks `hull_trace_guard.rs`'s detour against a real `hw.dll`.

`hull_trace_guard.rs` writes a jump over the first six bytes of
`PM_RecursiveHullCheck` and checks each node against the hull before the
game's code reads it (issue #384). The unit test proves the stub assembles as
documented; only the binary can prove the facts the stub relies on:

  0. Exactly one of the builds in `BUILDS` (pre-Anniversary, 25th
     Anniversary) matches this `hw.dll`, and the others match nothing.
  1. That build's signature matches exactly once, and starts with its
     stolen bytes.
  2. Nothing branches into the interior of the span, and no relocated dword
     points into it. The stolen instructions are not relative and read no
     flags.
  3. Every direct call to the function lands on its entry, including its own
     recursive calls -- so every level of a trace passes through the stub --
     and every caller pops 7 arguments (cdecl), so the stub's plain `ret` is
     right. The function itself returns with a plain `ret` everywhere.
  4. The stub's reads match the function's: the hull is `[ebp+8]` and the
     node number `[ebp+0xc]`; the early-out compares `hull+8` (first
     clipnode) with `hull+0xc` (last); a node is `[hull+0] + num*8` with its
     plane number at `+0`; and the plane read that crashed (`+0x6c8d1` in
     the pre-Anniversary build) is `[hull+4] + planenum*20`. The two builds'
     compilers spell these differently, so each has its own list.
  5. `PM_HullPointContents`, which walks the same hulls, refuses a node
     outside `hull+8 ..= hull+0xc` (with a `Sys_Error`), so real map data
     always passes the stub's range check.
  6. `STACK_MARGIN` is a small part of `hl.exe`'s stack, and `MAX_PLANE` is
     above any real map's plane count (`MAX_MAP_PLANES` 32767).

Every constant comes out of `hull_trace_guard.rs` rather than being restated
here, for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_hull_trace_offsets.py [path-to-hw.dll]
    python goldsrc-hooks/tools/verify_hull_trace_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the stock
25th Anniversary one. Needs `pip install pefile capstone`.
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
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)
ANNIVERSARY_DLL = Path(r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life\hw.dll")
RUST = Path(__file__).resolve().parent.parent / "src" / "hull_trace_guard.rs"

# Instructions the stub relies on, in the order the function runs them, as
# capstone prints them. Registers are the compiler's choice and are read from
# each match rather than assumed; `{hull}` and friends name them. One list per
# build in `BUILDS`, by its name.
PRE_READS = [
    ("num is [ebp+0xc]", r"mov (?P<num>e\w\w), dword ptr \[ebp \+ 0xc\]"),
    ("hull is [ebp+8]", r"mov (?P<hull>e\w\w), dword ptr \[ebp \+ 8\]"),
    ("first clipnode is hull+8", r"mov (?P<first>e\w\w), dword ptr \[{hull} \+ 8\]"),
    ("last clipnode is hull+0xc", r"mov (?P<last>e\w\w), dword ptr \[{hull} \+ 0xc\]"),
    ("no clipnodes: the early-out", r"cmp {first}, {last}"),
    ("clipnodes is [hull]", r"mov (?P<clip>e\w\w), dword ptr \[{hull}\]"),
    ("a node is clipnodes + num*8", r"lea (?P<node>e\w\w), \[{clip} \+ {num}\*8\]"),
    ("planes is hull+4", r"mov (?P<planes>e\w\w), dword ptr \[{hull} \+ 4\]"),
    ("planenum is the node's first dword", r"mov (?P<pn>e\w\w), dword ptr \[{node}\]"),
    ("plane stride 20, part 1", r"lea {pn}, \[{pn} \+ {pn}\*4\]"),
    ("plane stride 20, part 2", r"lea (?P<plane>e\w\w), \[{planes} \+ {pn}\*4\]"),
    ("the plane read that crashed", r"mov al, byte ptr \[{plane} \+ 0x10\]"),
]
# The Anniversary compiler keeps the node number in a local, compares the
# first clipnode against the last in memory, and folds the plane address into
# the read.
ANNIVERSARY_READS = [
    ("num is [ebp+0xc]", r"mov (?P<num>e\w\w), dword ptr \[ebp \+ 0xc\]"),
    ("num kept in a local", r"mov dword ptr \[ebp - (?P<slot>0x\w+)\], {num}"),
    ("hull is [ebp+8]", r"mov (?P<hull>e\w\w), dword ptr \[ebp \+ 8\]"),
    ("first clipnode is hull+8", r"mov (?P<first>e\w\w), dword ptr \[{hull} \+ 8\]"),
    ("no clipnodes: the early-out, against hull+0xc", r"cmp {first}, dword ptr \[{hull} \+ 0xc\]"),
    ("num back from the local", r"mov (?P<num2>e\w\w), dword ptr \[ebp - {slot}\]"),
    ("clipnodes is [hull]", r"mov (?P<clip>e\w\w), dword ptr \[{hull}\]"),
    ("a node is clipnodes + num*8", r"lea (?P<node>e\w\w), \[{clip} \+ {num2}\*8\]"),
    ("planes is hull+4", r"mov (?P<planes>e\w\w), dword ptr \[{hull} \+ 4\]"),
    ("planenum is the node's first dword", r"mov (?P<pn>e\w\w), dword ptr \[{node}\]"),
    ("plane stride 20, part 1", r"lea {pn}, \[{pn} \+ {pn}\*4\]"),
    ("the plane type read (planes + planenum*20 + 0x10)", r"mov \w\w, byte ptr \[{planes} \+ {pn}\*4 \+ 0x10\]"),
]
READS = {"pre-Anniversary": PRE_READS, "25th Anniversary": ANNIVERSARY_READS}


def rust_builds(src: str):
    """[(name, pattern, stolen bytes)] from `BUILDS` in hull_trace_guard.rs."""
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in hull_trace_guard.rs")
    builds = []
    for name, pattern, stolen in re.findall(
        r'name: "([^"]+)",\s*pattern: "(.*?)",\s*stolen: \[([^\]]*)\]', table.group(1), re.S
    ):
        pattern = " ".join(pattern.replace("\\", " ").split())
        builds.append((name, pattern, bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", stolen))))
    if not builds:
        raise SystemExit("`BUILDS` in hull_trace_guard.rs has no entries this script can read")
    return builds


def signature_matches(image, pattern):
    """Every offset `pattern` (hex bytes and ?? wildcards) matches in `image`."""
    rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern.split()), re.S)
    return [m.start() for m in rx.finditer(image)]


def rust_const(src: str, name: str) -> str:
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in hull_trace_guard.rs")
    return match.group(1)


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


def function_body(image, base, md, start, limit=0x600):
    """The function's instructions, up to the int3/nop padding after its last ret."""
    body = []
    for ins in md.disasm(image[start : start + limit], base + start):
        if ins.mnemonic in ("int3", "nop") and body and body[-1].mnemonic == "ret":
            break
        body.append(ins)
    return body


def main() -> int:
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    dll = ANNIVERSARY_DLL if arg == "--anniversary" else Path(arg) if arg else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no hw.dll at {dll}") or 2

    src = RUST.read_text(encoding="utf-8")
    builds = rust_builds(src)
    margin = int(rust_const(src, "STACK_MARGIN").replace("_", ""), 0)
    max_plane = int(rust_const(src, "MAX_PLANE").replace("_", ""), 0)

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
    hits = {name: signature_matches(image, pattern) for name, pattern, _ in builds}
    for name, found in hits.items():
        print(f"     {name} signature: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [b for b in builds if hits[b[0]]]
    if len(matching) != 1:
        print(f"FAIL {len(matching)} builds' signatures match; exactly one should")
        print("\nMISMATCH -- do not ship")
        return 1
    name, pattern, stolen = matching[0]
    print(f"     this is the {name} hw.dll")

    # -- 1. the signature --------------------------------------------------
    print("\n== the entry ==")
    matches = hits[name]
    if len(matches) != 1:
        print(f"FAIL the signature matches {len(matches)} times: {[hex(m) for m in matches]}")
        print("\nMISMATCH -- do not ship")
        return 1
    entry = matches[0]
    check(True, f"the signature matches exactly once, at +{entry:#x}")
    check(image[entry : entry + len(stolen)] == stolen, f"it starts with the stolen bytes {stolen.hex(' ')}")

    # -- 2. the span -------------------------------------------------------
    targets = branch_targets(pe, image, base, md)
    pointers = relocated_pointers(pe, image, base)
    interior = range(entry + 1, entry + len(stolen))
    inbound = [(s, t) for t in interior for s, _ in targets.get(t, ())]
    inbound += [(s, t) for t in interior for s in pointers.get(t, ())]
    check(not inbound, "no branch or relocated dword points inside the span"
          + "".join(f"\n       +{s:#x} -> +{t:#x}" for s, t in inbound))
    check(len(stolen) >= 5, f"the span is {len(stolen)} bytes; a near jump needs 5")
    for ins in md.disasm(stolen, 0):
        relative = {x86.X86_GRP_JUMP, x86.X86_GRP_CALL, x86.X86_GRP_BRANCH_RELATIVE} & set(ins.groups)
        reads_flags = ins.eflags & (
            x86.X86_EFLAGS_TEST_OF | x86.X86_EFLAGS_TEST_SF | x86.X86_EFLAGS_TEST_ZF
            | x86.X86_EFLAGS_TEST_PF | x86.X86_EFLAGS_TEST_CF | x86.X86_EFLAGS_TEST_AF
        )
        check(not relative and not reads_flags, f"stolen `{ins.mnemonic} {ins.op_str}` is position-independent and reads no flags")

    # -- 3. who calls it, and how -------------------------------------------
    print("\n== callers ==")
    body = function_body(image, base, md, entry)
    end = body[-1].address - base + body[-1].size
    callers = targets.get(entry, [])
    check(callers and all(m == "call" for _, m in callers), f"{len(callers)} direct calls, all `call`s to the entry")
    recursive = [s for s, _ in callers if entry <= s < end]
    check(len(recursive) == 4, f"{len(recursive)} of them are the function calling itself (expected 4)")
    for source, _ in callers:
        # The first `add esp` within a few instructions; one caller folds an
        # earlier call's 3 arguments into the same pop (0x28).
        after = list(md.disasm(image[source + 5 : source + 0x20], base + source + 5))[:3]
        pop = next((i for i in after if i.mnemonic == "add" and i.op_str.startswith("esp, ")), None)
        popped = int(pop.op_str.split(", ")[1], 0) if pop else 0
        check(popped >= 0x1c, f"+{source:#x} pops the 7 arguments itself after the call (`add esp, {popped:#x}`)")
    rets = [i for i in body if i.mnemonic == "ret"]
    check(rets and all(not i.op_str for i in rets), f"all {len(rets)} of its `ret`s are plain (cdecl)")

    # -- 4. the reads the stub mirrors ----------------------------------------
    print("\n== what the stub mirrors ==")
    regs, at = {}, 0
    for what, template in READS[name]:
        rx = re.compile(template.format(**regs) + "$")
        for index in range(at, len(body)):
            m = rx.match(f"{body[index].mnemonic} {body[index].op_str}")
            if m:
                regs.update(m.groupdict())
                at = index + 1
                check(True, f"{what}: +{body[index].address - base:#x} `{body[index].mnemonic} {body[index].op_str}`")
                break
        else:
            check(False, f"{what}: no `{rx.pattern}` after the previous match")
            break

    # -- 5. the sibling that keeps the range check ----------------------------
    print("\n== PM_HullPointContents ==")
    message = image.find(b"PM_HullPointContents: bad node number\0")
    check(message >= 0, "the engine has the `PM_HullPointContents: bad node number` error")
    pushers = [i for i in range(len(image) - 5) if image[i] == 0x68
               and int.from_bytes(image[i + 1 : i + 5], "little") == base + message]
    # The Anniversary build also inlines the check into PM_PointContents, so
    # it is pushed twice; the first push is PM_HullPointContents itself.
    check(pushers, f"`push`ed at {[hex(p) for p in pushers]}")
    if pushers:
        start = image.rfind(b"\x55\x8b\xec", 0, pushers[0])
        text = [f"{i.mnemonic} {i.op_str}" for i in md.disasm(image[start : pushers[0]], base + start)]
        hull = next((re.match(r"mov (e\w\w), dword ptr \[ebp \+ 8\]", t).group(1)
                     for t in text if re.match(r"mov (e\w\w), dword ptr \[ebp \+ 8\]", t)), None)
        compares = [t for t in text if hull and re.match(rf"cmp e\w\w, dword ptr \[{hull} \+ (8|0xc)\]$", t)]
        # The Anniversary build also makes the early-out's compare here.
        bounds = {re.search(r"\+ (8|0xc)\]$", t).group(1) for t in compares}
        check(bounds == {"8", "0xc"}, f"+{start:#x} compares the node with hull+8 and hull+0xc before the error: {compares}")

    # -- 6. the constants -----------------------------------------------------
    print("\n== constants ==")
    exe = pefile.PE(str(dll.with_name("hl.exe")), fast_load=True)
    stack = exe.OPTIONAL_HEADER.SizeOfStackReserve
    check(margin <= stack // 8, f"STACK_MARGIN {margin:#x} is at most an eighth of hl.exe's {stack:#x} stack")
    first_branch = next(i for i, ins in enumerate(body) if ins.mnemonic.startswith("j"))
    saved = sum(1 for ins in body[:first_branch] if ins.mnemonic == "push") - 1  # not ebp
    frame = stolen[5] + saved * 4 + 4 + 4 + 7 * 4  # locals, saved regs, ebp, return address, arguments
    print(f"     one level of recursion is {frame} bytes: a 44-deep trace uses {44 * frame} of the "
          f"{stack - margin} the guard allows ({(stack - margin) // frame} levels)")
    check(max_plane > 32767, f"MAX_PLANE {max_plane:#x} is above MAX_MAP_PLANES (32767)")

    print("\nDETOUR VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
