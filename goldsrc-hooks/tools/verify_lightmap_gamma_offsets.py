#!/usr/bin/env python3
"""Checks `lightmap_gamma.rs` against a real `hw.dll` (#365, #370).

`lightmap_gamma.rs` detours `GL_BuildLightmaps`' entry to call
`V_CheckGamma` first, so a map's lightmaps are built from gamma tables that
match the current cvars. The unit test proves the stub assembles as
documented; this proves what it and its explanation rely on.

For either build, derived from the binary alone (no signatures): the four
gamma cvars, `V_Init` (the one call of `BuildGammaTable` with the constant
2.5, made while the cvars still hold their defaults), `BuildGammaTable`
(writes three tables of 1024 entries), `R_BuildLightMap` (reads one of them
per luxel, `[reg*4 + table]`), `GL_BuildLightmaps` (clears the 0x8000-byte
allocation blocks and reaches `R_BuildLightMap`), `V_CheckGamma` (every
other caller of `BuildGammaTable`; it compares the cvars with the values it
saw last time), and the "flush" `V_CheckGamma` calls right after rebuilding
the tables. What that flush does decides whether the build has the bug:

  - pre-Anniversary: the flush is a lone `ret`, so a gamma change never
    rebuilds lightmaps and the first map keeps the ones built from the
    default-cvar table. The detour is needed, and the rest of the script
    then checks it (below).
  - 25th Anniversary: the flush is `GL_BuildLightmaps` itself (through a
    `jmp` thunk), the same function the map load calls. The first screen
    update's `V_CheckGamma` rebuilds the tables and every lightmap before
    the frame draws, so the first map is lit like every later one. The
    script proves that, and that the pre-Anniversary signatures match
    nothing in this build, so the module stays out of it (#370).

Pre-Anniversary only, the detour itself:

  1. Both signatures match exactly once, and agree with the derivation
     above. `GL_BuildLightmaps` starts with the stolen bytes, nothing
     branches into the span, and every direct caller passes no arguments
     (no `push` before the call, no `add esp` after). The stolen
     instructions are position-independent and read no flags.
  2. `V_CheckGamma` takes no arguments (it reads nothing off the stack) and
     returns with a plain `ret`, so calling it from the stub is safe.
  3. The explanation holds:
     - `V_CheckGamma` calls `BuildGammaTable`, and then a "flush" function
       that is a lone `ret` -- so a gamma change never rebuilds lightmaps;
     - `BuildGammaTable` writes the 1024-entry table `R_BuildLightMap` reads
       per luxel (`[reg*4 + table]`), and it's the same table;
     - `V_Init` calls `BuildGammaTable` with the constant 2.5, while the
       cvars still hold their defaults;
     - `GL_BuildLightmaps` reaches `R_BuildLightMap` (through
       `GL_CreateSurfaceLightmap`);
     - `V_CheckGamma`'s only callers are the screen update's, so without
       the detour nothing refreshes the table before a first map loads.

Every constant comes out of `lightmap_gamma.rs` rather than being restated
here, for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_lightmap_gamma_offsets.py [path-to-hw.dll]
    python goldsrc-hooks/tools/verify_lightmap_gamma_offsets.py --anniversary [path-to-hw.dll]

Defaults to the pre-Anniversary movies install; `--anniversary` is the stock
25th Anniversary one (the POST-Anniversary movies install has the same
`hw.dll`). Needs `pip install pefile capstone`.
"""

import re
import struct
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
RUST = Path(__file__).resolve().parent.parent / "src" / "lightmap_gamma.rs"

CVARS = ("gamma", "lightgamma", "texgamma", "brightness")
# The "flush" in V_CheckGamma and the map load both call this on the
# Anniversary build; its only use here is to name the function that does.
TEXTURE_OVERFLOW = b"Texture Overflow: MAX_GLTEXTURES"


def rust_const(src, name):
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in lightmap_gamma.rs")
    return match.group(1)


def pattern_of(src, name):
    return " ".join(rust_const(src, name).replace('"', " ").replace("\\", " ").split())


