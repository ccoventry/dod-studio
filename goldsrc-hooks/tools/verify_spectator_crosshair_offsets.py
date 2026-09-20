#!/usr/bin/env python3
"""Checks `spectator_crosshair.rs` against a real DoD `client.dll`.

The module rewrites four rect immediates and one displacement byte inside a
57-byte span, and claims the tile arithmetic it uses is DoD's own. Both claims
are about a binary that is not in this repository, and both would compile and
pass the unit tests if they were wrong -- a rect written into the wrong field,
or a grid invented rather than read, produces a crosshair drawn from the wrong
part of a sprite, which looks like bad art rather than a bug.

Every constant below is read out of the Rust rather than restated.

  1. The signature matches exactly once in `.text`.
  2. Each patched field lands on the operand of a whole instruction, by
     disassembling rather than by counting bytes -- the four rect writes are
     `mov dword ptr [ecx+disp], imm32` and the fifth is `mov eax, [ecx+disp]`.
  3. The four rect writes name the displacements `wrect_t` puts them at, in the
     order the Rust says they are in.
  4. The stock immediates in the binary are the ones `STOCK_RECT` claims, and
     the stock displacement is `STOCK_HANDLE`.
  5. `CUSTOM_HANDLE` is the offset `VidInit` stores `customXHair.spr`'s handle
     into, found independently by following the `pfnSPR_Load` call that is
     passed that string.
  6. The tile grid is DoD's: the POV path shifts its row and column left by the
     same `TILE` this module uses, and clamps at the same `MAX_STYLE`.
  7. The patched span decodes to the same instruction boundaries as the stock
     one, so nothing after it is left mid-instruction.

Usage:
    python goldsrc-hooks/tools/verify_spectator_crosshair_offsets.py [path-to-client.dll]

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
SRC = Path(__file__).resolve().parent.parent / "src" / "spectator_crosshair.rs"

CUSTOM_SPRITE = b"sprites/customXHair.spr\x00"


def rust_string(src: str, decl: str) -> str:
    m = re.search(rf"{decl}\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {decl}")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_const(src: str, name: str) -> int:
    m = re.search(rf"const {name}:\s*\w+\s*=\s*(0x[0-9a-fA-F]+|\d+);", src)
    if not m:
        sys.exit(f"could not find {name}")
    return int(m.group(1), 0)


def rust_i32_array(src: str, name: str):
    m = re.search(rf"const {name}:\s*\[i32;\s*\d+\]\s*=\s*\[(.*?)\];", src, re.S)
    if not m:
        sys.exit(f"could not find {name}")
    return [int(x) for x in re.findall(r"-?\d+", m.group(1))]


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
    span = rust_const(src, "SPAN")
    fields = {name: rust_const(src, f"{name}_AT") for name in ("LEFT", "TOP", "RIGHT", "BOTTOM")}
    handle_at = rust_const(src, "HANDLE_AT")
    stock_handle = rust_const(src, "STOCK_HANDLE")
    custom_handle = rust_const(src, "CUSTOM_HANDLE")
    stock_rect = rust_i32_array(src, "STOCK_RECT")
    tile = rust_const(src, "TILE")
    columns = rust_const(src, "COLUMNS")
    max_style = rust_const(src, "MAX_STYLE")

    pe = pefile.PE(str(dll), fast_load=True)
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
    print(f"image base {base:#x}\n")

    # 1. one match
    check(len(pattern) == span, f"PATTERN describes all {span} bytes of the span")
    hits = find_all(code, pattern)
    check(len(hits) == 1, f"PATTERN matches exactly once ({len(hits)} hit(s))")
    if len(hits) != 1:
        return 1
    at = hits[0]
    rva = trva + at
    present = code[at : at + span]
    print(f"\nspan at client+{rva:#x}\n")

    # 2 + 3. every patched field is an operand of a whole instruction
    boundaries = {}
    for ins in md.disasm(bytes(present), base + rva):
        boundaries[ins.address - base - rva] = ins
    for name, off in fields.items():
        ins = boundaries.get(off - 3)
        ok = ins is not None and ins.mnemonic == "mov" and len(ins.bytes) == 7
        check(ok, f"{name}_AT is the imm32 of a 7-byte mov")
        if ok:
            # `C7 41 disp imm32` -- displacement is the third byte.
            print(f"         {ins.mnemonic} {ins.op_str}")
    ins = boundaries.get(handle_at - 2)
    check(
        ins is not None and ins.mnemonic == "mov" and len(ins.bytes) == 3,
        "HANDLE_AT is the displacement of a 3-byte mov",
    )

    # wrect_t is { left, right, top, bottom }: 0x64, 0x68, 0x6c, 0x70.
    expected_disp = {"LEFT": 0x64, "RIGHT": 0x68, "TOP": 0x6C, "BOTTOM": 0x70}
    for name, off in fields.items():
        check(
            present[off - 1] == expected_disp[name],
            f"{name} writes [ecx+{expected_disp[name]:#04x}], as wrect_t lays it out",
        )

    # 4. the stock values in the binary are the ones the Rust claims
    found_rect = [
        struct.unpack_from("<i", present, fields["LEFT"])[0],
        struct.unpack_from("<i", present, fields["TOP"])[0],
        struct.unpack_from("<i", present, fields["RIGHT"])[0],
        struct.unpack_from("<i", present, fields["BOTTOM"])[0],
    ]
    check(found_rect == stock_rect, f"STOCK_RECT {stock_rect} is what ships ({found_rect})")
    check(
        present[handle_at] == stock_handle,
        f"STOCK_HANDLE {stock_handle:#04x} is what ships ({present[handle_at]:#04x})",
    )

    # 5. CUSTOM_HANDLE is where VidInit stores customXHair.spr's handle
    sprite_rva = image.find(CUSTOM_SPRITE)
    check(sprite_rva > 0, "the string \"sprites/customXHair.spr\" is in the image")
    push = bytes([0x68]) + struct.pack("<I", base + sprite_rva)
    pushes = [m.start() for m in re.finditer(re.escape(push), image, re.S)]
    check(len(pushes) == 1, f"it is pushed exactly once ({len(pushes)} site(s))")
    if pushes:
        # The `mov [esi+disp], eax` that stores what pfnSPR_Load returned.
        window = image[pushes[0] : pushes[0] + 0x40]
        store = re.search(rb"\x89\x46(.)", window)
        check(store is not None, "its handle is stored with `mov [esi+disp], eax`")
        if store:
            disp = store.group(1)[0]
            check(
                disp == custom_handle,
                f"CUSTOM_HANDLE {custom_handle:#04x} is that displacement ({disp:#04x})",
            )

    # 6. the tile grid is DoD's own, read off the POV path
    #    `shl reg, N` with N = log2(TILE), and `cmp eax, MAX_STYLE`.
    shift = tile.bit_length() - 1
    check(1 << shift == tile, f"TILE {tile} is a power of two, so DoD's `shl {shift}` matches")
    pov = re.search(
        rb"\x83\xf8" + bytes([max_style]) + rb"\x7e",  # cmp eax, MAX_STYLE ; jle
        code,
        re.S,
    )
    check(pov is not None, f"the POV path clamps at MAX_STYLE ({max_style})")
    if pov:
        pov_rva = trva + pov.start()
        window = code[pov.start() : pov.start() + 0x60]
        # `C1 /4 ib` -- shl r32, imm8; the modrm is 0xE0 + the register.
        shl = rb"\xc1[\xe0-\xe7]" + bytes([shift])
        shifts = re.findall(shl, window)
        check(
            len(shifts) >= 4,
            f"and shifts its row/column left by {shift} ({len(shifts)} `shl` found at client+{pov_rva:#x})",
        )
    check(columns * tile == 256, f"COLUMNS x TILE is customXHair.spr's 256 ({columns} x {tile})")

    # 7. patching does not move an instruction boundary
    patched = bytearray(present)
    for name, off in fields.items():
        struct.pack_into("<i", patched, off, tile)
    patched[handle_at] = custom_handle
    stock_ends = [i.address for i in md.disasm(bytes(present), 0)]
    patched_ends = [i.address for i in md.disasm(bytes(patched), 0)]
    check(stock_ends == patched_ends, "the patched span keeps every instruction boundary")

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
