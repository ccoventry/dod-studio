#!/usr/bin/env python3
"""Checks `weapon_list.rs`'s signature against a real DoD `client.dll`.

`weapon_list.rs` rewrites one displacement byte in `CHudAmmo::DrawWeaponList`.
Every constant below is read out of `weapon_list.rs` rather than restated.

Checks:

  1. The signature matches exactly once in `.text`.
  2. `DISP_AT` is the displacement of a two-byte `jne rel8`, and the byte
     there is `DRAW`.
  3. Shipped, the `jne` jumps past the early return (`xor eax, eax` ...
     `ret 4`); with `SKIP` written it lands on that `xor`, and every later
     instruction still starts where it did.
  4. The global the function tests is `gpActiveSel`: the `invnext` handler
     (the function the client registers next to the "invnext" string) calls
     a function that writes it.
  5. The function has exactly one caller, which ignores what it returns (the
     next instruction after the call doesn't read `eax`).

Usage:
    python goldsrc-hooks/tools/verify_weapon_list_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install (POST's `client.dll` is
byte-identical). Requires `pefile` and `capstone`.
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

DEFAULT_DLL = Path(
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\dod\cl_dlls\client.dll"
)
RUST = Path(__file__).resolve().parent.parent / "src" / "weapon_list.rs"


def rust_str(src: str, name: str) -> str:
    m = re.search(rf'const {name}:\s*&str\s*=\s*"([^"]*)";', src)
    if not m:
        sys.exit(f"could not find {name} in weapon_list.rs")
    return m.group(1)


def rust_int(src: str, name: str, ty: str) -> int:
    m = re.search(rf'const {name}:\s*{ty}\s*=\s*([0-9_xXa-fA-F]+);', src)
    if not m:
        sys.exit(f"could not find {name} in weapon_list.rs")
    return int(m.group(1).replace("_", ""), 0)


def find_all(code: bytes, pattern) -> list[int]:
    n = len(pattern)
    return [
        i
        for i in range(len(code) - n + 1)
        if code[i] == pattern[0] and all(w is None or code[i + k] == w for k, w in enumerate(pattern))
    ]


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")
    src = RUST.read_text(encoding="utf-8")
    pattern_text = rust_str(src, "PATTERN")
    pattern = [None if t == "??" else int(t, 16) for t in pattern_text.split()]
    disp_at = rust_int(src, "DISP_AT", "usize")
    draw = rust_int(src, "DRAW", "u8")
    skip = rust_int(src, "SKIP", "u8")

    pe = pefile.PE(str(dll))
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    t0 = text.VirtualAddress
    code = img[t0 : t0 + text.Misc_VirtualSize]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True

    print(f"client.dll: {dll}")
    print(f"pattern   : {pattern_text}")
    print(f"DISP_AT   : {disp_at}   DRAW={draw:#04x} SKIP={skip:#04x}\n")
    ok = True

    # 1. unique
    hits = find_all(code, pattern)
    if len(hits) != 1:
        print(f"  FAIL the signature matches {len(hits)} time(s)")
        return 1
    fn = t0 + hits[0]
    print(f"  OK   the signature matches exactly once, at +{fn:#x}")

    # 2. a jne rel8 whose displacement is DRAW
    window = bytes(img[fn : fn + len(pattern) + 4])
    shipped = list(md.disasm(window, base + fn))
    jne = next((i for i in shipped if i.address - base == fn + disp_at - 1), None)
    if jne is None or jne.mnemonic != "jne" or jne.size != 2:
        print(f"  FAIL +{fn + disp_at - 1:#x} is not a two-byte jne")
        return 1
    print(f"  OK   +{jne.address - base:#x} is `jne {jne.op_str}`, two bytes")
    if img[fn + disp_at] != draw:
        ok = False
        print(f"  FAIL the displacement is {img[fn + disp_at]:#04x}, not DRAW")

    # 3. shipped: past the early return; patched: onto it
    after_jne = jne.address + jne.size
    early = [i for i in shipped if after_jne <= i.address < jne.operands[0].imm]
    names = [i.mnemonic for i in early]
    if names == ["xor", "add", "ret"]:
        print(f"  OK   shipped, the jne skips `xor eax, eax; add esp; ret 4` to +{jne.operands[0].imm - base:#x}")
    else:
        ok = False
        print(f"  FAIL the jne skips {names}, not the early return")
    patched = bytearray(window)
    patched[disp_at] = skip
    redone = list(md.disasm(bytes(patched), base + fn))
    pj = next(i for i in redone if i.address == jne.address)
    if pj.operands[0].imm == after_jne:
        print("  OK   patched, both ways out of the jne land on the `xor`")
    else:
        ok = False
        print(f"  FAIL patched, the jne goes to {pj.operands[0].imm - base:#x}")
    if [i.address for i in redone] == [i.address for i in shipped]:
        print("  OK   every later instruction still starts where it did")
    else:
        ok = False
        print("  FAIL the rewrite moves later instruction boundaries")

    # 4. the tested global is the one invnext's path writes
    active_sel = struct.unpack_from("<I", img, fn + 1)[0]
    name_at = img.find(b"\0invnext\0") + 1
    push_name = b"\x68" + struct.pack("<I", base + name_at)
    i = code.find(push_name)
    if name_at <= 0 or i < 5 or code[i - 5] != 0x68:
        ok = False
        print("  FAIL could not find the \"invnext\" registration")
    else:
        handler = struct.unpack_from("<I", code, i - 4)[0] - base
        stub = list(md.disasm(bytes(img[handler : handler + 16]), base + handler))
        call = next((x for x in stub if x.mnemonic == "call"), None)
        target = call.operands[0].imm - base if call else None
        body = bytes(img[target : target + 0x100]) if target else b""
        writes = b"\xa3" + struct.pack("<I", active_sel) in body or any(
            body[k : k + 2] in (b"\x89\x15", b"\x89\x0d", b"\x89\x35", b"\xc7\x05")
            and body[k + 2 : k + 6] == struct.pack("<I", active_sel)
            for k in range(len(body) - 6)
        )
        if writes:
            print(f"  OK   {active_sel:#x} is gpActiveSel: UserCmd_NextWeapon (+{target:#x}) writes it")
        else:
            ok = False
            print(f"  FAIL UserCmd_NextWeapon doesn't write {active_sel:#x}")

    # 5. one caller, result ignored
    callers = [
        k
        for k in range(len(code) - 5)
        if code[k] == 0xE8 and t0 + k + 5 + struct.unpack_from("<i", code, k + 1)[0] == fn
    ]
    if len(callers) != 1:
        ok = False
        print(f"  FAIL {len(callers)} direct callers, expected one (CHudAmmo::Draw)")
    else:
        at = t0 + callers[0]
        nxt = next(md.disasm(bytes(img[at + 5 : at + 21]), base + at + 5))
        reads_eax = "eax" in nxt.op_str or nxt.mnemonic == "ret"
        if reads_eax:
            ok = False
            print(f"  FAIL the caller at +{at:#x} goes on with `{nxt.mnemonic} {nxt.op_str}`")
        else:
            print(f"  OK   one caller (+{at:#x}), which goes on with `{nxt.mnemonic} {nxt.op_str}` and ignores eax")

    print("\nSIGNATURE VERIFIED" if ok else "\nMISMATCH -- do not ship")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
