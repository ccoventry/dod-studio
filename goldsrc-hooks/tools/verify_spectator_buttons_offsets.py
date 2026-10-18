#!/usr/bin/env python3
"""Checks `spectator_buttons.rs` against a real DoD `client.dll`.

The module writes `ret 4` over the first instruction of
`CHudSpectator::HandleButtonsDown` while a review runs. A wrong pattern, a
first instruction of another width, or a function that pops a different
number of bytes would leave a broken instruction behind or unbalance the
stack: compiled fine, crashes in game.

Every constant below is read out of the Rust rather than restated.

  1. The pattern matches exactly once in `.text`.
  2. Its first instruction is `sub esp, 0xc`, exactly `STOCK`'s bytes.
  3. The function returns with `ret 4` (one argument) and nothing else.
  4. It tests the button bits the module docs list: 4 (duck), 0x20 (use),
     2 (jump), 0x801 (attack / attack2), and calls the target stepper.
  5. It is called from exactly nine sites, each `mov ecx, imm32; push <bit>;
     call` (thiscall, one argument) with the nine button bits the docs list.

Usage:
    python goldsrc-hooks/tools/verify_spectator_buttons_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install; `client.dll` is the same file
in the 25th Anniversary one (run it on both to check). Requires `pefile` and
`capstone`.
"""

import hashlib
import re
import struct
import sys
from pathlib import Path

try:
    import pefile
    import capstone
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\dod\cl_dlls\client.dll"
)
SRC = Path(__file__).resolve().parent.parent / "src" / "spectator_buttons.rs"

# +attack, +jump, +duck, +forward, +back, +use, +moveleft, +moveright, +attack2.
BUTTON_BITS = {0x1, 0x2, 0x4, 0x8, 0x10, 0x20, 0x200, 0x400, 0x800}


def rust_string(src: str, decl: str) -> str:
    m = re.search(rf"{decl}\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {decl}")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_bytes(src: str, name: str) -> bytes:
    m = re.search(rf"const {name}:\s*&\[u8\]\s*=\s*&\[(.*?)\];", src, re.S)
    if not m:
        sys.exit(f"could not find {name}")
    return bytes(int(t, 0) for t in m.group(1).split(",") if t.strip())


def parse_pattern(text):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(code, pattern):
    n = len(pattern)
    hits = []
    for i in range(len(code) - n + 1):
        if code[i] != pattern[0]:
            continue
        if all(w is None or code[i + k] == w for k, w in enumerate(pattern)):
            hits.append(i)
    return hits


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")

    src = SRC.read_text(encoding="utf-8")
    pattern = parse_pattern(rust_string(src, r"const PATTERN:\s*&str"))
    stock = rust_bytes(src, "STOCK")
    ignored = rust_bytes(src, "IGNORED")

    raw = dll.read_bytes()
    pe = pefile.PE(data=raw, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    text = pe.sections[0]
    trva = text.VirtualAddress
    code = image[trva : trva + text.Misc_VirtualSize]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    failures = []

    def check(ok, message):
        print(("  ok   " if ok else "  FAIL ") + message)
        if not ok:
            failures.append(message)

    print(f"client.dll at {dll}")
    print(f"sha256 {hashlib.sha256(raw).hexdigest()}")
    print(f"image base {base:#x}\n")

    hits = find_all(code, pattern)
    check(len(hits) == 1, f"PATTERN matches exactly once ({len(hits)} hit(s))")
    if len(hits) != 1:
        return 1
    at = hits[0]
    rva = trva + at
    print(f"\nHandleButtonsDown at client+{rva:#x}\n")

    body = []
    for ins in md.disasm(bytes(code[at : at + 0x400]), base + rva):
        body.append(ins)
        if ins.mnemonic == "ret":
            break

    first = body[0]
    check(
        first.mnemonic == "sub" and first.op_str == "esp, 0xc" and bytes(first.bytes) == stock,
        f"the first instruction is `sub esp, 0xc` = STOCK {stock.hex(' ')}",
    )
    check(len(ignored) == len(stock), "IGNORED is as wide as STOCK")
    check(
        ignored[0] == 0xC2 and struct.unpack_from("<H", ignored, 1)[0] == 4,
        f"IGNORED {ignored.hex(' ')} is `ret 4`",
    )
    rets = [i for i in body if i.mnemonic == "ret"]
    check(
        len(rets) == 1 and rets[0].op_str == "4",
        f"the function returns with `ret 4` ({[r.op_str for r in rets]})",
    )

    tests = set()
    for i in body:
        if i.mnemonic == "test" and len(i.operands) == 2 and i.operands[1].type == capstone.x86.X86_OP_IMM:
            tests.add(i.operands[1].imm)
    for bit in (0x4, 0x20, 0x2, 0x801):
        check(bit in tests, f"it tests button bits {bit:#x}")
    calls = [i for i in body if i.mnemonic == "call" and i.operands[0].type == capstone.x86.X86_OP_IMM]
    check(len(calls) >= 3, f"it calls the target stepper and SetMode ({len(calls)} direct calls)")

    callers = []
    for m in re.finditer(rb"\xe8", code):
        c = m.start()
        if c + 5 > len(code):
            continue
        rel = struct.unpack_from("<i", code, c + 1)[0]
        if c + 5 + rel == at:
            callers.append(c)
    check(len(callers) == 9, f"it is called from nine sites ({len(callers)})")
    bits = set()
    for c in callers:
        ok = False
        bit = None
        if code[c - 2] == 0x6A:  # push imm8
            bit = code[c - 1]
            ok = code[c - 7] == 0xB9  # mov ecx, imm32
        elif code[c - 5] == 0x68:  # push imm32
            bit = struct.unpack_from("<I", code, c - 4)[0]
            ok = code[c - 10] == 0xB9
        check(ok, f"caller client+{trva + c:#x} is `mov ecx, imm32; push {bit}; call` (thiscall)")
        bits.add(bit)
    check(bits == BUTTON_BITS, f"the callers pass the nine button bits ({sorted(bits)})")

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
