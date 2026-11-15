#!/usr/bin/env python3
"""Checks the sites `map_wind.rs` patches for `ds_map_wind`.

DoD's particles add `CDoDParticle::m_vGlobalWind`'s x and y to their velocity
as they are made, with `fld [wind.x]; fadd [p+0x80]; fstp [p+0x80]` and the
same for y at +0x84: once in `AddGlobalWind` (through ecx) and twice inlined
into the rain and snow particle makers (through esi). `map_wind.rs` finds
those by byte patterns, operands wildcarded, and points the reads at a
vector of its own. This proves, on real `client.dll` files:

  1. `AddGlobalWind`'s pattern matches once and the inlined one twice.
  2. All six operands name one vector inside the image, x then x + 4.
  3. Nothing else in the code reads it: the only other references are the
     three stores of `SetGlobalWind` (x, y and z), and nothing names z but
     that store.
  4. The sites are the ones found offline: the reads at +0x4d360, +0x4e1cc,
     +0x4e37c, the variable at +0x196c60.

client.dll is byte-identical in both movie installs; both are checked anyway.

Usage:
    python goldsrc-hooks/tools/verify_map_wind_offsets.py [client.dll ...]

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
# Must match `ADD_PATTERN`, `INLINED_PATTERN`, `X_AT` and `Y_AT` in
# goldsrc-hooks/src/map_wind.rs.
ADD_PATTERN = ("D9 05 ?? ?? ?? ?? D8 81 80 00 00 00 D9 99 80 00 00 00 "
               "D9 05 ?? ?? ?? ?? D8 81 84 00 00 00")
INLINED_PATTERN = ("D9 05 ?? ?? ?? ?? D8 86 80 00 00 00 D9 9E 80 00 00 00 "
                   "D9 05 ?? ?? ?? ?? D8 86 84 00 00 00")
X_AT, Y_AT = 2, 20
KNOWN_SITES = [0x4d360, 0x4e1cc, 0x4e37c]
KNOWN_VARIABLE = 0x196c60


def parse(pattern):
    return [None if t == "??" else int(t, 16) for t in pattern.split()]


def code_sections(pe):
    for s in pe.sections:
        if s.Characteristics & 0x20000000:
            yield s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize


def matches(pe, image, pattern):
    pat = parse(pattern)
    hits = []
    for lo, hi in code_sections(pe):
        at = image.find(bytes([pat[0]]), lo, hi)
        while at >= 0 and at + len(pat) <= hi:
            if all(p is None or image[at + i] == p for i, p in enumerate(pat)):
                hits.append(at)
            at = image.find(bytes([pat[0]]), at + 1, hi)
    return hits


def code_refs(pe, image, address):
    needle = struct.pack("<I", address)
    out = []
    for lo, hi in code_sections(pe):
        at = image.find(needle, lo, hi)
        while at >= 0:
            out.append(at)
            at = image.find(needle, at + 1, hi)
    return out


def check(path):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    add = matches(pe, image, ADD_PATTERN)
    inlined = matches(pe, image, INLINED_PATTERN)
    if len(add) != 1 or len(inlined) != 2:
        print(f"  FAIL AddGlobalWind matched {len(add)} times (want 1), "
              f"the inlined read {len(inlined)} times (want 2)")
        return False
    sites = add + inlined
    pairs = {(struct.unpack_from("<I", image, s + X_AT)[0],
              struct.unpack_from("<I", image, s + Y_AT)[0]) for s in sites}
    if len(pairs) != 1:
        print(f"  FAIL the sites name different variables: {pairs}")
        return False
    x, y = pairs.pop()
    variable = x - base
    if y != x + 4 or not 0 <= variable < len(image) - 12:
        print(f"  FAIL operands {x:#x}/{y:#x} are not one vector in the image")
        return False
    reads = {s + X_AT for s in sites} | {s + Y_AT for s in sites}
    others = [r for r in code_refs(pe, image, x) + code_refs(pe, image, y) if r not in reads]
    stores = [r for r in others if image[r - 2:r] in (b"\x89\x0d", b"\x89\x15", b"\x89\x05") or image[r - 1] == 0xA3]
    z_refs = code_refs(pe, image, x + 8)
    if len(others) != 2 or len(stores) != 2 or len(z_refs) != 1:
        print(f"  FAIL {len(others)} other refs to x/y ({len(stores)} stores), "
              f"{len(z_refs)} to z; expected 2 stores and one z store")
        return False
    known = ("as found offline" if sorted(sites) == KNOWN_SITES and variable == KNOWN_VARIABLE
             else "a different build")
    print(f"  ok   reads at " + ", ".join(f"+{s:#x}" for s in sorted(sites))
          + f"; m_vGlobalWind at +{variable:#x} ({known}); SetGlobalWind's stores at "
          + ", ".join(f"+{r:#x}" for r in sorted(stores + z_refs)))
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
