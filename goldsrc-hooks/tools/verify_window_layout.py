#!/usr/bin/env python3
"""Checks `window_layout.rs` against both movie installs (issue #408).

The module calls vgui2's `VGUI_Panel007` and the engine's `VGUI_Surface026`
by vftable slot, and GameUI's `Frame::SetSizeable` / `Frame::IsSizeable` by
address. Nothing at runtime can prove those are right, so this does, for every
build in the module's tables:

  GameUI.dll
  1. Its identity (PE timestamp, image size) is in `GAMEUI_BUILDS`.
  2. `is_sizeable` is `mov al, byte ptr [ecx + X]; ret`, and it is what
     `Frame@vgui2`'s vftable holds at `FRAME_SLOT_IS_SIZEABLE`.
  3. `set_sizeable` stores its one argument's low byte to that same
     `[ecx + X]`, calls a function that reads the flag back through vftable
     slot `FRAME_SLOT_IS_SIZEABLE` (setting up the resize grips), and pops 4
     bytes: `Frame::SetSizeable(bool)`. Dialog constructors call it, most with
     0 -- which is why they aren't resizable.

  vgui2.dll
  4. Its identity is in `VGUI2_BUILDS`; "VGUI_Panel007"'s factory returns one
     static object; `VPanelWrapper`'s vftable (RTTI) has the slot shapes the
     module calls: SetPos/GetPos/SetSize/GetSize take three arguments
     (`ret 0xc`), IsVisible/GetName/GetModuleName one (`ret 4`), GetPanel two
     (`ret 8`).

  hw.dll
  5. Its identity is in `HW_BUILDS`; "VGUI_Surface026"'s factory returns one
     static `BaseUISurface`, whose vftable has GetScreenSize taking two
     arguments (`ret 8`), GetPopupCount none (`ret`) and GetPopup one
     (`ret 4`).

Every constant comes out of `window_layout.rs`.

Usage:
    python goldsrc-hooks/tools/verify_window_layout.py [game-folder ...]

Needs `pip install pefile capstone`.
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
RUST = Path(__file__).resolve().parent.parent / "src" / "window_layout.rs"


def num(text):
    return int(text.replace("_", ""), 0)


def rust_usize(src, name):
    match = re.search(rf"const {name}: usize = (0x[0-9a-f_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}` in window_layout.rs")
    return num(match.group(1))


def rust_table(src, table):
    body = re.search(rf"pub const {table}: \[\w+; \d+\] = \[(.*?)\n\];", src, re.S).group(1)
    rows = []
    for block in re.findall(r"\{(.*?)\}", body, re.S):
        row = dict(re.findall(r"(\w+): (0x[0-9a-f_]+|\"[^\"]*\")", block))
        rows.append({k: (v.strip('"') if v.startswith('"') else num(v)) for k, v in row.items()})
    return rows


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path))
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.skipdata = True
        code = next(s for s in self.pe.sections if s.Characteristics & 0x20000000)
        self.code = (code.VirtualAddress, code.VirtualAddress + code.Misc_VirtualSize)

    def identity(self):
        return self.pe.FILE_HEADER.TimeDateStamp, self.pe.OPTIONAL_HEADER.SizeOfImage

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def body(self, rva, limit=0x200):
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            out.append(f"{ins.mnemonic} {ins.op_str}".strip())
            if ins.mnemonic == "ret":
                break
        return out

    def last_ret(self, rva, limit=0x1000):
        """The operand of the function's first `ret` (a stand-in for its stack cleanup)."""
        for t in self.body(rva, limit):
            if t.startswith("ret"):
                return t
        return None

    def refs(self, rva):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.img)]

    def vftable(self, cls):
        name = self.img.find(f".?AV{cls}@@\0".encode())
        for hit in self.refs(name - 8):
            col = hit - 12
            if self.u32(col) == 0 and self.u32(col + 4) == 0:
                refs = self.refs(col)
                if refs:
                    return refs[0] + 4
        return None

    def factory_object(self, interface):
        # The name can appear more than once (the pre-Anniversary hw.dll has it
        # three times); only one copy is pushed by an interface registration.
        for name in (m.start() for m in re.finditer(re.escape(interface.encode() + b"\0"), self.img)):
            for ref in self.refs(name):
                if self.img[ref - 1] == 0x68 and self.img[ref + 4] == 0x68:
                    factory = self.body(self.u32(ref + 5) - self.base, 0x10)
                    m = re.fullmatch(r"mov eax, (0x[0-9a-f]+)", factory[0])
                    if m and factory[1] == "ret":
                        return int(m.group(1), 16) - self.base
        return None

    def calls_to(self, target):
        lo, hi = self.code
        sites = []
        i = lo
        while True:
            i = self.img.find(b"\xe8", i, hi)
            if i < 0:
                return sites
            if i + 5 + struct.unpack_from("<i", self.img, i + 1)[0] == target:
                sites.append(i)
            i += 1


