#!/usr/bin/env python3
"""Checks `pov_live_bob.rs`'s call-site patterns against real `hw.dll` files.

`pov_live_bob.rs` redirects `V_RenderView`'s call to the demo view override.
The unit test proves each pattern's offset lands on its `E8`; only the
binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The call follows a call to `ClientDLL_CalcRefdef` (the wrapper that calls
     through the pointer `hw.dll` fills from client.dll's "V_CalcRefdef"
     export) by less than 0x80 bytes -- i.e. it is the override that runs
     right after the live view is worked out.
  3. Both calls get the same `ref_params_s` (the same `lea reg, [ebp-x]`).

Patterns and offsets are read out of `pov_live_bob.rs` rather than restated.

It also checks the numbers `pov_live_bob.rs` copies out of `client.dll`'s
`V_CalcNormalRefdef` (#889), in each install's `dod/cl_dlls/client.dll`:
`V_CalcBob`'s -7..+4 clamp, the gun moved by `0.4 * bob` along `forward`, its
`0.3` / `0.5` / `1` pitch / yaw / roll tilt, and no vertical bob on the gun
(the Half-Life SDK has one, DoD doesn't).

Usage:
    python goldsrc-hooks/tools/verify_pov_live_bob_offsets.py [hw.dll ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "pov_live_bob.rs"


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = [(m.group(1), m.group(2), int(m.group(3))) for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?call_at: (\d+)', text, re.S)]
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    return re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                               for t in pattern.split()), re.S)


def calc_refdef_pointer(image, base):
    """The global hw.dll stores client.dll's V_CalcRefdef in, as 4 bytes."""
    name = image.find(b"V_CalcRefdef\0")
    if name < 0:
        return None
    at = image.find(b"\x68" + struct.pack("<I", base + name))
    if at < 0:
        return None
    # `call esi`, then `mov [pointer], eax` within a few bytes.
    store = re.compile(rb"\xff\xd6.{0,4}?\xa3(.{4})", re.S).search(image, at, at + 40)
    return store.group(1) if store else None


def wrappers(image, pointer):
    """Starts of the small functions that call or jump through `pointer`."""
    out = []
    for m in re.finditer(re.escape(b"\xa1" + pointer), image):
        tail = image[m.start():m.start() + 24]
        if b"\xff\xe0" in tail or b"\xff\xd0" in tail:
            # The function begins with `push ebp; mov ebp, esp` just before.
            start = image.rfind(b"\x55\x8b\xec", m.start() - 8, m.start())
            if start >= 0:
                out.append(start)
    return out


def calls_to(image, targets, lo, hi):
    out = []
    for m in re.finditer(b"\xe8", image[lo:hi]):
        at = lo + m.start()
        if at + 5 + struct.unpack("<i", image[at + 1:at + 5])[0] in targets:
            out.append(at)
    return out


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    pointer = calc_refdef_pointer(image, base)
    targets = set(wrappers(image, pointer)) if pointer else set()
    ok = True
    matched = []
    for build, pattern, call_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            print(f"  FAIL {build}: {len(hits)} matches, expected one")
            ok = False
            continue
        call = hits[0] + call_at
        before = calls_to(image, targets, call - 0x80, call)
        if not before:
            print(f"  FAIL {build}: no call to ClientDLL_CalcRefdef in the 0x80 bytes before +{call:#x}")
            ok = False
            continue
        # `lea reg, [ebp+disp32]`: 8D /r disp32, the displacement compared.
        lea_here = image[hits[0] + 7:hits[0] + 11]
        lea_before = image[before[-1] - 5:before[-1] - 1]
        if lea_here != lea_before:
            print(f"  FAIL {build}: the override's ref_params ({lea_here.hex()}) isn't "
                  f"CalcRefdef's ({lea_before.hex()})")
            ok = False
            continue
        target = call + 5 + struct.unpack("<i", image[call + 1:call + 5])[0]
        print(f"  ok   {build}: call at +{call:#x} to the override +{target:#x}, "
              f"{call - before[-1]:#x} bytes after ClientDLL_CalcRefdef")
        matched.append(build)
    if len(matched) != 1:
        print(f"  FAIL: {len(matched)} builds matched, expected exactly one")
        ok = False
    return ok


# client.dll (the same file in both installs): (rva, expected bytes, what).
# A `None` byte is a wildcard; an `(op, kind)` pair after the bytes names the
# constant the instruction's absolute address operand points at.
CLIENT_CODE = [
    (0x4ed10, "57 8b 7c 24 08 83 7f 50 ff", "V_CalcBob: onground == -1 returns the last bob"),
    (0x4ee05, "d8 1d", "V_CalcBob: bob < -7 is clamped", ("f32", -7.0)),
    (0x4ede0, "d8 1d", "V_CalcBob: bob > 4 is clamped", ("f32", 4.0)),
    (0x4f49c, "e8", "V_CalcNormalRefdef calls V_CalcBob", ("call", 0x4ed10)),
    # for i: view->origin[i] += forward[i] * bob * 0.4
    (0x4f850, "d9 01 d8 4c 24 10", "gun origin += forward * bob ..."),
    (0x4f85d, "dc 0d", "... * 0.4", ("f64", 0.4)),
    # view->angles[YAW] -= bob * 0.5; [ROLL] -= bob; [PITCH] -= bob * 0.3
    (0x4f86b, "d9 87 58 0b 00 00 d9 44 24 10 dc 0d", "gun yaw -= bob * ...", ("f64", 0.5)),
    (0x4f883, "d9 87 5c 0b 00 00 d8 64 24 10 d9 9f 5c 0b 00 00", "gun roll -= bob"),
    (0x4f893, "d9 03 d9 44 24 10 dc 0d", "gun pitch -= bob * ...", ("f64", 0.3)),
    (0x4f8a1, "d9 1b 8b 4e 54", "then waterlevel: nothing else adds bob to the gun"),
]


def check_client(path):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    ok = True
    for entry in CLIENT_CODE:
        rva, expected, what = entry[:3]
        want = bytes.fromhex(expected)
        got = image[rva:rva + len(want)]
        good = got == want
        if good and len(entry) > 3:
            kind, value = entry[3]
            at = rva + len(want)
            if kind == "call":
                good = at + 4 + struct.unpack("<i", image[at:at + 4])[0] == value
            else:
                address = struct.unpack("<I", image[at:at + 4])[0] - base
                fmt, size = ("<f", 4) if kind == "f32" else ("<d", 8)
                good = abs(struct.unpack(fmt, image[address:address + size])[0] - value) < 1e-9
        print(f"  {'ok  ' if good else 'FAIL'} client.dll +{rva:#x}: {what}")
        ok &= good
    # The vertical gun bob the Half-Life SDK has (`view->origin[2] += bob`)
    # would store to view->origin[2] ([edi+0xb50]) inside the bob block.
    if b"\x9f\x50\x0b\x00\x00" in image[0x4f84b:0x4f8a3]:
        print("  FAIL client.dll: the gun's bob block writes origin[2]")
        ok = False
    return ok


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    all_sites = sites()
    ok = True
    for path in dlls:
        print(path)
        ok &= check(path, all_sites)
        client = path.parent / "dod" / "cl_dlls" / "client.dll"
        if client.is_file():
            ok &= check_client(client)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
