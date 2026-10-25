#!/usr/bin/env python3
"""Checks the site `own_blood.rs` patches for `ds_view_show_own_blood` (#1035).

`EV_BloodPuff` skips a blood puff closer than 32 units to the view with
`fcomp dword ptr [32.0]; fnstsw ax; test ah, 5; jnp <end>`. `own_blood.rs`
finds that by a byte pattern (the `fcomp` operand wildcarded) and points the
operand at a float of its own. This proves, on real `client.dll` files:

  1. The pattern matches exactly once in an executable section.
  2. The `fcomp`'s operand is an absolute address in the image holding the
     float 32.0, and the instruction is `fcomp m32fp` (`D8 1D`).
  3. The `jnp` after it is the skip: it jumps forward, past the draw.
  4. The site is the one #1018 found: `fcomp` at +0x22530, the constant at
     +0xa8894; and how many other reads share that constant (the reason the
     operand is repointed instead of the constant being written).

client.dll is byte-identical in both movie installs; both are checked anyway.

Usage:
    python goldsrc-hooks/tools/verify_own_blood_offsets.py [client.dll ...]

Defaults to both movie installs. Needs `pip install pefile`.
"""

import struct
import sys
from pathlib import Path

try:
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
# Must match `PATTERN` and `OPERAND_AT` in goldsrc-hooks/src/own_blood.rs.
PATTERN = ("D9 FA D9 5C 24 50 DD D8 DD D8 DD D8 D9 44 24 50 "
           "D8 1D ?? ?? ?? ?? DF E0 F6 C4 05 0F 8B")
OPERAND_AT = 18
KNOWN_FCOMP = 0x22530
KNOWN_CONSTANT = 0xa8894


def parse(pattern):
    return [None if t == "??" else int(t, 16) for t in pattern.split()]


def matches(image, start, end, pat):
    hits = []
    first = pat[0]
    at = image.find(bytes([first]), start, end)
    while at >= 0 and at + len(pat) <= end:
        if all(p is None or image[at + i] == p for i, p in enumerate(pat)):
            hits.append(at)
        at = image.find(bytes([first]), at + 1, end)
    return hits


def check(path):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    pat = parse(PATTERN)
    hits = []
    for s in pe.sections:
        if s.Characteristics & 0x20000000:
            lo = s.VirtualAddress
            hits += matches(image, lo, lo + s.Misc_VirtualSize, pat)
    if len(hits) != 1:
        print(f"  FAIL pattern matched {len(hits)} times, expected once")
        return False
    site = hits[0]
    fcomp = site + OPERAND_AT - 2
    operand = struct.unpack_from("<I", image, site + OPERAND_AT)[0]
    constant = operand - base
    if not 0 <= constant < len(image) - 4:
        print(f"  FAIL operand {operand:#x} is outside the image")
        return False
    value = struct.unpack_from("<f", image, constant)[0]
    if value != 32.0:
        print(f"  FAIL the operand reads {value}, not 32.0")
        return False
    jnp = site + len(pat) - 2
    rel = struct.unpack_from("<i", image, jnp + 2)[0]
    if rel <= 0:
        print(f"  FAIL the jnp at +{jnp:#x} jumps backward ({rel})")
        return False
    shared = image.count(struct.pack("<I", operand)) - 1
    known = "as #1018 found" if (fcomp, constant) == (KNOWN_FCOMP, KNOWN_CONSTANT) else "a different build"
    print(f"  ok   fcomp at +{fcomp:#x}, 32.0 at +{constant:#x}, skip to +{jnp + 6 + rel:#x} "
          f"({known}); {shared} other reads share the constant")
    return True


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    ok = True
    for path in dlls:
        print(path)
        ok &= check(path)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
