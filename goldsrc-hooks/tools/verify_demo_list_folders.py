#!/usr/bin/env python3
"""Checks `demo_list_folders.rs` against the real `GameUI.dll` and
`FileSystem_Stdio.dll` of both movie installs (issue #408).

The module repoints the Load Demo window's `OnCommand` slot and three of the
file system's `Find*` slots, and calls the window's fill method and its list's
methods by slot. Nothing at runtime can prove those are right, so this does,
for every `GameUiBuild` / `FileSystemBuild` in the module:

  GameUI.dll
  1. The build's identity (PE timestamp, image size) is one in the table.
  2. `file_dialog_vftable` is `CDemoPlayerFileDialog`'s vftable (RTTI).
  3. Its `OnCommand` slot compares the command with "load", reads the list at
     `this+list_field`, asks it for the selected item, checks it and gets its
     row through the `ListPanel` slots the module uses, reads the row's
     "demoname" through the `KeyValues` slot, and posts "DemoSelected".
  4. The double-click handler sends "load" through that same vftable slot,
     so the module's hook sees double-clicks too.
  5. `fill` is a plain `thiscall` method with no arguments that first empties
     the list (`this+list_field`), passes `wildcard` to the file system's
     `FindFirst` and walks it with `FindNext` / `FindClose` (slots 27, 28, 30);
     `wildcard` holds "*.dem" and nothing else in the image refers to it, so
     matching the pointer matches only this window.
  6. The demo bar runs "viewdemo %s" on the name, cutting it only at ";" or a
     newline, so a row carrying a path (or a quoted one) reaches `viewdemo`.

  FileSystem_Stdio.dll
  7. The build's identity is one in the table; it exports only
     `CreateInterface`, and "VFileSystem009"'s factory returns one static
     object, the one GameUI and the engine use.
  8. That object's vftable has `FindFirst` taking three arguments (`ret 0xc`)
     and `FindNext`, `FindIsDirectory`, `FindClose` taking one (`ret 4`).

Every constant comes out of `demo_list_folders.rs`.

Usage:
    python goldsrc-hooks/tools/verify_demo_list_folders.py [game-folder ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_list_folders.rs"


def num(text):
    return int(text.replace("_", ""), 0)


def rust_usize(src, name):
    match = re.search(rf"const {name}: usize = (0x[0-9a-f_]+|\d+);", src)
    if not match:
        raise SystemExit(f"could not find `const {name}` in demo_list_folders.rs")
    return num(match.group(1))


def rust_table(src, struct_name):
    body = re.search(rf"pub const \w+: \[{struct_name}; \d+\] = \[(.*?)\n\];", src, re.S).group(1)
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

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

    def string_at(self, va):
        at = va - self.base
        if not 0 <= at < len(self.img):
            return None
        return self.img[at:self.img.index(b"\0", at)].decode("ascii", "replace")

    def body(self, rva, limit=0x600):
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            text = f"{ins.mnemonic} {ins.op_str}".strip()
            if ins.mnemonic in ("int3", "nop") and out and out[-1].startswith("ret"):
                break
            out.append(text)
        return out

    def pushed_strings(self, body):
        return [self.string_at(int(t.split()[1], 16)) for t in body
                if re.fullmatch(r"push 0x[0-9a-f]{6,}", t)]

    def class_at(self, vftable):
        try:
            locator = self.u32(vftable - 4) - self.base
            return self.string_at(self.u32(locator + 12) + 8)
        except (struct.error, ValueError):
            return None

    def refs(self, rva):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.img)]


def verify(game, src):
    ui_builds = rust_table(src, "GameUiBuild")
    fs_builds = rust_table(src, "FileSystemBuild")
    on_command = rust_usize(src, "SLOT_ON_COMMAND")
    list_slots = [rust_usize(src, n) * 4 for n in
                  ("LIST_SLOT_GET_SELECTED_ITEM", "LIST_SLOT_IS_VALID_ITEM_ID", "LIST_SLOT_GET_ITEM")]
    get_string = rust_usize(src, "KEYVALUES_SLOT_GET_STRING") * 4
    fs_slots = {n: rust_usize(src, f"FS_SLOT_{n}") for n in ("FIND_FIRST", "FIND_NEXT", "FIND_IS_DIRECTORY", "FIND_CLOSE")}
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    ui = Image(game / "valve" / "cl_dlls" / "GameUI.dll")
    ident = (ui.pe.FILE_HEADER.TimeDateStamp, ui.pe.OPTIONAL_HEADER.SizeOfImage)
    build = next((b for b in ui_builds if (b["time_date_stamp"], b["size_of_image"]) == ident), None)
    check(build, f"GameUI.dll {ident[0]:#x}/{ident[1]:#x} is a build in GAMEUI_BUILDS ({build and build['name']})")
    if not build:
        return False
    vft, field = build["file_dialog_vftable"], build["list_field"]
    check(ui.class_at(vft) == ".?AVCDemoPlayerFileDialog@@", f"+{vft:#x} is CDemoPlayerFileDialog's vftable")

    cmd = ui.body(ui.u32(vft + 4 * on_command) - ui.base, 0x200)
    strings = ui.pushed_strings(cmd)
    texts = " | ".join(cmd)
    check(re.search(r"mov e\w\w, 0x[0-9a-f]+", texts) and "load" in
          [ui.string_at(int(m, 16)) for m in re.findall(r"mov e\w\w, (0x[0-9a-f]{6,})", texts)],
          f"slot {on_command} (OnCommand) compares the command with \"load\"")
    check(f"dword ptr [esi + {field:#x}]" in texts or f"dword ptr [ebx + {field:#x}]" in texts,
          f"and reads the list at this+{field:#x}")
    for s in list_slots:
        check(re.search(rf"(call|mov e\w\w,) dword ptr \[e\w\w \+ {s:#x}\]", texts), f"and calls ListPanel +{s:#x}")
    check(re.search(rf"call dword ptr \[e\w\w \+ {get_string:#x}\]", texts) and "demoname" in strings,
          f"and reads \"demoname\" through KeyValues +{get_string:#x}")
    check("DemoSelected" in strings, "and posts \"DemoSelected\"")

    clicked = [k for k in range(on_command + 1, 200)
               if (b := ui.body(ui.u32(vft + 4 * k) - ui.base, 0x20))
               and f"call dword ptr [eax + {on_command * 4:#x}]" in b and "load" in ui.pushed_strings(b)]
    check(clicked, f"a handler (slot {clicked}) sends \"load\" through slot {on_command}, so double-clicks reach the hook")

    fill = ui.body(build["fill"], 0x800)
    first_ret = next((t for t in fill if t.startswith("ret")), None)
    check(first_ret == "ret", f"fill +{build['fill']:#x} returns with a plain ret (thiscall, no arguments): {first_ret!r}")
    first = next((i for i, t in enumerate(fill) if "+ 0x294]" in t), None)
    check(first is not None and any(f"+ {field:#x}]" in t for t in fill[:first]), "and first empties the list (ListPanel +0x294)")
    fill_text = " | ".join(fill)
    check(f"push {ui.base + build['wildcard']:#x}" in fill_text, f"and passes +{build['wildcard']:#x} to FindFirst")
    for n in ("FIND_FIRST", "FIND_NEXT", "FIND_CLOSE"):
        check(re.search(rf"call dword ptr \[e\w\w \+ {fs_slots[n] * 4:#x}\]", fill_text), f"and calls file system slot {fs_slots[n]} ({n})")
    check(ui.string_at(ui.base + build["wildcard"]) == "*.dem", "the wildcard is \"*.dem\"")
    refs = ui.refs(build["wildcard"])
    check(len(refs) == 1, f"and only one place refers to it: {[hex(r) for r in refs]}")

    viewdemo = ui.img.find(b"viewdemo %s\n\0")
    users = ui.refs(viewdemo)
    cut = ui.img.find(b";\n\0")
    near = users and cut >= 0 and (b"\x68" + struct.pack("<I", ui.base + cut)) in ui.img[users[0] - 0x60:users[0]]
    check(near, "the demo bar runs \"viewdemo %s\" after cutting the name only at ';' or newline")

    fs = Image(game / "FileSystem_Stdio.dll")
    ident = (fs.pe.FILE_HEADER.TimeDateStamp, fs.pe.OPTIONAL_HEADER.SizeOfImage)
    fbuild = next((b for b in fs_builds if (b["time_date_stamp"], b["size_of_image"]) == ident), None)
    check(fbuild, f"FileSystem_Stdio.dll {ident[0]:#x}/{ident[1]:#x} is a build in FILESYSTEM_BUILDS ({fbuild and fbuild['name']})")
    exports = [e.name.decode() for e in fs.pe.DIRECTORY_ENTRY_EXPORT.symbols if e.name]
    check(exports == ["CreateInterface"], f"it exports only CreateInterface: {exports}")
    name = fs.img.find(b"VFileSystem009\0")
    obj = None
    for ref in fs.refs(name):
        if fs.img[ref - 1] == 0x68 and fs.img[ref + 4] == 0x68:
            factory = fs.body(fs.u32(ref + 5) - fs.base, 0x10)
            m = re.fullmatch(r"mov eax, (0x[0-9a-f]+)", factory[0])
            if m and factory[1] == "ret":
                obj = int(m.group(1), 16) - fs.base
    check(obj, f"VFileSystem009's factory returns one static object (+{obj or 0:#x})")
    vftable = None
    if obj:
        # The constructor stores the vftable: either straight into the object,
        # or into `this` after `mov ecx, <object>`.
        # A direct store is the static initializer's. The jump targets after
        # `mov ecx, <object>` are the constructor and the destructor, so they
        # are only the fallback: a destructor can store a base class' vftable.
        direct = re.search(re.escape(b"\xC7\x05" + struct.pack("<I", fs.base + obj)) + b"(....)", fs.img, re.S)
        if direct:
            vftable = struct.unpack("<I", direct.group(1))[0] - fs.base
        for m in [] if direct else re.finditer(re.escape(b"\xB9" + struct.pack("<I", fs.base + obj)), fs.img):
            nxt = fs.body(m.start(), 0x10)[1]
            target = re.fullmatch(r"(jmp|call) (0x[0-9a-f]+)", nxt)
            if target:
                for t in fs.body(int(target.group(2), 16) - fs.base, 0x40):
                    s = re.fullmatch(r"mov dword ptr \[e\w\w\], (0x[0-9a-f]+)", t)
                    if s:
                        vftable = int(s.group(1), 16) - fs.base
    check(vftable, f"the object's vftable is +{vftable or 0:#x}")
    if vftable:
        want = {"FIND_FIRST": "ret 0xc", "FIND_NEXT": "ret 4", "FIND_IS_DIRECTORY": "ret 4", "FIND_CLOSE": "ret 4"}
        for n, r in want.items():
            f = fs.u32(vftable + 4 * fs_slots[n]) - fs.base
            rets = sorted({t for t in fs.body(f, 0x800) if t.startswith("ret")})
            check(rets == [r], f"slot {fs_slots[n]} ({n}) at +{f:#x} returns with {rets}")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or [g for g in DEFAULT_GAMES if g.is_dir()]
    src = RUST.read_text(encoding="utf-8")
    ok = all([verify(g, src) for g in games])
    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
