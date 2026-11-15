#!/usr/bin/env python3
"""Checks `demo_console_keys.rs`'s one-byte patch against real `hw.dll` files.

`demo_console_keys.rs` turns the `jne` that `Key_Event` takes when
`cls.spectator` is set into a `jmp`, so in a POV demo a key no longer opens the
console unless it is the console key (#894). The unit tests prove the
patterns' offsets line up with their own tokens; only the binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. All five jumps land on one place, past the call.
  3. The call reaches the function the engine registers as `toggleconsole`
     (`push handler; push "toggleconsole"; call Cmd_AddCommand`).
  4. The first test reads `cls.demoplayback`: the global `CL_PlayDemo_f`'s
     "Playing demo from %s." path sets to 1 just before.
  5. The second test reads `cls.spectator`: the global `CL_Parse_HLTV` sets to
     1 for HLTV_ACTIVE (the case with no "unknown HLTV command" print).
  6. The fourth test reads `consolekeys`, and `Key_Init` sets Alt's entry
     (key 132) in it, while leaving out the console key (` and ~).

Patterns and offsets are read out of `demo_console_keys.rs` rather than
restated.

Usage:
    python goldsrc-hooks/tools/verify_demo_console_keys_offsets.py [--anniversary | hw.dll ...]

Defaults to both movie installs; `--anniversary` checks only the 25th
Anniversary one. Needs `pip install pefile`.
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
PRE_DLL = STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll"
ANNIVERSARY_DLL = STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_console_keys.rs"

K_ALT = 132


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = []
    for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?jumps_at: \[([\d, ]+)\].*?'
        r"spectator: (\d+).*?call_at: (\d+)",
        text,
        re.S,
    ):
        jumps = [int(x) for x in m.group(3).split(",")]
        out.append((m.group(1), m.group(2), jumps, int(m.group(4)), int(m.group(5))))
    if len(out) != 2:
        sys.exit(f"expected 2 sites in {RUST}, found {len(out)}")
    return out


def compile_pattern(pattern):
    parts = []
    for tok in pattern.split():
        parts.append(b"." if tok == "??" else re.escape(bytes([int(tok, 16)])))
    return re.compile(b"".join(parts), re.S)


def string_va(image, base, text):
    at = image.find(text)
    while at > 0 and image[at - 1] != 0:
        at = image.find(text, at + 1)
    return None if at <= 0 else base + at


def string_vas(image, base, text):
    """Every copy of a NUL-led string, as VAs."""
    out, at = [], image.find(text)
    while at > 0:
        if image[at - 1] == 0:
            out.append(base + at)
        at = image.find(text, at + 1)
    return out


def u32(image, at):
    return struct.unpack_from("<I", image, at)[0]


def short_target(image, at):
    return at + 2 + struct.unpack_from("<b", image, at + 1)[0]


def operand_address(image, at):
    """The absolute address a `39 3D` / `83 3D` / `39 3C 85` / `83 3C 85`
    compare at `at` reads."""
    if image[at + 1] == 0x3D:
        return u32(image, at + 2)
    return u32(image, at + 3)


def writes_of(image, va):
    """RVAs of every store to `va`: `mov [va], reg` (89 /r with disp32),
    `mov [va], eax` (A3) and `mov [va], imm32` (C7 05)."""
    out = []
    for m in re.finditer(re.escape(struct.pack("<I", va)), image):
        at = m.start()
        if image[at - 1] == 0xA3 or (image[at - 2] == 0x89 and image[at - 1] & 0xC7 == 0x05) or image[at - 2:at] == b"\xc7\x05":
            out.append(at)
    return out


def pushes(image, va):
    return [m.start() for m in re.finditer(re.escape(b"\x68" + struct.pack("<I", va)), image)]


def stores_of_one(image, va):
    """RVAs of every `mov dword [va], 1` (C7 05 va 01 00 00 00)."""
    needle = b"\xc7\x05" + struct.pack("<I", va) + b"\x01\x00\x00\x00"
    return [m.start() for m in re.finditer(re.escape(needle), image)]


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase

    toggle_name = string_va(image, base, b"toggleconsole\x00")
    playing = string_vas(image, base, b"Playing demo from %s.\n\x00")
    unknown_hltv = string_va(image, base, b" CL_Parse_HLTV: unknown HLTV command.\n\x00")
    if None in (toggle_name, unknown_hltv) or not playing:
        return [f"{path}: a reference string is missing"]
    # Cmd_AddCommand("toggleconsole", handler): `68 handler 68 name`.
    handlers = set()
    for m in re.finditer(re.escape(b"\x68" + struct.pack("<I", toggle_name)), image):
        if image[m.start() - 5] == 0x68:
            handlers.add(u32(image, m.start() - 4))
    if len(handlers) != 1:
        return [f"{path}: expected one toggleconsole registration, found {len(handlers)}"]
    handler = handlers.pop()

    problems, matched = [], []
    for build, pattern, jumps, spectator, call_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            problems.append(f"{build}: {len(hits)} matches, expected 1")
            continue
        at = hits[0]
        targets = {short_target(image, at + j) for j in jumps}
        landing = min(targets)
        if len(targets) != 1:
            problems.append(f"{build}: the jumps land in {len(targets)} places")
        if landing < at + call_at + 5:
            problems.append(f"{build}: the jumps land before the end of the call")
        call = at + call_at
        called = base + call + 5 + struct.unpack_from("<i", image, call + 1)[0]
        if called != handler:
            problems.append(f"{build}: calls {called:#x}, not toggleconsole's {handler:#x}")

        demoplayback = operand_address(image, at)
        spectator_va = operand_address(image, at + jumps[spectator - 1] + 2)
        consolekeys = operand_address(image, at + jumps[2] + 2)

        # cls.demoplayback is written by the demo-start code that prints
        # "Playing demo from %s." (playdemo and viewdemo each have one).
        near = [
            p for p in sum((pushes(image, va) for va in playing), [])
            if any(abs(s - p) < 0x400 for s in writes_of(image, demoplayback))
        ]
        if not near:
            problems.append(f"{build}: {demoplayback:#x} is not written near \"Playing demo from\"")

        # cls.spectator = 1 in CL_Parse_HLTV's HLTV_ACTIVE case, which sits
        # within a few dozen bytes after the "unknown HLTV command" print.
        push_unknown = image.find(b"\x68" + struct.pack("<I", unknown_hltv))
        if not any(0 < s - push_unknown < 0x100 for s in stores_of_one(image, spectator_va)):
            problems.append(f"{build}: {spectator_va:#x} is not CL_Parse_HLTV's HLTV_ACTIVE flag")

        # Key_Init: consolekeys[K_ALT] = 1 as `mov [consolekeys + 132*4], reg`.
        alt = struct.pack("<I", consolekeys + 4 * K_ALT)
        alt_set = [m.start() for m in re.finditer(re.escape(alt), image) if image[m.start() - 2] in (0x89, 0xC7) or image[m.start() - 1] == 0xA3]
        if not alt_set:
            problems.append(f"{build}: nothing writes consolekeys[K_ALT] at {consolekeys + 4 * K_ALT:#x}")

        patched = at + jumps[spectator]
        matched.append(
            f"{build}: Key_Event check +{at:#x}; patch +{patched:#x} 75 {image[patched + 1]:02x} -> eb "
            f"(all land +{landing:#x}); Con_ToggleConsole_f +{handler - base:#x}; "
            f"demoplayback {demoplayback:#x}, spectator {spectator_va:#x}, consolekeys {consolekeys:#x}"
        )
    if len(matched) != 1:
        problems.append(f"expected exactly one build to match, got {len(matched)}")
    return problems or [f"{path.parent.name}: OK -- {matched[0]}"]


def main():
    args = sys.argv[1:]
    if args == ["--anniversary"]:
        dlls = [ANNIVERSARY_DLL]
    else:
        dlls = [Path(p) for p in args] or [PRE_DLL, ANNIVERSARY_DLL]
    all_sites = sites()
    failed = False
    for dll in dlls:
        for line in check(dll, all_sites):
            print(line)
            failed |= "OK" not in line
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
