#!/usr/bin/env python3
"""Checks `engine_buttons.rs` against both movie installs' `GameUI.dll`
(issue #408).

`engine_buttons.rs` repoints one `call` -- `Frame::OnCommand`'s call to
`Panel::OnCommand` for commands it doesn't know -- at its own handler. This
proves, for every build in `GAMEUI_BUILDS`:

  1. The build's identity (PE timestamp, image size) is in the table.
  2. `panel_on_command` is `Panel@vgui2`'s vftable slot 87 (`OnCommand`) and
     is an empty `ret 4`: one argument, nothing done with it. It is shared
     (the linker folded identical empty methods into it), which is why the
     function itself can't be patched.
  3. `call_site` is an `E8` to that function inside `Frame::OnCommand`
     (`Frame@vgui2`'s slot 87), is the only call to it there, pushes the
     command (the register loaded from the function's argument) first, and
     is followed by the function's own return: the last stop for a command
     `Frame` doesn't know.
  4. `CBasePanel`, which runs the ESC menu's entries, looks for "engine " --
     the prefix this module takes.

Every constant comes out of `engine_buttons.rs`.

Usage:
    python goldsrc-hooks/tools/verify_engine_buttons.py [game-folder ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "engine_buttons.rs"
ON_COMMAND_SLOT = 87


def num(text):
    return int(text.replace("_", ""), 0)


def rust_table(src):
    body = re.search(r"pub const GAMEUI_BUILDS: \[\w+; \d+\] = \[(.*?)\n\];", src, re.S).group(1)
    rows = []
    for block in re.findall(r"\{(.*?)\}", body, re.S):
        row = dict(re.findall(r"(\w+): (0x[0-9a-f_]+|\"[^\"]*\")", block))
        rows.append({k: (v.strip('"') if v.startswith('"') else num(v)) for k, v in row.items()})
    return rows


class Image:
    def __init__(self, path):
        self.pe = pefile.PE(str(path), fast_load=True)
        self.base = self.pe.OPTIONAL_HEADER.ImageBase
        self.img = bytes(self.pe.get_memory_mapped_image())
        self.md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        self.md.skipdata = True

    def u32(self, rva):
        return struct.unpack_from("<I", self.img, rva)[0]

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

    def function(self, rva, limit=0x200):
        """(rva, text) through the last `ret` before padding."""
        out = []
        for ins in self.md.disasm(self.img[rva:rva + limit], self.base + rva):
            if ins.mnemonic in ("int3", "nop") and out and out[-1][1].startswith("ret"):
                break
            out.append((ins.address - self.base, f"{ins.mnemonic} {ins.op_str}".strip()))
        return out


def verify(game, src):
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    ui = Image(game / "valve" / "cl_dlls" / "GameUI.dll")
    ident = (ui.pe.FILE_HEADER.TimeDateStamp, ui.pe.OPTIONAL_HEADER.SizeOfImage)
    build = next((b for b in rust_table(src) if (b["time_date_stamp"], b["size_of_image"]) == ident), None)
    check(build, f"GameUI.dll {ident[0]:#x}/{ident[1]:#x} is in GAMEUI_BUILDS ({build and build['name']})")
    if not build:
        return False

    panel = ui.vftable("Panel@vgui2")
    empty = build["panel_on_command"]
    check(panel and ui.u32(panel + 4 * ON_COMMAND_SLOT) - ui.base == empty,
          f"+{empty:#x} is Panel@vgui2's slot {ON_COMMAND_SLOT} (OnCommand)")
    check(ui.img[empty:empty + 3] == b"\xc2\x04\x00", "and is an empty `ret 4`")
    shared = len(ui.refs(empty))
    print(f"     {shared} vftable entries share it, so it can't be patched itself")

    frame = ui.vftable("Frame@vgui2")
    on_command = ui.u32(frame + 4 * ON_COMMAND_SLOT) - ui.base
    body = ui.function(on_command, 0x300)
    site = build["call_site"]
    rel = struct.unpack_from("<i", ui.img, site + 1)[0]
    check(ui.img[site] == 0xE8 and site + 5 + rel == empty, f"+{site:#x} is `call Panel::OnCommand`")
    addresses = [a for a, _ in body]
    check(site in addresses, f"inside Frame::OnCommand (+{on_command:#x})")
    calls = [a for a, t in body if t == f"call {ui.base + empty:#x}"]
    check(calls == [site], f"and is its only call to it: {[hex(c) for c in calls]}")
    arg = next((re.fullmatch(r"mov (e\w\w), dword ptr \[(?:esp \+ 0x[0-9a-f]+|ebp \+ 8)\]", t) for _, t in body
                if re.fullmatch(r"mov (e\w\w), dword ptr \[(?:esp \+ 0x[0-9a-f]+|ebp \+ 8)\]", t)), None)
    at = addresses.index(site) if site in addresses else 0
    check(arg and body[at - 1][1] == f"push {arg.group(1)}",
          f"the command it passes on is the one Frame::OnCommand was given ({body[at - 1][1]!r})")
    tail = [t for _, t in body[at + 1:at + 7]]
    check(any(t == "ret 4" for t in tail) and not any(t.startswith("call") for t in tail),
          f"and Frame::OnCommand returns straight after it: {tail}")

    engine = ui.img.find(b"engine \0")
    check(engine >= 0 and ui.refs(engine), "CBasePanel looks for \"engine \" in the ESC menu's commands")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or [g for g in DEFAULT_GAMES if g.is_dir()]
    src = RUST.read_text(encoding="utf-8")
    ok = all([verify(g, src) for g in games])
    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
