#!/usr/bin/env python3
"""Checks `lightmap_rebuild.rs` against a real `hw.dll` (#662).

`lightmap_rebuild.rs` retargets two `call rel32`s in the pre-Anniversary
`hw.dll`, so a gamma change rebuilds the lightmaps the way the 25th
Anniversary engine does:

  1. `V_CheckGamma`'s call to its empty flush goes to our `rebuild_lightmaps`,
     which calls `GL_BuildLightmaps`;
  2. `GL_BuildLightmaps`' call to `BuildSurfaceDisplayList` goes to our
     `build_display_list_once`, which skips a surface whose `polys` is set.

Pre-Anniversary (the default), from the binary:

  - Both signatures match once and pin a `call rel32`; each call sits inside
    the function the module says (between its entry and its last `ret`), and
    `lightmap_gamma.rs`' `BUILD_PATTERN`/`CHECK_PATTERN` find those entries.
  - The flush call comes after `V_CheckGamma`'s call to `BuildGammaTable` and
    after it stores the cvar values it compares next time, so the nested
    `V_CheckGamma` that `lightmap_gamma`'s detour runs finds nothing changed.
    It pushes nothing and its target is a lone `ret`, so replacing it with a
    cdecl function of no arguments changes nothing else. Its other callers
    (the `sv_cheats` clamp, a host function) are left alone.
  - `BuildSurfaceDisplayList` is cdecl with one argument (the caller pushes
    the surface and pops it with `add esp, 4`), it is only called from
    `GL_BuildLightmaps`, and it links a new polygon in front of the
    surface's old one: it reads `[surface + POLYS]`, stores that in the new
    polygon's `next` (+0) and writes the new one to `[surface + POLYS]`. So
    without the guard every rebuild would chain another polygon, and a
    surface with `polys` set has its display list already.
  - The new polygon comes from `Hunk_Alloc`, which clears what it hands out
    (a `memset(.., 0, ..)` in `Hunk_AllocName`): a fresh map's surfaces,
    allocated the same way, start with `polys` null.

25th Anniversary (`--anniversary`):

  - Neither signature matches, so the module logs "not installed".
  - `GL_BuildLightmaps` calls its display-list builder only when the
    surface's `polys` is null -- the behaviour the guard copies. (That its
    `V_CheckGamma` flush is `GL_BuildLightmaps` is checked by
    `verify_lightmap_gamma_offsets.py --anniversary`.)

Every constant comes out of the Rust sources rather than being restated.

Usage:
    python goldsrc-hooks/tools/verify_lightmap_rebuild_offsets.py [path-to-hw.dll]
    python goldsrc-hooks/tools/verify_lightmap_rebuild_offsets.py --anniversary [path-to-hw.dll]

Defaults to the pre-Anniversary movies install; `--anniversary` defaults to
the POST-Anniversary movies install (the same `hw.dll` as stock). Needs
`pip install pefile capstone`.
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

COMMON = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLL = COMMON / "Half-Life - PRE-Anniversary for Movies" / "hw.dll"
ANNIVERSARY_DLL = COMMON / "Half-Life - POST-Anniversary for Movies" / "hw.dll"
SRC = Path(__file__).resolve().parent.parent / "src"
RUST = SRC / "lightmap_rebuild.rs"
GAMMA_RUST = SRC / "lightmap_gamma.rs"


def rust_const(src, name, file):
    match = re.search(rf"const {name}: [^=]+= (.*?);\n", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in {file}")
    return match.group(1)


def pattern_text(text):
    return " ".join(text.replace('"', " ").replace("\\", " ").split())


def build_entry(src, name):
    """The one `Build { pattern, at }` in a one-build table."""
    body = rust_const(src, name, RUST.name)
    pattern = pattern_text(re.search(r'pattern: (".*?"),\s*at', body, re.S).group(1))
    at = int(re.search(r"at: (\d+)", body).group(1))
    return pattern, at


def main():
    args = sys.argv[1:]
    anniversary = bool(args) and args[0] == "--anniversary"
    if anniversary:
        args = args[1:]
    dll = Path(args[0]) if args else ANNIVERSARY_DLL if anniversary else DEFAULT_DLL
    if not dll.is_file():
        return print(f"no hw.dll at {dll}") or 2
    src = RUST.read_text(encoding="utf-8")
    gamma_src = GAMMA_RUST.read_text(encoding="utf-8")
    flush_pattern, flush_at = build_entry(src, "FLUSH_BUILDS")
    list_pattern, list_at = build_entry(src, "DISPLAY_LIST_BUILDS")
    polys = int(rust_const(src, "SURFACE_POLYS_AT", RUST.name), 16)
    build_pattern = pattern_text(rust_const(gamma_src, "BUILD_PATTERN", GAMMA_RUST.name))
    check_pattern = pattern_text(rust_const(gamma_src, "CHECK_PATTERN", GAMMA_RUST.name))

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

    all_ins = [i for i in md.disasm(img[lo:hi], base + lo) if i.id]
    index = {i.address - base: n for n, i in enumerate(all_ins)}
    calls = {}
    for i in all_ins:
        if i.mnemonic in ("call", "jmp") and i.operands and i.operands[0].type == x86.X86_OP_IMM:
            calls.setdefault(i.operands[0].imm - base, []).append(i)
    starts = sorted(t for t, cs in calls.items() if any(c.mnemonic == "call" for c in cs))

    def function_span(entry):
        """Entry to the end of the last `ret` before the next call target."""
        end = next((t for t in starts if t > entry), hi)
        rets = [all_ins[n] for n in range(index[entry], len(all_ins))
                if all_ins[n].address - base < end and all_ins[n].mnemonic == "ret"]
        return entry, (rets[-1].address - base + rets[-1].size) if rets else end

    def function_ins(entry):
        first, last = function_span(entry)
        n = index[first]
        out = []
        while n < len(all_ins) and all_ins[n].address - base < last:
            out.append(all_ins[n])
            n += 1
        return out

    def call_target(at):
        return (at + 5 + int.from_bytes(img[at + 1:at + 5], "little", signed=True)) & 0xffffffff

    print(f"== {dll.name} ({dll.stat().st_size:,} bytes) ==")
    if anniversary:
        print("\n== 25th Anniversary: nothing to install ==")
        for name, pattern in (("FLUSH_BUILDS", flush_pattern), ("DISPLAY_LIST_BUILDS", list_pattern)):
            found = matches(pattern)
            check(not found, f"{name} matches nothing: {[hex(f) for f in found]}, so the module logs `not installed`")
        builders = set()
        for n, i in enumerate(all_ins):
            if i.mnemonic == "push" and i.op_str == "0x8000" and all_ins[n + 1].op_str == "0":
                entry = max((t for t in starts if t <= i.address - base), default=None)
                if entry is not None and any(j.mnemonic == "cmp" and j.op_str == "byte ptr [eax], 0x2a"
                                             for j in function_ins(entry)):
                    builders.add(entry)
        check(len(builders) == 1, f"GL_BuildLightmaps: clears 0x8000 bytes and skips `*` models: {[hex(b) for b in builders]}")
        if len(builders) != 1:
            print("\nMISMATCH")
            return 1
        body = function_ins(builders.pop())
        guarded = []
        for n, i in enumerate(body[:-3]):
            if (i.mnemonic == "mov" and re.match(rf"e\w\w, dword ptr \[e\w\w \+ {polys:#x}\]$", i.op_str)
                    and body[n + 1].mnemonic == "test" and body[n + 2].mnemonic == "je"):
                to = body[n + 2].operands[0].imm - base
                k = next((m for m, j in enumerate(body) if j.address - base == to), None)
                if k is not None and body[k].mnemonic == "push" and body[k + 1].mnemonic == "call":
                    guarded.append((i.address - base, body[k + 1].operands[0].imm - base))
        check(len(guarded) == 1,
              "it builds a surface's display list only while `polys` "
              f"(+{polys:#x}) is null: " + ", ".join(f"test at +{a:#x}, builder +{t:#x}" for a, t in guarded))
        print("\nNOTHING TO INSTALL -- the Anniversary engine already does this" if ok else "\nMISMATCH")
        return 0 if ok else 1

    print("\n== the signatures ==")
    found = {}
    for name, pattern, at in (("flush", flush_pattern, flush_at), ("display list", list_pattern, list_at),
                              ("GL_BuildLightmaps", build_pattern, 0), ("V_CheckGamma", check_pattern, 0)):
        hits = matches(pattern)
        check(len(hits) == 1, f"{name} matches once: {[hex(h) for h in hits]}")
        if len(hits) != 1:
            print("\nMISMATCH -- do not ship")
            return 1
        found[name] = hits[0] + at
    flush_site, list_site = found["flush"], found["display list"]
    build, vcheck = found["GL_BuildLightmaps"], found["V_CheckGamma"]
    for name, site in (("flush", flush_site), ("display list", list_site)):
        check(img[site] == 0xE8 and site in index, f"the {name} site +{site:#x} is a `call rel32`")

    print("\n== V_CheckGamma's flush ==")
    v_first, v_last = function_span(vcheck)
    check(v_first < flush_site < v_last, f"+{flush_site:#x} is inside V_CheckGamma +{v_first:#x}..+{v_last:#x}")
    flush = call_target(flush_site)
    first = next(md.disasm(img[flush:flush + 4], base + flush))
    check(first.mnemonic == "ret" and not first.op_str, f"its target +{flush:#x} is a lone `ret`")
    body = function_ins(vcheck)
    at = next(n for n, i in enumerate(body) if i.address - base == flush_site)
    before = body[:at]
    gamma_calls = [i for i in before if i.mnemonic == "call"]
    check(len(gamma_calls) >= 2, f"it comes after the clamp and BuildGammaTable: {[hex(c.operands[0].imm - base) for c in gamma_calls]}")
    stores = [i for i in body[index[gamma_calls[-1].address - base] - index[vcheck]:at]
              if i.mnemonic == "mov" and i.op_str.startswith("dword ptr [0x")]
    check(len(stores) == 4, f"and after the four stores of the values compared next time ({len(stores)})")
    check(body[at - 1].mnemonic != "push" and not (body[at + 1].mnemonic == "add" and body[at + 1].op_str.startswith("esp")),
          "the call passes no arguments")
    others = [hex(c.address - base) for c in calls.get(flush, []) if c.address - base != flush_site]
    print(f"     the flush's other callers, left alone: {others}")

    print("\n== GL_BuildLightmaps' display-list call ==")
    b_first, b_last = function_span(build)
    check(b_first < list_site < b_last, f"+{list_site:#x} is inside GL_BuildLightmaps +{b_first:#x}..+{b_last:#x}")
    bsdl = call_target(list_site)
    n = index[list_site]
    check(all_ins[n - 1].mnemonic == "push" and all_ins[n + 1].mnemonic == "add" and all_ins[n + 1].op_str == "esp, 4",
          f"it pushes one argument (`{all_ins[n - 1].op_str}`) and pops it after (cdecl)")
    callers = [hex(c.address - base) for c in calls.get(bsdl, [])]
    check(callers == [hex(list_site)], f"BuildSurfaceDisplayList +{bsdl:#x} is called only from there: {callers}")
    head = function_ins(bsdl)[:40]
    arg = next((i.op_str.split(",")[0] for i in head if i.mnemonic == "mov" and i.op_str.endswith("dword ptr [ebp + 8]")), None)
    check(arg, f"it keeps the surface in {arg}")
    read = next((i for i in head if i.mnemonic == "mov" and i.op_str.endswith(f"dword ptr [{arg} + {polys:#x}]")), None)
    old = read.op_str.split(",")[0] if read else None
    link = next((i for i in head if i.mnemonic == "mov" and re.fullmatch(rf"dword ptr \[(e\w\w)\], {old}", i.op_str)), None)
    new = re.fullmatch(r"dword ptr \[(e\w\w)\], \w+", link.op_str).group(1) if link else None
    write = next((i for i in head if i.mnemonic == "mov" and i.op_str == f"dword ptr [{arg} + {polys:#x}], {new}"), None)
    check(read and link and write,
          f"it links a new polygon in front of the old: reads [{arg}+{polys:#x}] into {old}, "
          f"stores it as the new one's next ([{new}]), writes {new} to [{arg}+{polys:#x}]")
    hunk = [i for i in head if i.mnemonic == "call"]
    check(hunk, f"the polygon comes from +{hunk[0].operands[0].imm - base:#x}" if hunk else "no allocation call")
    if hunk:
        alloc_name = [i for i in function_ins(hunk[0].operands[0].imm - base) if i.mnemonic == "call"]
        inner = alloc_name[0].operands[0].imm - base if alloc_name else None
        body_inner = function_ins(inner) if inner else []
        zeroed = any(i.mnemonic == "push" and i.op_str == "0" and body_inner[k + 2].mnemonic == "call"
                     for k, i in enumerate(body_inner[:-2]))
        check(zeroed, f"Hunk_Alloc -> Hunk_AllocName +{inner:#x} clears what it hands out (memset 0)"
              if inner else "could not follow Hunk_Alloc")

    print("\nRETARGETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
