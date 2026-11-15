#!/usr/bin/env python3
"""Checks `detail_cache.rs`'s patterns against real `hw.dll` files.

`detail_cache.rs` makes `DT_SetRenderState` skip its cache (the last detail
texture bound on unit 2 and the last scale set) by turning two branches into
plain jumps. The unit tests prove each offset sits on the instruction it
names; only the binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The match lies inside the function that prints "Loading Detail
     Textures..." (it starts at most 0x200 bytes before it), i.e. it really
     is `DT_SetRenderState`.
  3. The texture address read at the start is the one the pattern stores back
     (`mov [texture], ecx`), and the two scales follow it at +4 and +8, the
     layout both builds use.
  4. The function binds texture 0xDE1 (GL_TEXTURE_2D) and sets the texture
     matrix (`push 0x1702`, GL_TEXTURE) after the check -- what the cache
     lets it skip.
  5. The `je` at `bind_jump_at` goes to the path that stores the texture
     (`mov ecx, [esi+x]; mov [texture], ecx`), and the `jp` at
     `scale_jump_at` goes to the path that stores the horizontal scale -- so
     making both unconditional always takes the "different" side.

Patterns and offsets are read out of `detail_cache.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_detail_cache_offsets.py [hw.dll ...]

Defaults to both movie installs. Needs `pip install pefile`.
"""

import re
import struct
import sys
from pathlib import Path

try:
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "detail_cache.rs"


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = [(m.group(1), m.group(2), *(int(m.group(i)) for i in range(3, 8)))
           for m in re.finditer(r'build: "([^"]+)".*?pattern: "([^"]+)".*?texture_at: (\d+).*?'
                                r'hscale_at: (\d+).*?vscale_at: (\d+).*?bind_jump_at: (\d+).*?'
                                r'scale_jump_at: (\d+)', text, re.S)]
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    return re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                               for t in pattern.split()), re.S)


def u32(image, at):
    return struct.unpack("<I", image[at:at + 4])[0]


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    text_at = image.find(b"Loading Detail Textures")
    loader = image.find(b"\x68" + struct.pack("<I", base + text_at)) if text_at >= 0 else -1
    ok = True
    matched = []
    for build, pattern, texture_at, hscale_at, vscale_at, bind_at, scale_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            print(f"  FAIL {build}: {len(hits)} matches, expected one")
            ok = False
            continue
        at = hits[0]
        if loader < 0 or not (at - 0x200 <= loader < at):
            print(f"  FAIL {build}: +{at:#x} isn't just after the \"Loading Detail Textures\" push")
            ok = False
            continue
        texture, h, v = u32(image, at + texture_at), u32(image, at + hscale_at), u32(image, at + vscale_at)
        stored = re.compile(b"\x89\x0d" + re.escape(struct.pack("<I", texture))).search(image, at, at + 48)
        if not stored or (h, v) != (texture + 4, texture + 8):
            print(f"  FAIL {build}: texture {texture:#x}, scales {h:#x}/{v:#x} aren't one cached block")
            ok = False
            continue
        after = image[at:at + 0x100]
        if b"\x68\xe1\x0d\x00\x00" not in after or b"\x68\x02\x17\x00\x00" not in after:
            print(f"  FAIL {build}: no glBindTexture(GL_TEXTURE_2D) / GL_TEXTURE matrix after the check")
            ok = False
            continue
        bind_target = at + bind_at + 2 + struct.unpack("<b", image[at + bind_at + 1:at + bind_at + 2])[0]
        scale_target = at + scale_at + 2 + struct.unpack("<b", image[at + scale_at + 1:at + scale_at + 2])[0]
        # `mov ecx, [esi+x]; mov [texture], ecx` (8B 4E xx 89 0D <texture>).
        stores_texture = (image[bind_target:bind_target + 2] == bytes([0x8B, 0x4E])
                          and image[bind_target + 3:bind_target + 9]
                          == bytes([0x89, 0x0D]) + struct.pack("<I", texture))
        # `fstp [hscale]` (D9 1D) or `movss [hscale], xmm1` (F3 0F 11 0D).
        fstp = bytes([0xD9, 0x1D]) + struct.pack("<I", h)
        movss = bytes([0xF3, 0x0F, 0x11, 0x0D]) + struct.pack("<I", h)
        stores_scale = any(image[k:k + 6] == fstp or image[k:k + 8] == movss
                           for k in range(scale_target, scale_target + 16))
        if image[at + bind_at] != 0x74 or image[at + scale_at] != 0x7A or not stores_texture or not stores_scale:
            print(f"  FAIL {build}: the branches at +{at + bind_at:#x} / +{at + scale_at:#x} don't lead to the stores")
            ok = False
            continue
        print(f"  ok   {build}: DT_SetRenderState's cache at +{texture - base:#x} (texture), "
              f"+{h - base:#x}, +{v - base:#x} (scales); check at +{at:#x}")
        matched.append(build)
    if len(matched) != 1:
        print(f"  FAIL: {len(matched)} builds matched, expected exactly one")
        ok = False
    return ok


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    all_sites = sites()
    ok = True
    for path in dlls:
        print(path)
        ok &= check(path, all_sites)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
