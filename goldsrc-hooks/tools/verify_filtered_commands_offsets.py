#!/usr/bin/env python3
"""Checks `filtered_commands.rs`'s `Cbuf_Execute` patterns against real `hw.dll` files.

`filtered_commands.rs` empties `filteredcmd_text` by finding `Cbuf_Execute`
and reading the buffer address it passes. The unit test proves each pattern's
offset lines up with its own tokens; only the binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The address it reads is the buffer `Cbuf_Init` names "filteredcmd_text"
     (`push 0x4000; push <buffer>; push "filteredcmd_text"`), and the other
     buffer in the pattern is the one it names "cmd_text".
  3. The filtered buffer's executor is called with a line limit of 1 (`push 1;
     push 0; push <buffer>`), the reason a long seek's commands linger.

Patterns and offsets are read out of `filtered_commands.rs` rather than
restated.

Usage:
    python goldsrc-hooks/tools/verify_filtered_commands_offsets.py [hw.dll ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "filtered_commands.rs"


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = [(m.group(1), m.group(2), int(m.group(3))) for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?buffer_at: (\d+)', text, re.S)]
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    return re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                               for t in pattern.split()), re.S)


def named_buffer(image, base, name):
    """The buffer Cbuf_Init allocates under `name`, or None."""
    # Every place the name ends, "cmd_text" inside "filteredcmd_text" too:
    # only the one Cbuf_Init pushes can match below.
    for m in re.finditer(re.escape(name + b"\0"), image):
        string = base + m.start()
        found = re.search(b"\x68\x00\x40\x00\x00\x68(.{4})\x68" + re.escape(struct.pack("<I", string)),
                          image, re.S)
        if found:
            return struct.unpack("<I", found.group(1))[0]
    return None


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    filtered = named_buffer(image, base, b"filteredcmd_text")
    plain = named_buffer(image, base, b"cmd_text")
    ok = True
    matched = []
    for build, pattern, buffer_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            print(f"  FAIL {build}: {len(hits)} matches, expected one")
            ok = False
            continue
        start = hits[0]
        read = struct.unpack("<I", image[start + buffer_at:start + buffer_at + 4])[0]
        first = struct.unpack("<I", image[start + buffer_at - 14:start + buffer_at - 10])[0]
        if read != filtered:
            print(f"  FAIL {build}: reads {read:#x}, but Cbuf_Init's filteredcmd_text is "
                  f"{filtered if filtered is None else hex(filtered)}")
            ok = False
            continue
        if first != plain:
            print(f"  FAIL {build}: the first buffer is {first:#x}, not cmd_text "
                  f"({plain if plain is None else hex(plain)})")
            ok = False
            continue
        if image[start + buffer_at - 5:start + buffer_at - 1] != b"\x6a\x01\x6a\x00":
            print(f"  FAIL {build}: the filtered executor's line limit isn't `push 1`")
            ok = False
            continue
        print(f"  ok   {build}: Cbuf_Execute at +{start:#x} runs filteredcmd_text "
              f"(+{read - base:#x}) one line a frame")
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
