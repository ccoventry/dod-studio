#!/usr/bin/env python3
"""Checks `game_dir_check.rs`'s one-byte patch against real `hw.dll` files.

`game_dir_check.rs` turns the `je` that `CL_ParseServerInfo`'s game-folder
check takes when the server's folder matches the running game's into a `jmp`,
so demos recorded in `dod` play in a copy such as `dod_movies` (#402). The
unit tests prove the patterns' offsets line up with their own tokens; only the
binaries can prove:

  1. Each build's pattern matches exactly once in its own `hw.dll`, and not at
     all in the other build's.
  2. The pushed message is "Server is running game %s.  Restart in that game
     to connect.\\n", and the `push` the empty-folder `je` lands on is
     "Server didn't specify a gamedir, assuming no change\\n".
  3. The match `je` lands exactly past that print (push, call, add esp,4),
     where both the empty and the matching path go on parsing.
  4. The call before the compare passes `com_gamedir`'s buffer: the same
     address the `gamedir` command prints with "gamedir is %s".
  5. The match is in `CL_ParseServerInfo`'s path: on the pre-Anniversary
     build the function is called from the code that then reads the next
     string ("Remote host:  %s"); on the 25th Anniversary build that read is
     inline right after the check.

Patterns and offsets are read out of `game_dir_check.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_game_dir_check_offsets.py [--anniversary | hw.dll ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "game_dir_check.rs"

MESSAGE = b"Server is running game %s.  Restart in that game to connect.\n\x00"
NO_GAMEDIR = b"Server didn't specify a gamedir, assuming no change\n\x00"
GAMEDIR_IS = b"gamedir is %s\n\x00"
REMOTE_HOST = b"Remote host:  %s\n\x00"
NO_GAMEDIR_PRINT_LEN = 13


def sites():
    text = RUST.read_text(encoding="utf-8")
    out = []
    for m in re.finditer(
        r'build: "([^"]+)".*?pattern: "([^"]+)".*?empty_je_at: (\d+).*?'
        r"match_je_at: (\d+).*?message_at: (\d+)",
        text,
        re.S,
    ):
        out.append((m.group(1), m.group(2), int(m.group(3)), int(m.group(4)), int(m.group(5))))
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


def u32(image, at):
    return struct.unpack_from("<I", image, at)[0]


def je_target(image, at):
    if image[at] != 0x74:
        return None
    return at + 2 + struct.unpack_from("<b", image, at + 1)[0]


def pushes(image, base, va):
    """Every `push imm32` of `va`, as RVAs."""
    needle = b"\x68" + struct.pack("<I", va)
    return [m.start() for m in re.finditer(re.escape(needle), image)]


def calls_to(image, target):
    out = []
    for m in re.finditer(b"\xe8", image):
        at = m.start()
        if at + 5 <= len(image) and at + 5 + struct.unpack_from("<i", image, at + 1)[0] == target:
            out.append(at)
    return out


def check(path, all_sites):
    pe = pefile.PE(str(path), fast_load=True)
    image = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase
    message = string_va(image, base, MESSAGE)
    no_gamedir = string_va(image, base, NO_GAMEDIR)
    gamedir_is = string_va(image, base, GAMEDIR_IS)
    remote_host = string_va(image, base, REMOTE_HOST)
    if None in (message, no_gamedir, gamedir_is, remote_host):
        return [f"{path}: one of the four strings is missing"]

    # com_gamedir: what the `gamedir` command pushes just before "gamedir is %s".
    com_gamedir = None
    for at in pushes(image, base, gamedir_is):
        if image[at - 5] == 0x68:
            com_gamedir = u32(image, at - 4)
    if com_gamedir is None:
        return [f"{path}: could not find com_gamedir from the gamedir command"]

    problems, matched = [], []
    for build, pattern, empty_at, match_at, message_at in all_sites:
        hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
        if not hits:
            continue
        if len(hits) > 1:
            problems.append(f"{build}: {len(hits)} matches, expected 1")
            continue
        at = hits[0]
        if u32(image, at + message_at) != message:
            problems.append(f"{build}: pushes {u32(image, at + message_at):#x}, not the mismatch message {message:#x}")
        empty = je_target(image, at + empty_at)
        same = je_target(image, at + match_at)
        if empty is None or same is None:
            problems.append(f"{build}: a jump offset is not a je rel8")
            continue
        if image[empty] != 0x68 or u32(image, empty + 1) != no_gamedir:
            problems.append(f"{build}: the empty-folder je lands at +{empty:#x}, not on the \"didn't specify\" print")
        if same != empty + NO_GAMEDIR_PRINT_LEN:
            problems.append(f"{build}: the match je lands at +{same:#x}, not +{empty + NO_GAMEDIR_PRINT_LEN:#x}")
        # COM_FileBase(com_gamedir, buf): the `push imm32` before the first call.
        gamedir_push = image.find(b"\x50\x68", at, at + message_at)
        if gamedir_push < 0 or u32(image, gamedir_push + 2) != com_gamedir:
            problems.append(f"{build}: the compare does not read com_gamedir {com_gamedir:#x}")

        # Which function the match is in, and that CL_ParseServerInfo's next
        # read ("Remote host") follows the check on the path that goes on.
        if image[at] == 0x55:  # a function entry: the pre-Anniversary layout
            callers = calls_to(image, at)
            parse = [c for c in callers if image.find(b"\x68" + struct.pack("<I", remote_host), c, c + 0x40) >= 0]
            where = f"CL_CheckGameDirectory +{at:#x}, called from " + ", ".join(f"+{c:#x}" for c in callers)
            if len(parse) != 1:
                problems.append(f"{build}: expected one caller that reads \"Remote host\" next, found {len(parse)}")
            else:
                where += f" (CL_ParseServerInfo's is +{parse[0]:#x})"
        else:
            where = f"inline in CL_ParseServerInfo at +{at:#x}"
            if image.find(b"\x68" + struct.pack("<I", remote_host), same, same + 0x10) < 0:
                problems.append(f"{build}: \"Remote host\" is not read right after the check")
        matched.append(
            f"{build}: {where}; patch +{at + match_at:#x} 74 {image[at + match_at + 1]:02x} -> eb "
            f"(lands +{same:#x}, no-folder print +{empty:#x}); com_gamedir {com_gamedir:#x}"
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
