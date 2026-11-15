#!/usr/bin/env python3
"""Checks `deathmsg/name_cap.rs` against a real DoD `client.dll` (#1021).

`name_cap.rs` lifts the kill feed's 31-character name cap: it rewrites the
`push 0x20` length of the two `strncpy`s in `MsgFunc_DeathMsg` to 0x40 and
moves the two terminator writes after them from `[31]` to `[63]` of
`szKiller` / `szVictim`. Nothing at runtime can prove those RVAs land on the
operands the Rust assumes, so this does:

  1. Each `LEN_SITES` RVA is the imm8 of a `push 0x20`, the first of the
     three pushes before a `call` -- the same callee at both sites, the copy
     -- and the second push before it in each case is the destination,
     `szKiller` (`[esi + array]`) then `szVictim` (`[esi + array + 0x40]`).
  2. Each `TERMINATOR_SITES` RVA is the disp32 of `mov byte ptr [esi +
     disp32], 0`, pointing at item 0's field as the Rust says (0x1f, 0x5f),
     right after that call.
  3. The lifted terminators (0x3f, 0x7f) stay inside their 64-byte names:
     the array references `deathmsg.rs` lists put `szVictim` at 0x40 and
     `iId` at 0x80.
  4. With the four operands rewritten, `MsgFunc_DeathMsg` decodes to the same
     instruction sequence.
  5. `client.dll`'s copy of the engine table calls slot 21
     (`pfnGetPlayerInfo`, `call [gEngfuncs+0x54]`) 19 times, the slot
     `player_names.rs` detours.

Usage:
    python goldsrc-hooks/tools/verify_killfeed_name_cap_offsets.py [client.dll ...]

Defaults to both movie installs' `client.dll` (byte-identical). Needs
`pip install pefile capstone`.
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
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
SRC = Path(__file__).resolve().parent.parent / "src"
RUST = SRC / "deathmsg" / "name_cap.rs"
DEATHMSG = SRC / "deathmsg.rs"

# `MsgFunc_DeathMsg`, as verify_deathmsg_offsets.py bounds it.
MSGFUNC = (0x2B1A0, 0x2B4D2)
# `gEngfuncs` in client.dll: the table `Initialize` copies the engine's into.
GENGFUNCS_RVA = 0x176388
GET_PLAYER_INFO_SLOT = 21
GET_PLAYER_INFO_CALLS = 19


def num(text):
    return int(text.replace("_", ""), 0)


def rust_len_sites(src):
    body = re.search(r"const LEN_SITES: \[usize; 2\] = \[(.*?)\];", src, re.S).group(1)
    return [num(v) for v in re.findall(r"0x[0-9a-fA-F_]+", body)]


def rust_terminators(src):
    body = re.search(r"const TERMINATOR_SITES: \[\(usize, usize\); 2\] = \[(.*?)\];", src, re.S).group(1)
    return [(num(a), num(b)) for a, b in re.findall(r"\((0x[0-9a-fA-F_]+), (0x[0-9a-fA-F_]+)\)", body)]


def rust_const(src, name):
    m = re.search(rf"const {name}: \w+ = (0x[0-9a-fA-F_]+|\d+);", src)
    if not m:
        raise SystemExit(f"could not find `const {name}`")
    return num(m.group(1))


def check(dll, src, deathmsg):
    ok = True

    def say(passed, message):
        nonlocal ok
        ok &= bool(passed)
        print(("  OK   " if passed else "  FAIL ") + message)

    print(f"\n==== {dll} ====")
    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = bytes(pe.get_memory_mapped_image())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    array = base + rust_const(deathmsg, "ARRAY_RVA")
    stock = rust_const(src, "STOCK_LEN")
    lifted = rust_const(src, "LIFTED_LEN")
    shift = lifted - stock

    decoded = {}
    order = []
    for ins in md.disasm(img[MSGFUNC[0]:MSGFUNC[1]], base + MSGFUNC[0]):
        decoded[ins.address - base] = ins
        order.append(ins.address - base)

    def text(rva):
        ins = decoded.get(rva)
        return f"{ins.mnemonic} {ins.op_str}" if ins else "not an instruction boundary"

    # -- 1. the lengths -----------------------------------------------------
    callees = set()
    dests = [f"esi + {array:#x}", f"esi + {array + 0x40:#x}"]
    lens = rust_len_sites(src)
    for site, dest in zip(lens, dests):
        at = site - 1
        say(text(at) == f"push {stock:#x}" and img[site] == stock,
            f"+{at:#x} is `push {stock:#x}` with its imm8 at +{site:#x}: {text(at)}")
        i = order.index(at) if at in decoded else None
        if i is None:
            continue
        window = [text(r) for r in order[i:i + 6]]
        call = next((w for w in window if w.startswith("call 0x")), None)
        callees.add(call)
        pushes = [w for w in window[:4] if w.startswith("push ")]
        lea = [text(r) for r in order[max(0, i - 3):i] if text(r).startswith("lea ")]
        say(call is not None and len(pushes) == 3, f"    three pushes then `{call}`: {window}")
        say(any(dest in t for t in lea), f"    its destination is [{dest}] ({lea})")
    say(len(callees) == 1, f"both copies call the same function: {sorted(c or '-' for c in callees)}")

    # -- 2. the terminators -------------------------------------------------
    terms = rust_terminators(src)
    for (site, field), length in zip(terms, lens):
        at = site - 2
        want = f"mov byte ptr [esi + {array + field:#x}], 0"
        ins = decoded.get(at)
        say(ins is not None and text(at) == want and ins.encoding.disp_offset == 2,
            f"+{at:#x} is `{want}` with its disp32 at +{site:#x}: {text(at)}")
        say(0 < site - length < 0x20, f"    it follows its copy (+{length - 1:#x})")

    # -- 3. the lifted fields -----------------------------------------------
    refs = {num(a): num(b) for a, b in re.findall(r"\((0x[0-9a-f_]+),\s*(0x[0-9a-f_]+)\)",
                                                   deathmsg.split("const ARRAY_REFS")[1].split("];")[0])}
    fields = set(refs.values())
    say(0x40 in fields and 0x80 in fields, "deathmsg.rs puts szVictim at 0x40 and iId at 0x80")
    for site, field in terms:
        say(refs.get(site) == field, f"+{site:#x} is ARRAY_REFS' entry with field {field:#x}")
    say(terms[0][1] + shift == 0x3F and terms[1][1] + shift == 0x7F,
        f"lifted, the terminators land at 0x{terms[0][1] + shift:x} and 0x{terms[1][1] + shift:x}: "
        "the last byte of each 64-byte name")

    # -- 4. the patched function still decodes ------------------------------
    patched = bytearray(img)
    for site in lens:
        patched[site] = lifted
    for site, field in terms:
        struct.pack_into("<I", patched, site, array + field + shift)
    before = [i.mnemonic for i in md.disasm(img[MSGFUNC[0]:MSGFUNC[1]], base + MSGFUNC[0])]
    after = [i.mnemonic for i in md.disasm(bytes(patched[MSGFUNC[0]:MSGFUNC[1]]), base + MSGFUNC[0])]
    say(before == after, "lifted, MsgFunc_DeathMsg decodes to the same instructions")

    # -- 5. the detoured slot -----------------------------------------------
    needle = b"\xff\x15" + struct.pack("<I", base + GENGFUNCS_RVA + 4 * GET_PLAYER_INFO_SLOT)
    calls = len(re.findall(re.escape(needle), img))
    say(calls == GET_PLAYER_INFO_CALLS,
        f"client.dll calls gEngfuncs slot {GET_PLAYER_INFO_SLOT} {calls} times (expected {GET_PLAYER_INFO_CALLS})")
    return ok


def main():
    dlls = [Path(a) for a in sys.argv[1:]] or [d for d in DEFAULT_DLLS if d.is_file()]
    if not dlls:
        print("no client.dll found")
        return 2
    src = RUST.read_text(encoding="utf-8")
    deathmsg = DEATHMSG.read_text(encoding="utf-8")
    ok = all([check(d, src, deathmsg) for d in dlls])
    print("\nALL OK" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
