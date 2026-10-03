#!/usr/bin/env python3
"""Checks `pmove_guard.rs`'s two anchors against real `hw.dll` files.

`pmove_guard.rs` points hw.dll's `pmove` global at `g_clmove` before the first
demo loads (issue #546). The unit tests prove the patterns' offsets line up
with their own tokens; only the binaries can prove:

  1. Each build's `EV_SetTraceHull` and prediction-setup patterns match exactly
     once in its own `hw.dll`, and not at all in the other build's.
  2. Both name the same `pmove`, and `EV_SetTraceHull` is really the event
     API's `EV_SetTraceHull`: its address sits in the engine's event-API
     table right after `EV_SetUpPlayerPrediction` (offsets 0x38 and 0x28 of
     `event_api_s`, so 0x10 apart).
  3. `g_clmove` is the value the engine's own client code stores into `pmove`
     (every `mov [pmove], imm32` outside the server's is that value), and it
     lies inside the image.

Patterns and offsets are read out of `pmove_guard.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_pmove_guard.py [hw.dll ...]

Defaults to both movie installs. Needs `pip install pefile`.
"""

import re
import struct
import sys
from collections import Counter
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
RUST = Path(__file__).resolve().parent.parent / "src" / "pmove_guard.rs"


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = []
    for m in re.finditer(
        r'build: "([^"]+)".*?trace_hull: "([^"]+)".*?trace_hull_pmove_at: (\d+)'
        r'.*?setup: "([^"]+)".*?setup_store_at: (\d+)',
        text,
        re.S,
    ):
        out.append((m.group(1), m.group(2), int(m.group(3)), m.group(4), int(m.group(5))))
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    parts = []
    for tok in pattern.split():
        parts.append(b"." if tok == "??" else re.escape(bytes([int(tok, 16)])))
    return re.compile(b"".join(parts), re.S)


def u32(img, at):
    return struct.unpack_from("<I", img, at)[0]


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    img = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase
    matched = []
    for build, hull_pat, pmove_at, setup_pat, store_at in all_sites:
        hulls = [m.start() for m in compile_pattern(hull_pat).finditer(img)]
        setups = [m.start() for m in compile_pattern(setup_pat).finditer(img)]
        if not hulls and not setups:
            continue
        if len(hulls) != 1 or len(setups) != 1:
            return f"FAIL -- {build}: EV_SetTraceHull x{len(hulls)}, setup x{len(setups)} (need 1 each)"
        matched.append((build, hulls[0], pmove_at, setups[0], store_at))
    if len(matched) != 1:
        return f"FAIL -- {len(matched)} builds matched"
    build, hull, pmove_at, setup, store_at = matched[0]

    pmove = u32(img, hull + pmove_at)
    stored_at = u32(img, setup + store_at + 2)
    clmove = u32(img, setup + store_at + 6)
    if pmove != stored_at:
        return f"FAIL -- EV_SetTraceHull reads {pmove:#x}, setup stores to {stored_at:#x}"
    if not (base <= clmove < base + len(img)):
        return f"FAIL -- g_clmove {clmove:#x} outside the image"

    table = [m.start() for m in re.finditer(re.escape(struct.pack("<I", base + hull)), img)]
    if len(table) != 1:
        return f"FAIL -- EV_SetTraceHull's address appears {len(table)} times (want 1: the event API table)"
    setup_pred = u32(img, table[0] - 0x10) - base
    # EV_SetUpPlayerPrediction calls (or tail-jumps to) the setup function.
    body = img[setup_pred:setup_pred + 0x20]
    reaches = False
    for i, op in enumerate(body[:-4]):
        if op in (0xE8, 0xE9) and (setup_pred + i + 5 + struct.unpack_from("<i", body, i + 1)[0]) == setup:
            reaches = True
    if not reaches:
        return f"FAIL -- the entry before EV_SetTraceHull (+{setup_pred:#x}) doesn't reach the setup at +{setup:#x}"

    stores = Counter(
        u32(img, m.start() + 6)
        for m in re.finditer(re.escape(b"\xC7\x05" + struct.pack("<I", pmove)), img)
    )
    if stores.get(clmove, 0) < 2:
        return f"FAIL -- g_clmove {clmove:#x} stored {stores.get(clmove, 0)} time(s); all stores: {dict(stores)}"
    others = ", ".join(f"{v:#x} x{n}" for v, n in stores.items() if v != clmove)
    return (
        f"OK -- {build}: EV_SetTraceHull +{hull:#x}, setup +{setup:#x}; pmove {pmove:#x} -> "
        f"g_clmove {clmove:#x} (stored x{stores[clmove]}; server's: {others})"
    )


def main():
    dlls = [Path(a) for a in sys.argv[1:]] or DEFAULT_DLLS
    all_sites = sites()
    failed = False
    for dll in dlls:
        result = check(dll, all_sites)
        failed |= result.startswith("FAIL")
        print(f"{dll.parent.name}: {result}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
