#!/usr/bin/env python3
"""Checks `deathmsg/scale.rs` against a real DoD `client.dll` (issue #728).

`scale.rs` rewrites three operands in `CHudDeathNotice::Draw`: the two
`mov eax, [gHUD.m_rgrcRects]` reads become reads of a pointer the hook owns,
and the `call [gEngfuncs+0x18]` (`pfnSPR_DrawHoles`) becomes `call rel32;
nop`. All three are offsets from `font.rs`'s pattern match, so nothing at
runtime proves they land on the instructions the Rust assumes. This does:

  1. `font.rs`'s signature matches exactly once, inside `Draw`.
  2. Each `RECTS_READS` offset decodes as `mov eax, dword ptr [imm32]` (`A1`,
     five bytes) and both name `gHUD + RECTS_IN_HUD`; `SPRITES_READ` decodes
     as `mov edx, dword ptr [imm32]` naming the dword before it
     (`m_rghSprites`). `Draw` holds no other reference to `m_rgrcRects`.
  3. `DRAW_HOLES_CALL` is `call dword ptr [gEngfuncs + 4*6]` and `SPR_SET_CALL`
     `call dword ptr [gEngfuncs + 4*4]`, `gEngfuncs` being client.dll's own
     copy of the table in `.data`; their shared cleanup is `add esp, 0x20`.
  4. `gHUD` is the `this` `Draw` passes to `CHud` at its start, and
     `CHud::VidInit` allocates `[gHUD+0x18c] = new[16 * [gHUD+0x10]]` -- the
     field, the count and the 16-byte `wrect_t` stride `scale.rs` assumes.
  5. `Draw` uses the rect as `{left, right, top, bottom}`: the anchor reads
     `+4` then `+0`, the advance after the icon `right - left`.
  6. `.?AVCHudDeathNotice@@`'s vftable is where `hudelement.rs` says, its slot
     3 is `Draw`, and `Draw` returns with `ret 4` (one float argument).
  7. Nothing in `Draw` branches into the middle of a rewritten operand, and
     with the three rewritten `Draw` decodes to the same instructions plus
     one `nop`.
  8. Both movie installs ship the same `client.dll` (so one table serves both
     engine builds; nothing in `hw.dll` is touched).
  9. The line-start detour (`LINE_AT`, `LINE_STOLEN`): the bytes are
     `shl ebx, 4; add esp, 0xc`, right after the first rect read with `ebx`
     the line's sprite index (`[esp+0x1c]`, the one `SPR_Set`'s handle
     lookup reads) and nothing between writing `eax`/`ebx`; `edx` is written
     before it is next read; nothing branches into the span; and `[ebx+8]` /
     `[ebx+0xc]`, the stub's top and bottom, are what the line step
     subtracts. That same `ebx` is the index the stub stores for the icon
     draw (`ICON_INDEX`, the even layout, #1022).
 10. The even layout's names (#1022): `CHud::VidInit` allocates
     `[gHUD+NAMES_IN_HUD] = new[SPRITE_NAME_LEN * m_iSpriteCount]` right after
     the rects, and copies each `hud.txt` name into its entry with a
     `SPRITE_NAME_LEN`-byte `strncpy` (`even.rs`' `SPRITE_NAME_LEN`).

Usage:
    python goldsrc-hooks/tools/verify_killfeed_scale_offsets.py [client.dll] [game-folder ...]

Defaults to the pre-Anniversary movies install's `client.dll` and both movie
installs. Needs `pip install pefile capstone`.
"""

import hashlib
import re
import struct
import sys
from pathlib import Path

try:
    import capstone
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_killfeed_font_offsets as font  # noqa: E402

HERE = Path(__file__).resolve().parent.parent / "src"
SCALE_RS = HERE / "deathmsg" / "scale.rs"
EVEN_RS = HERE / "deathmsg" / "even.rs"
FONT_RS = HERE / "deathmsg" / "font.rs"
HUDELEMENT_RS = HERE / "hudelement.rs"

DRAW = font.DRAW
GENGFUNCS_RVA = font.GENGFUNCS_RVA
# `CHud::VidInit`, for the allocation check.
VIDINIT = (0x21B90, 0x21E40)


def num(text):
    return int(text.replace("_", ""), 0)


