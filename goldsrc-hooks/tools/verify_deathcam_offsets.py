#!/usr/bin/env python3
"""Checks `spectator_deathcam.rs` against a real DoD `client.dll`.

The module calls the game's own `V_GetDeathCam` and writes two of its globals,
all found from one signature plus two offsets past it. Every claim is about a
binary that is not in this repository and would compile fine if wrong: a
wrong global would corrupt an unrelated dword every death, and a wrong
function would be called with four pointers.

Every constant below is read out of the Rust rather than restated.

  1. The signature matches exactly once in `.text`.
  2. `RESET_CAMERA_OPERAND_AT` is the operand of `mov eax, [imm32]`.
  3. `LAST_ANGLES_PUSH_AT` is a `push imm32`, the first argument pushed before
     the `V_SmoothInterpolateAngles` call, and `LAST_ANGLES_STORE_AT` is a
     `mov [imm32], edx` naming the same address: the function reads and writes
     one `v_lastAngles`.
  4. Both globals are in a writable data section of the image.
  5. The constants the module docs quote are the ones in the binary: 96 u/s,
     64 at reset, 168 cap, 17 head height, 120 smoothing.
  6. The function is cdecl with four arguments: it ends in a plain `ret`, and
     its callers follow the call with `add esp, 0x10`.
  7. It reads `ent->origin` at the offset `ClEntityS` puts it (0xb48) and
     `ent->player` at 4.

Usage:
    python goldsrc-hooks/tools/verify_deathcam_offsets.py [path-to-client.dll]

Defaults to the pre-Anniversary movies install; `client.dll` is the same file
in the 25th Anniversary one. Requires `pefile` and `capstone`
(`pip install pefile capstone`); both are analysis-only and are not build
dependencies of anything in the workspace.
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
SRC = Path(__file__).resolve().parent.parent / "src" / "spectator_deathcam.rs"

# `cl_entity_s::origin` and `::player`, as `engine.rs`'s ClEntityS lays them out.
ORIGIN_OFFSET = 0xB48
PLAYER_OFFSET = 4

IMAGE_SCN_MEM_WRITE = 0x80000000


def rust_string(src: str, decl: str) -> str:
    m = re.search(rf"{decl}\s*=\s*(.*?);", src, re.S)
    if not m:
        sys.exit(f"could not find {decl}")
    body = re.sub(r"\\\s*\n\s*", "", m.group(1))
    return " ".join(p.strip() for p in re.findall(r'"([^"]*)"', body)).strip()


def rust_const(src: str, name: str) -> int:
    m = re.search(rf"const {name}:\s*\w+\s*=\s*(0x[0-9a-fA-F]+|\d+);", src)
    if not m:
        sys.exit(f"could not find {name}")
    return int(m.group(1), 0)


def parse_pattern(text):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(code, pattern):
    n = len(pattern)
    hits = []
    for i in range(len(code) - n + 1):
        if code[i] != pattern[0]:
            continue
        if all(w is None or code[i + k] == w for k, w in enumerate(pattern)):
            hits.append(i)
    return hits


def main() -> int:
    dll = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DLL
    if not dll.is_file():
        sys.exit(f"no client.dll at {dll}")

    src = SRC.read_text(encoding="utf-8")
    pattern = parse_pattern(rust_string(src, r"const PATTERN:\s*&str"))
    reset_at = rust_const(src, "RESET_CAMERA_OPERAND_AT")
    push_at = rust_const(src, "LAST_ANGLES_PUSH_AT")
    store_at = rust_const(src, "LAST_ANGLES_STORE_AT")

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    text = pe.sections[0]
    trva = text.VirtualAddress
    code = image[trva : trva + text.Misc_VirtualSize]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    failures = []

    def check(ok, message):
        print(("  ok   " if ok else "  FAIL ") + message)
        if not ok:
            failures.append(message)

    def float_at(va):
        return struct.unpack_from("<f", image, va - base)[0]

    def writable(va):
        for s in pe.sections:
            lo = base + s.VirtualAddress
            if lo <= va < lo + max(s.Misc_VirtualSize, s.SizeOfRawData):
                return bool(s.Characteristics & IMAGE_SCN_MEM_WRITE), s.Name.rstrip(b"\0").decode()
        return False, "?"

    print(f"client.dll at {dll}")
    print(f"image base {base:#x}\n")

    # 1. one match
    hits = find_all(code, pattern)
    check(len(hits) == 1, f"PATTERN matches exactly once ({len(hits)} hit(s))")
    if len(hits) != 1:
        return 1
    at = hits[0]
    rva = trva + at
    print(f"\nV_GetDeathCam at client+{rva:#x}\n")

    # Disassemble the function up to its ret.
    body = []
    for ins in md.disasm(bytes(code[at : at + 0x200]), base + rva):
        body.append(ins)
        if ins.mnemonic == "ret":
            break
    by_offset = {ins.address - base - rva: ins for ins in body}

    # 2. the reset-camera operand
    ins = by_offset.get(reset_at - 1)
    check(
        ins is not None and ins.bytes[0] == 0xA1 and len(ins.bytes) == 5,
        "RESET_CAMERA_OPERAND_AT is the operand of `mov eax, [imm32]`",
    )
    reset_camera = struct.unpack_from("<I", code, at + reset_at)[0]

    # 3. push and store of v_lastAngles
    push = by_offset.get(push_at)
    check(
        push is not None and push.bytes[0] == 0x68 and len(push.bytes) == 5,
        "LAST_ANGLES_PUSH_AT is a `push imm32`",
    )
    store = by_offset.get(store_at)
    check(
        store is not None and bytes(store.bytes[:2]) == b"\x89\x15" and len(store.bytes) == 6,
        "LAST_ANGLES_STORE_AT is a `mov [imm32], edx`",
    )
    last_angles = struct.unpack_from("<I", code, at + push_at + 1)[0]
    stored = struct.unpack_from("<I", code, at + store_at + 2)[0]
    check(last_angles == stored, f"push and store name the same v_lastAngles ({last_angles:#x})")
    # The push is the first argument: the next instruction is the call.
    following = by_offset.get(push_at + 5)
    check(
        following is not None and following.mnemonic == "call",
        "the push is immediately followed by the V_SmoothInterpolateAngles call",
    )

    # 4. both globals are writable data
    for name, va in (("v_resetCamera", reset_camera), ("v_lastAngles", last_angles)):
        ok, section = writable(va)
        check(ok, f"{name} {va:#x} is in a writable section ({section})")

    # 5. the camera's constants
    fmul = by_offset.get(0x9)
    check(
        fmul is not None and fmul.mnemonic == "fmul" and float_at(fmul.operands[0].mem.disp) == 96.0,
        "the distance grows at 96 units a second",
    )
    reset_mov = next((i for i in body if i.mnemonic == "mov" and i.bytes[:2] == b"\xc7\x05"), None)
    check(
        reset_mov is not None
        and reset_mov.operands[0].mem.disp != 0
        and struct.unpack("<f", struct.pack("<I", reset_mov.operands[1].imm & 0xFFFFFFFF))[0] == 64.0,
        "a reset puts the distance at 64",
    )
    cap = by_offset.get(0x14)
    check(
        cap is not None and struct.unpack("<f", struct.pack("<I", cap.operands[1].imm & 0xFFFFFFFF))[0] == 168.0,
        "the distance is capped at 168",
    )
    fadds = [i for i in body if i.mnemonic == "fadd" and i.operands[0].type == capstone.x86.X86_OP_MEM and i.operands[0].mem.base == 0]
    heads = [float_at(i.operands[0].mem.disp) for i in fadds]
    check(17.0 in heads, f"the head is 17 above the origin (fadd constants {heads})")
    pushes = [struct.unpack("<f", struct.pack("<I", i.operands[0].imm & 0xFFFFFFFF))[0] for i in body if i.mnemonic == "push" and i.operands[0].type == capstone.x86.X86_OP_IMM]
    check(120.0 in pushes, f"the angles smooth at 120 (pushed floats {pushes})")

    # 6. cdecl, four arguments
    check(body[-1].mnemonic == "ret" and len(body[-1].bytes) == 1, "the function ends in a plain `ret` (caller cleans up)")
    callers = []
    for m in re.finditer(rb"\xe8(....)", code, re.S):
        rel = struct.unpack("<i", m.group(1))[0]
        if m.start() + 5 + rel == at:
            callers.append(m.start())
    check(len(callers) >= 1, f"it is called from {len(callers)} site(s)")
    cleanups = [code[c + 5 : c + 8] == b"\x83\xc4\x10" for c in callers]
    check(
        callers and any(cleanups),
        f"a caller pops four arguments with `add esp, 0x10` right after the call ({sum(cleanups)} of {len(callers)})",
    )
    for c in callers:
        print(f"         caller at client+{trva + c:#x}")

    # 7. entity field offsets
    disps = {i.operands[1].mem.disp for i in body if i.mnemonic == "mov" and len(i.operands) == 2 and i.operands[1].type == capstone.x86.X86_OP_MEM}
    check(ORIGIN_OFFSET in disps, f"reads ent->origin at {ORIGIN_OFFSET:#x}")
    check(PLAYER_OFFSET in disps, f"reads ent->player at {PLAYER_OFFSET:#x}")

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
