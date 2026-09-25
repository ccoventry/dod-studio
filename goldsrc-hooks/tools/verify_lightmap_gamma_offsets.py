#!/usr/bin/env python3
"""Checks `lightmap_gamma.rs` against a real pre-Anniversary `hw.dll` (#365).

`lightmap_gamma.rs` detours `GL_BuildLightmaps`' entry to call
`V_CheckGamma` first, so a map's lightmaps are built from gamma tables that
match the current cvars. The unit test proves the stub assembles as
documented; this proves what it and its explanation rely on:

  1. Both signatures match exactly once. `GL_BuildLightmaps` starts with the
     stolen bytes, nothing branches into the span, and every direct caller
     passes no arguments (no `push` before the call, no `add esp` after).
     The stolen instructions are position-independent and read no flags.
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

Defaults to the pre-Anniversary movies install. Needs `pip install pefile
capstone`.
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
RUST = Path(__file__).resolve().parent.parent / "src" / "lightmap_gamma.rs"


def rust_const(src, name):
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in lightmap_gamma.rs")
    return match.group(1)


def pattern_of(src, name):
    return " ".join(rust_const(src, name).replace('"', " ").replace("\\", " ").split())


def main():
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
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

    print("== signatures ==")
    build = matches(pattern_of(src, "BUILD_PATTERN"))
    vcheck = matches(pattern_of(src, "CHECK_PATTERN"))
    check(len(build) == 1, f"GL_BuildLightmaps signature matches once: {[hex(b) for b in build]}")
    check(len(vcheck) == 1, f"V_CheckGamma signature matches once: {[hex(v) for v in vcheck]}")
    if len(build) != 1 or len(vcheck) != 1:
        print("\nMISMATCH -- do not ship")
        return 1
    build, vcheck = build[0], vcheck[0]
    check(img[build:build + len(stolen)] == stolen, f"GL_BuildLightmaps starts with the stolen bytes {stolen.hex(' ')}")

    print("\n== the detoured span ==")
    all_ins = [i for i in md.disasm(img[lo:hi], base + lo) if i.id]
    direct = {}
    for i in all_ins:
        if {x86.X86_GRP_JUMP, x86.X86_GRP_CALL} & set(i.groups):
            for op in i.operands:
                if op.type == x86.X86_OP_IMM:
                    direct.setdefault(op.imm - base, []).append(i)
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
    index = {i.address - base: n for n, i in enumerate(all_ins)}
    callers = [i for i in direct.get(build, []) if i.mnemonic == "call"]
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
        owner = max(t for t in direct if t <= read_at and any(c.mnemonic == "call" for c in direct[t]))
        check(img[owner:owner + 3] == b"\x55\x8b\xec", f"that read is in R_BuildLightMap (+{owner:#x})")
        # Each function that calls R_BuildLightMap, and whether GL_BuildLightmaps
        # itself calls that function.
        via = []
        for site in (i.address - base for i in direct.get(owner, []) if i.mnemonic == "call"):
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
