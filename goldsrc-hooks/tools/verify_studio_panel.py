#!/usr/bin/env python3
"""Checks `studio_panel.rs` against both movie installs (issue #408).

The DoD Studio window is a GameUI `Frame` the hook builds itself, which takes
five `GameUI.dll` addresses per build and two vftable slots. Nothing at run
time proves them, so this does, for every build in `BUILDS`, by reading how
GameUI builds its own Load Demo window (`CDemoPlayerFileDialog`):

  1. The build's identity (PE timestamp, image size) is in `BUILDS`.
  2. GameUI allocates that dialog with `push <size>; call operator_new`, and
     `operator_new` is the address in `BUILDS`.
  3. The dialog's constructor calls `Frame::Frame` at `frame_ctor`, which pops
     12 bytes (three arguments) or, where `frame_ctor_fourth_arg`, 16.
  4. It loads its layout through `load_control_settings`, which pops 8 bytes.
  5. `frame_size` is where the dialog's own first field sits: its allocation
     is exactly 4 bytes more (one field).
  6. After building it, GameUI shows it through vftable slot
     `FRAME_SLOT_ACTIVATE` (`jmp [reg + slot*4]`).
  7. `Frame@vgui2`'s vftable slot `FRAME_SLOT_ON_COMMAND` pops 4 bytes
     (`OnCommand(const char *)`), and the VCR bar overrides the same slot.

Usage:
    python goldsrc-hooks/tools/verify_studio_panel.py [game-folder ...]

Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_window_layout as vwl  # noqa: E402  (shares the PE helpers)

RUST = Path(__file__).resolve().parent.parent / "src" / "studio_panel.rs"


def rust_builds(src):
    body = re.search(r"pub const BUILDS: \[Build; \d+\] = \[(.*?)\n\];", src, re.S).group(1)
    rows = []
    for block in re.findall(r"Build \{(.*?)\}", body, re.S):
        row = {}
        for key, value in re.findall(r"(\w+): ([^,\n]+),", block):
            value = value.strip()
            if value.startswith('"'):
                row[key] = value.strip('"')
            elif value in ("true", "false"):
                row[key] = value == "true"
            else:
                row[key] = vwl.num(value)
        rows.append(row)
    return rows


def verify(game, src):
    on_command = vwl.rust_usize(src, "FRAME_SLOT_ON_COMMAND")
    activate = vwl.rust_usize(src, "FRAME_SLOT_ACTIVATE")
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    ui = vwl.Image(game / "valve" / "cl_dlls" / "GameUI.dll")
    build = next((b for b in rust_builds(src)
                  if (b["time_date_stamp"], b["size_of_image"]) == ui.identity()), None)
    check(build, f"GameUI.dll {ui.identity()[0]:#x}/{ui.identity()[1]:#x} is in BUILDS ({build and build['name']})")
    if not build:
        return False

    # The Load Demo window's constructor: the function that stores its vftable.
    vt = ui.vftable("CDemoPlayerFileDialog")
    needle = struct.pack("<I", ui.base + vt)
    ctor = None
    for m in re.finditer(re.escape(needle), ui.img):
        at = m.start()
        if at >= ui.code[1]:
            continue
        callers = []
        for start in range(at, at - 0x400, -1):
            if ui.img[start - 1] in (0xCC, 0x90, 0xC3) and ui.img[start] in (0x55, 0x53, 0x56, 0x57, 0x6A, 0x8B):
                callers = ui.calls_to(start)
                if callers:
                    ctor = (start, callers[0])
                    break
        if ctor:
            break
    check(ctor, f"CDemoPlayerFileDialog's constructor is +{(ctor or (0, 0))[0]:#x}, built at +{(ctor or (0, 0))[1]:#x}")
    if not ctor:
        return False
    start, site = ctor

    # 2 and 5: push <size>; call operator_new, a few instructions before.
    before = list(ui.md.disasm(ui.img[site - 0x40:site], ui.base + site - 0x40))
    alloc = None
    for a, b in zip(before, before[1:]):
        if a.mnemonic == "push" and b.mnemonic == "call" and a.op_str.startswith("0x") and int(a.op_str, 16) < 0x1000:
            alloc = (int(a.op_str, 16), int(b.op_str, 16) - ui.base)
    check(alloc and alloc[1] == build["operator_new"],
          f"GameUI allocates it with operator_new +{(alloc or (0, 0))[1]:#x} (BUILDS: +{build['operator_new']:#x})")
    check(alloc and alloc[0] == build["frame_size"] + 4,
          f"and {(alloc or (0, 0))[0]:#x} bytes: Frame's {build['frame_size']:#x} plus the dialog's one field")

    # 3 and 4: what the constructor calls.
    body = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[start:start + 0x400], ui.base + start)]
    calls = [int(t.split()[1], 16) - ui.base for t in body if re.fullmatch(r"call 0x[0-9a-f]+", t)]
    check(build["frame_ctor"] in calls, f"its constructor calls Frame::Frame +{build['frame_ctor']:#x}")
    want = "ret 0x10" if build["frame_ctor_fourth_arg"] else "ret 0xc"
    got = ui.last_ret(build["frame_ctor"])
    check(got == want, f"which returns with {got!r} ({'four' if build['frame_ctor_fourth_arg'] else 'three'} arguments)")
    check(build["load_control_settings"] in calls,
          f"and LoadControlSettings +{build['load_control_settings']:#x}")
    got = ui.last_ret(build["load_control_settings"])
    check(got == "ret 8", f"which returns with {got!r} (path, pathID)")

    # 6: shown through the Activate slot.
    after = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[site:site + 0x60], ui.base + site)]
    check(any(re.fullmatch(rf"(jmp|call) dword ptr \[e\w\w \+ {activate * 4:#x}\]", t) for t in after),
          f"GameUI then shows it through vftable slot {activate} (+{activate * 4:#x})")

    # 7: OnCommand.
    frame = ui.vftable("Frame@vgui2")
    got = ui.last_ret(ui.u32(frame + 4 * on_command) - ui.base)
    check(got == "ret 4", f"Frame's vftable slot {on_command} (OnCommand) returns with {got!r}")
    bar = ui.vftable("CDemoPlayerDialog")
    check(ui.u32(bar + 4 * on_command) != ui.u32(frame + 4 * on_command),
          f"and the VCR bar overrides slot {on_command}, as it must for its buttons")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or [g for g in vwl.DEFAULT_GAMES if g.is_dir()]
    src = RUST.read_text(encoding="utf-8")
    ok = all([verify(g, src) for g in games])
    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
