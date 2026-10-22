#!/usr/bin/env python3
"""Checks `sprite_blend.rs`'s two-byte patch against real `hw.dll` files.

`sprite_blend.rs` removes the `jnp` in `GL_Upload32` that skips the
transparent-texel colour bleed when `gl_spriteblend` is 0 (issue #467). The
unit tests prove the patterns' offsets line up with their own tokens; only the
binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The pattern's cvar operand is the address of `gl_spriteblend`'s `value`:
     the static `cvar_t` whose `name` points at the string "gl_spriteblend",
     plus 12 (`name`, `string`, `flags`, then `value`).
  3. The byte at the jnp offset is `7B` and its target lands after the
     `cmp esi, 4` type check -- i.e. it skips exactly the bleed.

Patterns and offsets are read out of `sprite_blend.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_spriteblend_offsets.py [hw.dll ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "sprite_blend.rs"


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = []
    for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?cvar_operand_at: (\d+).*?jnp_at: (\d+)',
        text,
        re.S,
    ):
        out.append((m.group(1), m.group(2), int(m.group(3)), int(m.group(4))))
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    parts = []
    for tok in pattern.split():
        parts.append(b"." if tok == "??" else re.escape(bytes([int(tok, 16)])))
    return re.compile(b"".join(parts), re.S)


def spriteblend_value(pe, image, base):
    name = image.find(b"\x00gl_spriteblend\x00") + 1
    if name <= 0:
        return None
    ptr = struct.pack("<I", base + name)
    for sec in pe.sections:
        if sec.Name.startswith(b".text"):
            continue
        lo, hi = sec.VirtualAddress, sec.VirtualAddress + sec.Misc_VirtualSize
        at = image.find(ptr, lo, hi)
        if at != -1:
            return base + at + 12
    return None


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase
    value = spriteblend_value(pe, image, base)
    if value is None:
        return [f"{path}: no gl_spriteblend cvar_t found"]
    problems, matched = [], []
    for build, pattern, operand_at, jnp_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            problems.append(f"{build}: {len(hits)} matches, expected 1")
            continue
        at = hits[0]
        operand = struct.unpack_from("<I", image, at + operand_at)[0]
        if operand != value:
            problems.append(f"{build}: operand {operand:#x} is not gl_spriteblend.value {value:#x}")
        if image[at + jnp_at] != 0x7B:
            problems.append(f"{build}: byte at jnp offset is {image[at + jnp_at]:02x}, not 7b")
        target = at + jnp_at + 2 + struct.unpack_from("<b", image, at + jnp_at + 1)[0]
        end_of_pattern = at + len(pattern.split())
        if target <= end_of_pattern:
            problems.append(f"{build}: jnp lands at +{target:#x}, inside the type check")
        matched.append(f"{build} at +{at:#x}, jnp +{at + jnp_at:#x} -> +{target:#x}")
    if len(matched) != 1:
        problems.append(f"expected exactly one build to match, got {len(matched)}")
    return problems or [f"{path.parent.name}: OK -- {matched[0]}; gl_spriteblend.value at {value:#x}"]


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    all_sites = sites()
    failed = False
    for dll in dlls:
        for line in check(dll, all_sites):
            print(line)
            failed |= "OK" not in line
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
