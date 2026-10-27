#!/usr/bin/env python3
"""Checks `hint_panel.rs`'s signature against real DoD `client.dll`s (#286).

`hint_panel.rs` writes `ret 4` over the first three bytes of
`DoDViewport::ShowHint`. Every constant below is read out of `hint_panel.rs`
rather than restated, so this cannot drift from what the Rust code uses.

Checks, per client.dll:

  1. The signature matches exactly once in `.text`.
  2. The match starts with a five-byte `mov eax, [abs32]`, so the three bytes
     the stub covers sit inside one instruction, and HIDDEN is `ret 4`.
  3. The function takes one stack argument: it reads `[esp+8]` after its one
     `push esi`, loads the hint panel from `[esi+HINT_PANEL_OFFSET]`, and
     leaves through `jmp [edx+SET_VISIBLE_SLOT*4]` (`SetVisible`, which
     returns `ret 4`) with no `ret` of its own before it.
  4. One pointer to it sits in `.rdata` (the viewport's vftable), at slot 5
     of its table, and `MsgFunc_HudText` calls slot 5 (`[edx+0x14]`) on
     `gViewPortInterface` (`VIEWPORT_CALL`, read for closing an open hint)
     for a flagged message.
  5. No `call`/`jmp rel32` or short/near conditional jump in `.text` lands
     inside the three patched bytes.

With two paths (PRE and POST by default), also says whether the files are
byte-identical, which is why one pattern covers both engine builds.

Usage:
    python goldsrc-hooks/tools/verify_hint_panel_offsets.py [client.dll ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "hint_panel.rs"


def rust_str(src: str, name: str) -> str:
    """`const NAME: &str = "...";`, joining `\\<newline>` continuations."""
    m = re.search(rf"const {name}:\s*&str\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {name} in hint_panel.rs")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_usize(src: str, name: str) -> int:
    m = re.search(rf"const {name}:\s*usize\s*=\s*([0-9a-fx_ /]+);", src)
    if not m:
        sys.exit(f"could not find {name} in hint_panel.rs")
    parts = [int(p.strip().replace("_", ""), 0) for p in m.group(1).split("/")]
    return parts[0] // parts[1] if len(parts) == 2 else parts[0]


def rust_hidden(src: str) -> bytes:
    m = re.search(r"const HIDDEN:\s*\[u8;\s*\d+\]\s*=\s*\[([^\]]*)\];", src)
    if not m:
        sys.exit("could not find HIDDEN in hint_panel.rs")
    return bytes(int(t.strip(), 0) for t in m.group(1).split(",") if t.strip())


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


def check(dll: Path, rs: dict) -> bool:
    pattern_text, hidden = rs["PATTERN"], rs["HIDDEN"]
    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    secs = {s.Name.rstrip(b"\0").decode(): s for s in pe.sections}
    text = secs[".text"]
    t0 = text.VirtualAddress
    code = image[t0 : t0 + text.Misc_VirtualSize]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    ok = True

    print(f"client.dll: {dll}")
    hits = find_all(code, parse_pattern(pattern_text))
    if len(hits) != 1:
        print(f"  FAIL the signature matches {len(hits)} time(s)")
        return False
    fn = t0 + hits[0]
    print(f"  OK   the signature matches once, at +{fn:#x}")

    ins = list(md.disasm(image[fn : fn + 0x40], base + fn))
    first = ins[0]
    if first.mnemonic == "mov" and first.size == 5 and first.op_str.startswith("eax, dword ptr [0x"):
        print(f"  OK   it starts `{first.mnemonic} {first.op_str}` (5 bytes), so the stub stays inside one instruction")
    else:
        ok = False
        print(f"  FAIL it starts `{first.mnemonic} {first.op_str}` ({first.size} bytes)")
    if hidden == bytes([0xC2, 0x04, 0x00]) and len(hidden) <= first.size:
        print("  OK   HIDDEN is `ret 4`")
    else:
        ok = False
        print(f"  FAIL HIDDEN is {hidden.hex(' ')}, not c2 04 00")

    pushes = 0
    reads_arg = False
    panel_loads = 0
    tail = None
    panel = f"ecx, dword ptr [esi + {rs['HINT_PANEL_OFFSET']:#x}]"
    jump = f"dword ptr [edx + {rs['SET_VISIBLE_SLOT'] * 4:#x}]"
    for i in ins:
        if i.mnemonic == "push" and i.op_str == "esi":
            pushes += 1
        if i.op_str.endswith("dword ptr [esp + 8]") and pushes == 1:
            reads_arg = True
        if i.mnemonic == "mov" and i.op_str == panel:
            panel_loads += 1
        if i.mnemonic == "ret":
            break
        if i.mnemonic == "jmp" and i.op_str == jump:
            tail = i
            break
    if reads_arg and tail is not None and panel_loads == 2:
        print(
            f"  OK   one argument ([esp+8] after one push), the panel from `{panel}` (twice) "
            f"and a tail-jump `jmp {jump}` to SetVisible at +{tail.address - base:#x}"
        )
    else:
        ok = False
        print(f"  FAIL argument read {reads_arg}, panel loads {panel_loads}, tail-jump {'found' if tail else 'not found'}")

    rdata = secs[".rdata"]
    r0 = rdata.VirtualAddress
    rblob = image[r0 : r0 + rdata.Misc_VirtualSize]
    needle = struct.pack("<I", base + fn)
    refs = [r0 + m.start() for m in re.finditer(re.escape(needle), rblob) if m.start() % 4 == 0]
    if len(refs) == 1:
        slot = refs[0]
        # A vftable's slots are all code pointers; walk back to its first one.
        start = slot
        while True:
            prev = struct.unpack_from("<I", image, start - 4)[0] - base
            if t0 <= prev < t0 + text.Misc_VirtualSize:
                start -= 4
            else:
                break
        index = (slot - start) // 4
        if index == 5:
            print(f"  OK   one .rdata pointer to it, slot 5 of the table at +{start:#x}")
        else:
            ok = False
            print(f"  FAIL its .rdata pointer is slot {index} of the table at +{start:#x}, not 5")
    else:
        ok = False
        print(f"  FAIL {len(refs)} .rdata pointer(s) to it")

    calls = find_all(code, parse_pattern(rs["VIEWPORT_CALL"]))
    if len(calls) == 1:
        at = t0 + calls[0]
        global_va = struct.unpack_from("<I", image, at + rs["VIEWPORT_AT"])[0]
        data = secs[".data"]
        in_data = data.VirtualAddress <= global_va - base < data.VirtualAddress + data.Misc_VirtualSize
        if in_data:
            print(
                f"  OK   VIEWPORT_CALL matches once (+{at:#x}): gViewPortInterface is .data "
                f"+{global_va - base:#x}, and slot 5 ([edx+0x14]) is called on it"
            )
        else:
            ok = False
            print(f"  FAIL VIEWPORT_CALL's global +{global_va - base:#x} is not in .data")
    else:
        ok = False
        print(f"  FAIL VIEWPORT_CALL matches {len(calls)} time(s)")

    inside = range(base + fn + 1, base + fn + len(hidden))
    into = []
    for i in range(len(code) - 5):
        op = code[i]
        if op in (0xE8, 0xE9):
            target = base + t0 + i + 5 + struct.unpack_from("<i", code, i + 1)[0]
            if target in inside:
                into.append(t0 + i)
        elif op == 0x0F and 0x80 <= code[i + 1] <= 0x8F:
            target = base + t0 + i + 6 + struct.unpack_from("<i", code, i + 2)[0]
            if target in inside:
                into.append(t0 + i)
    for i in range(max(0, hits[0] - 130), min(len(code) - 2, hits[0] + 130)):
        if 0x70 <= code[i] <= 0x7F or code[i] == 0xEB:
            target = base + t0 + i + 2 + struct.unpack_from("<b", code, i + 1)[0]
            if target in inside:
                into.append(t0 + i)
    if into:
        ok = False
        print("  FAIL something may branch into the patched bytes from " + ", ".join(f"+{a:#x}" for a in into))
    else:
        print("  OK   nothing branches into the three patched bytes")
    return ok


def main() -> int:
    dlls = [Path(a) for a in sys.argv[1:]] or DEFAULT_DLLS
    src = RUST.read_text(encoding="utf-8")
    rs = {
        "PATTERN": rust_str(src, "PATTERN"),
        "HIDDEN": rust_hidden(src),
        "VIEWPORT_CALL": rust_str(src, "VIEWPORT_CALL"),
        "VIEWPORT_AT": rust_usize(src, "VIEWPORT_AT"),
        "HINT_PANEL_OFFSET": rust_usize(src, "HINT_PANEL_OFFSET"),
        "SET_VISIBLE_SLOT": rust_usize(src, "SET_VISIBLE_SLOT"),
    }
    print(f"pattern: {rs['PATTERN']}\nHIDDEN : {rs['HIDDEN'].hex(' ')}\n")
    ok = True
    digests = []
    for dll in dlls:
        if not dll.is_file():
            print(f"no client.dll at {dll}")
            ok = False
            continue
        ok &= check(dll, rs)
        digests.append(hashlib.sha256(dll.read_bytes()).hexdigest())
        print()
    if len(digests) > 1:
        same = len(set(digests)) == 1
        print(f"{'OK  ' if same else 'NOTE'} the {len(digests)} files are {'byte-identical' if same else 'different builds'}")
    print("\nALL OK" if ok else "\nFAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
