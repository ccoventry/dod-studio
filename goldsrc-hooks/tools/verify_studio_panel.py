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
  8. GameUI calls the engine's `BaseUI001` slot before
     `BASEUI_SLOT_ACTIVATE_GAME_UI` (HideGameUI) after an `engine ...` menu
     command, and calls that slot (ActivateGameUI) itself.

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
SORT_ARROWS = RUST.parent / "studio_panel" / "hook" / "sort_arrows.rs"


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

    # 8-12: the tabs, read off how PropertyDialog and the Options pages are built.
    client_area = vwl.rust_usize(src, "FRAME_SLOT_GET_CLIENT_AREA")
    add_page = vwl.rust_usize(src, "SHEET_SLOT_ADD_PAGE")
    got = ui.last_ret(ui.u32(frame + 4 * client_area) - ui.base)
    check(got == "ret 0x10", f"Frame's slot {client_area} (GetClientArea, four out-params) returns with {got!r}")

    dialog = constructor_of(ui, "PropertyDialog@vgui2")
    sheet_new = None
    if dialog:
        lines = [(i.mnemonic, i.op_str) for i in ui.md.disasm(ui.img[dialog:dialog + 0x300], ui.base + dialog)]
        def is_sheet(op):
            if not op.startswith("0x"):
                return False
            at = int(op, 16) - ui.base
            return 0 < at < len(ui.img) and ui.img[at:at + 6] == b"Sheet\0"

        for k, (m, op) in enumerate(lines):
            if m == "push" and is_sheet(op):
                size = next((int(o, 16) for mm, o in reversed(lines[max(0, k - 8):k])
                             if mm == "push" and o.startswith("0x") and int(o, 16) < 0x1000), None)
                ctor = next((int(o, 16) - ui.base for mm, o in lines[k:k + 6] if mm == "call"), None)
                sheet_new = (size, ctor)
                break
    check(sheet_new and sheet_new[1] == build["sheet_ctor"],
          f"PropertyDialog builds its sheet with PropertySheet::PropertySheet +{(sheet_new or (0, 0))[1] or 0:#x}")
    check(sheet_new and sheet_new[0] == build["sheet_size"],
          f"allocating {(sheet_new or (0, 0))[0] or 0:#x} bytes (BUILDS: {build['sheet_size']:#x})")
    got = ui.last_ret(build["sheet_ctor"])
    check(got == "ret 8", f"PropertySheet::PropertySheet returns with {got!r} (parent, name)")

    sheet_vt = ui.vftable("PropertySheet@vgui2")
    got = ui.last_ret(ui.u32(sheet_vt + 4 * add_page) - ui.base)
    check(got == "ret 8", f"PropertySheet's slot {add_page} (AddPage) returns with {got!r} (page, title)")
    set_active = vwl.rust_usize(src, "SHEET_SLOT_SET_ACTIVE_PAGE")
    got = ui.last_ret(ui.u32(sheet_vt + 4 * set_active) - ui.base)
    check(got == "ret 4", f"PropertySheet's slot {set_active} (SetActivePage) returns with {got!r} (page)")
    forward = re.compile(rb"\x8b\x89(....)\x8b\x01(?:\x5d)?\xff\xa0" + struct.pack("<I", add_page * 4), re.S)
    check(forward.search(ui.img, *ui.code), f"and PropertyDialog::AddPage jumps there (jmp [eax+{add_page * 4:#x}])")

    options_page = constructor_of(ui, "COptionsSubAudio")
    first_call = None
    if options_page:
        for i in ui.md.disasm(ui.img[options_page:options_page + 0x80], ui.base + options_page):
            if i.mnemonic == "call":
                first_call = int(i.op_str, 16) - ui.base
                break
    check(first_call == build["page_ctor"],
          f"an Options page's constructor starts with PropertyPage::PropertyPage +{first_call or 0:#x}")
    got = ui.last_ret(build["page_ctor"])
    check(got == "ret 0xc", f"which returns with {got!r} (parent, name, bool)")
    page_vt = ui.vftable("PropertyPage@vgui2")
    body = ui.body(ui.u32(page_vt + 4 * on_command) - ui.base, 0x10)
    check(body[:1] == ["ret 4"], f"PropertyPage's own OnCommand is an empty {body[:1]} -- why pages get ours")

    # 14: the Load Demo window the Demos tab borrows from.
    check(start == build["file_dialog_ctor"],
          f"CDemoPlayerFileDialog's constructor is file_dialog_ctor +{build['file_dialog_ctor']:#x}")
    got = ui.last_ret(build["file_dialog_ctor"])
    check(got == "ret 8", f"which returns with {got!r} (parent, name)")
    dem = ui.base + ui.img.find(b"*.dem\0")
    fill = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[build["file_dialog_fill"]:build["file_dialog_fill"] + 0x200], ui.base + build["file_dialog_fill"])]
    check(f"push {hex(dem)}" in fill, f"file_dialog_fill +{build['file_dialog_fill']:#x} passes the \"*.dem\" wildcard")
    check(build["file_dialog_fill"] in calls, "and the constructor calls it")
    list_vt = ui.vftable("ListPanel@vgui2")
    for name, want in (("LIST_SLOT_GET_SELECTED_ITEM", "ret 4"), ("LIST_SLOT_IS_VALID_ITEM_ID", "ret 4"),
                       ("LIST_SLOT_GET_ITEM", "ret 4"), ("LIST_SLOT_FIRST_ITEM", "ret"),
                       ("LIST_SLOT_NEXT_ITEM", "ret 4"), ("LIST_SLOT_SET_ITEM_VISIBLE", "ret 8")):
        index = vwl.rust_usize(src, name)
        func = ui.u32(list_vt + 4 * index) - ui.base
        # A forwarder (Anniversary IsValidItemID: `add ecx, ...; jmp`) returns
        # with whatever it jumps to.
        head = ui.body(func, 0x20)
        tail = next((t for t in head if re.fullmatch(r"jmp 0x[0-9a-f]+", t)), None)
        if tail and not any(t.startswith("ret") for t in head[:head.index(tail)]):
            func = int(tail.split()[1], 16) - ui.base
        got = ui.last_ret(func)
        check(got == want, f"ListPanel slot {index} ({name}) returns with {got!r}")

    # 19: the Map and Date columns. The window's constructor makes its one
    # column through AddColumnHeader, just after pushing "demoname".
    for name, want in (("LIST_SLOT_ADD_COLUMN_HEADER", "ret 0x14"),
                       ("LIST_SLOT_SET_COLUMN_SORTABLE", "ret 8"), ("LIST_SLOT_APPLY_ITEM_CHANGES", "ret 4")):
        index = vwl.rust_usize(src, name)
        got = ui.last_ret(ui.u32(list_vt + 4 * index) - ui.base)
        check(got == want, f"ListPanel slot {index} ({name}) returns with {got!r}")
    # 25: the sort arrows (#611). "SetSortColumn"'s message handler is a
    # thunk to vftable slot 190, OnSetSortColumn: a repeat
    # click flips the ascending byte at list_sort_ascending; another column
    # moves the one at list_sort_column to the secondary (+4) and its flag to
    # +1. SetSortColumn (slot 146) stores at list_sort_column.
    on_set_sort_column = 190
    thunk = ["mov eax, dword ptr [ecx]", f"jmp dword ptr [eax + {hex(4 * on_set_sort_column)}]"]
    name_at = ui.img.find(b"\0SetSortColumn\0") + 1
    registered = any(
        ui.code[0] <= ui.u32(at) - ui.base < ui.code[1] and ui.body(ui.u32(at) - ui.base, 0x10)[:2] == thunk
        for ref in ui.refs(name_at) for at in range(ref - 0x20, ref + 0x40))
    check(registered, f"\"SetSortColumn\" is handled by ListPanel slot {on_set_sort_column} (OnSetSortColumn)")
    col, asc = build["list_sort_column"], build["list_sort_ascending"]
    on_set = ui.body(ui.u32(list_vt + 4 * on_set_sort_column) - ui.base, 0x80)
    check(any(x.endswith(f"[esi + {hex(col)}]") for x in on_set)
          and any(x == f"mov byte ptr [esi + {hex(asc)}], al" for x in on_set)
          and any(f"[esi + {hex(col + 4)}]," in x for x in on_set)
          and any(f"[esi + {hex(asc + 1)}]," in x for x in on_set),
          f"which reads the sort column +{col:#x} and its flag +{asc:#x}, each with its secondary after it")
    set_sort = ui.body(ui.u32(list_vt + 4 * 146) - ui.base, 0x20)
    check(f"mov dword ptr [ecx + {hex(col)}], eax" in set_sort,
          f"ListPanel slot 146 (SetSortColumn) stores the column at +{col:#x}")
    arrows = SORT_ARROWS.read_text(encoding="utf-8")
    for name, want, label_slot in (("LIST_SLOT_SET_COLUMN_HEADER_TEXT_WIDE", "ret 8", 134),
                                   ("LIST_SLOT_GET_COLUMN_HEADER_TEXT", "ret 0xc", 137)):
        index = vwl.rust_usize(arrows, name)
        func = ui.u32(list_vt + 4 * index) - ui.base
        got = ui.last_ret(func)
        on_heading = any(x.endswith(f"+ {hex(4 * label_slot)}]") for x in ui.body(func, 0x200))
        check(got == want and on_heading,
              f"ListPanel slot {index} ({name}) returns with {got!r} and calls the heading's slot {label_slot}")

    # SetColumnVisible(int, bool): writes the column's hidden byte (+0x1d)
    # unless its unhidable byte (+0x1e) is set.
    visible_slot = vwl.rust_usize(src, "LIST_SLOT_SET_COLUMN_VISIBLE")
    func = ui.u32(list_vt + 4 * visible_slot) - ui.base
    body = ui.body(func, 0x120)
    check(ui.last_ret(func) == "ret 8"
          and any(re.fullmatch(r"mov byte ptr \[e[a-z]{2} \+ 0x1d\], [a-d]l", x) for x in body)
          and any("+ 0x1e]" in x for x in body),
          f"ListPanel slot {visible_slot} is SetColumnVisible (hidden byte +0x1d, unhidable +0x1e)")
    # ApplyItemChanges: re-index the row, then InvalidateLayout (slot 58).
    body = ui.body(ui.u32(list_vt + 4 * vwl.rust_usize(src, "LIST_SLOT_APPLY_ITEM_CHANGES")) - ui.base, 0x40)
    calls = [x for x in body if x.startswith("call")]
    check(len(calls) == 2 and re.fullmatch(r"call 0x[0-9a-f]+", calls[0]) and calls[1].endswith("+ 0xe8]"),
          f"ApplyItemChanges re-indexes the row and lays the list out again: {calls}")
    # DeleteAllItems: the window's own fill empties the list through it.
    delete_all = 4 * vwl.rust_usize(src, "LIST_SLOT_DELETE_ALL_ITEMS")
    check(any(re.fullmatch(rf"call dword ptr \[e[a-z]{{2}} \+ {hex(delete_all)}\]", x) for x in fill),
          f"file_dialog_fill empties the list through slot {delete_all // 4} (DeleteAllItems)")
    got = ui.last_ret(ui.u32(list_vt + delete_all) - ui.base)
    check(got == "ret", f"which returns with {got!r} (no arguments)")
    demoname = ui.base + ui.img.find(b"demoname\0")
    ctor = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[build["file_dialog_ctor"]:build["file_dialog_ctor"] + 0x400], ui.base + build["file_dialog_ctor"])]
    add = 4 * vwl.rust_usize(src, "LIST_SLOT_ADD_COLUMN_HEADER")
    at = ctor.index(f"push {hex(demoname)}") if f"push {hex(demoname)}" in ctor else None
    near = ctor[at:at + 4] if at is not None else []
    check(any(re.fullmatch(rf"call dword ptr \[e[a-z]{{2}} \+ {hex(add)}\]", x) for x in near),
          f"the constructor adds its \"demoname\" column through that slot: {near}")
    sortable = ui.u32(list_vt + 4 * vwl.rust_usize(src, "LIST_SLOT_SET_COLUMN_SORTABLE")) - ui.base
    pushed = [ui.img[int(x.split()[1], 16) - ui.base:][:16].split(b"\0")[0]
              for x in ui.body(sortable, 0x60) if re.fullmatch(r"push 0x1[0-9a-f]{7}", x)]
    check(b"SetSortColumn" in pushed, f"SetColumnSortable makes a SetSortColumn command: {pushed}")
    # A row is `new KeyValues("data", "demoname", name)`, which calls SetString.
    data = ui.base + ui.img.find(b"\0data\0") + 1
    rows = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(
        ui.img[build["file_dialog_fill"]:build["file_dialog_fill"] + 0x400], ui.base + build["file_dialog_fill"])]
    k = next((n for n in range(len(rows) - 3) if rows[n] == f"push {hex(data)}"), None)
    kv_ctor = int(next(x for x in rows[k:k + 4] if x.startswith("call ")).split()[1], 16) - ui.base if k else 0
    body = ui.body(kv_ctor, 0x40)
    kv_vt = next((int(x.split(", ")[1], 16) - ui.base for x in body if x.startswith("mov dword ptr [esi], 0x")), 0)
    calls_in = [int(x.split()[1], 16) - ui.base for x in body if re.fullmatch(r"call 0x[0-9a-f]+", x)]
    set_string = calls_in[-1] if calls_in else 0
    index = vwl.rust_usize(src, "KEYVALUES_SLOT_SET_STRING")
    check(kv_vt and ui.u32(kv_vt + 4 * index) - ui.base == set_string,
          f"KeyValues slot {index} is the SetString the row constructor +{kv_ctor:#x} calls (+{set_string:#x})")
    got = ui.last_ret(set_string)
    check(got == "ret 8", f"which returns with {got!r} (key, value)")

    # 20: the Highlights tab makes its rows as the fill does: KEYVALUES_SIZE
    # bytes from keyvalues_new, keyvalues_ctor, then AddItem.
    check(kv_ctor == build["keyvalues_ctor"],
          f"the fill's row constructor is keyvalues_ctor +{build['keyvalues_ctor']:#x} (+{kv_ctor:#x})")
    got = ui.last_ret(kv_ctor)
    check(got == "ret 0xc", f"which returns with {got!r} (name, key, value)")
    size = vwl.rust_usize(src, "KEYVALUES_SIZE")
    before = rows[max(0, (k or 0) - 30):(k or 0)]
    new_at = [n for n, x in enumerate(before) if x == f"call {hex(ui.base + build['keyvalues_new'])}"]
    check(bool(new_at) and before[new_at[-1] - 1] == f"push {hex(size)}",
          f"the fill allocates each row's {size:#x} bytes through keyvalues_new +{build['keyvalues_new']:#x}")
    add_item = 4 * vwl.rust_usize(src, "LIST_SLOT_ADD_ITEM")
    check(any(re.fullmatch(rf"call dword ptr \[e[a-z]{{2}} \+ {hex(add_item)}\]", x) for x in rows[k:k + 16]),
          f"and hands it to AddItem, ListPanel slot {add_item // 4}")
    got = ui.last_ret(ui.u32(list_vt + add_item) - ui.base)
    check(got == "ret 0x10", f"which returns with {got!r} (data, userData, scrollTo, sortOnAdd)")

    # 22: the Demos tab's Type dropdown. The labeled combo box in the
    # Options dialog adds its items through slot COMBO_SLOT_ADD_ITEM's
    # function on PRE; on both builds that function makes the "SetText"
    # KeyValues each item carries.
    combo_vt = ui.vftable("ComboBox@vgui2")
    check(combo_vt == build["combo_box_vftable"],
          f"ComboBox's vftable is combo_box_vftable +{(combo_vt or 0):#x}")
    add_slot = vwl.rust_usize(src, "COMBO_SLOT_ADD_ITEM")
    add_fn = ui.u32(combo_vt + 4 * add_slot) - ui.base if combo_vt else 0
    pushed = [ui.img[int(x.split()[1], 16) - ui.base:][:16].split(b"\0")[0]
              for x in ui.body(add_fn, 0x80) if re.fullmatch(r"push 0x1[0-9a-f]{7}", x)]
    check(b"SetText" in pushed and b"text" in pushed,
          f"ComboBox slot {add_slot} (AddItem(const char *, KeyValues *)) makes a SetText item: {pushed}")
    row_slot = vwl.rust_usize(src, "COMBO_SLOT_ACTIVATE_ITEM_BY_ROW")
    row_fn = ui.u32(combo_vt + 4 * row_slot) - ui.base if combo_vt else 0
    body = ui.body(row_fn, 0x20)
    jumps = [x for x in body if re.fullmatch(r"jmp dword ptr \[eax \+ 0x[0-9a-f]+\]", x)]
    menu_vt = ui.vftable("Menu@vgui2")
    ok = False
    if jumps and menu_vt:
        menu_slot = int(jumps[0].split("+ ")[1].rstrip("]"), 16)
        menu_fn = ui.u32(menu_vt + menu_slot) - ui.base
        menu_body = ui.body(menu_fn, 0x80)
        # The menu's ActivateItemByRow turns the row into an item id and hands
        # it to ActivateItem, the slot before it.
        before = hex(menu_slot - 4)
        ok = any(x.endswith(f"+ {before}]") and x.split()[0] in ("jmp", "call") for x in menu_body)
    check(ok, f"ComboBox slot {row_slot} forwards to the menu's ActivateItemByRow, which calls ActivateItem: {jumps}")
    text_vt = ui.vftable("TextEntry@vgui2")
    get_text = vwl.rust_usize(src, "TEXT_ENTRY_SLOT_GET_TEXT")
    check(combo_vt and ui.u32(combo_vt + 4 * get_text) == ui.u32(text_vt + 4 * get_text),
          f"ComboBox reads its text through TextEntry's slot {get_text}")

    # 24: the Player box is emptied after a pick through TextEntry's
    # SetText(const char *), the slot that looks for a "#" localisation
    # token before handing on to the wide SetText.
    set_text = vwl.rust_usize(src, "TEXT_ENTRY_SLOT_SET_TEXT")
    set_fn = ui.u32(text_vt + 4 * set_text) - ui.base if text_vt else 0
    looks_for_hash = any(re.search(r"cmp (byte ptr \[\w+\]|\w+), 0x23$", x) for x in ui.body(set_fn, 0x60))
    check(looks_for_hash and combo_vt and ui.u32(combo_vt + 4 * set_text) == ui.u32(text_vt + 4 * set_text),
          f"TextEntry slot {set_text} is SetText(const char *) (checks for a # token), and ComboBox inherits it")

    # 23: the Player dropdown. DeleteAllItems hands on to the drop-down menu
    # kept at combo_menu, whose own walks the items marking each for deletion
    # (Panel slot 71); OnCommand("ButtonClicked") is what opens the list.
    clear_slot = vwl.rust_usize(src, "COMBO_SLOT_DELETE_ALL_ITEMS")
    clear_fn = ui.u32(combo_vt + 4 * clear_slot) - ui.base if combo_vt else 0
    clear = ui.body(clear_fn, 0x20)
    menu_at = re.fullmatch(r"mov ecx, dword ptr \[ecx \+ (0x[0-9a-f]+)\]", clear[0]) if clear else None
    check(menu_at and int(menu_at.group(1), 16) == build["combo_menu"],
          f"ComboBox slot {clear_slot} reads its menu at combo_menu: {clear[:1]}")
    hop = [x for x in clear if re.fullmatch(r"jmp dword ptr \[eax \+ 0x[0-9a-f]+\]", x)]
    marks = False
    if hop and menu_vt:
        menu_fn = ui.u32(menu_vt + int(hop[0].split("+ ")[1].rstrip("]"), 16)) - ui.base
        marks = any(x.endswith("+ 0x11c]") and x.startswith("call") for x in ui.body(menu_fn, 0x80))
    check(marks, f"which hands on to the menu's DeleteAllItems, marking each item for deletion: {hop}")
    on_cmd = ui.u32(combo_vt + 4 * on_command) - ui.base if combo_vt else 0
    pushed = []
    for x in ui.body(on_cmd, 0x30):
        for t in re.findall(r"0x1[0-9a-f]{7}", x):
            r = int(t, 16) - ui.base
            if 0 < r < len(ui.img):
                pushed.append(ui.img[r:r + 20].split(b"\x00")[0])
    check(b"ButtonClicked" in pushed,
          f"ComboBox's OnCommand (slot {on_command}) answers ButtonClicked: {pushed[:3]}")

    # 21: the Highlights tab's progress bar.
    bar_vt = ui.vftable("ProgressBar@vgui2")
    check(bar_vt == build["progress_bar_vftable"],
          f"ProgressBar's vftable is progress_bar_vftable +{(bar_vt or 0):#x}")
    set_progress = vwl.rust_usize(src, "PROGRESS_BAR_SLOT_SET_PROGRESS")
    func = ui.u32(bar_vt + 4 * set_progress) - ui.base if bar_vt else 0
    body = ui.body(func, 0x20)
    loads_float = any(x in ("fld dword ptr [esp + 4]", "movss xmm1, dword ptr [ebp + 8]") for x in body[:3])
    check(loads_float and ui.last_ret(func) == "ret 4",
          f"ProgressBar slot {set_progress} (SetProgress(float)) takes a float: {body[:3]}")
    check(ui.img.find(b"ProgressBar\0") > 0, "a .res can name the ProgressBar control")

    # 16: the Playback tab's time box.
    text_vt = ui.vftable("TextEntry@vgui2")
    get_text = vwl.rust_usize(src, "TEXT_ENTRY_SLOT_GET_TEXT")
    got = ui.last_ret(ui.u32(text_vt + 4 * get_text) - ui.base)
    check(got == "ret 8", f"TextEntry's slot {get_text} (GetText(buf, len)) returns with {got!r}")

    # 17: the help line's Label::SetText.
    label_vt = ui.vftable("Label@vgui2")
    set_text = vwl.rust_usize(src, "LABEL_SLOT_SET_TEXT")
    got = ui.last_ret(ui.u32(label_vt + 4 * set_text) - ui.base)
    check(got == "ret 4", f"Label's slot {set_text} (SetText(const char *)) returns with {got!r}")

    # 18: the Commands tab's RichText::SetText, which RichText's own
    # ApplySettings (slot 80) calls with a .res "text".
    rich_vt = ui.vftable("RichText@vgui2")
    apply = ui.u32(rich_vt + 4 * 80) - ui.base
    lines = list(ui.md.disasm(ui.img[apply:apply + 0x300], ui.base + apply))
    after_text = False
    found = None
    for k, i in enumerate(lines):
        if i.mnemonic == "push" and i.op_str.startswith("0x"):
            at = int(i.op_str, 16) - ui.base
            if 0 < at < len(ui.img) and ui.img[at:at + 5] == b"text\0":
                after_text = True
        if (after_text and i.mnemonic == "call" and i.op_str.startswith("0x") and k >= 2
                and lines[k - 1].op_str in ("ecx, esi", "ecx, edi") and lines[k - 2].mnemonic == "push"):
            found = int(i.op_str, 16) - ui.base
            break
    # ApplySettings converts the text and calls SetText(const wchar_t *)
    # (Anniversary), or calls SetText(const char *), which converts through a
    # 0x800-byte buffer and calls it (pre-Anniversary).
    wide = build["rich_text_set_text_wide"]
    via = found
    if found is not None and found != wide:
        body = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[found:found + 0x80], ui.base + found)]
        if "push 0x800" in body and f"call {hex(ui.base + wide)}" in body:
            via = wide
    check(via == wide, f"RichText's ApplySettings reaches rich_text_set_text_wide +{wide:#x} (via +{(found or 0):#x})")
    got = ui.last_ret(wide)
    check(got == "ret 4", f"which returns with {got!r} (const wchar_t *)")

    # 15: the Settings tab's check boxes.
    check_vt = ui.vftable("CheckButton@vgui2")
    check(check_vt == build["check_button_vftable"], f"CheckButton's vftable is check_button_vftable +{(check_vt or 0):#x}")
    set_sel = vwl.rust_usize(src, "BUTTON_SLOT_SET_SELECTED")
    is_sel = vwl.rust_usize(src, "BUTTON_SLOT_IS_SELECTED")
    setter = ui.u32(check_vt + 4 * set_sel) - ui.base
    msg = ui.base + ui.img.find(b"CheckButtonChecked\0")
    body = [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[setter:setter + 0x100], ui.base + setter)]
    check(f"push {hex(msg)}" in body and ui.last_ret(setter) == "ret 4",
          f"CheckButton's slot {set_sel} (SetSelected) posts CheckButtonChecked and pops one argument")
    getter = ui.body(ui.u32(check_vt + 4 * is_sel) - ui.base, 0x10)
    field = re.fullmatch(r"mov al, byte ptr \[ecx \+ (0x[0-9a-f]+)\]", getter[0]) if getter else None
    base_set = next((int(t.split()[1], 16) - ui.base for t in body if re.fullmatch(r"call 0x[0-9a-f]+", t)
                     and any(f"byte ptr [esi + {field.group(1)}], al" in x for x in ui.body(int(t.split()[1], 16) - ui.base, 0x30))), None) if field else None
    check(field and getter[1] == "ret" and base_set is not None,
          f"slot {is_sel} (IsSelected) reads the byte Button::SetSelected writes (this+{field.group(1) if field else '?'})")

    # InvalidItemID (one past NextItem) returns -1, which pins the order.
    invalid = ui.body(ui.u32(list_vt + 4 * (vwl.rust_usize(src, "LIST_SLOT_NEXT_ITEM") + 1)) - ui.base, 0x10)
    check(invalid[:2] == ["or eax, 0xffffffff", "ret"], f"ListPanel's InvalidItemID sits after NextItem: {invalid[:2]}")

    # 13: the two IPanel slots only this window uses (the rest are #410's).
    vg = vwl.Image(game / "vgui2.dll")
    wrapper = vg.vftable("VPanelWrapper")
    get_active = vwl.rust_usize(src, "SHEET_SLOT_GET_ACTIVE_PAGE")
    body = ui.body(ui.u32(sheet_vt + 4 * get_active) - ui.base, 0x10)
    check(len(body) == 2 and body[0].startswith("mov eax, dword ptr [ecx +") and body[1] == "ret",
          f"PropertySheet's slot {get_active} (GetActivePage) just returns a field: {body}")
    key = vwl.rust_usize(src, "PANEL_SLOT_ON_KEY_CODE_TYPED")
    got = ui.last_ret(ui.u32(page_vt + 4 * key) - ui.base)
    check(got == "ret 4", f"PropertyPage's slot {key} (OnKeyCodeTyped) returns with {got!r}")
    for name, want in (("IPANEL_SET_MINIMUM_SIZE", "ret 0xc"), ("IPANEL_SET_PARENT", "ret 8"),
                       ("IPANEL_REQUEST_FOCUS", "ret 8"), ("IPANEL_GET_ABS_POS", "ret 0xc"),
                       ("IPANEL_SET_KEYBOARD_INPUT_ENABLED", "ret 8")):
        index = vwl.rust_usize(src, name)
        got = vg.last_ret(vg.u32(wrapper + 4 * index) - vg.base)
        check(got == want, f"IPanel slot {index} ({name}) returns with {got!r}")

    # BaseUI001: GameUI keeps the engine's IBaseUI in a global, calls the slot
    # before BASEUI_SLOT_ACTIVATE_GAME_UI (HideGameUI) after an `engine ...`
    # menu command, and calls ActivateGameUI itself somewhere.
    activate_ui = vwl.rust_usize(src, "BASEUI_SLOT_ACTIVATE_GAME_UI")

    def pushes(text):
        at = ui.img.find(text.encode() + b"\0")
        return [m.start() for m in re.finditer(re.escape(b"\x68" + struct.pack("<I", ui.base + at)), ui.img)]

    def disasm(rva, size):
        return [f"{i.mnemonic} {i.op_str}" for i in ui.md.disasm(ui.img[rva:rva + size], ui.base + rva)]

    def stored_result(at):
        # `push "BaseUI001"; [store of the previous result]; call; mov [g], eax`
        after = disasm(at, 0x20)
        call = next((n for n, t in enumerate(after) if t.startswith("call ")), len(after))
        return next((m.group(1) for t in after[call + 1:]
                     if (m := re.fullmatch(r"mov dword ptr \[(0x[0-9a-f]+)\], eax", t))), None)

    store = next(filter(None, map(stored_result, pushes("BaseUI001"))), None)
    check(store, f"GameUI keeps BaseUI001 at {store}")
    loads = [m.start() for m in re.finditer(re.escape(b"\x8b\x0d" + struct.pack("<I", int(store or "0", 16))), ui.img)]

    def calls_slot(at, index):
        return any(re.fullmatch(rf"call dword ptr \[e\w\w \+ {index * 4:#x}\]", t) for t in disasm(at, 0x18)[1:6])

    engine_sites = pushes("engine ")
    check(any(calls_slot(at, activate_ui - 1) for at in loads
              if any(0 < at - site < 0x80 for site in engine_sites)),
          f"after an `engine ...` menu command GameUI calls BaseUI slot {activate_ui - 1} (HideGameUI)")
    check(any(calls_slot(at, activate_ui) for at in loads),
          f"and GameUI calls BaseUI slot {activate_ui} (ActivateGameUI) itself")

    # The main menu's items run in CTaskbar's OnCommand: the `engine ...`
    # handling above sits in its body.
    taskbar_vt = ui.vftable("CTaskbar")
    handler = taskbar_vt and ui.u32(taskbar_vt + 4 * on_command) - ui.base
    check(handler and any(0 < site - handler < 0x1000 for site in engine_sites),
          f"CTaskbar's slot {on_command} (OnCommand, +{handler or 0:#x}) handles `engine ...` menu commands")
    return ok


def constructor_of(ui, cls):
    """The function that stores `cls`'s vftable and has callers: its constructor."""
    vt = ui.vftable(cls)
    if vt is None:
        return None
    needle = struct.pack("<I", ui.base + vt)
    for m in re.finditer(re.escape(needle), ui.img):
        at = m.start()
        if at >= ui.code[1]:
            continue
        for start in range(at, at - 0x600, -1):
            if ui.img[start - 1] in (0xCC, 0x90, 0xC3) and ui.img[start] in (0x55, 0x53, 0x56, 0x57, 0x6A, 0x8B, 0x51, 0x83, 0x64):
                # A byte pair inside an instruction can look like a function
                # start; only one with callers is taken.
                if ui.calls_to(start):
                    return start
    return None


def main():
    games = [Path(a) for a in sys.argv[1:]] or [g for g in vwl.DEFAULT_GAMES if g.is_dir()]
    src = RUST.read_text(encoding="utf-8")
    ok = all([verify(g, src) for g in games])
    print("\nOFFSETS VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
