#!/usr/bin/env python3
"""Checks `msglog.rs`'s `MESSAGES` table against a real DoD `client.dll`.

Unlike `deathmsg.rs`/`scoreboard.rs`/etc., `msglog.rs` never patches a byte --
it only hooks and forwards, by name, through addresses it reads out of a
71-entry table that was typed in by hand from a `survey_client_dll.py
messages` run. A transcription slip here would not fail to compile: it would
call some *other* function as if it were the original handler for a message
whose name looks unrelated, and every message that slip touches would either
crash or silently misbehave the first time this DLL is actually loaded. So the
table is diffed against a fresh derivation of the same 71 entries, rather than
trusted.

Usage:
    python goldsrc-hooks/tools/verify_msglog_table.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone` (`pip install pefile capstone`); both are analysis-only and are not
build dependencies of anything in the workspace.
"""

import re
import sys
from pathlib import Path

try:
    import pefile  # noqa: F401  -- surfaced through survey_client_dll's own import
    import capstone  # noqa: F401
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

sys.path.insert(0, str(Path(__file__).resolve().parent))
from survey_client_dll import DEFAULT_DLL, Image, Owners, hooked_messages  # noqa: E402

RUST = Path(__file__).resolve().parent.parent / "src" / "msglog.rs"


def rust_table(src: str) -> dict[str, int]:
    match = re.search(r"static MESSAGES: &\[\(&str, usize\)\] = &\[(.*?)\];", src, re.S)
    if not match:
        raise SystemExit("could not find `static MESSAGES` in msglog.rs")
    entries = re.findall(r'\("([^"]+)",\s*(0x[0-9a-fA-F_]+)\)', match.group(1))
    return {name: int(rva.replace("_", ""), 16) for name, rva in entries}


def main(argv: list[str]) -> int:
    dll = Path(argv[0]) if argv else DEFAULT_DLL
    if not dll.is_file():
        print(f"no client.dll at {dll}")
        return 2

    img = Image(dll)
    owners = Owners(img)
    # A name of "?" means the string couldn't be recovered; a None rva means
    # the handler thunk pointer couldn't be recovered. Either way there is
    # nothing to compare this entry against, and leaving a None in `live`
    # crashes the `:#x` formatting below the moment one becomes mismatched.
    live = {
        name: rva
        for name, rva, _owner in hooked_messages(img, owners)
        if name != "?" and rva is not None
    }
    rs = rust_table(RUST.read_text())

    missing = sorted(set(live) - set(rs))
    extra = sorted(set(rs) - set(live))
    mismatched = sorted(n for n in (set(live) & set(rs)) if live[n] != rs[n])

    ok = True
    print(f"live scan: {len(live)} messages   MESSAGES table: {len(rs)} entries")
    if missing:
        ok = False
        print(f"FAIL  in the binary but missing from MESSAGES: {missing}")
    if extra:
        ok = False
        print(f"FAIL  in MESSAGES but not found in the binary: {extra}")
    if mismatched:
        ok = False
        for name in mismatched:
            print(f"FAIL  {name}: binary has {live[name]:#x}, MESSAGES has {rs[name]:#x}")
    if ok:
        print("OK    MESSAGES matches the binary exactly, name for name and RVA for RVA")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
