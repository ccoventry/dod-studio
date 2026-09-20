#!/usr/bin/env python3
"""Checks `scoreboard.rs`'s signature against a real DoD `client.dll`.

`scoreboard.rs` flips one byte inside a binary that is not in this repository.
One byte is small enough to look self-evidently safe, which is exactly why it
is worth checking: a pattern that matched the *hide* handler instead of the
show one, or a `JE_AT` off by a byte, would compile, pass the unit tests, and
patch the wrong instruction in the user's game.

Every constant below is read out of `scoreboard.rs` rather than restated, so
this cannot drift from what the Rust code actually uses.

Six checks:

  1. The signature matches exactly once in `.text`.
  2. `JE_AT` lands on a real `je rel8`, by disassembling the match rather than
     by counting bytes in the pattern.
  3. That `je` is the branch over the virtual call -- its target is the
     function's own `ret`, so making it unconditional cannot land anywhere
     else.
  4. The match really is the `+showscores` handler: it is the function pointer
     the client hands `pfnAddCommand` next to the "+showscores" string.
  5. The `-showscores` handler is a *different* function, and the signature
     does not match it.
  6. Rewriting the byte to `EB` leaves a function that still decodes cleanly
     and still returns.

Usage:
    python goldsrc-hooks/tools/verify_scoreboard_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone` (`pip install pefile capstone`); both are analysis-only and are not
build dependencies of anything in the workspace.
"""

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
RUST = Path(__file__).resolve().parent.parent / "src" / "scoreboard.rs"


def rust_pattern(src: str) -> str:
    """`const PATTERN: &str = "..." ;`, including its line continuations."""
    m = re.search(r'const PATTERN:\s*&str\s*=\s*(.*?);', src, re.S)
    if not m:
        sys.exit("could not find PATTERN in scoreboard.rs")
    # Strip the Rust string quoting and `\<newline>` continuations, which join
    # without whitespace -- the same way rustc joins them.
    body = m.group(1)
    body = re.sub(r'\\\s*\n\s*', '', body)
    parts = re.findall(r'"([^"]*)"', body)
    return " ".join(p.strip() for p in parts).strip()


def rust_usize(src: str, name: str) -> int:
    m = re.search(rf'const {name}:\s*usize\s*=\s*([0-9_x a-fA-F]+);', src)
    if not m:
        sys.exit(f"could not find {name} in scoreboard.rs")
    return int(m.group(1).replace("_", "").strip(), 0)


def rust_u8(src: str, name: str) -> int:
    m = re.search(rf'const {name}:\s*u8\s*=\s*(0[xX][0-9a-fA-F]+);', src)
    if not m:
        sys.exit(f"could not find {name} in scoreboard.rs")
    return int(m.group(1), 16)


def parse_pattern(text: str):
    out = []
    for token in text.split():
        out.append(None if token == "??" else int(token, 16))
    return out


