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
