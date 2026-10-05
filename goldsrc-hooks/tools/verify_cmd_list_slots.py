#!/usr/bin/env python3
"""Checks `demo_reload.rs`'s engine-command wrap against a real `hw.dll`.

`demo_reload.rs` walks the engine's command list through `cl_enginefunc_t`
slot 102 and swaps the `function` field of the `playdemo` and `viewdemo`
nodes (issue #330). It needs no per-build address, but it does rely on four
facts only the binary can prove:

  1. The engine's `cl_enginefunc_t` is where the SDK says: slots 38/39 are
     `Cmd_Argc`/`Cmd_Argv` and slot 20 is `pfnClientCmd`. The table is found
     by its shape -- `.text` pointers, then the six API-struct pointers at
     82..87 (`pTriAPI` .. `pVoiceTweak`), then `.text` again.
  2. Slot 102 returns the list head (`mov eax, [head]; ret`, possibly behind
     a `jmp`), slot 103 returns `[handle+0]` and slot 104 `[handle+4]`: the
     node's `next` and `name`.
  3. `Cmd_AddCommand` allocates 16 bytes per node and stores the handler at
     `+8` -- the field the wrap writes.
  4. The engine registers `playdemo` and `viewdemo` as commands at all.

The offsets come out of `demo_reload.rs` rather than being restated here.

Usage:
    python goldsrc-hooks/tools/verify_cmd_list_slots.py [path-to-hw.dll ...]

Defaults to both installs: the pre-Anniversary movies build and the stock
25th Anniversary one (read only). Needs `pip install pefile capstone`.
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
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll",
    STEAM / "Half-Life" / "hw.dll",
]
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_reload.rs"


def rust_const(name):
    m = re.search(rf"const {name}: usize = (\d+);", RUST.read_text(encoding="utf-8"))
    if not m:
        sys.exit(f"{name} not found in {RUST}")
    return int(m.group(1))


def verify(path):
    ok = True

    def check(cond, what):
        nonlocal ok
        print(f"  {'OK ' if cond else 'BAD'} {what}")
        ok &= bool(cond)

    pe = pefile.PE(str(path), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    print(f"\n{path}\n  TimeDateStamp {pe.FILE_HEADER.TimeDateStamp:#x}, "
          f"SizeOfImage {pe.OPTIONAL_HEADER.SizeOfImage:#x}")
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    t0, t1 = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize

    def u32(rva):
        return struct.unpack_from("<I", img, rva)[0]

    def is_text(va):
        return t0 <= va - base < t1

    def body(rva, limit=12):
        """Instructions from rva, following one leading jmp."""
        out = []
        for ins in md.disasm(img[rva:rva + 96], base + rva):
            if not out and ins.mnemonic == "jmp" and ins.op_str.startswith("0x"):
                return body(int(ins.op_str, 16) - base, limit)
            out.append(f"{ins.mnemonic} {ins.op_str}".strip())
            if ins.mnemonic == "ret" or len(out) >= limit:
                break
        return out

    # -- 1. the table ---------------------------------------------------------
    tables = []
    for sec in pe.sections:
        if sec.Name.startswith(b".text"):
            continue
        lo, hi = sec.VirtualAddress, sec.VirtualAddress + sec.Misc_VirtualSize
        for off in range(lo, hi - 4 * 110, 4):
            vals = [u32(off + 4 * i) for i in range(110)]
            if (all(is_text(v) for v in vals[:82])
                    and not any(is_text(v) for v in vals[82:88])
                    and all(v > base for v in vals[82:88])
                    and all(is_text(v) for v in vals[88:105])
                    and not is_text(u32(off - 4))):
                tables.append(off)
    check(len(tables) == 1, f"exactly one cl_enginefunc_t-shaped table: {[hex(t) for t in tables]}")
    if not tables:
        return False
    table = tables[0]

    def slot(n):
        return u32(table + 4 * n) - base

    argc = body(slot(38))
    check(any(re.match(r"mov eax, dword ptr \[0x[0-9a-f]+\]$", i) for i in argc) and argc[-1] == "ret",
          f"slot 38 (Cmd_Argc) returns a global: {argc}")
    argv = body(slot(39))
    check(any("[ebp + 8]" in i for i in argv), f"slot 39 (Cmd_Argv) takes an index: {argv[:6]}")
    client_cmd = rust_const("SLOT_CLIENT_CMD")
    check(client_cmd == 20, f"SLOT_CLIENT_CMD is 20 (pfnClientCmd), as engine.rs documents")

    # -- 2. the list walkers --------------------------------------------------
    first = rust_const("SLOT_GET_FIRST_CMD_FUNCTION_HANDLE")
    head = body(slot(first))
    check(len(head) == 2 and re.match(r"mov eax, dword ptr \[0x[0-9a-f]+\]$", head[0]) and head[1] == "ret",
          f"slot {first} returns the list head: {head}")
    nxt = body(slot(first + 1))
    check("mov eax, dword ptr [eax]" in nxt, f"slot {first + 1} returns [handle+0] (next): {nxt}")
    name = body(slot(first + 2))
    check("mov eax, dword ptr [eax + 4]" in name, f"slot {first + 2} returns [handle+4] (name): {name}")
    head_global = int(re.search(r"\[(0x[0-9a-f]+)\]", head[0]).group(1), 16) if head else None

    # -- 3. Cmd_AddCommand's node ---------------------------------------------
    message = img.find(b"Cmd_AddCommand: %s already defined\n\0")
    users = []
    if message >= 0:
        pattern = struct.pack("<I", base + message)
        i = img.find(pattern, t0, t1)
        while i != -1:
            users.append(i)
            i = img.find(pattern, i + 1, t1)
    check(users, f"`Cmd_AddCommand: %s already defined` is used at {[hex(u) for u in users]}")
    stores_function = False
    for use in users:
        start = img.rfind(b"\x55\x8b\xec", t0, use)
        text_ins = [f"{i.mnemonic} {i.op_str}" for i in md.disasm(img[start:use + 0x120], base + start)]
        allocs_16 = "push 0x10" in text_ins
        name_at_4 = any(re.match(r"mov dword ptr \[e\w\w \+ 4\], e\w\w$", t) for t in text_ins)
        fn_at_8 = any(re.match(r"mov dword ptr \[e\w\w \+ 8\], (e\w\w|0x[0-9a-f]+)$", t) for t in text_ins)
        links_head = head_global is not None and any(f"[{head_global:#x}]" in t for t in text_ins)
        if allocs_16 and name_at_4 and fn_at_8 and links_head:
            stores_function = True
            print(f"     +{start:#x}: 16-byte node, name at +4, handler at +8, linked into [{head_global:#x}]")
    check(stores_function, "a Cmd_AddCommand allocates a 16-byte node with the handler at +8 on the same list")

    # -- 4. the commands exist ------------------------------------------------
    for command in (b"playdemo", b"viewdemo"):
        check(img.find(command + b"\0") >= 0, f"`{command.decode()}` is a string in the engine")

    return ok


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    ok = all([verify(p) for p in dlls])
    print("\nCOMMAND LIST VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
