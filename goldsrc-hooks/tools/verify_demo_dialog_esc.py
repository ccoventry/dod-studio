#!/usr/bin/env python3
"""Checks `demo_dialog_esc.rs` against the real `GameUI.dll`s (issue #369).

`demo_dialog_esc.rs` repoints one slot of `CDemoPlayerDialog`'s vftable in
the 25th Anniversary GameUI at a stub that swallows ESC. The unit test
proves the stub assembles as documented; this proves what it relies on:

  1. `VFTABLE_RVA` is `CDemoPlayerDialog`'s vftable (by its RTTI).
  2. Its slot `SLOT` holds the function `PATTERN` finds, exactly once, in
     the code section, and the dialog inherits it: `Frame@vgui2`'s own
     vftable has the same function in the same slot, so nothing else of the
     dialog's handles keys first.
  3. That function is `Frame::OnKeyCodeTyped` as described: `thiscall`
     (`this` from `ecx`, every return `ret 4`), comparing its argument
     `[ebp+8]` with `KEY_ESCAPE`, and pushing `"CloseFrameButtonPressed"`
     at `CLOSE_PUSH_OFFSET`.
  4. The pre-Anniversary GameUI fails check 1, so the module refuses there,
     and its own `Frame::OnKeyCodeTyped` pushes no such message: nothing to
     fix.

Every constant comes out of `demo_dialog_esc.rs` rather than being restated
here, for the reason in `verify_deathmsg_offsets.py`.

Usage:
    python goldsrc-hooks/tools/verify_demo_dialog_esc.py [anniversary-GameUI.dll [pre-GameUI.dll]]

Defaults to the stock (Anniversary) install and the pre-Anniversary movies
install. Needs `pip install pefile capstone`.
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
ANNIVERSARY = STEAM / "Half-Life" / "valve" / "cl_dlls" / "GameUI.dll"
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "valve" / "cl_dlls" / "GameUI.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_dialog_esc.rs"


def rust_const(src, name):
    match = re.search(rf"const {name}: [^=]+= (.*?);", src, re.S)
    if not match:
        raise SystemExit(f"could not find `const {name}` in demo_dialog_esc.rs")
    return match.group(1).strip()


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path), fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        code = next(s for s in self.pe.sections if s.Characteristics & 0x20000000)
        self.code = (code.VirtualAddress, code.VirtualAddress + code.Misc_VirtualSize)

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def rva(self, va):
        return va - self.base

    def class_at(self, vftable):
        """The RTTI name vftable[-1] leads to, or None."""
        try:
            locator = self.rva(self.u32(vftable - 4))
            if not 0 <= locator < len(self.img) or self.u32(locator) != 0:
                return None
            name = self.rva(self.u32(locator + 12)) + 8
            end = self.img.index(b"\0", name)
            return self.img[name:end].decode("ascii")
        except (struct.error, ValueError, UnicodeDecodeError):
            return None

    def vftable_of(self, cls):
        """The primary (offset 0) vftable of `.?AV<cls>@@`."""
        td = self.img.find(b".?AV" + cls.encode() + b"@@\0") - 8
        for col in range(0, len(self.img) - 20, 4):
            if self.u32(col + 12) == self.base + td and self.u32(col) == 0 and self.u32(col + 4) == 0:
                at = self.img.find(struct.pack("<I", self.base + col))
                if at >= 0:
                    return at + 4
        return None

    def matches(self, pattern):
        rx = re.compile(b"".join(b"." if t == "??" else re.escape(bytes([int(t, 16)]))
                                 for t in pattern.split()), re.S)
        lo, hi = self.code
        return [lo + m.start() for m in rx.finditer(self.img[lo:hi])]


def main():
    anni_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ANNIVERSARY
    pre_path = Path(sys.argv[2]) if len(sys.argv) > 2 else PRE
    src = RUST.read_text(encoding="utf-8")
    vftable = int(rust_const(src, "VFTABLE_RVA"), 0)
    slot = int(rust_const(src, "SLOT"), 0)
    key = int(rust_const(src, "KEY_ESCAPE"), 0)
    push_offset = int(rust_const(src, "CLOSE_PUSH_OFFSET"), 0)
    cls = rust_const(src, "CLASS").strip('"')
    pattern = " ".join(rust_const(src, "PATTERN").replace('"', " ").replace("\\", " ").split())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"== {anni_path} ==")
    img = Image(anni_path)
    check(img.class_at(vftable) == cls, f"+{vftable:#x} is {cls}'s vftable (RTTI says {img.class_at(vftable)!r})")
    target = img.rva(img.u32(vftable + 4 * slot))
    found = img.matches(pattern)
    check(len(found) == 1, f"the signature matches exactly once in the code section: {[hex(f) for f in found]}")
    check(found == [target], f"slot {slot} holds that function (+{target:#x})")
    frame = img.vftable_of("Frame@vgui2")
    frame_slot = img.rva(img.u32(frame + 4 * slot)) if frame else None
    check(frame_slot == target, f"inherited: Frame@vgui2's slot {slot} is the same function (+{frame_slot or 0:#x})")

    body = []
    for ins in md.disasm(img.img[target:target + 0x200], img.base + target):
        body.append(ins)
        if ins.mnemonic == "int3":
            break
    text = [f"{i.mnemonic} {i.op_str}" for i in body]
    check("mov esi, ecx" in text, "`this` comes in ecx (thiscall)")
    rets = [t for t in text if t.startswith("ret")]
    check(rets and all(t == "ret 4" for t in rets), f"every return pops one argument: {rets}")
    check(f"cmp dword ptr [ebp + 8], {key:#x}" in text, f"it compares its argument with KEY_ESCAPE ({key:#x})")
    push = target + push_offset
    pushed = img.rva(img.u32(push + 1)) if img.img[push] == 0x68 else None
    message = img.img[pushed:img.img.index(b"\0", pushed)] if pushed else b""
    check(message == b"CloseFrameButtonPressed", f"+{push:#x} pushes {message.decode(errors='replace')!r}")

    print(f"\n== {pre_path} ==")
    pre = Image(pre_path)
    check(pre.class_at(vftable) != cls, f"+{vftable:#x} is not {cls} here ({pre.class_at(vftable)!r}): the module refuses")
    pre_frame = pre.vftable_of("Frame@vgui2")
    pre_fn = pre.rva(pre.u32(pre_frame + 4 * slot))
    close = pre.img.find(b"CloseFrameButtonPressed\0")
    pushes_close = close >= 0 and struct.pack("<BI", 0x68, pre.base + close) in pre.img[pre_fn:pre_fn + 0x100]
    check(not pushes_close, f"its Frame::OnKeyCodeTyped (+{pre_fn:#x}) doesn't post CloseFrameButtonPressed: nothing to fix")

    print("\nVERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