def rust_usize_list(src, name):
    body = re.search(rf"const {name}: &\[usize\] = &\[(.*?)\];", src, re.S).group(1)
    return [num(t) for t in re.findall(r"0x[0-9a-fA-F_]+|\d+", body)]


def rust_const(src, name):
    match = re.search(rf"const {name}: \w+ = (0x[0-9a-fA-F_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}`")
    return num(match.group(1))


def rust_byte_list(src, name):
    body = re.search(rf"const {name}: &\[u8\] = &\[(.*?)\];", src, re.S).group(1)
    return [num(t) for t in re.findall(r"0x[0-9a-fA-F_]+|\d+", body)]


def hudelement_vftable(src, cls):
    body = re.search(rf'class: "{re.escape(cls)}",\s*vftable_rva: (0x[0-9a-fA-F_]+)', src)
    return num(body.group(1)) if body else None


def branches_into(decoded, img, lo, hi):
    """Branch/call sites in `decoded` whose target lies strictly inside (lo, hi)."""
    hits = []
    for ins in decoded.values():
        if not ({capstone.x86.X86_GRP_JUMP, capstone.x86.X86_GRP_CALL} & set(ins.groups)):
            continue
        for op in ins.operands:
            if op.type == capstone.x86.X86_OP_IMM and lo < op.imm - img.base < hi:
                hits.append((ins.address - img.base, op.imm - img.base))
    return hits


