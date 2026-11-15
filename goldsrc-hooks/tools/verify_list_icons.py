#!/usr/bin/env python3
"""Checks `studio_panel/hook/list_icons.rs` against the real `GameUI.dll`,
`vgui2.dll` and `hw.dll` of both movie installs (#779).

The module hands the Demos tab's `ListPanel` an `ImageList` it lays out
itself, full of its own `IImage` objects, and paints them through the
surface. Nothing at runtime can prove the slots and layouts, so this does,
for each install:

  GameUI.dll
  1. `ListPanel`'s vftable (RTTI) has `LIST_SLOT_SET_IMAGE_LIST` as
     `SetImageList(ImageList *, bool)`: `ret 8`, stores a byte and a dword
     into `this`, and when the byte was set destroys and frees the old list.
  2. That destructor reads the list's delete flag at +0x14 and its size at
     +0xc: the `CUtlVector<IImage *>` + flag layout the module mirrors
     (`size` at +0xc, `delete_images` at +0x14; 0x18 bytes).
  3. `KeyValues`'s vftable has `KEYVALUES_SLOT_SET_INT` as `SetInt`: `ret 8`,
     stores the value at +8 and the type word 2 at +0xc.

  vgui2.dll
  4. `Bitmap`'s vftable (RTTI) orders `IImage` as the module's vftable does:
     slot 0 (Paint) calls the surface's `CreateNewTextureID` (slot 31),
     `DrawSetTextureFile` (25), `DrawSetColor` (8) and `DrawTexturedRect`
     (29); slot 2 (GetContentSize) jumps to slot 3 (GetSize), which calls
     `DrawGetTextureSize` (28); slots 1 and 4 (SetPos, SetSize) take two
     ints; slot 5 (SetColor) one.
  5. Pre-Anniversary only (the 25th Anniversary vgui2 has no MemoryBitmap):
     `MemoryBitmap`'s upload calls `DrawSetTextureRGBA` (26) with six
     arguments, and its Paint `DrawSetTexture` (27) with one.

  hw.dll
  6. Both builds implement the same surface interface, `VGUI_Surface026`,
     so the slots above hold on both.

Every constant comes out of `list_icons.rs`.

Usage:
    python goldsrc-hooks/tools/verify_list_icons.py [game-folder ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "studio_panel" / "hook" / "list_icons.rs"


def rust_usize(src, name):
    m = re.search(rf"const {name}: usize = (\d+);", src)
    if not m:
        sys.exit(f"{name} not found in {RUST}")
    return int(m.group(1))


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path))
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.skipdata = True
        for s in self.pe.sections:
            if s.Name.rstrip(b"\0") == b".text":
                self.text = (s.VirtualAddress, s.VirtualAddress + s.Misc_VirtualSize)

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def in_text(self, rva):
        return self.text[0] <= rva < self.text[1]

    def body(self, rva, limit=0x400):
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            text = f"{ins.mnemonic} {ins.op_str}".strip()
            if ins.mnemonic in ("int3", "nop") and out and out[-1].startswith("ret"):
                break
            out.append(text)
            if text.startswith("ret") and self.img[rva + (ins.address - self.base - rva) + ins.size] in (0xCC, 0x90):
                break
        return out

    def refs(self, rva):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.img)]

    def vftable(self, class_name):
        """The vftable of `class_name`'s own (offset 0) base, by RTTI."""
        desc = self.img.find(class_name.encode() + b"\0")
        if desc < 0:
            return None
        desc -= 8
        for ref in self.refs(desc):
            locator = ref - 12
            if locator < 0 or self.u32(locator) not in (0, 1) or self.u32(locator + 4) != 0:
                continue
            for vref in self.refs(locator):
                if self.in_text(self.u32(vref + 4) - self.base):
                    return vref + 4
        return None

    def slot_target(self, vft, index):
        return self.u32(vft + 4 * index) - self.base


