#!/usr/bin/env python3
"""Checks `hltv_fov.rs`'s addresses against real `client.dll` files (#830).

`hltv_fov.rs` clears the client's per-player HLTV zoom table, which it finds
through the `__MsgFunc_HLTV` thunk. The unit tests prove the module reads the
thunk's bytes the way it means to; only the binaries can prove:

  1. The thunk at THUNK_RVA is `mov eax/ecx/edx, [esp+..]; push x3;
     push gHUD; call CHud::MsgFunc_HLTV`, and the call lands on HANDLER_RVA.
  2. `gHUD` is `+0x1080b0`, and the handler writes one player's value at
     `[this + player*4 + TABLE_OFFSET]` (STORE_RVA) and all SLOTS of them
     with `lea edi, [ecx + TABLE_OFFSET]; mov ecx, SLOTS; rep stosd`.
  3. `CHud::Think` reads `[esi + g_iUser2*4 + TABLE_OFFSET]` at READ_RVA.
  4. No other instruction in `.text` addresses the table: a `[reg+reg*4+0x70]`
     operand outside the handler and `Think`, or an absolute operand inside
     the table, would be another writer the module doesn't know of.

Constants are read out of `hltv_fov.rs` rather than restated.

Usage:
    python -I goldsrc-hooks/tools/verify_hltv_fov_offsets.py [client.dll ...]

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
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "hltv_fov.rs"

GHUD = 0x1080B0
# `CHud::Think`'s HLTV block, which holds the one read.
THINK_FROM, THINK_TO = 0x36D72, 0x36DA4


def constants():
    text = RUST.read_text(encoding="utf-8")
    out = {}
    for name in ("THUNK_RVA", "HANDLER_RVA", "STORE_RVA", "READ_RVA", "TABLE_OFFSET", "SLOTS"):
        m = re.search(rf"const {name}: usize = (0x[0-9a-f_]+|\d+);", text)
        if not m:
            sys.exit(f"{name} not found in {RUST}")
        out[name] = int(m.group(1).replace("_", ""), 0)
    return out


def check(path, c):
    pe = pefile.PE(str(path))
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    problems = []

    def disasm(rva, n):
        return list(md.disasm(img[rva : rva + n], base + rva))

    thunk = disasm(c["THUNK_RVA"], 25)
    text = " ; ".join(f"{i.mnemonic} {i.op_str}" for i in thunk)
    want = (
        "mov eax, dword ptr [esp + 0xc] ; mov ecx, dword ptr [esp + 8] ; "
        "mov edx, dword ptr [esp + 4] ; push eax ; push ecx ; push edx ; "
        f"push {base + GHUD:#x} ; call {base + c['HANDLER_RVA']:#x}"
    )
    if text != want:
        problems.append(f"thunk is `{text}`")

    store = disasm(c["STORE_RVA"], 8)
    if [f"{i.mnemonic} {i.op_str}" for i in store] != [
        "mov eax, dword ptr [esp + 0xc]",
        f"mov dword ptr [eax + esi*4 + {c['TABLE_OFFSET']:#x}], ebx",
    ]:
        problems.append("the one-player store is not at STORE_RVA")

    handler = disasm(c["HANDLER_RVA"], 0xB1)
    joined = " ; ".join(f"{i.mnemonic} {i.op_str}" for i in handler)
    fill = (
        f"lea edi, [ecx + {c['TABLE_OFFSET']:#x}] ; mov ecx, {c['SLOTS']:#x} ; "
        "rep stosd dword ptr es:[edi], eax"
    )
    if fill not in joined:
        problems.append("the all-players fill is not `lea edi, [ecx+0x70]; mov ecx, 0x40; rep stosd`")

    read = disasm(c["READ_RVA"], 4)
    if not read or f"{read[0].mnemonic} {read[0].op_str}" != (
        f"mov eax, dword ptr [esi + eax*4 + {c['TABLE_OFFSET']:#x}]"
    ):
        problems.append("Think's read is not at READ_RVA")

    # Every other reference to the table, by register or absolute address.
    sect = next(s for s in pe.sections if s.Characteristics & 0x20000000)
    start, size = sect.VirtualAddress, sect.Misc_VirtualSize
    table_lo = base + GHUD + c["TABLE_OFFSET"]
    table_hi = table_lo + c["SLOTS"] * 4
    handler_span = (c["HANDLER_RVA"], c["HANDLER_RVA"] + 0xB1)
    md.skipdata = True
    for i in md.disasm(img[start : start + size], base + start):
        rva = i.address - base
        if i.id == 0 or handler_span[0] <= rva < handler_span[1] or THINK_FROM <= rva < THINK_TO:
            continue
        for op in i.operands:
            if op.type != capstone.x86.X86_OP_MEM:
                continue
            m = op.mem
            if m.index != 0 and m.scale == 4 and m.disp == c["TABLE_OFFSET"] and m.base != 0:
                if m.base != capstone.x86.X86_REG_ESP:
                    problems.append(f"another [reg+reg*4+0x70] at +{rva:#x}: {i.mnemonic} {i.op_str}")
            if m.base == 0 and table_lo <= (m.disp & 0xFFFFFFFF) < table_hi:
                problems.append(f"an absolute reference into the table at +{rva:#x}")
    return problems


def main():
    c = constants()
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    failed = False
    for dll in dlls:
        if not dll.exists():
            print(f"SKIP {dll} (not found)")
            continue
        problems = check(dll, c)
        if problems:
            failed = True
            print(f"FAIL {dll}")
            for p in problems:
                print(f"  - {p}")
        else:
            print(f"OK   {dll}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
