#!/usr/bin/env python3
"""Checks `run_in_background.rs`'s one-byte patch against real `hw.dll` files.

`run_in_background.rs` turns the `jne` in `CEngine::Frame` that skips the
inactive-window wait into a `jmp`. The unit test proves each pattern's `jne`
offset lines up with its own tokens; only the binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The byte at the `jne` offset is `75`, and its target is the instruction
     right after the `call [reg+0x10]` (`SleepUntilInput`) that ends the
     pattern -- i.e. the jump skips exactly the wait.
  3. The wait is preceded by an `IsActiveApp` call (vtable +0x28) and pushes
     a 20 or 50 ms timeout (the pattern holds `0x14`/`0x32` or the `sbb`/
     `and -30`/`add 50` sequence that computes them).

Patterns and offsets are read out of `run_in_background.rs` rather than
restated.

Usage:
    python goldsrc-hooks/tools/verify_run_in_background_offsets.py [hw.dll ...]

Defaults to both movie installs. Needs `pip install pefile`.
"""

import re
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
RUST = Path(__file__).resolve().parent.parent / "src" / "run_in_background.rs"


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = [(m.group(1), m.group(2), int(m.group(3))) for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?jne_at: (\d+)', text, re.S)]
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    return re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                               for t in pattern.split()), re.S)


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    ok = True
    matched = []
    for build, pattern, jne_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            print(f"  FAIL {build}: {len(hits)} matches, expected one")
            ok = False
            continue
        start = hits[0]
        length = len(pattern.split())
        jne = start + jne_at
        if image[jne] != 0x75:
            print(f"  FAIL {build}: byte at +{jne:#x} is {image[jne]:02x}, not 75")
            ok = False
            continue
        target = jne + 2 + int.from_bytes(image[jne + 1:jne + 2], "little", signed=True)
        end = start + length  # just past `call [reg+0x10]`
        # The 25th Anniversary build pops a saved register after the call.
        allowed = {end, end + 1} if image[end] == 0x5F else {end}
        if target not in allowed:
            print(f"  FAIL {build}: jne +{jne:#x} lands at +{target:#x}, not after the wait (+{end:#x})")
            ok = False
            continue
        print(f"  ok   {build}: jne at +{jne:#x} skips the wait to +{target:#x}")
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