def check_client(dll):
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("  OK   " if passed else "  FAIL ") + message)

    print(f"\n==== {dll} ====")
    img = font.Image(dll)
    scale_src = SCALE_RS.read_text(encoding="utf-8")
    font_src = FONT_RS.read_text(encoding="utf-8")
    pattern = font.rust_pattern(font_src)
    reads = rust_usize_list(scale_src, "RECTS_READS")
    read_len = rust_const(scale_src, "READ_LEN")
    sprites_read = rust_const(scale_src, "SPRITES_READ")
    draw_holes_call = rust_const(scale_src, "DRAW_HOLES_CALL")
    draw_holes_slot = rust_const(scale_src, "DRAW_HOLES_SLOT")
    spr_set_call = rust_const(scale_src, "SPR_SET_CALL")
    spr_set_slot = rust_const(scale_src, "SPR_SET_SLOT")
    rects_in_hud = rust_const(scale_src, "RECTS_IN_HUD")
    count_in_hud = rust_const(scale_src, "SPRITE_COUNT_IN_HUD")
    call_len = font.rust_const(font_src, "CALL_LEN")

    # -- 1. the signature -------------------------------------------------------
    rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern), re.S)
    matches = [m.start() for m in rx.finditer(img.img)]
    check(len(matches) == 1, f"font.rs's signature matches {len(matches)} time(s): {[hex(m) for m in matches]}")
    if len(matches) != 1:
        return False
    found = matches[0]
    check(DRAW[0] <= found < DRAW[1], f"the match +{found:#x} is inside CHudDeathNotice::Draw")

    decoded = {}
    for ins in img.md.disasm(img.img[DRAW[0]:DRAW[1]], img.base + DRAW[0]):
        decoded[ins.address - img.base] = ins

    def text_at(rva):
        ins = decoded.get(rva)
        return (f"{ins.mnemonic} {ins.op_str}", ins.size) if ins else ("not an instruction boundary", 0)

    # -- 2. the rect reads ------------------------------------------------------
    fields = set()
    for offset in reads:
        rva = found + offset
        text, size = text_at(rva)
        m = re.fullmatch(r"mov eax, dword ptr \[(0x[0-9a-f]+)\]", text)
        is_read = bool(m) and size == read_len and img.img[rva] == 0xA1
        check(is_read, f"+{rva:#x} is `mov eax, dword ptr [imm32]` (A1, {read_len} bytes): {text}")
        if is_read:
            fields.add(int(m.group(1), 16) - img.base)
    check(len(fields) == 1, f"both reads name one field: {[hex(f) for f in fields]}")
    rects_field = fields.pop() if len(fields) == 1 else None
    rva = found + sprites_read
    text, size = text_at(rva)
    m = re.fullmatch(r"mov edx, dword ptr \[(0x[0-9a-f]+)\]", text)
    sprites_field = int(m.group(1), 16) - img.base if m else None
    check(m and size == call_len, f"+{rva:#x} is `mov edx, dword ptr [imm32]` ({call_len} bytes): {text}")
    check(rects_field is not None and sprites_field == rects_field - 4,
          f"the sprite handles (+{(sprites_field or 0):#x}) sit four bytes before the rects (+{(rects_field or 0):#x})")
    if rects_field is not None:
        needle = struct.pack("<I", img.base + rects_field)
        refs = [m.start() for m in re.finditer(re.escape(needle), img.img[DRAW[0]:DRAW[1]])]
        refs = [DRAW[0] + r for r in refs]
        want = sorted(found + o + 1 for o in reads)
        check(sorted(refs) == want, f"Draw references m_rgrcRects exactly at the two reads: {[hex(r) for r in refs]}")

    # -- 3. the two sprite calls ------------------------------------------------
    gengfuncs = set()
    for offset, slot, what in ((draw_holes_call, draw_holes_slot, "SPR_DrawHoles"), (spr_set_call, spr_set_slot, "SPR_Set")):
        rva = found + offset
        text, size = text_at(rva)
        m = re.fullmatch(r"call dword ptr \[(0x[0-9a-f]+)\]", text)
        check(m and size == call_len, f"+{rva:#x} ({what}, slot {slot}) is `call dword ptr [imm32]`, {call_len} bytes: {text}")
        if m:
            gengfuncs.add(int(m.group(1), 16) - img.base - 4 * slot)
    check(gengfuncs == {GENGFUNCS_RVA}, f"both calls go through gEngfuncs = client+{GENGFUNCS_RVA:#x}: {[hex(g) for g in gengfuncs]}")
    data = next((s for s in img.pe.sections if s.Name.rstrip(b"\0") == b".data"), None)
    check(data and data.VirtualAddress <= GENGFUNCS_RVA < data.VirtualAddress + data.Misc_VirtualSize, "and it lies in .data")
    cleanup = None
    at = found + draw_holes_call + call_len
    for _ in range(12):
        nxt = decoded.get(at)
        if nxt is None or nxt.mnemonic == "call":
            break
        t = f"{nxt.mnemonic} {nxt.op_str}"
        if t.startswith("add esp, "):
            cleanup = t
            break
        at += nxt.size
    check(cleanup == "add esp, 0x20", f"SPR_Set's and SPR_DrawHoles' arguments are cleaned together by `add esp, 0x20`: {cleanup}")

    # -- 4. gHUD, the field, the count, the stride ------------------------------
    # `mov ecx, imm32; call CHud::X` -- the `this` loads before the loop.
    this_ptrs = []
    for rva, ins in sorted(decoded.items()):
        m = re.fullmatch(r"mov ecx, (0x[0-9a-f]+)", f"{ins.mnemonic} {ins.op_str}")
        nxt = decoded.get(rva + ins.size)
        if rva < found and m and nxt is not None and nxt.mnemonic == "call":
            this_ptrs.append(int(m.group(1), 16) - img.base)
    ghud = this_ptrs[0] if this_ptrs else None
    check(ghud is not None and len(set(this_ptrs)) == 1 and len(this_ptrs) == 2,
          f"Draw passes one `this` to CHud twice before the loop: gHUD = client+{(ghud or 0):#x}")
    check(ghud is not None and rects_field == ghud + rects_in_hud, f"m_rgrcRects is gHUD+{rects_in_hud:#x}")
    vid = {}
    for ins in img.md.disasm(img.img[VIDINIT[0]:VIDINIT[1]], img.base + VIDINIT[0]):
        vid[ins.address - img.base] = f"{ins.mnemonic} {ins.op_str}"
    seq = list(vid.values())
    store = next((i for i, t in enumerate(seq) if t == f"mov dword ptr [esi + {rects_in_hud:#x}], eax"), None)
    window = seq[store - 5:store] if store else []
    check(store is not None and f"mov ecx, dword ptr [esi + {count_in_hud:#x}]" in window and "shl ecx, 4" in window
          and any(t.startswith("call ") for t in window),
          f"VidInit allocates m_rgrcRects as new[16 * m_iSpriteCount] (count at gHUD+{count_in_hud:#x}): {window}")
    handles = next((i for i, t in enumerate(seq) if t == f"mov dword ptr [esi + {rects_in_hud - 4:#x}], eax"), None)
    check(handles is not None and handles < store, "and m_rghSprites, the dword before it, just before")

    # -- 10. the names the even layout reads (#1022) ---------------------------
    names_in_hud = rust_const(scale_src, "NAMES_IN_HUD")
    name_len = rust_const(EVEN_RS.read_text(encoding="utf-8"), "SPRITE_NAME_LEN")
    check(names_in_hud == rects_in_hud + 4, f"m_rgszSpriteNames (gHUD+{names_in_hud:#x}) is the dword after the rects")
    names_store = next((i for i, t in enumerate(seq) if t == f"mov dword ptr [esi + {names_in_hud:#x}], eax"), None)
    window = seq[names_store - 6:names_store] if names_store else []
    stride = name_len == 24 and "lea edx, [eax + eax*2]" in window and "shl edx, 3" in window
    check(names_store is not None and store is not None and names_store > store and stride
          and f"mov eax, dword ptr [esi + {count_in_hud:#x}]" in window and any(t.startswith("call ") for t in window),
          f"VidInit allocates m_rgszSpriteNames as new[{name_len} * m_iSpriteCount] after the rects: {window}")
    copies = [i for i, t in enumerate(seq) if t == f"mov eax, dword ptr [esi + {names_in_hud:#x}]"]
    strncpy = [i for i in copies if f"push {name_len:#x}" in seq[i - 16:i]
               and "push eax" in seq[i:i + 4] and any(t.startswith("call ") for t in seq[i:i + 4])]
    check(len(strncpy) == 1, f"and copies each name into it with a {name_len}-byte strncpy (push {name_len:#x} ... call): "
          f"{[seq[i - 16:i + 4] for i in copies]}")

    # -- 5. the wrect_t field order ---------------------------------------------
    after_first_read = [f"{decoded[r].mnemonic} {decoded[r].op_str}" for r in sorted(decoded) if found + reads[0] < r < found + reads[0] + 0x20]
    check("mov edx, dword ptr [ebx + eax + 4]" in after_first_read and "mov edx, dword ptr [ebx + eax]" in after_first_read,
          "the anchor reads rect+4 (right) and rect+0 (left): wrect_t is {left, right, top, bottom}")
    after_call = [f"{decoded[r].mnemonic} {decoded[r].op_str}" for r in sorted(decoded) if found + draw_holes_call < r < found + draw_holes_call + 0x14]
    check(after_call[:3] == ["mov ecx, dword ptr [esp + 0x6c]", "mov eax, dword ptr [esp + 0x68]", "sub ecx, eax"],
          f"after the icon, x advances by the stack copy's right - left: {after_call[:3]}")

    # -- 6. the vftable ---------------------------------------------------------
    vt = img.vftable("CHudDeathNotice")
    listed = hudelement_vftable(HUDELEMENT_RS.read_text(encoding="utf-8"), ".?AVCHudDeathNotice@@")
    check(vt is not None and vt == listed, f"CHudDeathNotice's vftable is +{(vt or 0):#x} (hudelement.rs says +{(listed or 0):#x})")
    if vt:
        draw = img.u32(vt + 4 * 3) - img.base
        check(draw == DRAW[0], f"its slot 3 (Draw) is +{draw:#x}")
    rets = {f"{i.mnemonic} {i.op_str}".strip() for i in decoded.values() if i.mnemonic == "ret"}
    check(rets == {"ret 4"}, f"Draw returns with {sorted(rets)} (thiscall, one float)")

    # -- 7. the rewrite keeps every boundary ------------------------------------
    spans = [(found + o, read_len) for o in reads] + [(found + draw_holes_call, call_len)]
    for start, length in spans:
        hits = branches_into(decoded, img, start, start + length)
        check(not hits, f"nothing in Draw branches into the operand at +{start:#x} {hits}")
    patched = bytearray(img.img)
    for o in reads:
        patched[found + o + 1:found + o + read_len] = struct.pack("<I", 0x60000000)
    rva = found + draw_holes_call
    patched[rva:rva + call_len] = b"\xe8" + struct.pack("<i", 0x60000000 - (img.base + rva + 5)) + b"\x90"
    before = [i.mnemonic for i in img.md.disasm(img.img[DRAW[0]:DRAW[1]], img.base + DRAW[0])]
    after = [i.mnemonic for i in img.md.disasm(bytes(patched[DRAW[0]:DRAW[1]]), img.base + DRAW[0])]
    strip = lambda ms: [m for m in ms if m != "nop"]  # noqa: E731
    check(after.count("nop") == before.count("nop") + 1 and strip(after) == strip(before),
          "with the three operands rewritten, Draw decodes to the same instructions plus one nop")

    # -- 9. the line-start detour -----------------------------------------------
    line_at = rust_const(scale_src, "LINE_AT")
    line_stolen = rust_byte_list(scale_src, "LINE_STOLEN")
    start = found + line_at
    present = bytes(img.img[start:start + len(line_stolen)])
    check(present == bytes(line_stolen), f"+{start:#x} holds LINE_STOLEN {bytes(line_stolen).hex(' ')}: {present.hex(' ')}")
    span = []
    at = start
    while at < start + len(line_stolen) and at in decoded:
        span.append(f"{decoded[at].mnemonic} {decoded[at].op_str}")
        at += decoded[at].size
    check(span == ["shl ebx, 4", "add esp, 0xc"] and at == start + len(line_stolen),
          f"and decodes as `shl ebx, 4; add esp, 0xc`, ending on a boundary: {span}")
    between = [f"{decoded[r].mnemonic} {decoded[r].op_str}" for r in sorted(decoded) if found <= r < start]
    read_text = f"mov eax, dword ptr [{img.base + rects_field:#x}]" if rects_field is not None else None
    after_read = between[between.index(read_text) + 1:] if read_text in between else None
    check(after_read is not None and all(not re.match(r"\w+ (eax|ebx|ax|al|bx|bl),", t) for t in after_read),
          f"between the first rect read and the span nothing writes eax or ebx: {after_read}")
    loads = [t for t in between if t.startswith("mov ebx, ")]
    pushes = sum(t.startswith("push ") for t in between)
    check(loads == ["mov ebx, dword ptr [esp + 0x28]"] and pushes == 3 and not any(t.startswith("add esp") for t in between),
          f"ebx is [esp+0x28] under three pushes, i.e. the local [esp+0x1c]: {loads}, {pushes} pushes")
    sprite = [r for r in sorted(decoded) if f"{decoded[r].mnemonic} {decoded[r].op_str}" == "mov eax, dword ptr [esp + 0x1c]"
              and found + sprites_read - 0x20 < r < found + sprites_read]
    check(len(sprite) == 1, "the same local [esp+0x1c] is the index SPR_Set's handle lookup reads (the line's sprite)")
    nxt = decoded.get(start + len(line_stolen))
    nxt_text = f"{nxt.mnemonic} {nxt.op_str}" if nxt else ""
    check(nxt_text.startswith("mov edx, "), f"edx, which the stub uses, is written next: {nxt_text}")
    hits = branches_into(decoded, img, start, start + len(line_stolen))
    check(not hits, f"nothing in Draw branches into the line-start span {hits}")
    copies = [f"{decoded[r].mnemonic} {decoded[r].op_str}" for r in sorted(decoded) if found + reads[1] < r < found + sprites_read]
    check("mov eax, dword ptr [ebx + 8]" in copies and "mov ecx, dword ptr [ebx + 0xc]" in copies
          and "mov dword ptr [esp + 0x50], eax" in copies and "mov dword ptr [esp + 0x54], ecx" in copies,
          "Draw copies rect+8 / rect+0xc to the [esp+0x50] / [esp+0x54] its line step subtracts: the stub's top and bottom")
    return ok


def check_installs(games):
    digests = {}
    for game in games:
        dll = game / "dod" / "cl_dlls" / "client.dll"
        if dll.is_file():
            digests[game.name] = hashlib.sha256(dll.read_bytes()).hexdigest()
    same = len(set(digests.values())) <= 1
    print(("  OK   " if same else "  FAIL ") + f"the installs ship one client.dll: {digests}")
    return same


def main():
    args = [Path(a) for a in sys.argv[1:]]
    dll = next((a for a in args if a.suffix.lower() == ".dll"), font.DEFAULT_DLL)
    games = [a for a in args if a.is_dir()] or [g for g in font.DEFAULT_GAMES if g.is_dir()]
    if not dll.is_file():
        return print(f"no client.dll at {dll}") or 2
    ok = check_client(dll)
    print()
    ok &= check_installs(games)
    print("\nALL OK" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