def find_all(code: bytes, pattern) -> list[int]:
    n = len(pattern)
    first = pattern[0]
    hits = []
    for i in range(len(code) - n + 1):
        if code[i] != first:
            continue
        if all(w is None or code[i + k] == w for k, w in enumerate(pattern)):
            hits.append(i)
    return hits


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")

    src = RUST.read_text(encoding="utf-8")
    pattern_text = rust_pattern(src)
    pattern = parse_pattern(pattern_text)
    je_at = rust_usize(src, "JE_AT")
    je_byte = rust_u8(src, "JE")
    jmp_byte = rust_u8(src, "JMP")

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    blob = bytes(pe.__data__)
    text = pe.sections[0]
    code = blob[text.PointerToRawData : text.PointerToRawData + text.SizeOfRawData]
    text_rva = text.VirtualAddress

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True

    print(f"client.dll: {dll}")
    print(f"pattern   : {pattern_text}")
    print(f"JE_AT     : {je_at}   JE={je_byte:#04x} JMP={jmp_byte:#04x}\n")

    ok = True

    # ── 1. unique ───────────────────────────────────────────────────────────
    hits = find_all(code, pattern)
    if len(hits) == 1:
        print(f"  OK   the signature matches exactly once, at +{text_rva + hits[0]:#x}")
    else:
        where = ", ".join(f"+{text_rva + h:#x}" for h in hits) or "nowhere"
        print(f"  FAIL the signature matches {len(hits)} time(s): {where}")
        return 1

    start_rva = text_rva + hits[0]
    start_off = text.PointerToRawData + hits[0]

    # ── 2. JE_AT lands on a real je rel8 ────────────────────────────────────
    instructions = list(md.disasm(code[hits[0] : hits[0] + len(pattern) + 8], base + start_rva))
    je_rva = start_rva + je_at
    je_ins = next((i for i in instructions if i.address - base == je_rva), None)
    if je_ins is None:
        print(f"  FAIL +{je_rva:#x} is not an instruction boundary -- JE_AT is off")
        return 1
    if je_ins.mnemonic == "je" and je_ins.size == 2:
        print(f"  OK   +{je_rva:#x} is `{je_ins.mnemonic} {je_ins.op_str}`, a two-byte je rel8")
    else:
        ok = False
        print(f"  FAIL +{je_rva:#x} is `{je_ins.mnemonic} {je_ins.op_str}` ({je_ins.size} bytes), not a je rel8")
    if blob[start_off + je_at] != je_byte:
        ok = False
        print(f"  FAIL the byte there is {blob[start_off + je_at]:#04x}, not JE ({je_byte:#04x})")

    # ── 3. the branch skips to the function's own ret ───────────────────────
    target = je_ins.operands[0].imm - base
    landing = next((i for i in instructions if i.address - base == target), None)
    if landing is not None and landing.mnemonic == "ret":
        print(f"  OK   the je jumps to +{target:#x}, which is the function's own `ret`")
    else:
        ok = False
        shown = f"`{landing.mnemonic} {landing.op_str}`" if landing else "not an instruction boundary"
        print(f"  FAIL the je jumps to +{target:#x}, which is {shown} -- not a ret")

    # ── 4. it is the function registered as "+showscores" ───────────────────
    def string_va(needle: bytes) -> int | None:
        i = blob.find(needle + b"\0")
        if i < 0:
            return None
        for s in pe.sections:
            if s.PointerToRawData <= i < s.PointerToRawData + s.SizeOfRawData:
                return base + s.VirtualAddress + (i - s.PointerToRawData)
        return None

    show_str = string_va(b"+showscores")
    hide_str = string_va(b"-showscores")
    if show_str is None or hide_str is None:
        ok = False
        print("  FAIL could not find the \"+showscores\"/\"-showscores\" strings")
    else:
        # `push handler; push name; call [pfnAddCommand]` -- the handler is the
        # imm32 of the push five bytes before the name's push.
        def handler_for(name_va: int) -> int | None:
            needle = b"\x68" + struct.pack("<I", name_va)
            i = code.find(needle)
            if i < 5 or code[i - 5] != 0x68:
                return None
            return struct.unpack("<I", code[i - 4 : i])[0] - base

        show_fn = handler_for(show_str)
        hide_fn = handler_for(hide_str)
        if show_fn == start_rva:
            print(f"  OK   +{start_rva:#x} is the handler registered for \"+showscores\"")
        else:
            ok = False
            shown = f"+{show_fn:#x}" if show_fn is not None else "not found"
            print(f"  FAIL \"+showscores\" registers {shown}, but the signature matched +{start_rva:#x}")

        # ── 5. the hide handler is a different function the pattern misses ──
        if hide_fn is None:
            ok = False
            print("  FAIL could not find the \"-showscores\" handler to compare against")
        elif hide_fn == start_rva:
            ok = False
            print("  FAIL \"+showscores\" and \"-showscores\" resolve to the same function")
        else:
            matched_hide = any(text_rva + h == hide_fn for h in hits)
            if matched_hide:
                ok = False
                print(f"  FAIL the signature also matches the \"-showscores\" handler at +{hide_fn:#x}")
            else:
                print(f"  OK   \"-showscores\" is a separate function (+{hide_fn:#x}) the signature does not match")

    # ── 6. the patched function still decodes ───────────────────────────────
    patched = bytearray(code[hits[0] : hits[0] + len(pattern) + 8])
    patched[je_at] = jmp_byte
    decoded = list(md.disasm(bytes(patched), base + start_rva))
    before = [(i.address - base, i.mnemonic) for i in instructions]
    after = [(i.address - base, i.mnemonic) for i in decoded]
    same_shape = len(before) == len(after) and all(
        b[0] == a[0] and (b[1] == a[1] or (b[1] == "je" and a[1] == "jmp"))
        for b, a in zip(before, after)
    )
    if same_shape:
        print(f"  OK   with {jmp_byte:#04x} written, every later instruction still starts where it did")
    else:
        ok = False
        print("  FAIL the rewrite changes where later instructions begin")
        print(f"       before: {before}")
        print(f"       after : {after}")

    print("\nSIGNATURE VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
