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
  - GL_LoadTexture2's cache lookup (LOAD_TEXTURE2_HEAD) matches once, at the
    function's entry; its name-matched span and servercount operand are where
    install_stale_fix expects.
  - The studio API's GetModelByIndex calls CL_GetModelByIndex, whose
    `mov esi, [edi*4 + model_precache]` is where find_precache_table reads
    the per-map model list from.

With `--anniversary`, checks the 25th Anniversary port (`mod anniversary`)
against the stock Anniversary hw.dll instead, reading its constants out of
the Rust rather than restating them:
  - TAIL and HEAD each match exactly once, in the same function.
  - The swap's span is two whole instructions (`cmp [ebp+0xc], 5` and
    `mov eax, [ebp+0x20]`), followed by the `jne` the stub resumes at.
  - The tail's call lands on UPLOAD32_ENTRY; the `jne` lands on the
    GL_Upload8 argument setup, whose call lands on UPLOAD8_ENTRY and whose
    `add esp, 0x1c` falls into the exit the GL_Upload32 path jumps to.
  - Every caller of GL_Upload32 pops six arguments, as the stub does, and
    its ceiling is still the stock `cmp eax, 0x80000`.
  - GL_Upload8's gamma-table and gl_dither operands are where the Rust says,
    and the dither operand is that cvar's value field.
  - `ebx` holds the cache record at the tail: the lookup saves it to a frame
    slot, and the one loop that reuses `ebx` reloads it from that slot.
  - The leftover-texture fix's span is two whole instructions ending at the
    width compare's `jne`, the lookup's next-record path is where the stub
    jumps, and the hit path reads the servercount the record is stamped with.
  - No branch in GL_LoadTexture2 lands inside either span except on its
    first byte.
  - The ceiling (CEILING): the size check is `cmp eax, 0x80000; jbe` onto
    SIZE_OK past the "too big" Sys_Error push; every relocated reference into
    GL_Upload32's 2 MB scratch buffer is one of the listed five; GL_Upload8's
    buffer starts exactly 2 MB later; both resample helpers have the same
    stack frames as the pre-Anniversary ones (so the same 1024 width bound);
    esi is the scaled width at the check; nothing branches into the span.
  - Detail textures (DETAIL): the loader matches once, its two pushes are
    `push 0x100000`, the redirect span is `lea eax, [ebp-0x108]` (the buffer
    the `snprintf` of "gfx/%s.tga" fills, 0x104 bytes), LoadTGA compares the
    TGA against the size it's told, the load goes to GL_LoadTexture2, and
    nothing branches into the span.
  - Skies (SKY): both patterns match once, inside one function; the malloc
    is `push 0x40000`; the dims are `push 0x100`s; the hooked instruction is
    `mov eax, [esi*4 + ...]`, reached by the gl_dither skip on its first byte
    and nothing else; the path buffer is `[ebp-0x44]` (64 bytes); the gamma
    loop uses GL_Upload8's texgamma table and gl_dither.

Run after touching the patterns or offsets in texture_hires.rs. Not part of
`cargo test`: it needs the real DLL, which CI does not have. Requires pefile
and capstone.

Usage:
    python goldsrc-hooks/tools/verify_texture_hires_offsets.py
    python goldsrc-hooks/tools/verify_texture_hires_offsets.py --anniversary
