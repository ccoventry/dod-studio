#!/usr/bin/env python3
"""Checks `demo_other_drives.rs` against the real `FileSystem_Stdio.dll` and
`Core.dll` of both movie installs (issue #776).

The module repoints the file system's `Open` slot and calls its
`AddSearchPath` slot. Nothing at runtime can prove those are right, so this
does, for each install:

  FileSystem_Stdio.dll
  1. The build's identity (PE timestamp, image size) is one in
     `demo_list_folders.rs`' FILESYSTEM_BUILDS, the table the module shares.
  2. "VFileSystem009"'s factory returns one static object; its vftable is
     found.
  3. `FS_SLOT_OPEN` is `Open(name, options, pathID)`: it returns with
     `ret 0xc`, walks the search paths (the vector at this+0x40, count at
     this+0x4c) and prints "_stat on file %s which appeared to exist
     failed" -- so a name is only ever tried under each search path.
  4. `FS_SLOT_ADD_SEARCH_PATH` is `AddSearchPath(path, pathID)`: `ret 8`, a
     thin wrapper around a function that tests the path for a ':' (a drive
     letter is kept as is) and for ".bsp", and appends to that vector.
  5. The 25th Anniversary `Open` starts with `strchr(name, ':')` and
     `_wassert` on a hit, "!strchr(pFileName, ':')" -- the assert the
     rewrite must happen before. The pre-Anniversary `Open` has no ':' test.

  Core.dll
  6. `DemoFile::LoadDemo` (the function that pushes "rb" for a call through
     slot 10 and prints "Coudn't open demo file %s.") passes a null path ID,
     so a search path with the module's own PATH_ID is searched.

Every constant comes out of `demo_other_drives.rs` and `demo_list_folders.rs`.

Usage:
    python goldsrc-hooks/tools/verify_demo_other_drives.py [game-folder ...]

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
SRC = Path(__file__).resolve().parent.parent / "src"
RUST = SRC / "demo_other_drives.rs"
BUILDS_RUST = SRC / "demo_list_folders.rs"


def num(text):
    return int(text.replace("_", ""), 0)


def rust_usize(src, name):
    m = re.search(rf"const {name}: usize = (\d+);", src)
    if not m:
        sys.exit(f"{name} not found in {RUST}")
    return int(m.group(1))


def rust_table(src, struct_name):
    rows = []
    for block in re.findall(rf"{struct_name} \{{(.*?)\}}", src, re.S):
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

    def wstring_at(self, va):
        at = va - self.base
        end = self.img.find(b"\0\0", at)
        return self.img[at:end + 1].decode("utf-16-le", "replace")

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

    def refs(self, rva):
        needle = struct.pack("<I", self.base + rva)
        return [m.start() for m in re.finditer(re.escape(needle), self.img)]

    def import_name(self, iat_va):
        for entry in self.pe.DIRECTORY_ENTRY_IMPORT:
            for imp in entry.imports:
                if imp.address == iat_va and imp.name:
                    return imp.name.decode()
        return None


def file_system_object(fs):
    """The static VFileSystem009 object and its vftable (RVAs)."""
    name = fs.img.find(b"VFileSystem009\0")
    obj = None
    for ref in fs.refs(name):
        if fs.img[ref - 1] == 0x68 and fs.img[ref + 4] == 0x68:
            factory = fs.body(fs.u32(ref + 5) - fs.base, 0x10)
            m = re.fullmatch(r"mov eax, (0x[0-9a-f]+)", factory[0])
            if m and factory[1] == "ret":
                obj = int(m.group(1), 16) - fs.base
    vftable = None
    if obj:
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
    return obj, vftable


def verify(game, src, builds_src):
    fs_builds = rust_table(builds_src, "FileSystemBuild")
    slot_open = rust_usize(src, "FS_SLOT_OPEN")
    slot_add = rust_usize(src, "FS_SLOT_ADD_SEARCH_PATH")
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    fs = Image(game / "FileSystem_Stdio.dll")
    ident = (fs.pe.FILE_HEADER.TimeDateStamp, fs.pe.OPTIONAL_HEADER.SizeOfImage)
    # (the struct's own definition is picked up as an empty row; `.get` skips it)
    build = next((b for b in fs_builds if (b.get("time_date_stamp"), b.get("size_of_image")) == ident), None)
    check(build, f"FileSystem_Stdio.dll {ident[0]:#x}/{ident[1]:#x} is a build in FILESYSTEM_BUILDS ({build and build['name']})")
    if not build:
        return False
    obj, vftable = file_system_object(fs)
    check(obj and vftable, f"VFileSystem009 is one static object (+{obj or 0:#x}) with its vftable at +{vftable or 0:#x}")
    if not vftable:
        return False

    open_rva = fs.u32(vftable + 4 * slot_open) - fs.base
    open_body = fs.body(open_rva, 0x800)
    open_text = " | ".join(open_body)
    rets = sorted({t for t in open_body if t.startswith("ret")})
    check(rets == ["ret 0xc"], f"slot {slot_open} (Open) at +{open_rva:#x} returns with {rets}")
    check(re.search(r"\[e\w\w \+ 0x4c\]", open_text) and re.search(r"\[e\w\w \+ 0x40\]", open_text),
          "and walks the search paths (this+0x40, count at this+0x4c)")
    check(any(s and s.startswith("_stat on file %s which appeared to exist failed") for s in fs.pushed_strings(open_body)),
          "and prints \"_stat on file %s which appeared to exist failed\"")

    add_rva = fs.u32(vftable + 4 * slot_add) - fs.base
    add_body = fs.body(add_rva, 0x40)
    add_rets = sorted({t for t in add_body if t.startswith("ret")})
    check(add_rets == ["ret 8"], f"slot {slot_add} (AddSearchPath) at +{add_rva:#x} returns with {add_rets}")
    inner = next((int(m.group(1), 16) - fs.base for t in add_body if (m := re.fullmatch(r"call (0x[0-9a-f]+)", t))), None)
    inner_body = fs.body(inner, 0x900) if inner else []
    check(inner and "push 0x3a" in inner_body and ".bsp" in fs.pushed_strings(inner_body),
          f"and wraps +{inner or 0:#x}, which tests the path for ':' and \".bsp\"")
    check(re.search(r"\[e\w\w \+ 0x4c\]", " | ".join(inner_body)), "and appends to the same search-path vector")

    colon_at = next((i for i, t in enumerate(open_body) if t == "push 0x3a"), None)
    if build["name"] == "25th Anniversary":
        after = open_body[colon_at + 1:colon_at + 12] if colon_at is not None else []
        iat = next((int(m.group(1), 16) for t in after if (m := re.fullmatch(r"call dword ptr \[(0x[0-9a-f]+)\]", t))), None)
        expression = next((fs.wstring_at(int(m.group(1), 16)) for t in after if (m := re.fullmatch(r"push (0x[0-9a-f]{6,})", t))
                           and "strchr" in fs.wstring_at(int(m.group(1), 16))), None)
        check(colon_at is not None and colon_at < 24 and iat and fs.import_name(iat) == "_wassert",
              f"the Anniversary Open starts with strchr(name, ':') (instruction {colon_at}) and _wassert "
              f"(IAT {iat or 0:#x} = {iat and fs.import_name(iat)})")
        check(expression and "strchr(pFileName, ':')" in expression, f"asserting {expression!r}")
    else:
        check(colon_at is None, "the pre-Anniversary Open never tests for ':'")

    core = Image(game / "Core.dll")
    rb = next((at for at in (m.start() for m in re.finditer(rb"rb\0", core.img)) if core.img[at - 1] == 0), None)
    load_demo = None
    for ref in core.refs(rb) if rb is not None else []:
        push = ref - 1
        if core.img[push] != 0x68:
            continue
        following = core.body(push, 0x14)
        if any(re.fullmatch(r"call dword ptr \[e\w\w \+ 0x28\]", t) for t in following[:4]):
            load_demo = push
    check(load_demo, f"Core.dll: one \"rb\" push feeds a call through slot 10 (at +{load_demo or 0:#x})")
    if load_demo:
        before = core.img[load_demo - 2:load_demo]
        null_path_id = before == b"\x6a\x00"
        if not null_path_id and 0x50 <= before[1] <= 0x57:
            reg = before[1] - 0x50
            zero = bytes([0x33, 0xC0 | (reg << 3) | reg])
            null_path_id = zero in core.img[load_demo - 0x200:load_demo] or bytes([0x31, zero[1]]) in core.img[load_demo - 0x200:load_demo]
        check(null_path_id, "and the path ID pushed before it is null (push 0, or a register zeroed earlier)")
        near = " | ".join(core.body(load_demo, 0x60))
        check(any(s and s.startswith("Coudn't open demo file") for s in fs.pushed_strings(core.body(load_demo, 0x60))) or
              any(s and s.startswith("Coudn't open demo file") for s in core.pushed_strings(core.body(load_demo, 0x60))),
              "and a miss prints \"Coudn't open demo file %s.\" (DemoFile::LoadDemo)")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or DEFAULT_GAMES
    src = RUST.read_text(encoding="utf-8")
    builds_src = BUILDS_RUST.read_text(encoding="utf-8")
    ok = True
    for game in games:
        if not (game / "FileSystem_Stdio.dll").exists():
            print(f"\n==== {game} ====\nSKIP no FileSystem_Stdio.dll here")
            continue
        ok &= verify(game, src, builds_src)
    print("\nALL OK" if ok else "\nFAILURES above")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
