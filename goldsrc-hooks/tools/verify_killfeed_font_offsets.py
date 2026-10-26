#!/usr/bin/env python3
"""Checks `deathmsg/font.rs` against a real DoD `client.dll`, and its ISurface
slots against both movie installs' `hw.dll` (issue #562).

`font.rs` rewrites six `call [gEngfuncs+N]` instructions in
`CHudDeathNotice::Draw` into `call rel32; nop`, and detours the 16-byte line
step after them. The signature it scans for covers only the first site; the
other five, the `SPR_Set` call it reads as an anchor and the step span are
offsets from the match (the `SPR_DrawHoles` call beside it is `scale.rs`'s,
checked by `verify_killfeed_scale_offsets.py`). Nothing at runtime can prove those offsets land on
whole instructions of the kind the Rust assumes, so this does:

  client.dll
  1. The signature matches exactly once.
  2. Every site in `SITES` decodes as `call dword ptr [imm32]` and the six
     agree, with the two `ANCHORS`, on one `gEngfuncs` -- each site's
     immediate is `gEngfuncs + 4*slot` for the slot the table names -- and
     each is followed by `add esp, 0xc` (three cdecl arguments, as the Rust
     signatures have).
  3. `gEngfuncs` is where `engine.rs`'s client-cmd watch expects it:
     `client.dll`'s own copy of the table, in `.data`.
  4. The step span holds `STEP_STOLEN`, nothing in `Draw` branches into its
     middle, and it is long enough for the jump.
  5. With the six sites rewritten, `Draw` decodes to the same instruction
     sequence plus six `nop`s.

  hw.dll (each install present)
  6. `VGUI_Surface026`'s factory returns one static `BaseUISurface`, whose
     vftable has, at the slots `font.rs` names, functions popping the
     argument bytes those calls push (`ret 4` for DrawSetTextFont, `ret 0x10`
     for the four-int DrawSetTextColor, `ret 8` for DrawSetTextPos and
     DrawPrintText, `ret 0x24` for AddGlyphSetToFont, ...).
  7. `SupportsFeature(2)` (drop-shadow fonts) returns true.
  8. The width `font.rs` sums is the one the draw advances by (#1052):
     `DrawPrintText` calls only `DrawUnicodeChar` (slot 19), once per
     character, and that calls `GetCharABCwide` (`GET_CHAR_ABC_WIDE`).

Usage:
    python goldsrc-hooks/tools/verify_killfeed_font_offsets.py [client.dll] [game-folder ...]

Defaults to the pre-Anniversary movies install's `client.dll` and both movie
installs. Needs `pip install pefile capstone`.
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

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_GAMES = [
    STEAM / "Half-Life - PRE-Anniversary for Movies",
    STEAM / "Half-Life - POST-Anniversary for Movies",
]
DEFAULT_DLL = DEFAULT_GAMES[0] / "dod" / "cl_dlls" / "client.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "deathmsg" / "font.rs"

# `CHudDeathNotice::Draw`, for the branch scan (verify_deathmsg_offsets.py's bounds).
DRAW = (0x2AE90, 0x2B198)
# `gEngfuncs` in client.dll: the table `Initialize` copies the engine's into.
GENGFUNCS_RVA = 0x176388
# What each ISurface slot `font.rs` names must return with (thiscall: `ret` pops the arguments).
SURFACE_RETS = {
    "DRAW_SET_TEXT_FONT": "ret 4",
    "DRAW_SET_TEXT_COLOR_RGBA": "ret 0x10",
    "DRAW_SET_TEXT_POS": "ret 8",
    "DRAW_PRINT_TEXT": "ret 8",
    "DRAW_FLUSH_TEXT": "ret",
    "CREATE_FONT": "ret",
    "ADD_GLYPH_SET_TO_FONT": "ret 0x24",
    "GET_FONT_TALL": "ret 4",
    "GET_CHAR_ABC_WIDE": "ret 0x14",
}
SUPPORTS_FEATURE_SLOT = 50
DRAW_UNICODE_CHAR_SLOT = 19
DROPSHADOW_FONTS = 2


def num(text):
    return int(text.replace("_", ""), 0)


def rust_const(src, name):
    match = re.search(rf"const {name}: \w+ = (0x[0-9a-fA-F_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}` in font.rs")
    return num(match.group(1))


def rust_pattern(src):
    body = re.search(r'const PATTERN: &str = "(.*?)";', src, re.S).group(1)
    return body.replace("\\", "").split()


def rust_sites(src):
    body = re.search(r"const SITES: &\[\(usize, Call\)\] = &\[(.*?)\];", src, re.S).group(1)
    return [(num(o), c) for o, c in re.findall(r"\((0x[0-9a-fA-F_]+), Call::(\w+)\)", body)]


def rust_slots(src):
    body = re.search(r"impl Call \{.*?fn slot.*?\{(.*?)\n        \}", src, re.S).group(1)
    # An arm may name several calls: `Call::Len | Call::VictimLen => 29`.
    return {c: int(s) for arm, s in re.findall(r"((?:Call::\w+(?: \| )?)+) => (\d+)", body)
            for c in re.findall(r"Call::(\w+)", arm)}


def rust_anchors(src):
    body = re.search(r"const ANCHORS: &\[\(usize, usize\)\] = &\[(.*?)\];", src, re.S).group(1)
    return [(num(o), int(s)) for o, s in re.findall(r"\((0x[0-9a-fA-F_]+), (\d+)\)", body)]


def rust_bytes(src, name):
    body = re.search(rf"const {name}: &\[u8\] = &\[(.*?)\];", src, re.S).group(1)
    return bytes(int(b, 16) for b in re.findall(r"0x([0-9a-fA-F]{2})", body))


def rust_surface_slots(src):
    return {n: int(v) for n, v in re.findall(r"const SURFACE_(\w+): usize = (\d+);", src)}


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path))
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.detail = True
        self.md.skipdata = True
        code = next(s for s in self.pe.sections if s.Characteristics & 0x20000000)
        self.code = (code.VirtualAddress, code.VirtualAddress + code.Misc_VirtualSize)

    def identity(self):
        return self.pe.FILE_HEADER.TimeDateStamp, self.pe.OPTIONAL_HEADER.SizeOfImage

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def refs(self, rva):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.img)]

    def text(self, rva, limit=0x200):
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            out.append(f"{ins.mnemonic} {ins.op_str}".strip())
            if ins.mnemonic == "ret":
                break
        return out

    def ret_of(self, rva, depth=0):
        """The `ret` a slot's function ends with, so its argument bytes.

        Follows a direct `jmp` (the font slots are `call FontManager(); mov ecx,
        eax; jmp CFontManager::X` thunks) and, for the drawing slots that end
        in `mov ecx, [ecx+0xc]; ...; jmp dword ptr [reg+N]` -- a tail call into
        the inner engine surface -- the `ret` of slot N/4 in every
        `*EngineSurface*` implementation, which must agree."""
        for ins in self.md.disasm(self.img[rva:rva + 0x200], self.base + rva):
            t = f"{ins.mnemonic} {ins.op_str}".strip()
            if ins.mnemonic == "ret":
                return t
            if ins.mnemonic == "jmp":
                m = re.fullmatch(r"jmp (0x[0-9a-f]+)", t)
                if m:
                    return self.ret_of(int(m.group(1), 16) - self.base, depth + 1) if depth < 3 else None
                m = re.fullmatch(r"jmp dword ptr \[e\w\w \+ (0x[0-9a-f]+)\]", t)
                if m:
                    return self.inner_ret(int(m.group(1), 16) // 4)
                return None
        return None

    def vftable(self, cls):
        name = self.img.find(f".?AV{cls}@@\0".encode())
        if name < 0:
            return None
        for hit in self.refs(name - 8):
            col = hit - 12
            if self.u32(col) == 0 and self.u32(col + 4) == 0:
                refs = self.refs(col)
                if refs:
                    return refs[0] + 4
        return None

    def inner_classes(self):
        """The object behind `BaseUISurface`'s `this+0xc`: `EngineSurface`, the
        engine's own implementation. `EngineSurfaceWrap` also exists, but it is
        the wrapper handed to vgui2 clients with its own, different slot
        layout (it forwards through a pointer at its +0x28), so it is not what
        these thunks jump into -- every thunked slot's expected size matches
        `EngineSurface`'s and not the wrapper's."""
        return [c.decode() for c in (b"EngineSurface",) if self.vftable(c.decode())]

    def inner_ret(self, slot):
        rets = {}
        for cls in self.inner_classes():
            vt = self.vftable(cls)
            if vt:
                rets[cls] = self.ret_of(self.u32(vt + 4 * slot) - self.base, 1)
        answers = set(rets.values())
        if len(answers) == 1:
            return answers.pop()
        return f"inner slot {slot} disagrees: {rets}"

    def all_text(self, rva, limit):
        """Every instruction in `limit` bytes, through `ret`s, up to padding."""
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            if ins.mnemonic in ("int3",) or (ins.mnemonic == "nop" and out and out[-1].startswith("ret")):
                break
            out.append(f"{ins.mnemonic} {ins.op_str}".strip())
        return out

    def factory_object(self, interface):
        for name in (m.start() for m in re.finditer(re.escape(interface.encode() + b"\0"), self.img)):
            for ref in self.refs(name):
                if self.img[ref - 1] == 0x68 and self.img[ref + 4] == 0x68:
                    factory = self.text(self.u32(ref + 5) - self.base, 0x10)
                    m = re.fullmatch(r"mov eax, (0x[0-9a-f]+)", factory[0])
                    if m and factory[1] == "ret":
                        return int(m.group(1), 16) - self.base
        return None

    def surface_vftable(self):
        obj = self.factory_object("VGUI_Surface026")
        if obj is None:
            return None, None
        for m in re.finditer(re.escape(b"\xb9" + struct.pack("<I", self.base + obj)), self.img):
            nxt = self.text(m.start(), 0x10)
            target = next((re.fullmatch(r"(?:jmp|call) (0x[0-9a-f]+)", t) for t in nxt[1:2]), None)
            if target:
                for t in self.text(int(target.group(1), 16) - self.base, 0x60):
                    s = re.fullmatch(r"mov dword ptr \[e\w\w\], (0x[0-9a-f]+)", t)
                    if s:
                        return obj, int(s.group(1), 16) - self.base
        return obj, None

    def class_of_vftable(self, vt):
        locator = self.u32(vt - 4) - self.base
        at = self.u32(locator + 12) - self.base + 8
        return self.img[at:self.img.index(b"\0", at)].decode()


def check_client(dll, src):
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("  OK   " if passed else "  FAIL ") + message)

    print(f"\n==== {dll} ====")
    img = Image(dll)
    pattern = rust_pattern(src)
    sites = rust_sites(src)
    slots = rust_slots(src)
    anchors = rust_anchors(src)
    call_len = rust_const(src, "CALL_LEN")
    step_at = rust_const(src, "STEP_AT")
    stolen = rust_bytes(src, "STEP_STOLEN")

    # -- 1. the signature -------------------------------------------------------
    rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)])) for t in pattern), re.S)
    matches = [m.start() for m in rx.finditer(img.img)]
    check(len(matches) == 1, f"the signature matches {len(matches)} time(s): {[hex(m) for m in matches]}")
    if len(matches) != 1:
        return False
    found = matches[0]
    check(DRAW[0] <= found < DRAW[1], f"the match +{found:#x} is inside CHudDeathNotice::Draw")

    # -- 2. the sites -----------------------------------------------------------
    decoded = {}
    for ins in img.md.disasm(img.img[DRAW[0]:DRAW[1]], img.base + DRAW[0]):
        decoded[ins.address - img.base] = ins
    gengfuncs = set()
    for offset, call in sites + [(o, f"anchor slot {s}") for o, s in anchors]:
        slot = slots[call] if call in slots else int(call.split()[-1])
        rva = found + offset
        ins = decoded.get(rva)
        text = f"{ins.mnemonic} {ins.op_str}" if ins else "not an instruction boundary"
        m = re.fullmatch(r"call dword ptr \[(0x[0-9a-f]+)\]", text)
        is_call = bool(m) and ins.size == call_len
        check(is_call, f"+{rva:#x} ({call}) is `call dword ptr [imm32]`, {call_len} bytes: {text}")
        if not is_call:
            continue
        g = int(m.group(1), 16) - img.base - 4 * slot
        gengfuncs.add(g)
        if call in slots:
            # The caller's cleanup: the next `add esp, imm` after the call,
            # before any other call (the width calls' comes a few loads later).
            cleanup = None
            at = rva + call_len
            for _ in range(8):
                nxt = decoded.get(at)
                if nxt is None:
                    break
                t = f"{nxt.mnemonic} {nxt.op_str}"
                if nxt.mnemonic == "call":
                    break
                if t.startswith("add esp, "):
                    cleanup = t
                    break
                at += nxt.size
            check(cleanup == "add esp, 0xc", f"    cleaned up by `add esp, 0xc` (three cdecl arguments): {cleanup}")
    check(len(gengfuncs) == 1, f"the sites and anchors agree on one gEngfuncs: {[hex(g) for g in gengfuncs]}")

    # -- 3. gEngfuncs -----------------------------------------------------------
    if len(gengfuncs) == 1:
        g = gengfuncs.pop()
        check(g == GENGFUNCS_RVA, f"gEngfuncs is client+{g:#x} (expected +{GENGFUNCS_RVA:#x}, client.dll's own copy of the table)")
        data = next((s for s in img.pe.sections if s.Name.rstrip(b"\0") == b".data"), None)
        in_data = data and data.VirtualAddress <= g < data.VirtualAddress + data.Misc_VirtualSize
        check(in_data, "and it lies in .data")

    # -- 4. the step span -------------------------------------------------------
    step = found + step_at
    present = img.img[step:step + len(stolen)]
    check(present == stolen, f"+{step:#x} holds the {len(stolen)} stolen bytes ({present.hex(' ')})")
    check(step in decoded and (step + len(stolen)) in decoded, "the span starts and ends on instruction boundaries")
    inside = []
    for ins in decoded.values():
        if not ({capstone.x86.X86_GRP_JUMP, capstone.x86.X86_GRP_CALL} & set(ins.groups)):
            continue
        for op in ins.operands:
            if op.type == capstone.x86.X86_OP_IMM:
                t = op.imm - img.base
                if step < t < step + len(stolen):
                    inside.append((ins.address - img.base, t))
    check(not inside, f"nothing in Draw branches into the span's middle {inside}")
    check(len(stolen) >= 5, f"{len(stolen)} bytes leaves room for the 5-byte jump")
    # And nothing branches into the middle of any rewritten site either.
    for offset, call in sites:
        rva = found + offset
        hits = [(a, op.imm - img.base) for a, ins in decoded.items()
                for op in ins.operands
                if ({capstone.x86.X86_GRP_JUMP, capstone.x86.X86_GRP_CALL} & set(ins.groups))
                and op.type == capstone.x86.X86_OP_IMM and rva < op.imm - img.base < rva + call_len]
        check(not hits, f"nothing branches into the {call} site at +{rva:#x}")

    # -- 5. the rewritten image still decodes -----------------------------------
    patched = bytearray(img.img)
    for offset, _ in sites:
        rva = found + offset
        patched[rva:rva + call_len] = b"\xe8" + struct.pack("<i", 0x60000000 - (img.base + rva + 5)) + b"\x90"
    before = [i.mnemonic for i in img.md.disasm(img.img[DRAW[0]:DRAW[1]], img.base + DRAW[0])]
    after = [i.mnemonic for i in img.md.disasm(bytes(patched[DRAW[0]:DRAW[1]]), img.base + DRAW[0])]
    without_nops = lambda ms: [m for m in ms if m != "nop"]  # noqa: E731
    check(after.count("nop") == before.count("nop") + len(sites) and without_nops(after) == without_nops(before),
          f"with the {len(sites)} sites rewritten, Draw decodes to the same instructions plus {len(sites)} nops")
    return ok


def check_hw(game, src):
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("  OK   " if passed else "  FAIL ") + message)

    print(f"\n==== {game / 'hw.dll'} ====")
    hw = Image(game / "hw.dll")
    print(f"  hw.dll {hw.identity()[0]:#x}/{hw.identity()[1]:#x}")
    obj, vt = hw.surface_vftable()
    check(obj is not None and vt is not None, f"VGUI_Surface026's factory returns one static object (+{(obj or 0):#x}), vftable +{(vt or 0):#x}")
    if not vt:
        return False
    cls = hw.class_of_vftable(vt)
    check(cls == ".?AVBaseUISurface@@", f"its constructor stores the {cls} vftable")
    print(f"  inner engine surface classes: {hw.inner_classes()}")
    for name, slot in rust_surface_slots(src).items():
        fn = hw.u32(vt + 4 * slot) - hw.base
        got = hw.ret_of(fn)
        want = SURFACE_RETS.get(name)
        check(got == want, f"ISurface slot {slot} ({name}) +{fn:#x} returns with {got!r} (want {want!r})")
    # SupportsFeature(2): both builds answer features 1 and 2 with `mov al, 1`
    # (a `dec eax` ladder on PRE, a jump table on POST) and the rest with 0.
    fn = hw.u32(vt + 4 * SUPPORTS_FEATURE_SLOT) - hw.base
    body = hw.all_text(fn, 0x40)
    rets = {t for t in body if t.startswith("ret")}
    check(rets == {"ret 4"}, f"ISurface slot {SUPPORTS_FEATURE_SLOT} (SupportsFeature) +{fn:#x} takes one argument: {sorted(rets)}")
    check("mov al, 1" in body and "xor al, al" in body,
          "SupportsFeature answers some features with `mov al, 1` and the rest with 0 (drop-shadow fonts, feature 2, are a yes on both builds -- see docs)")
    # What the draw advances by is what `text_width` sums (#1052).
    slots = rust_surface_slots(src)
    vcalls = lambda body: [int(m.group(1), 16) // 4 for t in body  # noqa: E731
                           for m in [re.fullmatch(r"call dword ptr \[e\w\w \+ (0x[0-9a-f]+)\]", t)] if m]
    fn = hw.u32(vt + 4 * slots["DRAW_PRINT_TEXT"]) - hw.base
    got = vcalls(hw.all_text(fn, 0x60))
    check(got == [DRAW_UNICODE_CHAR_SLOT],
          f"DrawPrintText +{fn:#x} makes one virtual call, DrawUnicodeChar (slot {DRAW_UNICODE_CHAR_SLOT}), in its loop: {got}")
    fn = hw.u32(vt + 4 * DRAW_UNICODE_CHAR_SLOT) - hw.base
    got = vcalls(hw.all_text(fn, 0x200))
    check(slots["GET_CHAR_ABC_WIDE"] in got,
          f"DrawUnicodeChar +{fn:#x} calls GetCharABCwide (slot {slots['GET_CHAR_ABC_WIDE']}): {got}")
    return ok


def main():
    args = [Path(a) for a in sys.argv[1:]]
    dll = next((a for a in args if a.suffix.lower() == ".dll"), DEFAULT_DLL)
    games = [a for a in args if a.is_dir()] or [g for g in DEFAULT_GAMES if g.is_dir()]
    src = RUST.read_text(encoding="utf-8")
    if not dll.is_file():
        return print(f"no client.dll at {dll}") or 2
    ok = check_client(dll, src)
    for game in games:
        ok &= check_hw(game, src)
    print("\nALL OK" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