def vcalls(body):
    return [int(m.group(1), 16) // 4 for t in body if (m := re.fullmatch(r"call dword ptr \[e\w\w \+ (0x[0-9a-f]+)\]", t))]


def verify(game, src):
    set_image_list = rust_usize(src, "LIST_SLOT_SET_IMAGE_LIST")
    set_int = rust_usize(src, "KEYVALUES_SLOT_SET_INT")
    surface = {n: rust_usize(src, f"SURFACE_{n}") for n in
               ("DRAW_SET_COLOR", "DRAW_SET_TEXTURE_RGBA", "DRAW_SET_TEXTURE", "DRAW_TEXTURED_RECT", "CREATE_NEW_TEXTURE_ID")}
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    ui = Image(game / "valve" / "cl_dlls" / "GameUI.dll")
    vft = ui.vftable(".?AVListPanel@vgui2@@")
    check(vft, f"GameUI.dll has ListPanel's vftable (+{vft or 0:#x})")
    if vft:
        target = ui.slot_target(vft, set_image_list)
        body = ui.body(target, 0x100)
        text = " | ".join(body)
        rets = sorted({t for t in body if t.startswith("ret")})
        stores = re.findall(r"mov (byte|dword) ptr \[e\w\w \+ (0x[0-9a-f]+)\], e?\w\w", text)
        kinds = {k for k, _ in stores}
        check(rets == ["ret 8"] and kinds == {"byte", "dword"},
              f"slot {set_image_list} (SetImageList) at +{target:#x} returns with {rets} and stores a byte and a dword into this")
        dtor = next((int(m.group(1), 16) - ui.base for t in body if (m := re.fullmatch(r"call (0x[0-9a-f]+)", t))), None)
        dtor_text = " | ".join(ui.body(dtor, 0x80)) if dtor else ""
        check(dtor and "+ 0x14]" in dtor_text and "+ 0xc]" in dtor_text,
              f"and its destructor +{dtor or 0:#x} reads the delete flag at +0x14 and the size at +0xc (ImageList's layout)")
        post_free = re.search(r"push 0x18 \| push e\w\w \| call", text)
        print(f"     (frees {'0x18 bytes' if post_free else 'the list without a size'}: sizeof(ImageList) {'checked' if post_free else 'not stated'})")

    kv = ui.vftable(".?AVKeyValues@@")
    check(kv, f"GameUI.dll has KeyValues' vftable (+{kv or 0:#x})")
    if kv:
        target = ui.slot_target(kv, set_int)
        body = ui.body(target, 0x100)
        text = " | ".join(body)
        check("ret 8" in body and re.search(r"mov word ptr \[e\w\w \+ 0xc\], (2|ax)", text) and re.search(r"mov dword ptr \[e\w\w \+ 8\], eax", text),
              f"slot {set_int} (SetInt) at +{target:#x} stores the value at +8 and type 2 at +0xc")

    v2 = Image(game / "vgui2.dll")
    bitmap = v2.vftable(".?AVBitmap@vgui2@@")
    check(bitmap, f"vgui2.dll has Bitmap's vftable (+{bitmap or 0:#x})")
    if bitmap:
        paint = v2.body(v2.slot_target(bitmap, 0), 0x200)
        called = vcalls(paint)
        for name, slot in (("CREATE_NEW_TEXTURE_ID", surface["CREATE_NEW_TEXTURE_ID"]), ("DRAW_SET_COLOR", surface["DRAW_SET_COLOR"]),
                           ("DRAW_TEXTURED_RECT", surface["DRAW_TEXTURED_RECT"])):
            check(slot in called, f"Bitmap slot 0 (Paint) calls surface slot {slot} ({name}); it calls {sorted(set(called))}")
        check(25 in called, "and DrawSetTextureFile (25), the file Bitmap's own upload")
        content = v2.body(v2.slot_target(bitmap, 2), 0x20)
        check(any(re.fullmatch(r"jmp dword ptr \[e\w\w \+ 0xc\]", t) for t in content),
              "slot 2 (GetContentSize) forwards to slot 3 (GetSize)")
        size = v2.body(v2.slot_target(bitmap, 3), 0x100)
        check(28 in vcalls(size) and "ret 8" in size, "slot 3 (GetSize) asks the surface for the texture size (28) and takes two pointers")
        for s in (1, 4):
            b = v2.body(v2.slot_target(bitmap, s), 0x40)
            check("ret 8" in b, f"slot {s} ({'SetPos' if s == 1 else 'SetSize'}) takes two ints")
        check("ret 4" in v2.body(v2.slot_target(bitmap, 5), 0x40), "slot 5 (SetColor) takes one Color")
    memory = v2.vftable(".?AVMemoryBitmap@vgui2@@")
    if memory:
        paint = v2.body(v2.slot_target(memory, 0), 0x200)
        check(surface["DRAW_SET_TEXTURE"] in vcalls(paint),
              f"MemoryBitmap's Paint binds with DrawSetTexture ({surface['DRAW_SET_TEXTURE']})")
        upload = next((int(m.group(1), 16) - v2.base for t in paint if (m := re.fullmatch(r"call (0x[0-9a-f]+)", t))), None)
        ub = v2.body(upload, 0x100) if upload else []
        at = next((i for i, t in enumerate(ub) if t == f"call dword ptr [eax + {surface['DRAW_SET_TEXTURE_RGBA'] * 4:#x}]"), None)
        pushes = sum(1 for t in ub[max(0, (at or 0) - 12):at or 0] if t.startswith("push")) if at is not None else 0
        check(at is not None and pushes == 6,
              f"and its upload +{upload or 0:#x} calls DrawSetTextureRGBA ({surface['DRAW_SET_TEXTURE_RGBA']}) with six arguments ({pushes})")
    else:
        print("     (no MemoryBitmap in this vgui2.dll: DrawSetTextureRGBA and DrawSetTexture come from the interface version)")

    hw = (game / "hw.dll").read_bytes()
    check(b"VGUI_Surface026\0" in hw, "hw.dll implements VGUI_Surface026, the one surface interface version")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or DEFAULT_GAMES
    src = RUST.read_text(encoding="utf-8")
    ok = True
    for game in games:
        if not (game / "valve" / "cl_dlls" / "GameUI.dll").exists():
            print(f"\n==== {game} ====\nSKIP no GameUI.dll here")
            continue
        ok &= verify(game, src)
    print("\nALL OK" if ok else "\nFAILURES above")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
