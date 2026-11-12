#!/usr/bin/env python3
"""Checks `demoplayer_dict_cache.rs`'s detour against a real `DemoPlayer.dll`.

`demoplayer_dict_cache.rs` writes a jump over the two stores at the end of
`ObjectDictionary::CheckSize` and clears the dictionary's lookup cache after
them (part of #1392). The unit tests prove the stub assembles as documented;
only the binary can prove the facts the stub relies on:

  0. Exactly one of the builds in `BUILDS` matches this `DemoPlayer.dll`.
  1. Its signature matches exactly once, and holds the stolen bytes at
     `site`: `mov [this+0xc], <entries>` and `mov [this+0x118], <capacity>`,
     through the register the build names.
  2. Nothing branches into the interior of the span, no relocated dword
     points into it, and the stolen stores are position-independent.
  3. The code after the span pops and returns `al = 1` without reading
     `eax`, `ecx`, `edx` or the flags first, so the stub may change them.
  4. The function holding the span is `CheckSize`: it reads the size from
     `this+0x114` and the capacity from `this+0x118`, and allocates and
     frees. Its caller `AddToDictionary` calls it, then `FindClosestAsIndex`.
  5. `FindClosestAsIndex` (or the cache lookup it calls first) scans 32
     entries of `{pointer, key}` from `this+0x10`, skipping a null pointer,
     and turns a hit into `(pointer - entries) / 8`. So a zeroed cache is an
     empty one, and a stale pointer is the bad index.
  6. The engine's own cache clear (pre-Anniversary `ClearCache`, inlined at
     the end of the 25th Anniversary `AddToDictionary`) zeroes the same span
     `CACHE_OFFSET` .. `+0x110` and the index at `+0x110`, which is what
     `CLEAR_DWORDS` from `CACHE_OFFSET` covers, ending at the size.

Every constant comes out of `demoplayer_dict_cache.rs` rather than being
restated here, for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_demoplayer_dict_cache_offsets.py [path-to-DemoPlayer.dll]
    python goldsrc-hooks/tools/verify_demoplayer_dict_cache_offsets.py --anniversary

Defaults to the pre-Anniversary movies install; `--anniversary` is the
25th Anniversary movies install. Needs `pip install pefile capstone`.
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

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLL = STEAM / "Half-Life - PRE-Anniversary for Movies" / "DemoPlayer.dll"
ANNIVERSARY_DLL = STEAM / "Half-Life - POST-Anniversary for Movies" / "DemoPlayer.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "demoplayer_dict_cache.rs"

REGS = {"Ebx": "ebx", "Edi": "edi"}

# The cache lookup, as each build's compiler spells it (capstone text, in
# order). `{this}` is the dictionary register and `{obj}` the entry pointer.
CACHE_SCAN = {
    "pre-Anniversary": [
        ("the cache's first key is this+0x14", r"lea (?P<at>e\w\w), \[(?P<this>e\w\w) \+ 0x14\]"),
        ("each entry's pointer is 4 bytes before its key", r"mov (?P<obj>e\w\w), dword ptr \[{at} - 4\]"),
        ("a null pointer is skipped", r"test {obj}, {obj}"),
        ("", r"je 0x\w+"),
        ("entries are 8 bytes apart", r"add {at}, 8"),
        ("32 of them", r"cmp e\w\w, 0x20"),
        ("a hit subtracts the array", r"mov (?P<arr>e\w\w), dword ptr \[{this} \+ 0xc\]"),
        ("", r"sub (?P<idx>e\w\w), {arr}"),
        ("and divides by 8", r"sar {idx}, 3"),
    ],
    "25th Anniversary": [
        ("the cache's first key is this+0x14", r"add (?P<at>e\w\w), 0x14"),
        ("each entry's pointer is 4 bytes before its key", r"mov (?P<obj>e\w\w), dword ptr \[{at} - 4\]"),
        ("a null pointer is skipped", r"test {obj}, {obj}"),
        ("", r"je 0x\w+"),
        ("entries are 8 bytes apart", r"add {at}, 8"),
        ("32 of them", r"cmp e\w\w, 0x20"),
        ("a hit subtracts the array", r"sub {obj}, (?P<arr>e\w\w)"),
        ("and divides by 8", r"sar {obj}, 3"),
        ("refusing only a negative index", r"test {obj}, {obj}"),
        ("", r"js 0x\w+"),
    ],
}

# The engine's own clear: where it starts, how much it zeroes, the index.
ENGINE_CLEAR = {
    "pre-Anniversary": [
        ("ClearCache zeroes 0x40 dwords", r"mov ecx, 0x40"),
        ("", r"xor eax, eax"),
        ("from this+0x10", r"lea edi, \[(?P<this>e\w\w) \+ 0x10\]"),
        ("", r"rep stosd dword ptr es:\[edi\], eax"),
        ("then the index at this+0x110", r"mov dword ptr \[{this} \+ 0x110\], eax"),
    ],
    "25th Anniversary": [
        ("AddToDictionary's clear starts at this+0x14", r"lea eax, \[(?P<this>e\w\w) \+ 0x14\]"),
        ("and zeroes 0xfc bytes (to +0x110)", r"push 0xfc"),
        ("", r"push 0"),
        ("then the index at this+0x110", r"mov dword ptr \[{this} \+ 0x110\], 0"),
        ("and puts the new entry first, at this+0x10", r"mov dword ptr \[{this} \+ 0x10\], e\w\w"),
    ],
}


def rust_builds(src: str):
    """[(name, pattern, site, stolen, this register)] from `BUILDS`."""
    table = re.search(r"const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S)
    if not table:
        raise SystemExit("could not find `const BUILDS` in demoplayer_dict_cache.rs")
    builds = []
    for name, pattern, site, stolen, reg in re.findall(
        r'name: "([^"]+)",.*?pattern: "(.*?)",\s*site: (\d+),\s*stolen: \[([^\]]*)\],\s*this: Reg::(\w+),',
        table.group(1),
        re.S,
    ):
        pattern = " ".join(pattern.replace("\\", " ").split())
        stolen = bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", stolen))
        builds.append((name, pattern, int(site), stolen, REGS[reg]))
    if not builds:
        raise SystemExit("`BUILDS` in demoplayer_dict_cache.rs has no entries this script can read")
    return builds


def rust_const(src: str, name: str) -> int:
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in demoplayer_dict_cache.rs")
    return int(match.group(1).replace("_", ""), 0)


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
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_BASERELOC"]])
    found = {}
    for block in getattr(pe, "DIRECTORY_ENTRY_BASERELOC", []):
        for entry in block.entries:
            if entry.type == 3:
                value = int.from_bytes(image[entry.rva : entry.rva + 4], "little")
                found.setdefault(value - base, []).append(entry.rva)
    return found


def function_start(starts, rva):
    """The nearest known function entry at or below `rva`."""
    return max((s for s in starts if s <= rva), default=None)


def function_body(image, base, md, start, limit=0x400):
    """The function's instructions, up to the int3/nop padding after a ret."""
    body = []
    for ins in md.disasm(image[start : start + limit], base + start):
        if ins.mnemonic in ("int3", "nop") and body and body[-1].mnemonic == "ret":
            break
        body.append(ins)
    return body