def main():
    args = sys.argv[1:]
    anniversary = bool(args) and args[0] == "--anniversary"
    if anniversary:
        args = args[1:]
    dll = Path(args[0]) if args else ANNIVERSARY_DLL if anniversary else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no hw.dll at {dll}") or 2
    src = RUST.read_text(encoding="utf-8")
    stolen = bytes(int(b, 16) for b in re.findall(r"0x([0-9a-f]{2})", rust_const(src, "STOLEN")))

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = bytes(pe.get_memory_mapped_image())
    code = next(s for s in pe.sections if s.Characteristics & 0x20000000)
    lo, hi = code.VirtualAddress, code.VirtualAddress + code.Misc_VirtualSize
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    md.skipdata = True
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    def matches(pattern):
        rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                                 for t in pattern.split()), re.S)
        return [lo + m.start() for m in rx.finditer(img[lo:hi])]

    def body(rva, limit=0x800):
        out = []
        for ins in md.disasm(img[rva:rva + limit], base + rva):
            out.append(ins)
            if ins.mnemonic == "ret" and ins.id:
                break
        return out

    all_ins = [i for i in md.disasm(img[lo:hi], base + lo) if i.id]
    index = {i.address - base: n for n, i in enumerate(all_ins)}
    direct = {}
    for i in all_ins:
        if {x86.X86_GRP_JUMP, x86.X86_GRP_CALL} & set(i.groups):
            for op in i.operands:
                if op.type == x86.X86_OP_IMM:
                    direct.setdefault(op.imm - base, []).append(i)
    starts = sorted(t for t in direct if any(c.mnemonic == "call" for c in direct[t]))

    def owner(rva):
        """The function containing `rva`: the nearest call target at or before it."""
        return max((t for t in starts if t <= rva), default=None)

    def calls_from(function):
        """Direct call targets between a function's entry and the next one's."""
        end = next((t for t in starts if t > function), hi)
        n = index.get(function)
        out = []
        while n is not None and n < len(all_ins) and all_ins[n].address - base < end:
            i = all_ins[n]
            if i.mnemonic == "call" and i.operands[0].type == x86.X86_OP_IMM:
                out.append(i.operands[0].imm - base)
            n += 1
        return out

    def callers_of(function):
        return [i for i in direct.get(function, []) if i.mnemonic == "call"]

    def follow_thunks(rva):
        """`jmp imm` thunks forward to the function that does the work."""
        for _ in range(4):
            first = next(md.disasm(img[rva:rva + 8], base + rva))
            if first.mnemonic != "jmp" or first.operands[0].type != x86.X86_OP_IMM:
                return rva
            rva = first.operands[0].imm - base
        return rva

    # -- derived from the binary, for either build ---------------------------
    print(f"== {dll.name} ({dll.stat().st_size:,} bytes): the gamma machinery, derived ==")
    cvars = {}
    for name in CVARS:
        strings = [m.start() for m in re.finditer(re.escape(name.encode()) + b"\0", img)
                   if m.start() == 0 or img[m.start() - 1] == 0]
        structs = [m.start() for s in strings for m in re.finditer(re.escape(struct.pack("<I", base + s)), img)]
        check(len(structs) == 1, f"cvar `{name}` is at +{structs[0]:#x}" if len(structs) == 1
              else f"cvar `{name}`: {len(structs)} candidates {[hex(s) for s in structs]}")
        cvars[name] = structs[0] if len(structs) == 1 else None
    if None in cvars.values():
        print("\nMISMATCH -- could not find the gamma cvars")
        return 1
    values = {cvars[n] + 0xc: n for n in CVARS}

    init_sites = [all_ins[n + 1] for n, i in enumerate(all_ins)
                  if i.mnemonic in ("push", "mov") and i.op_str.endswith("0x40200000")
                  and all_ins[n + 1].mnemonic == "call" and all_ins[n + 1].operands[0].type == x86.X86_OP_IMM]
    check(len(init_sites) == 1, f"one call passes the constant 2.5: {[hex(i.address - base) for i in init_sites]}")
    if len(init_sites) != 1:
        print("\nMISMATCH -- could not find V_Init")
        return 1
    v_init, build_gamma = owner(init_sites[0].address - base), init_sites[0].operands[0].imm - base
    print(f"     V_Init +{v_init:#x} calls BuildGammaTable +{build_gamma:#x} with 2.5")
    gamma_body = body(build_gamma, 0x400)
    read = {values[int(m.group(1), 16) - base] for i in gamma_body
            for m in [re.search(r"\[(0x[0-9a-f]+)\]", i.op_str)] if m and int(m.group(1), 16) - base in values}
    check({"lightgamma", "brightness", "texgamma"} <= read, f"BuildGammaTable reads the cvars {sorted(read)}")
    tables = [int(re.search(r"0x[0-9a-f]+", i.op_str).group(), 16) - base for i in gamma_body
              if i.mnemonic == "mov" and re.match(r"dword ptr \[e\w\w\*4 \+ 0x[0-9a-f]+\], e\w\w", i.op_str)]
    check(len(tables) == 3 and any(i.mnemonic == "cmp" and i.op_str.endswith("0x400") for i in gamma_body),
          f"it fills three 1024-entry tables: {[hex(t) for t in tables]}")
    reads = {}
    for i in all_ins:
        m = re.match(r"e\w\w, dword ptr \[e\w\w\*4 \+ (0x[0-9a-f]+)\]$", i.op_str) if i.mnemonic == "mov" else None
        if m and int(m.group(1), 16) - base in tables:
            reads.setdefault(int(m.group(1), 16) - base, set()).add(owner(i.address - base))
    check(len(reads) == 1 and len(next(iter(reads.values()))) == 1,
          f"one of them is read per luxel, by one function: {({hex(k): [hex(f) for f in v] for k, v in reads.items()})}")
    if len(reads) != 1:
        print("\nMISMATCH -- could not find R_BuildLightMap")
        return 1
    light_table, (r_build_lightmap, *_) = next(iter(reads.items()))
    print(f"     lightgammatable +{light_table:#x}, R_BuildLightMap +{r_build_lightmap:#x}")

    clears = {owner(i.address - base) for n, i in enumerate(all_ins)
              if i.mnemonic == "push" and i.op_str == "0x8000" and all_ins[n + 1].op_str == "0"}
    builders = [f for f in clears if f and (r_build_lightmap in calls_from(f)
                                            or any(r_build_lightmap in calls_from(c) for c in calls_from(f)))]
    check(len(builders) == 1, f"GL_BuildLightmaps clears the 0x8000-byte allocation blocks and reaches R_BuildLightMap: {[hex(b) for b in builders]}")
    if len(builders) != 1:
        print("\nMISMATCH -- could not find GL_BuildLightmaps")
        return 1
    gl_build_lightmaps = builders[0]
    map_load = [c.address - base for c in callers_of(gl_build_lightmaps)]
    check(map_load, f"the map load (and the sv_cheats clamp) call it from {[hex(c) for c in map_load]}")

    checks = sorted({owner(c.address - base) for c in callers_of(build_gamma)} - {v_init})
    check(checks, f"V_CheckGamma, every other caller of BuildGammaTable: {[hex(v) for v in checks]}"
                  + (" (the second is the palette pass with it inlined)" if len(checks) == 2 else ""))
    flushes = {}
    for v in checks:
        saved = [i for i in body(v) if i.mnemonic == "ucomiss" or (i.mnemonic == "fld" and "0x" in i.op_str)]
        check(len(saved) >= 4, f"+{v:#x} compares the cvars with the values it saw last time ({len(saved)} compares)")
        site = next(c for c in callers_of(build_gamma) if owner(c.address - base) == v)
        after = next(i for i in all_ins[index[site.address - base] + 1:] if i.mnemonic == "call")
        flushes[v] = follow_thunks(after.operands[0].imm - base)
    vcheck_callers = {v: [c.address - base for c in callers_of(v)] for v in checks}
    check(all(owner(c) != gl_build_lightmaps for cs in vcheck_callers.values() for c in cs),
          f"GL_BuildLightmaps itself never calls V_CheckGamma; its callers are {({hex(v): [hex(c) for c in cs] for v, cs in vcheck_callers.items()})}")
    overflow = img.find(TEXTURE_OVERFLOW)
    loader = {owner(i.address - base) for i in all_ins if overflow > 0 and i.op_str == hex(base + overflow)}
    for v, cs in vcheck_callers.items():
        for c in cs:
            if owner(c) in loader:
                print(f"     the texture loader +{owner(c):#x} calls V_CheckGamma +{v:#x} too (at +{c:#x})")

    if anniversary:
        print("\n== 25th Anniversary: is the first-load bug there? ==")
        for v, flush in flushes.items():
            check(flush == gl_build_lightmaps,
                  f"V_CheckGamma +{v:#x}'s flush is GL_BuildLightmaps +{flush:#x}: a gamma change rebuilds every lightmap")
        check(any(owner(c) != v and owner(c) not in loader for cs in vcheck_callers.values() for c in cs),
              "the screen update calls V_CheckGamma, so the first frame rebuilds the first map's lightmaps")
        print("\n== the pre-Anniversary signatures ==")
        for name in ("BUILD_PATTERN", "CHECK_PATTERN"):
            found = matches(pattern_of(src, name))
            check(not found, f"{name} matches nothing: {[hex(f) for f in found]}, so the module logs `not installed`")
        print("\nBUG ABSENT -- nothing to install on this build" if ok else "\nMISMATCH -- the explanation no longer holds")
        return 0 if ok else 1

    print("\n== pre-Anniversary: the first-load bug ==")
    for v, flush in flushes.items():
        first = next(md.disasm(img[flush:flush + 4], base + flush))
        check(first.mnemonic == "ret",
              f"V_CheckGamma +{v:#x}'s flush +{flush:#x} is a lone `ret`: a gamma change never rebuilds lightmaps")

    print("\n== signatures ==")
    build = matches(pattern_of(src, "BUILD_PATTERN"))
    vcheck = matches(pattern_of(src, "CHECK_PATTERN"))
    check(len(build) == 1, f"GL_BuildLightmaps signature matches once: {[hex(b) for b in build]}")
    check(len(vcheck) == 1, f"V_CheckGamma signature matches once: {[hex(v) for v in vcheck]}")
    if len(build) != 1 or len(vcheck) != 1:
        print("\nMISMATCH -- do not ship")
        return 1
    build, vcheck = build[0], vcheck[0]
    check(build == gl_build_lightmaps and vcheck in checks, "both signatures agree with the derivation above")
    check(img[build:build + len(stolen)] == stolen, f"GL_BuildLightmaps starts with the stolen bytes {stolen.hex(' ')}")

    print("\n== the detoured span ==")
    inbound = [hex(i.address - base) for t in range(build + 1, build + len(stolen)) for i in direct.get(t, [])]
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_BASERELOC"]])
    for block in getattr(pe, "DIRECTORY_ENTRY_BASERELOC", []):
        for entry in block.entries:
            if entry.type == 3 and build < struct.unpack_from("<I", img, entry.rva)[0] - base < build + len(stolen):
                inbound.append(f"reloc {entry.rva:#x}")
    check(not inbound, f"nothing branches or points inside the span {inbound}")
    for ins in md.disasm(stolen, 0):
        relative = {x86.X86_GRP_JUMP, x86.X86_GRP_CALL, x86.X86_GRP_BRANCH_RELATIVE} & set(ins.groups)
        check(not relative and not ins.eflags & (x86.X86_EFLAGS_TEST_ZF | x86.X86_EFLAGS_TEST_CF | x86.X86_EFLAGS_TEST_SF
                                                 | x86.X86_EFLAGS_TEST_OF | x86.X86_EFLAGS_TEST_PF),
              f"stolen `{ins.mnemonic} {ins.op_str}` is position-independent and reads no flags")
    callers = callers_of(build)
    check(callers, f"{len(callers)} direct callers")
    for c in callers:
        n = index[c.address - base]
        before, after = all_ins[n - 1], all_ins[n + 1]
        no_args = before.mnemonic != "push" and not (after.mnemonic == "add" and after.op_str.startswith("esp"))
        check(no_args, f"+{c.address - base:#x} passes no arguments (before: `{before.mnemonic} {before.op_str}`, after: `{after.mnemonic} {after.op_str}`)")

    print("\n== V_CheckGamma ==")
    vbody = body(vcheck)
    check(not any("esp + 4" in i.op_str or "ebp + 8" in i.op_str for i in vbody), "it reads no stack arguments")
    rets = [i for i in vbody if i.mnemonic == "ret"]
    # The function has an early `xor eax, eax; ret` and a late one; walk to the second.
    tail = body(rets[0].address - base + 1) if rets else []
    rets += [i for i in tail if i.mnemonic == "ret"]
    check(rets and all(not r.op_str for r in rets), f"its returns are plain `ret`s ({len(rets)})")
    calls = [i for i in vbody + tail if i.mnemonic == "call" and i.operands[0].type == x86.X86_OP_IMM]
    targets = [c.operands[0].imm - base for c in calls]
    check(len(targets) >= 3, f"it calls the clamp, BuildGammaTable and the flush: {[hex(t) for t in targets]}")
    build_gamma, flush = targets[1], targets[2]
    flush_first = next(md.disasm(img[flush:flush + 4], base + flush))
    check(flush_first.mnemonic == "ret", f"the flush at +{flush:#x} is a lone `ret`: lightmaps are never rebuilt on a gamma change")
    screen_callers = direct.get(vcheck, [])
    check(len(screen_callers) == 2, f"only two callers (the screen update and its palette pass): {[hex(i.address - base) for i in screen_callers]}")

    print("\n== the table ==")
    writes = [i for i in body(build_gamma, 0x400)
              if i.mnemonic == "mov" and re.match(r"dword ptr \[esi\*4 \+ 0x[0-9a-f]+\], eax", i.op_str)]
    tables = [int(re.search(r"0x[0-9a-f]+", i.op_str).group(), 16) for i in writes]
    reads = {}
    for i in all_ins:
        m = re.match(r"eax, dword ptr \[eax\*4 \+ (0x[0-9a-f]+)\]$", i.op_str) if i.mnemonic == "mov" else None
        if m and int(m.group(1), 16) in tables:
            reads.setdefault(int(m.group(1), 16), []).append(i.address - base)
    check(len(reads) == 1, f"BuildGammaTable writes {[hex(t) for t in tables]}; the one read per luxel is {({hex(k): [hex(a) for a in v] for k, v in reads.items()})}")
    if reads:
        table, (read_at, *_) = next(iter(reads.items()))
        owner_fn = max(t for t in direct if t <= read_at and any(c.mnemonic == "call" for c in direct[t]))
        check(img[owner_fn:owner_fn + 3] == b"\x55\x8b\xec", f"that read is in R_BuildLightMap (+{owner_fn:#x})")
        # Each function that calls R_BuildLightMap, and whether GL_BuildLightmaps
        # itself calls that function.
        via = []
        for site in (i.address - base for i in direct.get(owner_fn, []) if i.mnemonic == "call"):
            function = max(t for t in direct if t <= site and any(c.mnemonic == "call" for c in direct[t]))
            via += [(function, c.address - base) for c in direct.get(function, [])
                    if c.mnemonic == "call" and build < c.address - base < build + 0x400]
        check(via, "GL_BuildLightmaps reaches it through GL_CreateSurfaceLightmap: "
                   + ", ".join(f"+{f:#x} called at +{at:#x}" for f, at in via))

    print("\n== V_Init ==")
    pushes = [i for i in direct.get(build_gamma, []) if i.mnemonic == "call"]
    constant = [c for c in pushes if all_ins[index[c.address - base] - 1].op_str == "0x40200000"]
    check(constant, f"V_Init calls BuildGammaTable with the constant 2.5 at {[hex(c.address - base) for c in constant]}")

    print("\nDETOUR VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