"""

import re
import struct
import sys
from pathlib import Path

import pefile
from capstone import CS_ARCH_X86, CS_MODE_32, Cs

HW_DLL = (
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)
ANNIVERSARY_DLL = r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life\hw.dll"
SRC = Path(__file__).resolve().parent.parent / "src" / "texture_hires.rs"

# Kept in sync with texture_hires.rs by hand.
LOAD_TEXTURE2_TAIL = (
    "A1 ?? ?? ?? ?? 85 C0 74 0F 8B 4D 14 8B 55 08 57 51 53 52 FF D0 "
    "83 C4 10 83 7D 0C 05 75 20 83 7D 20 04 75 1A 8B 45 28 8B 4D 1C 8B 55 14 50 6A 04 51 52 53 57 "
    "E8 ?? ?? ?? ?? 83 C4 18 EB 1E 8B 45 28 8B 4D 24 8B 55 20 50 8B 45 1C 51 8B 4D 14 52 50 51 53 "
    "57 E8 ?? ?? ?? ?? 83 C4 1C"
)
TAIL_BRANCH, TAIL_RESUME, TAIL_CALL32, TAIL_UPLOAD8_ARGS, TAIL_CALL8, TAIL_AFTER = 24, 30, 52, 62, 84, 92
LOAD_TEXTURE2_HEAD = (
    "55 8B EC B8 0C 40 00 00 E8 ?? ?? ?? ?? 8B 45 08 53 33 DB 56 8A 08 57 84 C9 "
    "89 5D F4 74 61 33 F6 BF ?? ?? ?? ?? 3B 35 ?? ?? ?? ?? 7D 5F 66 83 7F 04 00 7D 0C 85 DB 75 1C 8B DF 46 "
    "83 C7 54 EB E5 8B 55 08 8D 4F 14 51 52 E8 ?? ?? ?? ?? 83 C4 08 85 C0 74 06 46 83 C7 54 EB CB 8B 45 10 "
    "8B 4F 08 3B C1 75 0A 8B 4D 14 8B 47 0C 3B C8 74 59 8B 45 08 8A 50 03 8A 08 FE C2 84 C9 88 50 03 75 9F "
    "68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 04 85 DB 75 6D 8B 0D ?? ?? ?? ?? 8D 04 CD 00 00 00 00 2B C1 41 81 "
    "F9 C0 12 00 00 89 0D ?? ?? ?? ?? 8D 04 40 8D 34 85 ?? ?? ?? ?? 7C 47 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 "
    "C4 04 EB 38 66 83 7F 04 00 7E 0B 66 8B 15 ?? ?? ?? ?? 66 89 57 04"
)
HEAD_NAME_MATCHED, HEAD_SERVERCOUNT = 0x5A, 0xCE
NAME_MATCHED_STOLEN = bytes([0x8B, 0x45, 0x10, 0x8B, 0x4F, 0x08])
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

    # What the replacement size check assumes about GL_Upload32's registers
    # (texture_hires.rs's PRE_SIZE_REGS): esi/edi hold the rounded width and
    # height, ebx and [ebp+0x10] the original ones, [ebp-0xc] the product.
    md0 = Cs(CS_ARCH_X86, CS_MODE_32)
    text = lambda a, b: [f"{i.mnemonic} {i.op_str}" for i in md0.disasm(bytes(img[a:b]), base + a)]
    pre = text(up32 + SIZE_CHECK - 0x14, up32 + SIZE_CHECK)
    check(all(t in pre for t in ("mov esi, dword ptr [ebp + 0xc]", "mov edi, dword ptr [ebp - 0xc]", "mov eax, esi", "imul eax, edi")),
          f"size check: esi/edi are the rounded width/height and eax their product ({pre})")
    check("mov ebx, dword ptr [ebp + 0xc]" in text(up32, up32 + 0x30)
          and not any(t.startswith(("mov ebx", "pop ebx", "xor ebx")) for t in text(up32 + 0x30, up32 + SIZE_CHECK)),
          "size check: ebx still holds the original width")
    post = text(up32 + SIZE_OK, up32 + SIZE_OK + 0xa0)
    check(all(t in post for t in ("mov ecx, dword ptr [ebp + 0x10]", "cmp esi, ebx", "cmp edi, ecx")),
          "size check: the own-size test compares esi/edi with ebx/[ebp+0x10]")
    check("mov eax, dword ptr [ebp - 0xc]" in text(up32 + SIZE_OK, up32 + 0x2a0),
          "size check: [ebp-0xc] is the product read back after it (a shrink stores it again)")
    check(text(up32 + BUFFER_PUSHES[1] - 2, up32 + BUFFER_PUSHES[1]) == ["push edi", "push esi"],
          "size check: the resample call takes edi/esi as the output height/width")
    helpers = []
    for t in text(up32 + SIZE_OK, up32 + 0x230):
        if t.startswith("call 0x"):
            r = int(t[5:], 16) - base
            body = text(r, r + 0x70)
            arrays = [b for b in body if "- 0x1014]" in b or "- 0x414]" in b]
            if arrays:
                helpers.append((r, any("[ebp + 0x18]" in b for b in body)))
    check(len(helpers) == 2 and all(bound for _, bound in helpers),
          f"size check: both resample helpers index their 1024-entry arrays by the output width, their fifth argument ({[hex(r) for r, _ in helpers]})")

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
    lt2_start = tail - 0x2BD  # GL_LoadTexture2 begins 0x2bd bytes before the tail

    # The leftover-texture fix (install_stale_fix): the cache lookup at the
    # function's start, the span it detours once a name matched, and the
    # servercount global it reads.
    heads = find_all(img, parse(LOAD_TEXTURE2_HEAD))
    check(heads == [lt2_start], f"LOAD_TEXTURE2_HEAD matches once, at GL_LoadTexture2's entry ({[hex(base + h) for h in heads]})")
    check(img[lt2_start + HEAD_NAME_MATCHED: lt2_start + HEAD_NAME_MATCHED + 6] == NAME_MATCHED_STOLEN,
          "name-matched span bytes")
    check(img[lt2_start + HEAD_NAME_MATCHED - 8: lt2_start + HEAD_NAME_MATCHED - 6] == bytes([0x74, 0x06]),
          "the je after the name compare targets the span's first byte")
    sc_at = lt2_start + HEAD_SERVERCOUNT
    servercount = u32(sc_at + 3) if img[sc_at: sc_at + 3] == bytes([0x66, 0x8B, 0x15]) else None
    # The record's servercount is written from the same global when a world
    # texture is created (`mov cx, word ptr [servercount]`).
    check(servercount is not None and struct.pack("<I", servercount) in img[lt2_start: tail]
          and img.find(bytes([0x66, 0x8B, 0x0D]) + struct.pack("<I", servercount), lt2_start, tail) > 0,
          f"servercount operand is the global new world records are stamped with ({servercount and hex(servercount)})")

    spans = [(base + tail + TAIL_BRANCH, 6), (base + up32 + SIZE_CHECK, 10),
             (base + lt2_start + HEAD_NAME_MATCHED, 6)]
    for start, end in ((lt2_start, tail + TAIL_AFTER + 0x30), (up32, up32 + 0x310)):
        for ins in md.disasm(img[start:end], base + start):
            if ins.mnemonic.startswith(("j", "loop")) and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                for s, n in spans:
                    if s < t < s + n:
                        check(False, f"{ins.address:#x} {ins.mnemonic} lands inside the span at {s:#x}")
    check(img[lt2_start: lt2_start + 3] == b"\x55\x8b\xec", "GL_LoadTexture2 prologue where expected")

    # dodstudio_debug_hd_misses' per-map list: find_precache_table follows the studio
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


def anniversary():
    """The 25th Anniversary port's checks (see the module doc)."""
    src = SRC.read_text(encoding="utf-8")
    anni = src[src.index("mod anniversary {"):]

    def pattern(name):
        m = re.search(r"pub const " + name + r': &str = "(.*?)";', anni, re.S)
        return parse(" ".join(m.group(1).replace("\\\n", " ").split()))

    def offset(module, name):
        m = re.search(r"pub mod " + module + r" \{.*?pub const " + name + r": usize = (0x[0-9a-f]+|\d+);", anni, re.S)
        return int(m.group(1), 0)

    def byte_list(text):
        return bytes(int(b, 16) for b in re.findall(r"0x([0-9A-Fa-f]{2})", text))

    def const_bytes(name):
        return byte_list(re.search(r"pub const " + name + r": &\[u8\] = &?\[(.*?)\];", anni, re.S).group(1))

    def const_operand(name):
        m = re.search(r"pub const " + name + r": \(usize, &\[u8\]\) = \((0x[0-9a-f]+|\d+), &\[(.*?)\]\);", anni)
        return int(m.group(1), 0), byte_list(m.group(2))

    pe = pefile.PE(ANNIVERSARY_DLL)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    md = Cs(CS_ARCH_X86, CS_MODE_32)
    u32 = lambda rva: struct.unpack_from("<I", img, rva)[0]
    rel32 = lambda at, n: (at + n + 4 + struct.unpack_from("<i", img, at + n)[0]) & 0xFFFFFFFF
    failures = []

    def check(ok, what):
        print(("ok   " if ok else "FAIL ") + what)
        if not ok:
            failures.append(what)

    def instructions(start, end):
        return [(i.address - base, f"{i.mnemonic} {i.op_str}") for i in md.disasm(bytes(img[start:end]), base + start)]

    print(f"hw.dll at {ANNIVERSARY_DLL}\n")
    tails, heads = find_all(img, pattern("TAIL")), find_all(img, pattern("HEAD"))
    check(len(tails) == 1, f"TAIL matches once ({[hex(t) for t in tails]})")
    check(len(heads) == 1, f"HEAD matches once ({[hex(h) for h in heads]})")
    if failures:
        sys.exit(1)
    tail, head = tails[0], heads[0]
    check(head < tail < head + 0x800, "the cache lookup and the upload branch are in the same function")
    entry = img.rfind(b"\x55\x8b\xec", head - 0x100, head)
    check(entry > 0, f"GL_LoadTexture2 begins at +{entry:#x}")

    # The swap's span, and where the stub goes back to.
    branch, jne = tail + offset("tail", "BRANCH"), tail + offset("tail", "JNE_UPLOAD8")
    stolen = const_bytes("BRANCH_STOLEN")
    check(img[branch:branch + len(stolen)] == stolen and branch + len(stolen) == jne, "swap span bytes, ending at the jne")
    check(instructions(branch, jne) == [(branch, "cmp dword ptr [ebp + 0xc], 5"), (branch + 4, "mov eax, dword ptr [ebp + 0x20]")],
          "the swap span is `cmp [ebp+0xc], 5; mov eax, [ebp+0x20]`")
    check(img[jne:jne + 2] == b"\x0f\x85", "JNE_UPLOAD8 is a jne rel32")

    call32, jmp_after = tail + offset("tail", "CALL_UPLOAD32"), tail + offset("tail", "JMP_AFTER")
    check(img[call32] == 0xE8 and img[jmp_after] == 0xE9, "CALL_UPLOAD32 is a call, JMP_AFTER a jmp")
    up32 = rel32(call32, 1)
    check(img[up32:up32 + len(const_bytes("UPLOAD32_ENTRY"))] == const_bytes("UPLOAD32_ENTRY"), f"the tail calls GL_Upload32 (+{up32:#x})")
    args8 = rel32(jne, 2)
    upload8_args = const_bytes("UPLOAD8_ARGS")
    check(img[args8:args8 + len(upload8_args)] == upload8_args, f"the jne goes to GL_Upload8's argument setup (+{args8:#x})")
    call8 = args8 + len(upload8_args)
    up8 = rel32(call8, 1) if img[call8] == 0xE8 else 0
    check(up8 and img[up8:up8 + len(const_bytes("UPLOAD8_ENTRY"))] == const_bytes("UPLOAD8_ENTRY"), f"which calls GL_Upload8 (+{up8:#x})")
    after = rel32(jmp_after, 1)
    pop = call8 + 5
    check(img[pop:pop + 3] == const_bytes("UPLOAD8_POP") and pop + 3 == after,
          f"GL_Upload8's `add esp, 0x1c` falls into the exit GL_Upload32's path jumps to (+{after:#x})")

    # GL_Upload32 takes the six arguments the stub pushes, and still stops at 512x1024.
    # (One caller pushes another call's argument first and pops both at once.)
    def popped_by(call):
        pushed = 0
        for _, t in instructions(call + 5, call + 0x40):
            if t.startswith("push "):
                pushed += 4
            elif t.startswith("add esp, "):
                return int(t.split(", ")[1], 16) - pushed
            elif t.startswith(("call ", "ret", "jmp ")) and pushed == 0:
                return None
        return None

    callers = [i for i in range(len(img) - 5) if img[i] == 0xE8 and rel32(i, 1) == up32]
    pops = {popped_by(c) for c in callers}
    check(callers and pops == {0x18}, f"all {len(callers)} caller(s) of GL_Upload32 pop its six arguments ({pops})")
    check(any(t == "cmp eax, 0x80000" for _, t in instructions(up32, up32 + 0x300)),
          "GL_Upload32's ceiling is the stock `cmp eax, 0x80000` (replacements capped at 512)")

    # GL_Upload8's palette operands.
    for name in ("UPLOAD8_GAMMA", "UPLOAD8_DITHER"):
        off, op = const_operand(name)
        check(img[up8 + off:up8 + off + len(op)] == op, f"{name} opcode at GL_Upload8+{off:#x}")
    off, op = const_operand("UPLOAD8_DITHER")
    dither = u32(up8 + off + len(op))
    name_ptr = u32(dither - 0xC - base)
    cvar_name = img[name_ptr - base:name_ptr - base + 16].split(b"\0")[0]
    check(cvar_name == b"gl_dither", f"the dither operand is gl_dither.value ({cvar_name!r})")
    off, op = const_operand("UPLOAD8_GAMMA")
    gamma = u32(up8 + off + len(op))
    check(any(f"{gamma:#x}" in t for _, t in instructions(up8, up8 + off + 8)[-2:]),
          f"the gamma operand is the table the palette loop reads ({gamma:#x})")

    # ebx is the cache record at the tail.
    body = instructions(entry, tail)
    saves = [a for a, t in body if t == "mov dword ptr [ebp - 0x4328], ebx"]
    writes = [(a, t) for a, t in body
              if re.match(r"(mov|xor|add|sub|lea|imul|movzx|pop|inc|dec|cmov\w+) ebx,", t)]
    reloads = [a for a, t in writes if t == "mov ebx, dword ptr [ebp - 0x4328]"]
    check(len(saves) >= 2 and any(head <= a < head + len(pattern("HEAD")) for a in saves),
          f"the lookup saves the record in ebx to [ebp-0x4328], as does the new-record path ({[hex(a) for a in saves]})")
    reuse = [a for a, _ in writes if a > saves[-1] and a not in reloads]
    check(reuse and reloads and max(reuse) < reloads[-1],
          f"the one loop that reuses ebx ({hex(min(reuse)) if reuse else '?'}..) reloads it from there after ({[hex(r) for r in reloads]})")
    check(body[-1][1] == "mov word ptr [ebx + 6], ax", f"the instruction before the tail writes the record: `{body[-1][1]}`")

    # The leftover-texture fix.
    name_matched, size_branch = head + offset("head", "NAME_MATCHED"), head + offset("head", "SIZE_BRANCH")
    stolen_cache = const_bytes("NAME_MATCHED_STOLEN")
    check(img[name_matched:name_matched + len(stolen_cache)] == stolen_cache and name_matched + len(stolen_cache) == size_branch,
          "name-matched span bytes, ending at the width compare's branch")
    check([t for _, t in instructions(name_matched, size_branch + 2)] ==
          ["mov eax, dword ptr [ebp + 0x10]", "cmp eax, dword ptr [esi + 8]", f"jne {base + size_branch + 2 + 0xC:#x}"],
          "the span is `mov eax, [ebp+0x10]; cmp eax, [esi+8]`, then a jne")
    into = [a for a, t in body if t == f"je {base + name_matched:#x}"]
    check(len(into) == 1 and head < into[0] < name_matched,
          f"the name compare's je is the one branch to the span, onto its first byte ({[hex(a) for a in into]})")
    je_hit = head + offset("head", "JE_HIT")
    check(img[je_hit:je_hit + 2] == b"\x0f\x84", "JE_HIT is a je rel32")
    hit = rel32(je_hit, 2)
    hit_bytes = const_bytes("HIT")
    check(img[hit:hit + len(hit_bytes)] == hit_bytes, f"the hit path is `cmp word [esi+4], 0; jle; mov ax, [servercount]` (+{hit:#x})")
    servercount = u32(hit + len(hit_bytes))
    stamped = [a for a, t in body if t == f"movzx eax, word ptr [{servercount:#x}]"]
    check(len(stamped) == 1 and hit < stamped[0] < tail,
          f"the new-record path stamps records from the same servercount ({servercount:#x}, read at {[hex(a) for a in stamped]})")
    next_record = head + offset("head", "NEXT_RECORD")
    check([t for _, t in instructions(next_record, next_record + 6)][:1] == [f"mov ecx, dword ptr [{u32(head + 2):#x}]"],
          "NEXT_RECORD reloads the record count, as the loop's top does")

    # Nothing branches into either span except onto its first byte.
    spans = [(branch, len(stolen)), (name_matched, len(stolen_cache))]
    bad = []
    for a, t in instructions(entry, after + 0x40):
        m = re.match(r"(j\w+|loop\w*|call) 0x([0-9a-f]+)$", t)
        if m:
            target = int(m.group(2), 16) - base
            bad += [(a, target) for s0, n in spans if s0 < target < s0 + n]
    check(not bad, f"no branch lands inside a detoured span {[(hex(a), hex(t)) for a, t in bad]}")

    # dodstudio_debug_hd_misses' per-map list: the studio API's GetModelByIndex
    # (slot 5 of the table HUD_GetStudioModelInterface is handed) is a tail
    # call here, and CL_GetModelByIndex reads the table at +0x17, not +0x1b.
    studio_push = img.find(b"\x68" + struct.pack("<I", base + img.find(b"HUD_GetStudioModelInterface\0")))
    pushes = [t for _, t in instructions(studio_push, studio_push + 0x30)]
    # push "HUD_GetStudioModelInterface" ... push pstudio; push ppinterface; push 1
    later = [t for t in pushes if t.startswith("push 0x")]
    studio_api = int(later[1].split()[1], 16) - base if len(later) > 2 else 0
    wrapper = u32(studio_api + 5 * 4) - base if studio_api else 0
    check(img[wrapper:wrapper + 5] == b"\x55\x8b\xec\x5d\xe9",
          f"studio API slot 5 (table +{studio_api:#x}) is `push ebp; mov ebp, esp; pop ebp; jmp` (+{wrapper:#x})")
    body = rel32(wrapper + 4, 1)
    check(img[body + 0xB:body + 0x11] == bytes.fromhex("81ff00020000") and img[body + 0x17:body + 0x1a] == b"\x8b\x34\xbd",
          f"CL_GetModelByIndex (+{body:#x}) bounds the index at 0x200 and reads the table at +0x17")

    def site(name):
        return re.search(r"const " + name + r": \w+ = \w+ \{(.*?\n)    \};", anni, re.S).group(1)

    def field(body, name):
        m = (re.search(r"\b" + name + r": (&\[.*?\]),\n", body, re.S)
             or re.search(r"\b" + name + r": ([^\n]*?),\s*(?://[^\n]*)?\n", body))
        return m.group(1).strip()

    def num(text):
        return int(text, 0)

    def branches_into(lo, hi, start, length):
        bad = []
        for a, t in instructions(lo, hi):
            m = re.match(r"(j\w+|loop\w*|call) 0x([0-9a-f]+)$", t)
            if m and start < int(m.group(2), 16) - base < start + length:
                bad.append(hex(a))
        return bad

    # The ceiling.
    c = site("CEILING")
    size_check, too_big, size_ok = (num(field(c, k)) for k in ("size_check", "too_big", "size_ok"))
    stolen_c = byte_list(field(c, "stolen"))
    refs = [(int(a, 0), int(b, 0)) for a, b in re.findall(r"\((0x[0-9a-f]+), (0x[0-9A-F]+)\)", field(c, "buffer_refs"))]
    exp_off, exp_op = re.match(r"\((0x[0-9a-f]+), &\[(.*?)\]\)", field(c, "expansion")).groups()
    check(img[up32 + size_check:up32 + size_check + len(stolen_c)] == stolen_c, "ceiling: size-check span bytes")
    jbe_to = up32 + size_check + len(stolen_c) + stolen_c[-1]
    check(jbe_to == up32 + size_ok and up32 + size_check + len(stolen_c) == up32 + too_big,
          "ceiling: the jbe goes to SIZE_OK, the fallthrough is TOO_BIG")
    too_big_str = u32(up32 + too_big + 1) - base
    check(img[up32 + too_big] == 0x68 and img[too_big_str:too_big_str + 23] == b"GL_LoadTexture: too big",
          'ceiling: TOO_BIG pushes "GL_LoadTexture: too big"')
    pre_check = [t for a, t in instructions(up32 + size_check - 8, up32 + size_check)]
    check("imul eax, ebx" in pre_check and "mov eax, esi" in pre_check,
          f"ceiling: eax = esi (the scaled width) * ebx at the check ({pre_check})")
    buf = {u32(up32 + off + 1) for off, op in refs if img[up32 + off] == op}
    check(len(buf) == 1 and len(refs) == 5, f"ceiling: the five buffer references agree ({[hex(b) for b in buf]})")
    buf = buf.pop() if len(buf) == 1 else 0
    exp = u32(up8 + int(exp_off, 0) + 1) if img[up8 + int(exp_off, 0)] == byte_list(exp_op)[0] else 0
    check(exp == buf + 0x200000, f"ceiling: GL_Upload8's buffer starts 2 MB after it ({buf:#x} -> {exp:#x})")
    rpe = pefile.PE(ANNIVERSARY_DLL, fast_load=True)
    rpe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_BASERELOC"]])
    into = sorted(e.rva for b in rpe.DIRECTORY_ENTRY_BASERELOC for e in b.entries
                  if e.type == 3 and buf <= struct.unpack_from("<I", img, e.rva)[0] < buf + 0x200000)
    check(into == sorted(up32 + off + 1 for off, _ in refs),
          f"ceiling: nothing else refers into the scratch buffer ({[hex(r) for r in into]})")
    resamplers = [int(t.split()[1], 16) - base for a, t in instructions(up32, up32 + 0x300) if t.startswith("call 0x")]
    frames, resample_fns = [], []
    for r in resamplers:
        body_r = [t for _, t in instructions(r, r + 0x10)]
        if body_r[2:3] == ["sub esp, 0x81c"]:
            frames.append(0x81c)
        elif body_r[2:3] == ["mov eax, 0x2018"]:
            frames.append(0x2018)
        else:
            continue
        resample_fns.append(r)
    check(sorted(frames) == [0x81c, 0x2018],
          f"ceiling: the two resample helpers have the pre-Anniversary frames plus a stack cookie ({[hex(f) for f in frames]})")
    check(not branches_into(up32, up32 + 0x4c0, up32 + size_check, len(stolen_c)), "ceiling: nothing branches into the span")
    # What the replacement size check assumes about the registers (SIZE_REGS):
    # esi/ebx rounded, edi/[ebp+0x10] original, [ebp-0x10] the product and
    # [ebp-4] a second copy of the rounded width the mipmap loop reads.
    pre = [t for _, t in instructions(up32 + size_check - 0x14, up32 + size_check)]
    check("mov edi, dword ptr [ebp + 0xc]" in pre and "mov dword ptr [ebp - 0x10], eax" in pre,
          f"ceiling: edi is the original width and [ebp-0x10] the product at the check ({pre})")
    post = [t for _, t in instructions(up32 + size_ok, up32 + size_ok + 0x90)]
    check(all(t in post for t in ("mov eax, dword ptr [ebp + 0x10]", "cmp esi, edi", "cmp ebx, eax")),
          "ceiling: the own-size test compares esi/ebx with edi/[ebp+0x10]")
    body = [t for _, t in instructions(up32 + size_ok, up32 + 0x4c0)]
    check("mov eax, dword ptr [ebp - 4]" in body and "mov dword ptr [ebp - 4], esi" in [t for _, t in instructions(up32, up32 + size_check)],
          "ceiling: [ebp-4] is the rounded width the mipmap loop reads (a shrink stores it)")
    site_r = [t for _, t in instructions(up32 + refs[1][0] - 2, up32 + refs[1][0])]
    check(site_r == ["push ebx", "push esi"], f"ceiling: the resample call takes ebx/esi as the output height/width ({site_r})")
    bounded = []
    for r in resample_fns:
        body_r = [t for _, t in instructions(r, r + 0x80)]
        bounded.append(any("- 0x1004]" in t or "- 0x404]" in t for t in body_r) and any("[ebp + 0x18]" in t for t in body_r))
    check(bounded == [True, True], f"ceiling: both resample helpers index their 1024-entry arrays by the output width, their fifth argument ({[hex(r) for r in resample_fns]})")

    # Detail textures.
    d = site("DETAIL")
    dpat = pattern("DETAIL_LOADER")
    dl = find_all(img, dpat)
    check(len(dl) == 1, f"detail: DETAIL_LOADER matches once ({[hex(x) for x in dl]})")
    if dl:
        dl = dl[0]
        for off in re.findall(r"0x[0-9a-f]+", field(d, "sizes")):
            check(img[dl + int(off, 0):dl + int(off, 0) + 5] == bytes.fromhex("6800001000"), f"detail: push 0x100000 at +{off}")
        path_at = num(field(d, "path_at"))
        check(instructions(dl + path_at, dl + path_at + 6)[0][1] == "lea eax, [ebp - 0x108]", "detail: the redirect span is `lea eax, [ebp-0x108]`")
        fmt = [t for _, t in instructions(dl, dl + path_at)]
        gfx = img.find(b"gfx/%s.tga\0")
        check(f"push {base + gfx:#x}" in fmt and "lea eax, [ebp - 0x108]" in fmt and "push 0x104" in fmt,
              "detail: that is the 0x104-byte buffer the snprintf of \"gfx/%s.tga\" fills")
        load_tga = rel32(dl + path_at + 8, 1)
        lt = [t for _, t in instructions(load_tga, load_tga + 0x220)]
        check("cmp ecx, dword ptr [ebp + 0x10]" in lt, "detail: LoadTGA compares the image against the size it is told ([ebp+0x10])")
        lt2 = rel32(dl + len(dpat) - 5, 1)
        check(lt2 == entry, "detail: the loader uploads through GL_LoadTexture2")
        fn = img.rfind(b"\xcc\x55\x8b\xec", 0, dl) + 1
        check(not branches_into(fn, dl + 0x200, dl + path_at, 6), f"detail: nothing in the loader (+{fn:#x}) branches into the span")

    # Skies.
    sk = site("SKY")
    sl, su = find_all(img, pattern("SKY_LOADER")), find_all(img, pattern("SKY_UPLOAD"))
    check(len(sl) == 1 and len(su) == 1, f"sky: both patterns match once ({[hex(x) for x in sl]}, {[hex(x) for x in su]})")
    if len(sl) == 1 and len(su) == 1:
        sl, su = sl[0], su[0]
        check(sl < su < sl + 0x400, "sky: the upload is inside R_LoadSkys")
        check(img[sl + num(field(sk, "malloc_at")):][:5] == bytes.fromhex("6800000400"), "sky: malloc push 0x40000")
        for k in ("height_push", "width_push"):
            check(img[su + num(field(sk, k)):][:5] == bytes.fromhex("6800010000"), f"sky: {k} is push 0x100")
        hook = su + num(field(sk, "hook_at"))
        check(instructions(hook, hook + 7)[0][1].startswith("mov eax, dword ptr [esi*4 + 0x"), "sky: the hook span is `mov eax, [esi*4 + ...]`")
        body_s = instructions(sl, su + 0x80)
        to_hook = [a for a, t in body_s if re.match(r"j\w+ 0x", t) and int(t.split()[1], 16) - base == hook]
        check(len(to_hook) == 1, f"sky: the gl_dither skip jumps onto its first byte ({[hex(a) for a in to_hook]})")
        check(not branches_into(sl, su + 0x80, hook, 7), "sky: nothing branches inside the span")
        texts = [t for _, t in body_s]
        tga = img.find(b"gfx/env/%s%s.tga\0")
        k = texts.index(f"push {base + tga:#x}") if f"push {base + tga:#x}" in texts else -1
        check(k > 0 and "push 0x40" in texts[k:k + 3] and "lea eax, [ebp - 0x44]" in texts[k - 3:k + 3]
              and num(field(sk, "path_offset")) == 0x44, "sky: the path is the 64-byte buffer at [ebp-0x44]")
        check(any(f"{gamma:#x}]" in t for t in texts), f"sky: the gamma loop uses GL_Upload8's texgamma table ({gamma:#x})")
        check(any(f"[{dither:#x}]" in t for t in texts), "sky: and gl_dither")

    print(f"\nGL_LoadTexture2 +{entry:#x} (tail +{tail:#x}), GL_Upload32 +{up32:#x}, GL_Upload8 +{up8:#x}")
    print(f"{len(failures)} check(s) FAILED" if failures else "all checks passed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    anniversary() if "--anniversary" in sys.argv[1:] else main()
