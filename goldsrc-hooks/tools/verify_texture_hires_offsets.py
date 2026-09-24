"""Independently verifies texture_hires.rs's signatures and offsets against the
real pre-Anniversary-for-Movies hw.dll -- a second implementation of every
check `install()` makes, so a bug in one shows up as a disagreement.

Checks:
  - LOAD_TEXTURE2_TAIL and UPLOAD32 each match EXACTLY ONCE in the file.
  - The tail's `call GL_Upload32` lands on UPLOAD32's match.
  - Every byte span the hook overwrites is what the Rust constants say.
  - GL_Upload32's five scratch-buffer pushes agree, and the next static buffer
    (GL_Upload8's) starts exactly 2 MB later -- the proof of its size.
  - GL_Upload8's gamma-table and gl_dither operands sit where expected, and the
    gl_dither operand really is that cvar's value field.
  - No branch anywhere in either function lands inside a detoured span (other
    than on its first byte).

Run after touching the patterns or offsets in texture_hires.rs. Not part of
`cargo test`: it needs the real DLL, which CI does not have. Requires pefile
and capstone.
"""

import struct
import sys

import pefile
from capstone import CS_ARCH_X86, CS_MODE_32, Cs

HW_DLL = (
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)

# Kept in sync with texture_hires.rs by hand.
LOAD_TEXTURE2_TAIL = (
    "A1 ?? ?? ?? ?? 85 C0 74 0F 8B 4D 14 8B 55 08 57 51 53 52 FF D0 "
    "83 C4 10 83 7D 0C 05 75 20 83 7D 20 04 75 1A 8B 45 28 8B 4D 1C 8B 55 14 50 6A 04 51 52 53 57 "
    "E8 ?? ?? ?? ?? 83 C4 18 EB 1E 8B 45 28 8B 4D 24 8B 55 20 50 8B 45 1C 51 8B 4D 14 52 50 51 53 "
    "57 E8 ?? ?? ?? ?? 83 C4 1C"
)
TAIL_BRANCH, TAIL_RESUME, TAIL_CALL32, TAIL_UPLOAD8_ARGS, TAIL_CALL8, TAIL_AFTER = 24, 30, 52, 62, 84, 92
BRANCH_STOLEN = bytes([0x83, 0x7D, 0x0C, 0x05, 0x75, 0x20])

UPLOAD32 = (
    "55 8B EC 83 EC 14 53 56 8D 45 FC 57 8D 4D F0 50 8D 55 EC 51 52 E8 ?? ?? ?? ?? "
    "8B 5D 0C 8B 4D 10 8B D3 8B 35 ?? ?? ?? ?? 0F AF D1 03 F2 83 C4 0C 89 35 ?? ?? ?? ?? 8B 75 18 "
    "83 FE 02 89 55 F8 74 0D A1 ?? ?? ?? ?? 8D 04 50 A3 ?? ?? ?? ?? D9 05 ?? ?? ?? ?? A1 ?? ?? ?? ?? "
    "D8 1D ?? ?? ?? ?? 40 A3 ?? ?? ?? ?? DF E0 F6 C4 44 7B 41 83 FE 01 74 0A 83 FE 03 74 05 83 FE 04 "
    "75 32 33 F6 85 D2 7E 2C 8B 7D 08 83 3F 00 75 1C 8B C6 99 F7 FB 50 52 51 8B 4D 08 53 51 57 E8 "
    "?? ?? ?? ?? 8B 55 F8 8B 4D 10 83 C4 18 46 83 C7 04 3B F2 7C D7 51 8D 55 F4 53 8D 45 0C 52 50 "
    "E8 ?? ?? ?? ?? 8B 75 0C 8B 7D F4 8B C6 83 C4 10 0F AF C7 3D 00 00 08 00 89 45 F4 76 0D"
)
SIZE_CHECK, TOO_BIG, SIZE_OK = 0xCA, 0xD4, 0xE1
SIZE_CHECK_STOLEN = bytes([0x3D, 0x00, 0x00, 0x08, 0x00, 0x89, 0x45, 0xF4, 0x76, 0x0D])
BUFFER_PUSHES = [0x1EB, 0x1FC, 0x22B, 0x26C, 0x29F]
U8_GAMMA, U8_DITHER, U8_EXPANSION = (0x53, b"\x8a\x91"), (0x94, b"\xd9\x05"), (0x276, b"\x68")


