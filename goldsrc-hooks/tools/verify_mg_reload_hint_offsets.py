#!/usr/bin/env python3
"""Checks `mg_reload_hint.rs`'s signature against real DoD `client.dll`s (#342).

`mg_reload_hint.rs` points one `call` at a filter of its own while
`ds_hud_hide_hints` is on, so the machine gun's "Deploy your machine gun to
Reload!" prompt is never queued. Every constant below is read out of
`mg_reload_hint.rs` rather than restated.

Checks, per client.dll:

  1. The signature matches exactly once in `.text`, with the call's five bytes
     left open (what the DLL scans for, so it also finds its own filter).
  2. The pushed key (`KEY_AT`) is the address of the string `Dod_mg_reload`,
     and that string is pushed nowhere else.
  3. The call at `CALL_AT` goes where the pinned `rel32` says, to a function
     whose first `ret` is `ret 0x18`: six stack arguments, callee-cleaned, as
     the six pushes in the signature and the filter's `thiscall` signature say.
  4. No `call`/`jmp rel32`, near conditional jump or nearby short jump lands
     inside the five patched bytes.

With two paths (PRE and POST by default), also says whether the files are
byte-identical, which is why one pattern covers both engine builds.

Usage:
    python goldsrc-hooks/tools/verify_mg_reload_hint_offsets.py [client.dll ...]

Requires `pefile` and `capstone` (analysis-only, not build dependencies).
"""

import hashlib
import re
import struct
import sys
from pathlib import Path

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "mg_reload_hint.rs"
CALL_LEN = 5


def rust_str(src: str, name: str) -> str:
    """`const NAME: &str = "...";`, joining `\\<newline>` continuations."""
    m = re.search(rf"const {name}:\s*&str\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {name} in mg_reload_hint.rs")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_usize(src: str, name: str) -> int:
    m = re.search(rf"const {name}:\s*usize\s*=\s*([0-9a-fx_]+);", src)
    if not m:
        sys.exit(f"could not find {name} in mg_reload_hint.rs")
    return int(m.group(1).replace("_", ""), 0)


def parse_pattern(text: str):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(code: bytes, pattern) -> list[int]:
    first = pattern[0]
    hits = []
    i = code.find(bytes([first]))
    while 0 <= i <= len(code) - len(pattern):
        if all(w is None or code[i + k] == w for k, w in enumerate(pattern)):
            hits.append(i)
        i = code.find(bytes([first]), i + 1)
    return hits


def check(dll: Path, pattern_text: str, key_at: int, call_at: int) -> bool:
    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.rstrip(b"\0") == b".text")
    t0 = text.VirtualAddress
    code = image[t0 : t0 + text.Misc_VirtualSize]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    ok = True
    print(f"client.dll: {dll}")

    pinned = parse_pattern(pattern_text)
    stock_call = bytes(pinned[call_at : call_at + CALL_LEN])
    open_pattern = list(pinned)
    open_pattern[call_at : call_at + CALL_LEN] = [None] * CALL_LEN
    hits = find_all(code, open_pattern)
    if len(hits) != 1:
        print(f"  FAIL the signature (call left open) matches {len(hits)} time(s)")
        return False
    at = t0 + hits[0]
    print(f"  OK   the signature matches once, at +{at:#x} (the call at +{at + call_at:#x})")
    if len(find_all(code, pinned)) == 1:
        print(f"  OK   the call there is the pinned {stock_call.hex(' ')}")
    else:
        ok = False
        print("  FAIL the pinned call does not match there")

    key_va = struct.unpack_from("<I", image, at + key_at)[0]
    key = image[key_va - base : key_va - base + 32].split(b"\0")[0]
    pushes = len(re.findall(re.escape(b"\x68" + struct.pack("<I", key_va)), code))
    if key == b"Dod_mg_reload" and pushes == 1:
        print(f"  OK   the pushed key is \"Dod_mg_reload\" (+{key_va - base:#x}), pushed only here")
    else:
        ok = False
        print(f"  FAIL the pushed key is {key!r}, pushed {pushes} time(s)")

    call = at + call_at
    rel = struct.unpack_from("<i", image, call + 1)[0]
    target = call + CALL_LEN + rel
    ret = None
    for ins in md.disasm(image[target : target + 0x200], base + target):
        if ins.mnemonic == "ret":
            ret = ins
            break
    if ret is not None and ret.op_str == "0x18":
        print(f"  OK   the callee +{target:#x} returns `ret 0x18` (+{ret.address - base:#x}): six stack arguments")
    else:
        ok = False
        print(f"  FAIL the callee +{target:#x}'s first ret is {ret.op_str if ret else 'missing'}")

    inside = range(base + call + 1, base + call + CALL_LEN)
    into = []
    for i in range(len(code) - 6):
        op = code[i]
        if op in (0xE8, 0xE9):
            dest = base + t0 + i + 5 + struct.unpack_from("<i", code, i + 1)[0]
            if dest in inside:
                into.append(t0 + i)
        elif op == 0x0F and 0x80 <= code[i + 1] <= 0x8F:
            dest = base + t0 + i + 6 + struct.unpack_from("<i", code, i + 2)[0]
            if dest in inside:
                into.append(t0 + i)
    for i in range(max(0, hits[0] - 130), min(len(code) - 2, hits[0] + 130)):
        if 0x70 <= code[i] <= 0x7F or code[i] == 0xEB:
            dest = base + t0 + i + 2 + struct.unpack_from("<b", code, i + 1)[0]
            if dest in inside:
                into.append(t0 + i)
    if into:
        ok = False
        print("  FAIL something may branch into the call from " + ", ".join(f"+{a:#x}" for a in into))
    else:
        print("  OK   nothing branches into the call's bytes")
    return ok


def main() -> int:
    dlls = [Path(a) for a in sys.argv[1:]] or DEFAULT_DLLS
    src = RUST.read_text(encoding="utf-8")
    pattern = rust_str(src, "PATTERN")
    key_at = rust_usize(src, "KEY_AT")
    call_at = rust_usize(src, "CALL_AT")
    print(f"pattern: {pattern}\nKEY_AT {key_at}, CALL_AT {call_at}\n")
    ok = True
    digests = []
    for dll in dlls:
        if not dll.is_file():
            print(f"no client.dll at {dll}")
            ok = False
            continue
        ok &= check(dll, pattern, key_at, call_at)
        digests.append(hashlib.sha256(dll.read_bytes()).hexdigest())
        print()
    if len(digests) > 1:
        same = len(set(digests)) == 1
        print(f"{'OK  ' if same else 'NOTE'} the {len(digests)} files are {'byte-identical' if same else 'different builds'}")
    print("\nALL OK" if ok else "\nFAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
