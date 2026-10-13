#!/usr/bin/env python3
"""Checks how `screen_effects.rs` finds hw.dll's ScreenFade/ScreenShake handlers.

`screen_effects.rs` has no per-build table: it finds the message name string
and reads the handler from the registration in `ClientDLL_Init`
(`push handler; push "<name>"; call HookUserMsg`). This proves, on real
`hw.dll` files:

  1. Each name appears as its own NUL-delimited string.
  2. Exactly one `push imm32; push <string>; call` names it.
  3. The handler it pushes lies in an executable section, and both messages
     are registered through the same `call` target (HookUserMsg).
  4. The handlers are the ones #606 found by hand: pre-Anniversary
     `V_ScreenFade` +0xc2580 / `V_ScreenShake` +0xc24e0, 25th Anniversary
     +0x232160 / +0x2322c0 (other builds just print what they find).

Usage:
    python goldsrc-hooks/tools/verify_screen_effects_offsets.py [hw.dll ...]

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
KNOWN = {
    (0xc2580, 0xc24e0): "pre-Anniversary",
    (0x232160, 0x2322c0): "25th Anniversary",
}


def check(path):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    code = [(s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize)
            for s in pe.sections if s.Characteristics & 0x20000000]
    found, calls = [], set()
    for message in (b"ScreenFade", b"ScreenShake"):
        at = image.find(b"\0" + message + b"\0")
        if at < 0:
            print(f"  FAIL no \"{message.decode()}\" string")
            return False
        string = base + at + 1
        pattern = re.compile(rb"\x68(.{4})\x68" + re.escape(struct.pack("<I", string)) + rb"\xe8(.{4})", re.S)
        hits = list(pattern.finditer(image))
        if len(hits) != 1:
            print(f"  FAIL {message.decode()}: {len(hits)} registrations, expected one")
            return False
        handler = struct.unpack("<I", hits[0].group(1))[0] - base
        if not any(lo <= handler < hi for lo, hi in code):
            print(f"  FAIL {message.decode()}: handler +{handler:#x} isn't code")
            return False
        call_end = hits[0].end()
        calls.add(call_end + struct.unpack("<i", hits[0].group(2))[0])
        found.append(handler)
    if len(calls) != 1:
        print("  FAIL the two registrations call different functions")
        return False
    build = KNOWN.get(tuple(found), "unknown build")
    print(f"  ok   {build}: ScreenFade +{found[0]:#x}, ScreenShake +{found[1]:#x}, "
          f"HookUserMsg +{calls.pop():#x}")
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
