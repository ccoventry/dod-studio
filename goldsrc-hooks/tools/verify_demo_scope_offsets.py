#!/usr/bin/env python3
"""Checks `demo_scope.rs`'s two one-byte patches against real `client.dll` files.

`demo_scope.rs` lifts the demo gate in `CHudScope::DrawTriApiScope` and
rewrites the in-eye `m_iWeaponId` store (#307). The unit tests prove the
pattern's offsets line up with its own tokens; only the binaries can prove:

  1. The pattern matches exactly once, at `+0x466c7`.
  2. Its first operand is `g_iUser1` (`+0xe88d4`, `spectator_target.rs`'s
     `MODE_RVA`), and the in-eye store is `C7 46 14 09 00 00 00`: the stock
     immediate the module puts back is 9.
  3. The demo gate is `mov edx, [pDemoAPI]; call [edx+4]; test eax, eax;
     je +2; xor bl, bl`, the `je` is `74 02`, and `EB 02` (what the module
     writes) lands on the same `test edi, edi` as the `je` does.
  4. The sprite switch at `+0x46750` draws a scope for every id the module
     writes (6, 9, 25), and 6 reads the same sprite member as 23 (the
     scoped FG42) without 23's `g_iWeaponFlags` test.

Constants are read out of `demo_scope.rs` rather than restated.

Usage:
    python -I goldsrc-hooks/tools/verify_demo_scope_offsets.py [client.dll ...]

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
RUST = Path(__file__).resolve().parent.parent / "src" / "demo_scope.rs"

EXPECTED_SITE = 0x466C7
G_IUSER1 = 0xE88D4
# The sprite switch: `add eax, -6; cmp eax, 0x1b; ja; mov cl, [eax+index];
# jmp [ecx*4+table]`.
SWITCH_INDEX = 0x470E4
SWITCH_TABLE = 0x470CC
SWITCH_DEFAULT = 0x470BF
FIRST_ID = 6


def constants():
    text = RUST.read_text(encoding="utf-8")
    m = re.search(r'const PATTERN: &str = "((?:[^"\\]|\\.)*)";', text, re.S)
    if not m:
        sys.exit(f"no PATTERN in {RUST}")
    pattern = " ".join(m.group(1).replace(chr(92), " ").split())
    found = {}
    for name in ["IN_EYE_ID_AT", "JE_AT"]:
        n = re.search(rf"const {name}: usize = (\d+);", text)
        if not n:
            sys.exit(f"no {name} in {RUST}")
        found[name] = int(n.group(1))
    ids = {}
    for name, value in re.findall(r"const (WEAPON_\w+): u8 = (\d+);", text):
        ids[name] = int(value)
    return pattern, found["IN_EYE_ID_AT"], found["JE_AT"], ids


def compile_pattern(pattern):
    parts = []
    for tok in pattern.split():
        parts.append(b"." if tok == "??" else re.escape(bytes([int(tok, 16)])))
    return re.compile(b"".join(parts), re.S)


def sprite_case(image, base, weapon_id):
    """The code address the sprite switch jumps to for an id, as an RVA."""
    index = weapon_id - FIRST_ID
    if not 0 <= index <= 0x1B:
        return SWITCH_DEFAULT
    case = image[SWITCH_INDEX + index]
    return struct.unpack_from("<I", image, SWITCH_TABLE + 4 * case)[0] - base


def first_instructions(image, rva, count):
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    return list(md.disasm(image[rva:rva + 32], rva))[:count]


def sprite_member(image, rva):
    """`(flag_tested, member)` for a sprite case: `test byte [flags], 1; je`
    first or not, then the `mov edi, [esi+member]` it loads."""
    flag = False
    for ins in first_instructions(image, rva, 3):
        if ins.mnemonic == "test" and "byte ptr" in ins.op_str:
            flag = True
        if ins.mnemonic == "mov" and ins.op_str.startswith("edi, dword ptr [esi + "):
            return flag, int(ins.op_str.rsplit("+ ", 1)[1].rstrip("]"), 16)
    return flag, None


def check(path, pattern, in_eye_at, je_at, ids):
    pe = pefile.PE(str(path), fast_load=True)
    image = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase
    problems = []

    hits = [m.start() for m in compile_pattern(pattern).finditer(image)]
    if len(hits) != 1:
        return [f"{path}: {len(hits)} matches, expected 1"]
    at = hits[0]
    if at != EXPECTED_SITE:
        problems.append(f"site at +{at:#x}, expected +{EXPECTED_SITE:#x}")

    mode = struct.unpack_from("<I", image, at + 1)[0] - base
    if mode != G_IUSER1:
        problems.append(f"first operand is +{mode:#x}, not g_iUser1 +{G_IUSER1:#x}")

    # The "not in-eye" jne: stock 75 24 past the store and the v_angles
    # copy; the module's 75 07 (in a demo) lands on the copy instead.
    view_jne = image[at + 11:at + 13]
    if view_jne != bytes.fromhex("75 24"):
        problems.append(f"in-eye jne is {view_jne.hex(' ')}, not 75 24")
    copy = image[at + 13 + 7:at + 13 + 9]
    if copy != bytes.fromhex("8B 15"):
        problems.append(f"jne +7 lands on {copy.hex(' ')}, not the v_angles copy (8b 15)")

    store = image[at + in_eye_at - 3:at + in_eye_at + 4]
    if store != bytes.fromhex("C7 46 14 09 00 00 00"):
        problems.append(f"in-eye store is {store.hex(' ')}, not c7 46 14 09 00 00 00")

    gate = image[at + je_at - 11:at + je_at + 4]
    if gate[:2] != b"\x8b\x15" or gate[6:] != bytes.fromhex("FF 52 04 85 C0 74 02 32 DB"):
        problems.append(f"demo gate is {gate.hex(' ')}")
    je_target = at + je_at + 2 + struct.unpack_from("<b", image, at + je_at + 1)[0]
    # EB takes the same rel8, so the jmp lands where the je did: past the xor.
    if je_target != at + je_at + 4:
        problems.append(f"je lands at +{je_target:#x}, not right after xor bl, bl")
    after = first_instructions(image, je_target, 1)
    if not after or (after[0].mnemonic, after[0].op_str) != ("test", "edi, edi"):
        problems.append(f"je target +{je_target:#x} is not test edi, edi (the FOV gate)")

    written = sorted({ids["WEAPON_SCOPEDKAR"], ids["WEAPON_SPRING"], ids["WEAPON_ENFIELD"]})
    for weapon_id in written + [ids["WEAPON_FG42"], ids["WEAPON_BINOC"]]:
        if sprite_case(image, base, weapon_id) == SWITCH_DEFAULT:
            problems.append(f"id {weapon_id} draws no scope")
    kar = sprite_member(image, sprite_case(image, base, ids["WEAPON_SCOPEDKAR"]))
    fg42 = sprite_member(image, sprite_case(image, base, ids["WEAPON_FG42"]))
    if kar[0] or not fg42[0] or kar[1] is None or kar[1] != fg42[1]:
        problems.append(f"scoped kar case {kar} and FG42 case {fg42} differ from what the module assumes")

    if problems:
        return [f"{path}: {p}" for p in problems]
    return [
        f"{path.parents[2].name}: OK -- site +{at:#x}; in-eye imm32 +{at + in_eye_at:#x} = 09 "
        f"(c7 46 14 09 00 00 00); demo gate +{at + je_at:#x} = 74 02, EB 02 lands +{je_target:#x}; "
        f"ids {written} draw a scope; 6 and 23 share sprite member +{kar[1]:#x}, only 23 tests the flag"
    ]


def check_view_operands(path):
    """The angle copy reads v_angles and the quad placement reads v_origin:
    the six absolute operands demo_scope.rs repoints. v_angles is what
    V_CalcRefdef's tail stores at +0x5128f; v_origin at +0x512a8."""
    text = RUST.read_text(encoding="utf-8")
    m = re.search(r'const ORIGIN_PATTERN: &str = "((?:[^"\\]|\\.)*)";', text, re.S)
    if not m:
        return [f"{path}: no ORIGIN_PATTERN in {RUST}"]
    origin_pattern = " ".join(m.group(1).replace(chr(92), " ").split())
    pe = pefile.PE(str(path), fast_load=True)
    image = bytes(pe.get_memory_mapped_image())
    base = pe.OPTIONAL_HEADER.ImageBase
    hits = [h.start() for h in compile_pattern(origin_pattern).finditer(image)]
    if len(hits) != 1:
        return [f"{path}: ORIGIN_PATTERN matches {len(hits)} times, expected 1"]
    at = hits[0]
    v_origin = struct.unpack_from("<I", image, 0x512A8 + 1)[0]
    v_angles = struct.unpack_from("<I", image, 0x5128F + 1)[0]
    origin = [struct.unpack_from("<I", image, at + o)[0] for o in (2, 12, 44)]
    angles = [struct.unpack_from("<I", image, EXPECTED_SITE + o)[0] for o in (22, 27, 33)]
    problems = []
    if origin != [v_origin, v_origin + 4, v_origin + 8]:
        problems.append(f"origin reads {[hex(a) for a in origin]}, not v_origin {v_origin:#x}")
    if angles != [v_angles, v_angles + 4, v_angles + 8]:
        problems.append(f"angle copy reads {[hex(a) for a in angles]}, not v_angles {v_angles:#x}")
    m = re.search(r'const SETTLE_PATTERN: &str = "([^"]*)";', text)
    if not m:
        return [f"{path}: no SETTLE_PATTERN in {RUST}"]
    settle_hits = [h.start() for h in compile_pattern(m.group(1)).finditer(image)]
    settle = settle_hits[0] if len(settle_hits) == 1 else None
    if settle is None:
        problems.append(f"SETTLE_PATTERN matches {len(settle_hits)} times, expected 1")
    else:
        jne = image[settle + 8:settle + 10]
        target = settle + 14 + struct.unpack_from("<i", image, settle + 10)[0]
        if jne != b"\x0f\x85" or target != SWITCH_DEFAULT:
            problems.append(f"zoom-settled test at +{settle:#x} is {jne.hex(' ')} to +{target:#x}, "
                            f"not jne to the no-scope exit +{SWITCH_DEFAULT:#x}")
    if problems:
        return [f"{path}: {p}" for p in problems]
    return [
        f"{path.parents[2].name}: OK -- quad origin reads at +{at:#x} read v_origin "
        f"+{v_origin - base:#x}; the angle copy reads v_angles +{v_angles - base:#x}; "
        f"zoom-settled jne at +{settle + 8:#x} goes to the no-scope exit"
    ]


def main():
    dlls = [Path(p) for p in sys.argv[1:]] or DEFAULT_DLLS
    pattern, in_eye_at, je_at, ids = constants()
    failed = False
    for dll in dlls:
        for line in check(dll, pattern, in_eye_at, je_at, ids) + check_view_operands(dll):
            print(line)
            failed |= ": OK --" not in line
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
