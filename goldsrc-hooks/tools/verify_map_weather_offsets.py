#!/usr/bin/env python3
"""Checks the site `map_weather.rs` patches for `ds_map_weather` (#1129).

The weather updater reads `g_iWeatherType` once a frame with
`mov eax, [g_iWeatherType]; dec eax; je <rain>; dec eax; jne <none>`, right
after its `cl_particlefx >= 2` check. `map_weather.rs` finds that by a byte
pattern (both absolute operands wildcarded) and points the read at an int of
its own. This proves, on real `client.dll` files:

  1. The pattern matches exactly once in an executable section.
  2. The read's operand is an address in the image, and the only other code
     that names it is the BSP parse's store and two stores of 0
     (`CWeatherManager::Init` / `::VidInit`): it is `g_iWeatherType`, and
     the updater is its one reader.
  3. The `fcomp` before it compares against the float 2.0 (`cl_particlefx`).
  4. The sites are the ones #1129 found: the read at +0x4c951, the variable
     at +0x196c74.

client.dll is byte-identical in both movie installs; both are checked anyway.

Usage:
    python goldsrc-hooks/tools/verify_map_weather_offsets.py [client.dll ...]

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
# Must match `PATTERN` and `OPERAND_AT` in goldsrc-hooks/src/map_weather.rs.
PATTERN = ("D8 1D ?? ?? ?? ?? DF E0 25 00 01 00 00 75 31 "
           "A1 ?? ?? ?? ?? 48 74 14 48 75 26")
OPERAND_AT = 16
KNOWN_READ = 0x4c951
KNOWN_VARIABLE = 0x196c74


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


def code_refs(pe, image, address):
    """Offsets of every 4-byte copy of `address` in executable sections."""
    needle = struct.pack("<I", address)
    out = []
    for s in pe.sections:
        if s.Characteristics & 0x20000000:
            lo, hi = s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize
            at = image.find(needle, lo, hi)
            while at >= 0:
                out.append(at)
                at = image.find(needle, at + 1, hi)
    return out


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
    read = site + OPERAND_AT - 1
    operand = struct.unpack_from("<I", image, site + OPERAND_AT)[0]
    variable = operand - base
    if not 0 <= variable < len(image) - 4:
        print(f"  FAIL operand {operand:#x} is outside the image")
        return False
    two = struct.unpack_from("<f", image, struct.unpack_from("<I", image, site + 2)[0] - base)[0]
    if two != 2.0:
        print(f"  FAIL the fcomp before the read compares against {two}, not 2.0")
        return False
    refs = code_refs(pe, image, operand)
    others = [r for r in refs if r != site + OPERAND_AT]
    zero_stores = [r for r in others
                   if image[r - 2:r] == b"\xc7\x05" and image[r + 4:r + 8] == b"\0\0\0\0"]
    parse_stores = [r for r in others if image[r - 1] == 0xA3]
    if len(others) != 3 or len(zero_stores) != 2 or len(parse_stores) != 1:
        print(f"  FAIL the variable has {len(others)} other refs "
              f"({len(zero_stores)} stores of 0, {len(parse_stores)} stores of eax), expected 2 + 1")
        return False
    known = "as #1129 found" if (read, variable) == (KNOWN_READ, KNOWN_VARIABLE) else "a different build"
    print(f"  ok   read at +{read:#x}, g_iWeatherType at +{variable:#x} ({known}); "
          f"BSP-parse store at +{parse_stores[0] - 1:#x}, zeroed at "
          + ", ".join(f"+{r - 2:#x}" for r in zero_stores))
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