def parse(text):
    return [None if t == "??" else int(t, 16) for t in text.split()]


def find_all(img, pat):
    hits, first, n = [], bytes([pat[0]]), len(pat)
    i = img.find(first)
    while i != -1 and i <= len(img) - n:
        if all(p is None or img[i + j] == p for j, p in enumerate(pat)):
            hits.append(i)
        i = img.find(first, i + 1)
    return hits


def main():
    pe = pefile.PE(HW_DLL)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    u32 = lambda rva: struct.unpack_from("<I", img, rva)[0]
    failures = []

    def check(ok, what):
        print(("ok   " if ok else "FAIL ") + what)
        if not ok:
            failures.append(what)

    tails = find_all(img, parse(LOAD_TEXTURE2_TAIL))
    ups = find_all(img, parse(UPLOAD32))
    check(len(tails) == 1, f"LOAD_TEXTURE2_TAIL matches once ({[hex(base + t) for t in tails]})")
    check(len(ups) == 1, f"UPLOAD32 matches once ({[hex(base + u) for u in ups]})")
    if failures:
        sys.exit(1)
    tail, up32 = tails[0], ups[0]

    call = lambda rva: (rva + 5 + struct.unpack_from("<i", img, rva + 1)[0]) & 0xFFFFFFFF
    check(img[tail + TAIL_CALL32] == 0xE8 and call(tail + TAIL_CALL32) == up32, "tail calls GL_Upload32")
    check(img[tail + TAIL_CALL8] == 0xE8, "tail calls GL_Upload8")
    up8 = call(tail + TAIL_CALL8)
    check(img[tail + TAIL_BRANCH: tail + TAIL_BRANCH + 6] == BRANCH_STOLEN, "branch span bytes")
    check(img[up32 + SIZE_CHECK: up32 + SIZE_CHECK + 10] == SIZE_CHECK_STOLEN, "size-check span bytes")

    pushes = [u32(up32 + off + 1) for off in BUFFER_PUSHES if img[up32 + off] == 0x68]
    check(len(pushes) == 5 and len(set(pushes)) == 1, f"5 identical buffer pushes ({[hex(p) for p in pushes]})")
    for name, (off, op) in (("gamma", U8_GAMMA), ("dither", U8_DITHER), ("expansion", U8_EXPANSION)):
        check(img[up8 + off: up8 + off + len(op)] == op, f"GL_Upload8 {name} opcode at +{off:#x}")
    expansion = u32(up8 + U8_EXPANSION[0] + 1)
    check(pushes and expansion == pushes[0] + 0x80000 * 4, f"scratch buffer is 2 MB ({pushes[0]:#x} -> {expansion:#x})")
    dither = u32(up8 + U8_DITHER[0] + 2)
    name_ptr = u32(dither - 0xC - base)
    cvar_name = img[name_ptr - base: name_ptr - base + 16].split(b"\0")[0]
    check(cvar_name == b"gl_dither", f"dither operand is gl_dither.value ({cvar_name!r})")

    # Every direct branch in both functions: none may land inside a span
    # except on its first byte.
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    spans = [(base + tail + TAIL_BRANCH, 6), (base + up32 + SIZE_CHECK, 10)]
    lt2_start = tail - 0x2BD  # GL_LoadTexture2 begins 0x2bd bytes before the tail
    for start, end in ((lt2_start, tail + TAIL_AFTER + 0x30), (up32, up32 + 0x310)):
        for ins in md.disasm(img[start:end], base + start):
            if ins.mnemonic.startswith(("j", "loop")) and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                for s, n in spans:
                    if s < t < s + n:
                        check(False, f"{ins.address:#x} {ins.mnemonic} lands inside the span at {s:#x}")
    check(img[lt2_start: lt2_start + 3] == b"\x55\x8b\xec", "GL_LoadTexture2 prologue where expected")
    print("ok   no branch lands inside a detoured span" if not failures else "")

    print(f"\nGL_LoadTexture2 tail +{tail:#x}, GL_Upload32 +{up32:#x}, GL_Upload8 +{up8:#x}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
