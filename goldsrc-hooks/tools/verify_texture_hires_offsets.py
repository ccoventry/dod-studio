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
  - The detail-texture loader matches once, its two `push 0x100000`s (buffer
    size and the size LoadTGA is told) sit where raise_detail_limit patches,
    and it uploads through the same GL_LoadTexture2.
  - R_LoadSkys's face-buffer malloc, its hardcoded 256x256 glTexImage2D size
    and the hooked instruction are where install_sky expects, nothing branches
    into the hook span, and it shares GL_Upload8's texgamma table.
  - No branch anywhere in either function lands inside a detoured span (other
    than on its first byte).
  - The studio API's GetModelByIndex calls CL_GetModelByIndex, whose
    `mov esi, [edi*4 + model_precache]` is where find_precache_table reads
    the per-map model list from.

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
DETAIL_LOADER = (
    "55 8B EC 81 EC 0C 01 00 00 53 56 57 68 00 00 10 00 E8 ?? ?? ?? ?? 8B 5D 08 "
    "8B F0 53 68 ?? ?? ?? ?? 8D 85 F4 FE FF FF 68 04 01 00 00 50 83 CF FF E8 ?? ?? ?? ?? 83 C4 14 85 F6 "
    "74 4D 8D 4D F8 6A 00 8D 55 FC 51 52 68 00 00 10 00 8D 85 F4 FE FF FF 56 50 E8 ?? ?? ?? ?? 83 C4 18 "
    "85 C0 74 21 8B 4D F8 8B 55 FC 68 03 27 00 00 6A 00 6A 04 6A 01 56 51 52 6A 05 53 E8 ?? ?? ?? ??"
)
DETAIL_ALLOC, DETAIL_LOADTGA = 0x0C, 0x46
SKY_LOADER = (
    "55 8B EC 83 EC 6C A1 ?? ?? ?? ?? 56 57 33 FF 3B C7 89 7D F4 75 25 BE ?? ?? ?? ?? "
    "39 3E 74 0B 56 6A 01 FF 15 ?? ?? ?? ?? 89 3E 83 C6 04 81 FE ?? ?? ?? ?? 7C E6 5F 5E 8B E5 5D C3 39 3D "
    "?? ?? ?? ?? 74 1D D9 05 ?? ?? ?? ?? D8 1D ?? ?? ?? ?? DF E0 F6 C4 44 7B 0A 89 7D F8 E8 ?? ?? ?? ?? EB 07 "
    "C7 45 F8 01 00 00 00 68 00 00 04 00 E8 ?? ?? ?? ?? 83 C4 04"
)
SKY_UPLOAD = (
    "8D 0C 02 81 F9 00 00 04 00 0F 8C 7B FF FF FF 8B 04 9D ?? ?? ?? ?? 85 C0 75 0C "
    "E8 ?? ?? ?? ?? 89 04 9D ?? ?? ?? ?? 8B 14 9D ?? ?? ?? ?? 52 E8 ?? ?? ?? ?? 8D 45 E0 8D 4D D8 50 8D 55 D4 "
    "51 52 E8 ?? ?? ?? ?? 8B 45 E0 83 C4 10 83 F8 20 56 68 01 14 00 00 68 08 19 00 00 6A 00 68 00 01 00 00 "
    "68 00 01 00 00 75 07 68 58 80 00 00 EB 05 68 57 80 00 00 6A 00 68 E1 0D 00 00 FF 15 ?? ?? ?? ??"
)
SKY_MALLOC_AT, SKY_HOOK_AT, SKY_HEIGHT, SKY_WIDTH = 0x67, 0x0F, 0x5A, 0x5F
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

    details = find_all(img, parse(DETAIL_LOADER))
    check(len(details) == 1, f"DETAIL_LOADER matches once ({[hex(base + d) for d in details]})")
    if details:
        dl = details[0]
        for off in (DETAIL_ALLOC, DETAIL_LOADTGA):
            check(img[dl + off: dl + off + 5] == bytes.fromhex("6800001000"), f"detail loader push 0x100000 at +{off:#x}")
        # The loader's second call must be LoadTGA (whose error string names the limit)
        # and its last GL_LoadTexture2 -- the same function the swap hooks.
        check(img[dl + 0x4B: dl + 0x51] == bytes.fromhex('8D85F4FEFFFF'), 'detail path lea at +0x4b (redirect span)')
        for ins in Cs(CS_ARCH_X86, CS_MODE_32).disasm(img[dl:dl + 0x92], base + dl):
            if ins.mnemonic.startswith('j') and ins.op_str.startswith('0x'):
                t = int(ins.op_str, 16) - base - dl
                check(not (0x4B < t < 0x51), f'{ins.address:#x} does not branch into the detail redirect span')
        lt2 = call(dl + 0x76)
        check(lt2 == tail - 0x2BD, "detail loader uploads through GL_LoadTexture2")

    skl = find_all(img, parse(SKY_LOADER))
    sku = find_all(img, parse(SKY_UPLOAD))
    check(len(skl) == 1, f"SKY_LOADER matches once ({[hex(base + x) for x in skl]})")
    check(len(sku) == 1, f"SKY_UPLOAD matches once ({[hex(base + x) for x in sku]})")
    if skl and sku:
        sl, su = skl[0], sku[0]
        check(img[sl + SKY_MALLOC_AT: sl + SKY_MALLOC_AT + 5] == bytes.fromhex("6800000400"), "sky malloc push 0x40000")
        for off in (SKY_HEIGHT, SKY_WIDTH):
            check(img[su + off: su + off + 5] == bytes.fromhex("6800010000"), f"sky push 0x100 at +{off:#x}")
        check(img[su + SKY_HOOK_AT: su + SKY_HOOK_AT + 3] == bytes.fromhex("8B049D"), "sky hook span is mov eax,[ebx*4+...]")
        # The upload block must be inside R_LoadSkys, and nothing in R_LoadSkys may
        # branch into the 7-byte hook span except onto its first byte.
        check(sl < su < sl + 0x400, "sky upload block is inside R_LoadSkys")
        hook = base + su + SKY_HOOK_AT
        for ins in Cs(CS_ARCH_X86, CS_MODE_32).disasm(img[sl:su + 0x100], base + sl):
            if ins.mnemonic.startswith("j") and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                if hook < t < hook + 7:
                    check(False, f"{ins.address:#x} branches into the sky hook span")
        # R_LoadSkys's face gamma uses the same texgamma table GL_Upload8 does.
        check(img.find((u32(up8 + U8_GAMMA[0] + 2)).to_bytes(4, "little"), sl, su) != -1,
              "R_LoadSkys uses the same texgamma table as GL_Upload8")

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

    # dodstudio_hd_misses' per-map list: find_precache_table follows the studio
    # API's GetModelByIndex (slot 5) into CL_GetModelByIndex and reads
    # cl.model_precache from `mov esi, [edi*4 + table]`.
    bodies = find_all(img, parse(
        "55 8B EC 83 EC 10 56 57 8B 7D 08 81 FF 00 02 00 00 7C 08 5F 33 C0 5E 8B E5 5D C3 8B 34 BD"))
    check(len(bodies) == 1, f"CL_GetModelByIndex matches once ({[hex(base + b) for b in bodies]})")
    # Two thin `push index; call CL_GetModelByIndex` wrappers exist; the one
    # the studio API hands the client is in slot 5 of a table code points at.
    wrappers = [w for w in find_all(img, parse("55 8B EC 8B 45 08 50 E8"))
                if bodies and call(w + 7) == bodies[0]]
    tables = [(w, i - 0x14) for w in wrappers
              for i in range(0, len(img) - 4, 4) if img[i:i + 4] == struct.pack("<I", base + w)]
    referenced = [(w, t) for w, t in tables if struct.pack("<I", base + t) in img]
    check(len(referenced) == 1,
          f"a GetModelByIndex wrapper sits in slot 5 of a table the code points at "
          f"({[(hex(base + w), hex(base + t)) for w, t in referenced]})")
    print("ok   no branch lands inside a detoured span" if not failures else "")

    print(f"\nGL_LoadTexture2 tail +{tail:#x}, GL_Upload32 +{up32:#x}, GL_Upload8 +{up8:#x}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
