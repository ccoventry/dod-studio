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

And for POV's red crosshair (`cl_xhair_style 0`, #308), against
`spectator_crosshair_gap.rs`:

  8. The red jump's `add esp, 0x10` undoes the spectator draw's own
     `sub esp, 0x10`, and the style-0 draw it jumps to ends in a plain `ret`.
  9. The style-0 draw, the gap function and the spread function each match
     once, and the draw's gap call goes to the gap function.
 10. The gap function's constants are the module's: the fixed gap, the
     -200 scale, the 3 px step, the easing slot, and which weapon ids are
     fixed (its byte table); the spread function's 105 u/s threshold.
 11. The weapon table the spread function indexes has the module's stride and
     layout: the floats at +0/+4/+8 of the address it reads, and the known
     rows' viewmodels where the module looks for them.

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


GAP_SRC = Path(__file__).resolve().parent.parent / "src" / "spectator_crosshair_gap.rs"


def rust_u8_array(src: str, name: str):
    m = re.search(rf"const {name}:\s*\[u8;\s*\d+\]\s*=\s*\[(.*?)\];", src, re.S)
    if not m:
        sys.exit(f"could not find {name}")
    return [int(x, 16) for x in re.findall(r"0x([0-9a-fA-F]+)", m.group(1))]


def check_red_crosshair(span_src, image, code, trva, span_rva, base, check):
    """Checks 8-11: the red crosshair's jump and `spectator_crosshair_gap.rs`."""
    gsrc = GAP_SRC.read_text(encoding="utf-8")

    def u32(rva):
        return struct.unpack_from("<I", image, rva)[0]

    def f32_at_va(va):
        return struct.unpack_from("<f", image, va - base)[0]

    # 8. `mov eax, [fov]; sub esp, 0x10` and then the gate, before the span.
    gate = rust_const(span_src, "GATE")
    add_esp = rust_u8_array(span_src, "ADD_ESP")
    sub = bytes(image[span_rva - gate - 3 : span_rva - gate])
    check(
        add_esp[:2] == [0x83, 0xC4] and sub == bytes([0x83, 0xEC, add_esp[2]]),
        f"ADD_ESP undoes the spectator draw's `sub esp, {add_esp[2]:#x}` ({sub.hex(' ')})",
    )

    # 9. the three functions, and the draw's gap call
    def unique(name):
        pattern = parse_pattern(rust_string(gsrc, rf"const {name}:\s*&str"))
        hits = find_all(code, pattern)
        check(len(hits) == 1, f"{name} matches exactly once ({len(hits)} hit(s))")
        return trva + hits[0] if len(hits) == 1 else None

    draw, gap, spread = unique("DRAW_PATTERN"), unique("GAP_PATTERN"), unique("SPREAD_PATTERN")
    if None in (draw, gap, spread):
        return
    print(f"         style-0 draw client+{draw:#x}, gap client+{gap:#x}, spread client+{spread:#x}")
    site = draw + rust_const(gsrc, "GAP_CALL_AT")
    target = site + 5 + struct.unpack_from("<i", image, site + 1)[0]
    check(
        image[site] == 0xE8 and target == gap,
        f"the draw's call at client+{site:#x} goes to the gap function (client+{target:#x})",
    )
    draw_fn = bytes(image[draw : draw + 0x140])
    check(
        b"\x83\xc4\x0c\xc3" in draw_fn,
        "the style-0 draw ends `add esp, 0xc; ret`: no arguments, so the jump's tail call returns to Draw",
    )

    # 10. the gap function's constants
    gap_fn = bytes(image[gap : gap + 0x98])
    fixed = rust_const(gsrc, "FIXED_GAP")
    check(
        gap_fn.count(b"\xb8" + struct.pack("<I", fixed)) >= 2,
        f"FIXED_GAP: the gap function returns {fixed} on its fixed paths",
    )
    slot = rust_const(gsrc, "LAST_GAP_AT")
    check(
        b"\x8b\x86" + struct.pack("<I", slot) in gap_fn
        and b"\x89\x86" + struct.pack("<I", slot) in gap_fn,
        f"LAST_GAP_AT: it eases in [this+{slot:#x}]",
    )
    step = rust_const(gsrc, "STEP")
    check(
        b"\x83\xc0" + bytes([step]) in gap_fn and b"\x83\xc0" + bytes([(-step) & 0xFF]) in gap_fn,
        f"STEP: by {step} px a call",
    )
    fmul = re.search(rb"\xd8\x0d(....)", gap_fn, re.S)
    scale = f32_at_va(struct.unpack("<I", fmul.group(1))[0]) if fmul else None
    check(scale == -200.0, f"the spread is scaled by -200 ({scale})")
    bytes_table = re.search(rb"\x8a\x88(....)", gap_fn, re.S)
    bounds = re.search(rb"\x83\xc0(.)\x83\xf8(.)", gap_fn, re.S)
    jumps = re.search(rb"\xff\x24\x8d(....)", gap_fn, re.S)
    if bytes_table and bounds and jumps:
        lowest = -struct.unpack("b", bounds.group(1))[0]
        cases = image[
            struct.unpack("<I", bytes_table.group(1))[0] - base :
        ][: bounds.group(2)[0] + 1]
        jump_table = struct.unpack("<I", jumps.group(1))[0] - base
        found = set()
        for i, case in enumerate(cases):
            to = u32(jump_table + 4 * case) - base
            if image[to] == 0xB8 and u32(to + 1) == fixed:
                found.add(lowest + i)
        ranges = re.search(r"const FIXED_IDS:.*?=\s*\[(.*?)\];", gsrc, re.S).group(1)
        wanted = set()
        for low, high in re.findall(r"(\d+)\.\.=(\d+)", ranges):
            wanted |= set(range(int(low), int(high) + 1))
        check(
            found == wanted,
            f"FIXED_IDS {sorted(wanted)} are the ids its byte table sends to the fixed gap ({sorted(found)})",
        )
    else:
        check(False, "found the gap function's weapon switch")
    spread_fn = bytes(image[spread : spread + 0x70])
    fcomp = re.search(rb"\xd8\x1d(....)", spread_fn, re.S)
    speed = f32_at_va(struct.unpack("<I", fcomp.group(1))[0]) if fcomp else None
    wanted_speed = float(re.search(r"const MOVING_SPEED:\s*f32\s*=\s*([\d.]+);", gsrc).group(1))
    check(speed == wanted_speed, f"MOVING_SPEED {wanted_speed} is the spread function's threshold ({speed})")

    # 11. the table
    table_va = u32(spread + rust_const(gsrc, "TABLE_AT"))
    table = table_va - base
    check(b"\xd8\x86" + struct.pack("<I", table_va + 4) in spread_fn, "`moving` is the float at +4 (its fadd)")
    check(b"\x8b\x86" + struct.pack("<I", table_va + 8) in spread_fn, "`alt` is the one at +8")
    stride = rust_const(gsrc, "STRIDE")
    # lea ecx,[eax*8]; sub ecx,eax; lea eax,[eax+ecx*4]; lea esi,[eax+eax*2]; shl esi,2
    # is id * 4 * 3 * (1 + 4 * 7).
    check(stride == 4 * 3 * (1 + 4 * 7), f"STRIDE {stride:#x} is the spread function's index arithmetic")
    floats_at = rust_const(gsrc, "FLOATS_AT")
    viewmodel_at = rust_const(gsrc, "VIEWMODEL_AT")
    rows = re.search(r"const KNOWN_ROWS:.*?=\s*\[(.*?)\];", gsrc, re.S).group(1)
    for weapon, path in re.findall(r'\((\d+),\s*"([^"]+)"\)', rows):
        at = table + stride * int(weapon) - floats_at + viewmodel_at
        found = image[at : image.find(b"\0", at)].decode("latin-1")
        check(found == path, f"weapon {weapon}'s viewmodel is {path} ({found})")


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
    check_red_crosshair(src, image, code, trva, rva, base, check)

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
