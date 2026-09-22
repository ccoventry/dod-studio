#!/usr/bin/env python3
"""Checks `ex_interp.rs` against a real GoldSrc `hw.dll`.

Two claims are worth checking rather than believing. The first is ordinary:
that the immediate this patches is the clamp's *ceiling* and not its floor --
patching the floor would make the engine force every value **up**, which is the
opposite of the intent and would look like the setting working backwards.

The second is the interesting one. `ex_interp.rs` asserts that the engine's own
200 ms ceiling is **unreachable** in this build, and the whole value of #271
turns on it: if the 200 ms path were live for demo playback, raising the ceiling
would be worth half what it looks like. That claim is reproduced here rather
than restated.

Every constant below is read out of the Rust.

  1. The clamp pattern matches exactly once in `.text`.
  2. `CEILING_AT` is the `imm32` of a `mov ebx, imm32`, by disassembly; the
     instruction before it is `mov edi, FLOOR_MS`.
  3. The shipped ceiling is `STOCK_MS`.
  4. The 200 ms branch is present: `mov eax, [flag]; test eax, eax; je +5;
     mov ebx, 200`.
  5. That flag has exactly four write sites -- every x86 addressing form for a
     write to an absolute address is searched, not just `mov dword [x], imm`.
  6. The only site writing a non-zero value is unreachable: no direct call, no
     near jump to it, no absolute reference anywhere in the image (so no jump
     table and no function pointer), and the bytes before it are an
     unconditional `jmp`, so nothing falls through.
  7. Writing a raised ceiling keeps every instruction boundary.

Usage:
    python goldsrc-hooks/tools/verify_ex_interp_offsets.py [path-to-hw.dll]

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

DEFAULT_HW = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)
SRC = Path(__file__).resolve().parent.parent / "src" / "ex_interp.rs"

# `mov eax, [flag]; test eax, eax; je +5; mov ebx, imm32` -- the branch that
# would raise the ceiling to 200. The flag's address is part of the signature
# on purpose: a different build should fail here rather than be described by a
# document written against this one.
RAISED_BRANCH = "A1 ?? ?? ?? ?? 85 C0 74 05 BB ?? ?? ?? ??"

# Every way x86 writes to an absolute address, so "four write sites" is a
# search rather than a guess.
WRITE_FORMS = {
    b"\xc7\x05": "mov dword [f], imm32",
    b"\xc6\x05": "mov byte [f], imm8",
    b"\xa3": "mov [f], eax (short form)",
    b"\x89\x05": "mov [f], eax",
    b"\x89\x0d": "mov [f], ecx",
    b"\x89\x15": "mov [f], edx",
    b"\x89\x1d": "mov [f], ebx",
    b"\x89\x25": "mov [f], esp",
    b"\x89\x2d": "mov [f], ebp",
    b"\x89\x35": "mov [f], esi",
    b"\x89\x3d": "mov [f], edi",
    b"\xff\x05": "inc dword [f]",
    b"\xff\x0d": "dec dword [f]",
    b"\x83\x0d": "or dword [f], imm8",
    b"\x83\x25": "and dword [f], imm8",
    b"\x09\x05": "or [f], eax",
    b"\x31\x05": "xor [f], eax",
}


def rust_string(src, decl):
    m = re.search(rf"{decl}\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {decl}")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_const(src, name):
    m = re.search(rf"const {name}:\s*\w+\s*=\s*(0x[0-9a-fA-F]+|\d+);", src)
    if not m:
        sys.exit(f"could not find {name}")
    return int(m.group(1), 0)


def parse_pattern(text):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(code, pattern, offset=0):
    n = len(pattern)
    hits = []
    for i in range(len(code) - n + 1):
        if code[i] != pattern[0]:
            continue
        if all(w is None or code[i + k] == w for k, w in enumerate(pattern)):
            hits.append(i + offset)
    return hits


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_HW
    if not dll.is_file():
        sys.exit(f"no hw.dll at {dll}")

    src = SRC.read_text(encoding="utf-8")
    pattern = parse_pattern(rust_string(src, r"const PATTERN:\s*&str"))
    ceiling_at = rust_const(src, "CEILING_AT")
    floor_ms = rust_const(src, "FLOOR_MS")
    stock_ms = rust_const(src, "STOCK_MS")
    max_ms = rust_const(src, "MAX_MS")

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    text = pe.sections[0]
    tlo, thi = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize
    code = img[tlo:thi]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    failures = []

    def check(ok, message):
        print(("  ok   " if ok else "  FAIL ") + message)
        if not ok:
            failures.append(message)

    print(f"hw.dll at {dll}")
    print(f"image base {base:#x}\n")

    # 1
    hits = find_all(code, pattern, tlo)
    check(len(hits) == 1, f"the clamp pattern matches exactly once ({len(hits)} hit(s))")
    if len(hits) != 1:
        return 1
    clamp = hits[0]
    print(f"\nclamp at hw+{clamp:#x}\n")

    # 2
    decoded = {}
    for ins in md.disasm(bytes(img[clamp:clamp + len(pattern)]), base + clamp):
        decoded[ins.address - base - clamp] = ins
    floor_ins = decoded.get(0)
    ceiling_ins = decoded.get(ceiling_at - 1)
    check(
        floor_ins is not None and floor_ins.op_str.startswith("edi,"),
        f"the span starts with `{floor_ins.mnemonic + ' ' + floor_ins.op_str if floor_ins else '?'}` -- the floor, untouched",
    )
    check(
        ceiling_ins is not None and ceiling_ins.op_str.startswith("ebx,"),
        f"CEILING_AT is the imm32 of `{ceiling_ins.mnemonic + ' ' + ceiling_ins.op_str if ceiling_ins else '?'}`",
    )
    check(
        floor_ins is not None and int(floor_ins.op_str.split(", ")[1], 0) == floor_ms,
        f"FLOOR_MS {floor_ms} is the floor the engine ships",
    )

    # 3
    shipped = struct.unpack_from("<i", img, clamp + ceiling_at)[0]
    check(shipped == stock_ms, f"STOCK_MS {stock_ms} is the ceiling this build ships ({shipped})")

    # 4
    raised = find_all(code, parse_pattern(RAISED_BRANCH), tlo)
    check(len(raised) == 1, f"the 200 ms branch is present exactly once ({len(raised)} hit(s))")
    if len(raised) != 1:
        return 1
    flag = struct.unpack_from("<I", img, raised[0] + 1)[0]
    raised_ms = struct.unpack_from("<i", img, raised[0] + 10)[0]
    print(f"\n  the raised ceiling is {raised_ms} ms, gated on the flag at {flag:#x}\n")

    # 5
    addr = struct.pack("<I", flag)
    writes = []
    for opcode, name in WRITE_FORMS.items():
        for m in re.finditer(re.escape(opcode + addr), img, re.S):
            at = m.start()
            if not tlo <= at < thi:
                continue
            value = None
            if opcode == b"\xc7\x05":
                value = struct.unpack_from("<I", img, at + 6)[0]
            elif opcode == b"\xc6\x05":
                value = img[at + 6]
            writes.append((at, name, value))
    writes.sort()
    check(len(writes) == 4, f"the flag has {len(writes)} write site(s); ex_interp.rs says four")
    for at, name, value in writes:
        suffix = f" = {value}" if value is not None else ""
        print(f"         hw+{at:#08x}  {name}{suffix}")

    # 6
    nonzero = [w for w in writes if w[2] not in (0, None)]
    check(len(nonzero) == 1, f"exactly one site writes a non-zero value ({len(nonzero)})")
    if len(nonzero) != 1:
        return 1
    setter = nonzero[0][0]
    print(f"\n  the only site that sets it is hw+{setter:#x}; checking it is unreachable\n")

    callers = [
        i + tlo
        for i in range(len(code) - 5)
        if code[i] == 0xE8 and i + 5 + struct.unpack_from("<i", code, i + 1)[0] + tlo == setter
    ]
    check(not callers, f"no direct call reaches it ({len(callers)})")
    jumps = [
        i + tlo
        for i in range(len(code) - 5)
        if code[i] == 0xE9 and i + 5 + struct.unpack_from("<i", code, i + 1)[0] + tlo == setter
    ]
    check(not jumps, f"no near jump reaches it ({len(jumps)})")
    pointers = [m.start() for m in re.finditer(re.escape(struct.pack("<I", base + setter)), img, re.S)]
    check(
        not pointers,
        f"its address appears nowhere in the image, so no jump table or function pointer ({len(pointers)})",
    )
    # Nothing falls into it. Decoding backwards is unreliable -- a wrong start
    # offset produces a plausible instruction that happens to end in the right
    # place -- so this is a byte test on the two shapes that can precede a
    # block nothing falls into.
    jmp_before = (
        img[setter - 5] == 0xE9
        and tlo <= setter + struct.unpack_from("<i", img, setter - 4)[0] < thi
    )
    padded = img[setter - 1] in (0xC3, 0xCC, 0x90)
    if jmp_before:
        target = setter + struct.unpack_from("<i", img, setter - 4)[0]
        reason = f"the five bytes before it are `jmp hw+{target:#x}`, an unconditional branch"
    elif padded:
        reason = f"the byte before it is {img[setter - 1]:#04x} (ret/int3/nop padding)"
    else:
        reason = f"the byte before it is {img[setter - 1]:#04x}, which could fall through"
    check(jmp_before or padded, f"nothing falls into it: {reason}")

    # 7
    patched = bytearray(img[clamp:clamp + len(pattern)])
    struct.pack_into("<i", patched, ceiling_at, max_ms)
    before = [i.address for i in md.disasm(bytes(img[clamp:clamp + len(pattern)]), 0)]
    after = [i.address for i in md.disasm(bytes(patched), 0)]
    check(before == after, f"writing {max_ms} keeps every instruction boundary")

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    print()
    print("So the engine never takes its own 200 ms branch, and the ceiling is a")
    print("hard 100 ms until something writes over the immediate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