def verify(game, src):
    slot = rust_usize(src, "FRAME_SLOT_IS_SIZEABLE")
    panel_slots = {n: rust_usize(src, f"PANEL_SLOT_{n}") for n in
                   ("SET_POS", "GET_POS", "SET_SIZE", "GET_SIZE", "IS_VISIBLE", "GET_NAME", "GET_PANEL", "GET_MODULE_NAME")}
    panel_rets = {"SET_POS": "ret 0xc", "GET_POS": "ret 0xc", "SET_SIZE": "ret 0xc", "GET_SIZE": "ret 0xc",
                  "IS_VISIBLE": "ret 4", "GET_NAME": "ret 4", "GET_PANEL": "ret 8", "GET_MODULE_NAME": "ret 4"}
    surface_slots = {n: rust_usize(src, f"SURFACE_SLOT_{n}") for n in ("GET_SCREEN_SIZE", "GET_POPUP_COUNT", "GET_POPUP")}
    surface_rets = {"GET_SCREEN_SIZE": "ret 8", "GET_POPUP_COUNT": "ret", "GET_POPUP": "ret 4"}
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")

    # -- GameUI ----------------------------------------------------------------
    ui = Image(game / "valve" / "cl_dlls" / "GameUI.dll")
    build = next((b for b in rust_table(src, "GAMEUI_BUILDS")
                  if (b["time_date_stamp"], b["size_of_image"]) == ui.identity()), None)
    check(build, f"GameUI.dll {ui.identity()[0]:#x}/{ui.identity()[1]:#x} is in GAMEUI_BUILDS ({build and build['name']})")
    if build:
        getter = ui.body(build["is_sizeable"], 0x10)
        m = re.fullmatch(r"mov al, byte ptr \[ecx \+ (0x[0-9a-f]+)\]", getter[0])
        field = m.group(1) if m else None
        check(field and getter[1] == "ret", f"is_sizeable +{build['is_sizeable']:#x} returns the byte at this+{field}")
        frame = ui.vftable("Frame@vgui2")
        held = ui.u32(frame + 4 * slot) - ui.base if frame else None
        check(held == build["is_sizeable"], f"and is what Frame@vgui2's vftable holds at slot {slot} (+{(held or 0):#x})")

        setter = ui.body(build["set_sizeable"], 0x20)
        stores = any(re.fullmatch(rf"mov byte ptr \[ecx \+ {field}\], al", t) for t in setter)
        reads_arg = any(re.fullmatch(r"mov al, byte ptr \[(esp \+ 4|ebp \+ 8)\]", t) for t in setter)
        check(stores and reads_arg and setter[-1] == "ret 4",
              f"set_sizeable +{build['set_sizeable']:#x} stores its argument to this+{field} and pops 4 bytes")
        callee = next((int(t.split()[1], 16) - ui.base for t in setter if re.fullmatch(r"call 0x[0-9a-f]+", t)), None)
        cursors = ui.body(callee, 0x40) if callee else []
        check(any(f"+ {slot * 4:#x}]" in t for t in cursors),
              f"and then calls +{(callee or 0):#x}, which reads the flag back through slot {slot} (the resize grips)")
        callers = ui.calls_to(build["set_sizeable"])
        with_zero = [c for c in callers if ui.img[c - 4:c] == b"\x6a\x00\x8b\xce" or ui.img[c - 2:c] == b"\x6a\x00"]
        check(len(callers) >= 5, f"{len(callers)} constructors call it; {len(with_zero)} of them visibly with 0")

    # -- vgui2 -----------------------------------------------------------------
    vg = Image(game / "vgui2.dll")
    vbuild = next((b for b in rust_table(src, "VGUI2_BUILDS")
                   if (b["time_date_stamp"], b["size_of_image"]) == vg.identity()), None)
    check(vbuild, f"vgui2.dll {vg.identity()[0]:#x}/{vg.identity()[1]:#x} is in VGUI2_BUILDS ({vbuild and vbuild['name']})")
    check(vg.factory_object("VGUI_Panel007") is not None, "VGUI_Panel007's factory returns one static object")
    vt = vg.vftable("VPanelWrapper")
    check(vt, f"VPanelWrapper's vftable is +{(vt or 0):#x}")
    if vt:
        for n, s in panel_slots.items():
            got = vg.last_ret(vg.u32(vt + 4 * s) - vg.base)
            check(got == panel_rets[n], f"IPanel slot {s} ({n}) returns with {got!r}")

    # -- hw --------------------------------------------------------------------
    hw = Image(game / "hw.dll")
    hbuild = next((b for b in rust_table(src, "HW_BUILDS")
                   if (b["time_date_stamp"], b["size_of_image"]) == hw.identity()), None)
    check(hbuild, f"hw.dll {hw.identity()[0]:#x}/{hw.identity()[1]:#x} is in HW_BUILDS ({hbuild and hbuild['name']})")
    obj = hw.factory_object("VGUI_Surface026")
    check(obj is not None, f"VGUI_Surface026's factory returns one static object (+{(obj or 0):#x})")
    surface_vt = None
    if obj is not None:
        for m in re.finditer(re.escape(b"\xb9" + struct.pack("<I", hw.base + obj)), hw.img):
            nxt = hw.body(m.start(), 0x10)
            target = next((re.fullmatch(r"(?:jmp|call) (0x[0-9a-f]+)", t) for t in nxt[1:2]), None)
            if target:
                for t in hw.body(int(target.group(1), 16) - hw.base, 0x60):
                    s = re.fullmatch(r"mov dword ptr \[e\w\w\], (0x[0-9a-f]+)", t)
                    if s:
                        surface_vt = int(s.group(1), 16) - hw.base
                        break
            if surface_vt:
                break
    cls = None
    if surface_vt:
        locator = hw.u32(surface_vt - 4) - hw.base
        at = hw.u32(locator + 12) - hw.base + 8
        cls = hw.img[at:hw.img.index(b"\0", at)].decode()
    check(cls == ".?AVBaseUISurface@@", f"its constructor stores the {cls} vftable (+{(surface_vt or 0):#x})")
    if surface_vt:
        for n, s in surface_slots.items():
            got = hw.last_ret(hw.u32(surface_vt + 4 * s) - hw.base)
            check(got == surface_rets[n], f"ISurface slot {s} ({n}) returns with {got!r}")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or [g for g in DEFAULT_GAMES if g.is_dir()]
    src = RUST.read_text(encoding="utf-8")
    ok = all([verify(g, src) for g in games])
    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
