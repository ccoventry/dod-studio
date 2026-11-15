#!/usr/bin/env python3
"""Checks the review's control lock (#826) against both movie installs.

`studio_panel/hook/lock_controls.rs` greys the demo controls out through
vgui2's `Panel::SetEnabled(bool)` (slot `PANEL_SLOT_SET_ENABLED`, checked by
`verify_studio_panel.py`) and reads them back through `Panel::IsEnabled()`.
It has no per-build address; what only the binaries can prove is:

  1. `Panel@vgui2` slot `PANEL_SLOT_IS_ENABLED` is `IsEnabled()`: it takes no
     argument (a plain `ret`) and returns `ipanel()->IsEnabled(vpanel)`, the
     IPanel slot (+0xe0) `SetEnabled` itself compares against first.
  2. A disabled control takes no click: `Button` and `Slider` both start
     `OnMousePressed` (Panel slot 96) by calling that slot and returning when
     it says no.
  3. The VCR bar and the Load Demo window are named as `review_lock.rs`
     names them (the panel names are in GameUI.dll).
  4. Every control in the stock `valve/resource/DemoPlayerDialog.res` that
     moves the demo is one `review_lock::locks` greys (by its naming rule:
     the slider, and names ending in "Button"), and its labels are not.

Usage:
    python goldsrc-hooks/tools/verify_review_lock.py [game-folder ...]

Defaults to both movie installs. Needs `pip install pefile capstone`.
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import verify_window_layout as vwl  # noqa: E402  (shares the PE helpers)

SRC = Path(__file__).resolve().parent.parent / "src"
PANEL_RS = SRC / "studio_panel.rs"
LOCK_RS = SRC / "review_lock.rs"
STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_GAMES = [
    STEAM / "Half-Life - PRE-Anniversary for Movies",
    STEAM / "Half-Life - POST-Anniversary for Movies",
]
ON_MOUSE_PRESSED = 96


def rust_str(src, name):
    m = re.search(rf'pub const {name}: &str = "([^"]+)";', src)
    if not m:
        sys.exit(f"{name} not found")
    return m.group(1)


def locked_on_bar(name):
    """`review_lock::locks(Holder::VcrBar, name)`."""
    return name.lower() == "timeslider" or name.lower().endswith("button")


def verify(game, panel_src, lock_src):
    ok = True

    def check(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("OK   " if passed else "FAIL ") + message)

    print(f"\n==== {game} ====")
    ui = vwl.Image(game / "valve" / "cl_dlls" / "GameUI.dll")
    is_enabled = vwl.rust_usize(panel_src, "PANEL_SLOT_IS_ENABLED")
    set_enabled = vwl.rust_usize(panel_src, "PANEL_SLOT_SET_ENABLED")
    panel = ui.vftable("Panel@vgui2")
    check(panel, "GameUI.dll has Panel@vgui2's vftable")
    if not panel:
        return False

    # 1.
    get = ui.u32(panel + 4 * is_enabled) - ui.base
    body = ui.body(get, 0x80)
    asks = any(re.fullmatch(r"(call dword ptr \[e\wx \+ 0xe0\]|mov eax, dword ptr \[e\wx \+ 0xe0\])", x)
               for x in body)
    check(asks and body[-1] == "ret",
          f"Panel slot {is_enabled} is IsEnabled(): asks IPanel +0xe0, plain ret: {body[-4:]}")
    put = ui.body(ui.u32(panel + 4 * set_enabled) - ui.base, 0x80)
    check(any(x.endswith("+ 0xe0]") for x in put),
          f"and SetEnabled (slot {set_enabled}) compares against the same IPanel slot first")

    # 2.
    want_call = f"+ {is_enabled * 4:#x}]"
    for cls in ("Button@vgui2", "Slider@vgui2"):
        vt = ui.vftable(cls)
        head = ui.body(ui.u32(vt + 4 * ON_MOUSE_PRESSED) - ui.base, 0x40)[:10] if vt else []
        k = next((i for i, x in enumerate(head) if x.endswith(want_call)), None)
        gated = k is not None and k + 2 < len(head) and head[k + 1 if head[k + 1] != "call eax" else k + 2].startswith("test al, al")
        check(gated, f"{cls} OnMousePressed (slot {ON_MOUSE_PRESSED}) returns early unless IsEnabled: {head[:8]}")

    # 3.
    for name in ("VCR_BAR", "LOAD_DEMO"):
        value = rust_str(lock_src, name)
        check(ui.img.find(value.encode() + b"\0") > 0, f"GameUI.dll names a panel {value!r} ({name})")

    # 4.
    res = (game / "valve" / "resource" / "DemoPlayerDialog.res").read_text(encoding="latin-1")
    fields = re.findall(r'"fieldName"\s+"([^"]+)"', res, re.I)
    commands = dict(re.findall(r'"fieldName"\s+"([^"]+)"(?:(?!"fieldName").)*?"command"\s+"([^"]+)"', res, re.I | re.S))
    check(len(fields) >= 14, f"the stock VCR bar layout lists {len(fields)} controls")
    for name in fields:
        moves = name in commands or name.lower() in ("timeslider", "masterbutton")
        check(locked_on_bar(name) == moves,
              f"{name}: {'locked' if locked_on_bar(name) else 'left alone'} ({commands.get(name, 'no command')})")
    return ok


def main():
    games = [Path(a) for a in sys.argv[1:]] or DEFAULT_GAMES
    panel_src = PANEL_RS.read_text(encoding="utf-8")
    lock_src = LOCK_RS.read_text(encoding="utf-8")
    results = [verify(g, panel_src, lock_src) for g in games]
    print("\nALL OK" if all(results) else "\nSOME CHECKS FAILED")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
