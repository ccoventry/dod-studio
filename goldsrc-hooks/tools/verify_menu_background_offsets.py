#!/usr/bin/env python3
"""Checks `menu_background.rs` against the real `GameUI.dll`s (#1045).

`menu_background.rs` points the one `call DrawBackgroundImage` in
`CBasePanel::PaintBackground` at its own function and reads the panel's
background fields. This proves, on both builds:

  1. Each `BUILDS` pattern matches exactly once in its own GameUI's code and
     not at all in the other's; the `E8` sits at `at`, after `mov ecx, esi`.
  2. The call goes to `DrawBackgroundImage`: the function that reads the
     image vector (`images`, `count`), the layout's `resolution` (`res_x`,
     `res_y`), each tile's `scaled` flag at +0x14 (and on the Anniversary
     build its `fit` flag at +0x15), and paints through the surface slots the
     Rust uses: `GetScreenSize` (+0x80, slot 32), `DrawSetColor` (+0x20, 8),
     `DrawSetTexture` (+0x6c, 27), `DrawTexturedRect` (+0x74, 29).
  3. The layout loader (the function that opens
     `resource/BackgroundLayout.txt`) fills the same fields: `resolution` into
     `res_x`/`res_y`, 24-byte tiles, `scaled` (+0x14) and, Anniversary only,
     `fit` (+0x15); it creates each texture with `CreateNewTextureID`
     (+0x7c, slot 31). `DrawSetTextureRGBA` (slot 26) is checked by
     `verify_list_icons.py`.
  4. Where the build picks `HD_BackgroundLayout.txt` (Anniversary only).

Every constant comes out of `menu_background.rs` rather than being restated
here, for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_menu_background_offsets.py [pre-GameUI.dll anniversary-GameUI.dll]

Defaults to the two movie installs. Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll"
ANNIVERSARY = STEAM / "Half-Life - POST-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "menu_background.rs"

FAILS = []


def check(ok, what):
    print(("  ok    " if ok else "  FAIL  ") + what)
    if not ok:
        FAILS.append(what)


def rust_builds(src):
    body = re.search(r"pub\(crate\) const BUILDS: \[Build; 2\] = \[(.*?)\n\];", src, re.S).group(1)
    builds = []
    for block in re.findall(r"Build \{(.*?)\n    \}", body, re.S):
        b = {}
        b["name"] = re.search(r'name: "([^"]+)"', block).group(1)
        b["pattern"] = re.search(r'pattern: "([^"]+)"', block).group(1)
        for key in ("at", "images", "count", "res_x", "res_y"):
            b[key] = int(re.search(rf"\b{key}: (0x[0-9a-f]+|\d+),", block).group(1), 0)
        b["fits"] = re.search(r"fits: (true|false)", block).group(1) == "true"
        builds.append(b)
    return builds


def rust_const(src, name):
    return int(re.search(rf"const {name}: usize = (0x[0-9a-f]+|\d+);", src).group(1), 0)


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path), fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        code = next(s for s in self.pe.sections if s.Characteristics & 0x20000000)
        self.code = (code.VirtualAddress, code.VirtualAddress + code.Misc_VirtualSize)
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)

    def find(self, pattern):
        tokens = pattern.split()
        regex = b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in tokens)
        lo, hi = self.code
        return [lo + m.start() for m in re.finditer(regex, self.img[lo:hi], re.S)]

    def function(self, rva, limit=0x500):
        """Instructions from `rva` to its last `ret` before a run of padding."""
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            out.append(f"{ins.mnemonic} {ins.op_str}")
            if ins.mnemonic == "ret" and self.img[ins.address - self.base + ins.size] in (0xCC, 0x90):
                break
        return out

    def string_va(self, text):
        at = self.img.find(text.encode() + b"\0")
        return None if at < 0 else self.base + at

    def function_start(self, rva):
        """The nearest `push ebp; mov ebp, esp` before `rva`: both builds'
        loaders open with that frame (checked by the loader checks after)."""
        at = self.img.rfind(b"\x55\x8b\xec", 0, rva)
        return max(at, 0)


def mentions(lines, regex):
    return any(re.search(regex, line) for line in lines)


def main():
    src = RUST.read_text(encoding="utf-8")
    builds = rust_builds(src)
    tile_size = rust_const(src, "TILE_SIZE")
    tile_scaled = rust_const(src, "TILE_SCALED")
    slots = {name: rust_const(src, f"SURFACE_{name}") for name in (
        "DRAW_SET_COLOR", "DRAW_SET_TEXTURE", "DRAW_TEXTURED_RECT", "CREATE_NEW_TEXTURE_ID", "GET_SCREEN_SIZE")}
    paths = sys.argv[1:3] if len(sys.argv) >= 3 else [PRE, ANNIVERSARY]
    images = [Image(p) for p in paths]
    for index, (build, image) in enumerate(zip(builds, images)):
        other = images[1 - index]
        print(f"== {build['name']}: {paths[index]}")
        hits = image.find(build["pattern"])
        check(len(hits) == 1, f"pattern matches once ({len(hits)})")
        check(not other.find(build["pattern"]), "and not in the other build's GameUI")
        if len(hits) != 1:
            continue
        call = hits[0] + build["at"]
        check(image.img[call] == 0xE8 and image.img[call - 2:call] == b"\x8b\xce",
              f"`mov ecx, esi; call` at +{call:#x}")
        target = call + 5 + struct.unpack_from("<i", image.img, call + 1)[0]
        print(f"     PaintBackground +{call:#x} -> DrawBackgroundImage +{target:#x}")
        paint = image.function(target)
        reg = r"\[e[a-z]{2} \+ "
        for key in ("images", "count", "res_x", "res_y"):
            check(mentions(paint, reg + f"{build[key]:#x}\\]"), f"draw reads {key} at +{build[key]:#x}")
        check(mentions(paint, f"byte ptr {reg}{tile_scaled:#x}\\]"), f"draw tests a tile's scaled flag (+{tile_scaled:#x})")
        fit_flag = tile_scaled + 1
        check(mentions(paint, f"byte ptr {reg}{fit_flag:#x}\\]") == build["fits"],
              f"draw {'tests' if build['fits'] else 'has no'} fit flag (+{fit_flag:#x})")
        for name in ("GET_SCREEN_SIZE", "DRAW_SET_COLOR", "DRAW_SET_TEXTURE", "DRAW_TEXTURED_RECT"):
            check(mentions(paint, f"call dword ptr \\[e[a-z]x \\+ {slots[name] * 4:#x}\\]"),
                  f"draw calls surface slot {slots[name]} ({name})")

        layout_va = image.string_va("resource/BackgroundLayout.txt")
        refs = [i for i in range(*image.code) if image.img[i:i + 4] == struct.pack("<I", layout_va)]
        check(len(refs) == 1, "one reference to resource/BackgroundLayout.txt")
        if not refs:
            continue
        loader_rva = image.function_start(refs[0])
        loader = image.function(loader_rva, 0x600)
        print(f"     layout loader +{loader_rva:#x}")
        check(mentions(loader, reg + f"{build['res_x']:#x}\\], eax") and mentions(loader, reg + f"{build['res_y']:#x}\\], eax"),
              "loader stores resolution into res_x / res_y")
        check(mentions(loader, r"\*8\]") and mentions(loader, r"\*2\]"), f"loader indexes {tile_size}-byte tiles (i*3*8)")
        check(mentions(loader, f"mov byte ptr {reg}{tile_scaled:#x}\\]"), "loader sets the scaled flag")
        check(mentions(loader, f"mov byte ptr {reg}{fit_flag:#x}\\]") == build["fits"],
              f"loader {'sets' if build['fits'] else 'has no'} fit flag")
        check(mentions(loader, f"call dword ptr \\[e[a-z]x \\+ {slots['CREATE_NEW_TEXTURE_ID'] * 4:#x}\\]"),
              "loader calls CreateNewTextureID (slot 31)")
        hd = image.string_va("resource/HD_BackgroundLayout.txt")
        print(f"     HD_BackgroundLayout.txt: {'yes (hd_background 1)' if hd else 'no'}")
        check(bool(hd) == build["fits"], "HD layout only on the build that fits")

    if FAILS:
        print(f"\n{len(FAILS)} check(s) failed")
        sys.exit(1)
    print("\nall checks passed")


if __name__ == "__main__":
    main()