def text(ins):
    return f"{ins.mnemonic} {ins.op_str}".strip()


def match_sequence(body, steps, regs, base, check):
    """Finds each step's regex in order in `body`; True when all were found."""
    at = 0
    for what, template in steps:
        rx = re.compile(template.format(**regs) + "$")
        for index in range(at, len(body)):
            m = rx.match(text(body[index]))
            if m:
                regs.update(m.groupdict())
                at = index + 1
                if what:
                    check(True, f"{what}: +{body[index].address - base:#x} `{text(body[index])}`")
                break
        else:
            return False
    return True


def direct_calls(body, base):
    return [ins.operands[0].imm - base for ins in body
            if ins.mnemonic == "call" and ins.operands and ins.operands[0].type == x86.X86_OP_IMM]


def main() -> int:
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    dll = ANNIVERSARY_DLL if arg == "--anniversary" else Path(arg) if arg else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no DemoPlayer.dll at {dll}") or 2

    src = RUST.read_text(encoding="utf-8")
    builds = rust_builds(src)
    cache_offset = rust_const(src, "CACHE_OFFSET")
    clear_dwords = rust_const(src, "CLEAR_DWORDS")
    stolen_len = rust_const(src, "STOLEN_LEN")

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
    print(f"     TimeDateStamp {pe.FILE_HEADER.TimeDateStamp:#x}, SizeOfImage {pe.OPTIONAL_HEADER.SizeOfImage:#x}")
    hits = {b[0]: signature_matches(image, b[1]) for b in builds}
    for name, found in hits.items():
        print(f"     {name} signature: {len(found)} match(es) {[hex(m) for m in found]}")
    matching = [b for b in builds if hits[b[0]]]
    if len(matching) != 1:
        print(f"FAIL {len(matching)} builds' signatures match; exactly one should")
        print("\nMISMATCH -- do not ship")
        return 1
    name, pattern, site, stolen, this = matching[0]
    print(f"     this is the {name} DemoPlayer.dll")

    # -- 1. the span ---------------------------------------------------------
    print("\n== the span ==")
    if len(hits[name]) != 1:
        print(f"FAIL the signature matches {len(hits[name])} times")
        print("\nMISMATCH -- do not ship")
        return 1
    span = hits[name][0] + site
    check(len(stolen) == stolen_len, f"{len(stolen)} stolen bytes (STOLEN_LEN {stolen_len})")
    check(image[span : span + len(stolen)] == stolen, f"+{span:#x} holds the stolen bytes {stolen.hex(' ')}")
    stores = list(md.disasm(stolen, 0))
    check(len(stores) == 2
          and re.fullmatch(rf"dword ptr \[{this} \+ 0xc\], e\w\w", stores[0].op_str) and stores[0].mnemonic == "mov"
          and re.fullmatch(rf"dword ptr \[{this} \+ 0x118\], e\w\w", stores[1].op_str) and stores[1].mnemonic == "mov",
          f"they are the two stores through {this}: " + "; ".join(text(i) for i in stores))

    # -- 2. nothing lands inside it --------------------------------------------
    targets = branch_targets(pe, image, base, md)
    pointers = relocated_pointers(pe, image, base)
    interior = range(span + 1, span + len(stolen))
    inbound = [(s, t) for t in interior for s, _ in targets.get(t, ())]
    inbound += [(s, t) for t in interior for s in pointers.get(t, ())]
    check(not inbound, "no branch or relocated dword points inside the span"
          + "".join(f"\n       +{s:#x} -> +{t:#x}" for s, t in inbound))
    into_start = [s for s, _ in targets.get(span, ())]
    print(f"     branches to the span's first byte (they take the jump, as they should): {[hex(s) for s in into_start]}")
    for ins in stores:
        relative = {x86.X86_GRP_JUMP, x86.X86_GRP_CALL, x86.X86_GRP_BRANCH_RELATIVE} & set(ins.groups)
        check(not relative and ins.eflags == 0, f"stolen `{text(ins)}` is position-independent and touches no flags")

    # -- 3. what runs after it -------------------------------------------------
    print("\n== after the span ==")
    tail = []
    for ins in md.disasm(image[span + len(stolen) : span + len(stolen) + 0x20], base + span + len(stolen)):
        tail.append(ins)
        if ins.mnemonic == "ret":
            break
    print("     " + "; ".join(text(i) for i in tail))
    reads_scratch = []
    for ins in tail:
        read, _written = ins.regs_access()
        names = {ins.reg_name(r) for r in read}
        if names & {"eax", "ecx", "edx", "al", "ax", "cl", "cx", "dl", "dx"}:
            reads_scratch.append(text(ins))
        flags_read = ins.eflags & (
            x86.X86_EFLAGS_TEST_OF | x86.X86_EFLAGS_TEST_SF | x86.X86_EFLAGS_TEST_ZF
            | x86.X86_EFLAGS_TEST_PF | x86.X86_EFLAGS_TEST_CF | x86.X86_EFLAGS_TEST_AF
        )
        if flags_read:
            reads_scratch.append(text(ins) + " (flags)")
    check(tail and tail[-1].mnemonic == "ret", "it reaches a `ret` with no branch")
    check(not reads_scratch, f"nothing before the `ret` reads eax, ecx, edx or the flags {reads_scratch}")
    check(any(text(i) == "mov al, 1" for i in tail), "it returns `al = 1` (resized)")

    # -- 4. CheckSize and its caller ---------------------------------------------
    print("\n== CheckSize ==")
    starts = set(targets) | {p for p in pointers if any(
        s.contains_rva(p) and s.Characteristics & 0x20000000 for s in pe.sections)}
    check_size = function_start([s for s, srcs in targets.items() if any(m == "call" for _, m in srcs)], span)
    check(check_size is not None and span - check_size < 0x100, f"CheckSize starts at +{check_size:#x}")
    body = function_body(image, base, md, check_size)
    lines = [text(i) for i in body]
    check(any(re.fullmatch(rf"mov e\w\w, dword ptr \[{this} \+ 0x114\]", t) for t in lines),
          f"it reads the size from {this}+0x114")
    check(any(re.fullmatch(rf"mov e\w\w, dword ptr \[{this} \+ 0x118\]", t) for t in lines),
          f"it reads the capacity from {this}+0x118")
    calls = [t for t in lines if t.startswith("call")]
    check(len(calls) >= 2, f"it calls an allocator and a free: {calls}")
    callers = [s for s, m in targets.get(check_size, ()) if m == "call"]
    print(f"     called from {[hex(c) for c in callers]}")

    def cache_scan_in(start):
        """The body holding the cache scan: `start`'s own, or a callee's."""
        fbody = function_body(image, base, md, start)
        for candidate in [fbody] + [function_body(image, base, md, c) for c in direct_calls(fbody, base)]:
            if match_sequence(candidate, CACHE_SCAN[name], {}, base, lambda *_: None):
                return candidate
        return None

    # AddToDictionary is the caller whose next call after CheckSize is the
    # lookup; other callers (Clear, RemoveIndex) shrink and then clear.
    adder = find_closest = scan = None
    for caller in callers:
        start = function_start(starts, caller)
        if start is None:
            continue
        seq = direct_calls(function_body(image, base, md, start), base)
        if check_size not in seq or seq.index(check_size) == len(seq) - 1:
            continue
        following = seq[seq.index(check_size) + 1]
        scan = cache_scan_in(following)
        if scan is not None:
            adder, find_closest = start, following
            break
    check(adder is not None, f"AddToDictionary (+{adder:#x}) calls CheckSize, then FindClosestAsIndex (+{find_closest:#x})"
          if adder is not None else "no caller calls CheckSize and then a function that scans the cache")
    if adder is None:
        print("\nMISMATCH -- do not ship")
        return 1

    # -- 5. the cache lookup -------------------------------------------------------
    print("\n== the cache lookup (FindClosestAsIndex) ==")
    match_sequence(scan, CACHE_SCAN[name], {}, base, check)
    check(True, "the lookup scans 32 {pointer, key} entries from this+0x10, skipping null pointers")

    # -- 6. the engine's own clear ----------------------------------------------------
    print("\n== the engine's own cache clear ==")
    abody = function_body(image, base, md, adder)
    cleared = False
    for candidate in [abody] + [function_body(image, base, md, c) for c in direct_calls(abody, base)]:
        regs = {}
        if match_sequence(candidate, ENGINE_CLEAR[name], regs, base, check):
            cleared = True
            break
    check(cleared, "the engine clears this+0x10 .. +0x110 and the index at +0x110 itself")
    end = cache_offset + clear_dwords * 4
    check(cache_offset == 0x10 and end == 0x114,
          f"the stub zeroes +{cache_offset:#x} .. +{end:#x}: the cache, its index, up to the size at +0x114")

    print("\nDETOUR VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
