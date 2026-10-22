#!/usr/bin/env python3
"""Checks `spectator_bars.rs`'s IPanel slots against real `vgui2.dll` files.

`spectator_bars.rs` swaps vgui2's `IPanel::PaintTraverse` vtable slot for a
filter (issue #328), and calls `GetName` through the same table. The unit
tests prove the filter's own logic; only the binaries can prove the slot
numbers:

  1. `vgui2.dll` has exactly one RTTI class `.?AVVPanelWrapper@@`, with one
     vtable of 60 slots, and it is the vtable of the `VGUI_Panel007` singleton
     (the interface name string is in the file).
  2. Each of the two slots the module uses ends in the `ret` its argument
     count demands, found by the *same* first-`ret` scan the DLL runs at
     install (so a build where that scan would misread is caught here).
  3. Each forwards where it should: `GetName` into the VPanel object's own
     vtable (+0x88), `PaintTraverse` to the client panel's vtable +0xc after
     fetching it through +0xe8 (`Client()`).

The slot numbers and ret sizes are read out of `spectator_bars.rs` rather
than restated.

Usage:
    python goldsrc-hooks/tools/verify_vgui2_ipanel.py [vgui2.dll ...]

Defaults to both movie installs. Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

try:
    import capstone
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile capstone`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "vgui2.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "vgui2.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "spectator_bars.rs"
CLASS = b".?AVVPanelWrapper@@"
RET_WINDOW = 64
# What each wrapper must forward through, as displacements in its calls.
FORWARDS = {"GET_NAME": [0x88], "PAINT_TRAVERSE": [0xE8, 0x0C]}


def constants():
    text = RUST.read_text(encoding="utf-8")
    out = {}
    for name in FORWARDS:
        slot = re.search(rf"const SLOT_{name}: usize = (\d+);", text)
        ret = re.search(rf"const RET_{name}: u16 = (0x[0-9a-f]+|\d+);", text)
        if not slot or not ret:
            sys.exit(f"SLOT_{name} / RET_{name} not found in {RUST}")
        out[name] = (int(slot.group(1)), int(ret.group(1), 0))
    count = re.search(r"const SLOT_COUNT: usize = (\d+);", text)
    window = re.search(r"const RET_WINDOW: usize = (\d+);", text)
    if not count or not window or int(window.group(1)) != RET_WINDOW:
        sys.exit("SLOT_COUNT / RET_WINDOW not found, or RET_WINDOW differs from this script's")
    return out, int(count.group(1))


def first_ret_size(code):
    """The same scan as `spectator_bars::first_ret_size`."""
    i = 0
    while i < len(code):
        if code[i] == 0xC3:
            return 0
        if code[i] == 0xC2 and i + 2 < len(code) and code[i + 2] == 0:
            return code[i + 1]
        i += 1
    return None


def check(path, slots, slot_count):
    pe = pefile.PE(str(path), fast_load=True)
    img = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase
    text = next(s for s in pe.sections if s.Characteristics & 0x20000000)
    code_lo, code_hi = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize

    if b"VGUI_Panel007" not in img:
        return "FAIL -- no VGUI_Panel007 string"
    descriptors = [m.start() - 8 for m in re.finditer(re.escape(CLASS) + b"\x00", img)]
    if len(descriptors) != 1:
        return f"FAIL -- {len(descriptors)} type descriptors for {CLASS.decode()}"
    # Complete object locators point at the descriptor from +0xc; a vtable's
    # slot -1 points at its locator.
    td = struct.pack("<I", base + descriptors[0])
    locators = [m.start() - 0xC for m in re.finditer(re.escape(td), img)
                if struct.unpack_from("<I", img, m.start() - 0xC)[0] == 0]
    vtables = []
    for loc in locators:
        for m in re.finditer(re.escape(struct.pack("<I", base + loc)), img):
            vtables.append(m.start() + 4)
    if len(vtables) != 1:
        return f"FAIL -- {len(vtables)} vtables for {CLASS.decode()}"
    vt = vtables[0]

    def slot(i):
        return struct.unpack_from("<I", img, vt + i * 4)[0] - base

    count = 0
    while code_lo <= slot(count) < code_hi:
        count += 1
    if count != slot_count:
        return f"FAIL -- the vtable has {count} slots, the module assumes {slot_count}"

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    parts = []
    for name, (index, ret) in slots.items():
        rva = slot(index)
        code = img[rva:rva + RET_WINDOW]
        got = first_ret_size(code)
        if got != ret:
            return f"FAIL -- slot {index} ({name}) at +{rva:#x}: the scan reads ret {got}, wants {ret:#x}"
        # A real disassembly must agree with the byte scan, and show the forwards.
        disps, real_ret = [], None
        for ins in md.disasm(code, rva):
            if ins.mnemonic == "ret":
                real_ret = int(ins.op_str, 0) if ins.op_str else 0
                break
            for op in ins.operands:
                if op.type == capstone.x86.X86_OP_MEM and op.mem.disp and ins.mnemonic in ("call", "mov", "jmp"):
                    disps.append(op.mem.disp)
        if real_ret != ret:
            return f"FAIL -- slot {index} ({name}): disassembles to ret {real_ret}, the scan said {got}"
        missing = [d for d in FORWARDS[name] if d not in disps]
        if missing:
            return (f"FAIL -- slot {index} ({name}) at +{rva:#x} doesn't forward through "
                    f"{[hex(d) for d in missing]} (saw {[hex(d) for d in disps]})")
        parts.append(f"{name.lower()} slot {index} +{rva:#x} ret {ret:#x}")
    return f"OK -- vtable +{vt:#x}, {count} slots; " + "; ".join(parts)


def main():
    dlls = [Path(a) for a in sys.argv[1:]] or DEFAULT_DLLS
    slots, slot_count = constants()
    failed = False
    for dll in dlls:
        result = check(dll, slots, slot_count)
        failed |= result.startswith("FAIL")
        print(f"{dll.parent.name}: {result}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
