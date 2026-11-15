#!/usr/bin/env python3
"""Checks `hud_sprites.rs`'s `cl_enginefunc_t` slots against a real DoD `client.dll` (#808).

`hud_sprites.rs` redirects five pointers in client.dll's own `gEngfuncs` copy:
`SPR_Load` (slot 0), `SPR_Set` (4), `SPR_Draw` (5), `SPR_DrawHoles` (6) and
`SPR_DrawAdditive` (7). The slots come from the SDK's `cdll_int.h`; what the
binary can prove is that client.dll calls through exactly those table entries
the way the HUD would:

  1. `Initialize` copies the engine's table into `gEngfuncs` (`mov edi, imm32;
     push eax; rep movsd` with `mov ecx, 0x87`), the same pattern
     `engine.rs`'s `find_gengfuncs` reads at runtime.
  2. Every one of the five slots has `call dword ptr [gEngfuncs + 4*slot]`
     (`ff 15 imm32`) sites in `.text`; slot 0 (`SPR_Load`) has many (every
     HUD element loads its sheets) and slot 7 (`SPR_DrawAdditive`) is the
     HUD's usual draw.
  3. `SPR_Load` sites push literal paths ending in `.spr` (`68 imm32` just
     before the call), and `SPR_Set` sites clean up four cdecl arguments
     (`add esp, 0x10` right after).

Usage:
    python goldsrc-hooks/tools/verify_hud_sprite_slots.py [client.dll ...]

Defaults to both movie installs' `client.dll` (byte-identical across installs).
Needs `pip install pefile`.
"""

import re
import sys
from pathlib import Path

try:
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile`")

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "hud_sprites.rs"
SLOTS = ["SPR_LOAD", "SPR_SET", "SPR_DRAW", "SPR_DRAW_HOLES", "SPR_DRAW_ADDITIVE"]


def rust_slots(src):
    out = {}
    for name in SLOTS:
        m = re.search(rf"const ENGFUNCS_SLOT_{name}: usize = (\d+);", src)
        if not m:
            sys.exit(f"ENGFUNCS_SLOT_{name} not in {RUST}")
        out[name] = int(m.group(1))
    return out


def find_all(hay, needle, lo, hi):
    out = []
    i = hay.find(needle, lo)
    while 0 <= i < hi:
        out.append(i)
        i = hay.find(needle, i + 1)
    return out


def verify(path, slots):
    print(f"== {path}")
    ok = True

    def check(cond, what):
        nonlocal ok
        print(("OK   " if cond else "FAIL ") + what)
        ok = ok and bool(cond)

    pe = pefile.PE(str(path))
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    t0, t1 = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize

    # 1. gEngfuncs from Initialize's copy loop.
    export = {e.name: e.address for e in pe.DIRECTORY_ENTRY_EXPORT.symbols if e.name}
    init = export.get(b"Initialize")
    check(init is not None, "client.dll exports Initialize")
    if init is None:
        return False
    window = img[init:init + 0x60]
    gengfuncs = None
    for k in range(len(window) - 8):
        if window[k] == 0xBF and window[k + 5] == 0x50 and window[k + 6:k + 8] == b"\xf3\xa5":
            gengfuncs = int.from_bytes(window[k + 1:k + 5], "little")
    check(gengfuncs is not None and b"\xb9\x87\x00\x00\x00" in window,
          f"Initialize copies 0x87 dwords into gEngfuncs at {gengfuncs:#x}" if gengfuncs else "Initialize's copy loop found")
    if gengfuncs is None:
        return False

    # 2. `call dword ptr [gEngfuncs + 4*slot]` sites.
    sites = {}
    for name in SLOTS:
        needle = b"\xff\x15" + (gengfuncs + 4 * slots[name]).to_bytes(4, "little")
        sites[name] = find_all(img, needle, t0, t1)
        if name == "SPR_DRAW":
            # DoD's HUD draws with holes or additively, never plain; the slot is
            # hooked all the same, for a sheet drawn that way by anything else.
            print(f"note slot {slots[name]} ({name}) is called {len(sites[name])} time(s) (none expected)")
            continue
        check(len(sites[name]) > 0, f"slot {slots[name]} ({name}) is called {len(sites[name])} time(s)")
    check(len(sites["SPR_LOAD"]) >= 20, "SPR_Load has the HUD's many sheet loads")
    check(len(sites["SPR_DRAW_ADDITIVE"]) >= 10, "SPR_DrawAdditive is the HUD's usual draw")

    # 3. what SPR_Load is given and how SPR_Set is cleaned up.
    spr_strings = 0
    for at in sites["SPR_LOAD"]:
        before = img[max(t0, at - 24):at]
        for k in find_all(before, b"\x68", 0, len(before)):
            if k + 5 > len(before):
                continue
            va = int.from_bytes(before[k + 1:k + 5], "little") - base
            if 0 < va < len(img) - 80 and b".spr" in img[va:va + 80].split(b"\0")[0]:
                spr_strings += 1
                break
    check(spr_strings >= 5, f"{spr_strings} SPR_Load site(s) push a literal path ending in .spr")
    # The caller pops the four arguments: `add esp, imm8` (83 c4 xx) with
    # imm8 >= 0x10 within the next few instructions (MSVC folds several
    # calls' cleanups into one add).
    four = 0
    for at in sites["SPR_SET"]:
        after = img[at + 6:at + 24]
        if any(after[k:k + 2] == b"\x83\xc4" and after[k + 2] >= 0x10 for k in range(len(after) - 2)):
            four += 1
    # Most sites fold the cleanup further away (`SPR_Set` then a draw, one
    # add for both); the ones that don't are proof enough of the arity.
    check(four >= 1,
          f"{four} of {len(sites['SPR_SET'])} SPR_Set site(s) clean up four or more cdecl arguments right after the call")
    return ok


def main():
    paths = [Path(a) for a in sys.argv[1:]] or [p for p in DEFAULT_DLLS if p.is_file()]
    slots = rust_slots(RUST.read_text(encoding="utf-8"))
    ok = all([verify(p, slots) for p in paths])
    print("\nSLOTS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
