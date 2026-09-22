#!/usr/bin/env python3
"""Checks `decals.rs` against a real GoldSrc `hw.dll`.

`decals.rs` calls an engine function by address and then writes 114KB of the
engine's own data. Both are recovered from two signatures rather than written
down, which means the thing worth checking is not "is this address right" but
"is the function at the end of that relative call really `R_DecalUnlink`".

Every constant below is read out of the Rust rather than restated.

  1. Both signatures match exactly once in `.text`.
  2. Each value `decals.rs` reads out of a match lands on an instruction
     *operand*, by disassembling rather than by counting bytes.
  3. The call `decals.rs` resolves really is `R_DecalUnlink`, identified
     independently: it is the only function in the image that references the
     string `"Bad decal list"`, which is what it prints when a decal is not in
     its surface's list.
  4. The two signatures agree about where the decal pool starts.
  5. The pool span equals the `memset` length `R_DecalInit` passes, is a whole
     number of `DECAL_SIZE` decals, and comes to `MAX_RENDER_DECALS` of them.
  6. The engine's own remove loop reads `psurface` from the offset `decals.rs`
     reads it from -- that is, the field `R_DecalUnlink` itself dereferences.

Run it against the other installed engine to see the supported-build claim hold:

    python goldsrc-hooks/tools/verify_decal_offsets.py "<...POST-Anniversary...>/hw.dll"

which is expected to fail check 1 on the remove loop, and say so.

Usage:
    python goldsrc-hooks/tools/verify_decal_offsets.py [path-to-hw.dll]

Defaults to the pre-Anniversary movies install. Requires `pefile` and
`capstone` (`pip install pefile capstone`); both are analysis-only and are not
build dependencies of anything in the workspace.
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
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)
SRC = Path(__file__).resolve().parent.parent / "src" / "decals.rs"

# What R_DecalUnlink prints when a decal is not in its surface's list. Used to
# identify the function without trusting any address.
UNLINK_STRING = b"Bad decal list\x00"


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
        sys.exit(f"no hw.dll at {dll}")

    src = SRC.read_text(encoding="utf-8")
    remove_pat = parse_pattern(rust_string(src, r"const REMOVE_LOOP:\s*&str"))
    init_pat = parse_pattern(rust_string(src, r"const DECAL_INIT:\s*&str"))
    pool_base_at = rust_const(src, "POOL_BASE_AT")
    unlink_call_at = rust_const(src, "UNLINK_CALL_AT")
    pool_end_at = rust_const(src, "POOL_END_AT")
    init_size_at = rust_const(src, "INIT_POOL_SIZE_AT")
    init_base_at = rust_const(src, "INIT_POOL_BASE_AT")
    init_count_at = rust_const(src, "INIT_DECAL_COUNT_AT")
    decal_size = rust_const(src, "DECAL_SIZE")
    psurface_at = rust_const(src, "PSURFACE_AT")

    pe = pefile.PE(str(dll), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    img = pe.get_memory_mapped_image()
    text = pe.sections[0]
    tlo, thi = text.VirtualAddress, text.VirtualAddress + text.Misc_VirtualSize
    code = img[tlo:thi]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    failures = []

    def check(ok, message):
        print(("  ok   " if ok else "  FAIL ") + message)
        if not ok:
            failures.append(message)

    print(f"hw.dll at {dll}")
    print(f"image base {base:#x}\n")

    # 1. one match each
    remove_hits = find_all(code, remove_pat)
    check(
        len(remove_hits) == 1,
        f"the remove loop matches exactly once ({len(remove_hits)} hit(s))"
        + ("" if remove_hits else " -- not the pre-Anniversary engine"),
    )
    init_hits = find_all(code, init_pat)
    check(len(init_hits) == 1, f"R_DecalInit matches exactly once ({len(init_hits)} hit(s))")
    if len(remove_hits) != 1 or len(init_hits) != 1:
        print("\n1 or more check(s) FAILED")
        return 1

    remove = tlo + remove_hits[0]
    init = tlo + init_hits[0]
    print(f"\nremove loop at hw+{remove:#x}, R_DecalInit at hw+{init:#x}\n")

    def u32(rva):
        return struct.unpack_from("<I", img, rva)[0]

    # 2. every read lands on an operand
    boundaries = {}
    for ins in md.disasm(bytes(img[remove : remove + len(remove_pat)]), base + remove):
        boundaries[ins.address - base - remove] = ins
    ins = boundaries.get(pool_base_at - 1)
    check(
        ins is not None and ins.mnemonic == "mov" and len(ins.bytes) == 5,
        f"POOL_BASE_AT is the imm32 of `{ins.mnemonic + ' ' + ins.op_str if ins else '?'}`",
    )
    ins = boundaries.get(unlink_call_at)
    check(
        ins is not None and ins.mnemonic == "call",
        f"UNLINK_CALL_AT is `{ins.mnemonic + ' ' + ins.op_str if ins else '?'}`",
    )
    ins = boundaries.get(pool_end_at - 2)
    check(
        ins is not None and ins.mnemonic == "cmp" and len(ins.bytes) == 6,
        f"POOL_END_AT is the imm32 of `{ins.mnemonic + ' ' + ins.op_str if ins else '?'}`",
    )

    init_boundaries = {}
    for ins in md.disasm(bytes(img[init : init + len(init_pat)]), base + init):
        init_boundaries[ins.address - base - init] = ins
    ins = init_boundaries.get(init_size_at - 1)
    check(
        ins is not None and ins.mnemonic == "push",
        f"INIT_POOL_SIZE_AT is the imm32 of `{ins.mnemonic + ' ' + ins.op_str if ins else '?'}`",
    )
    ins = init_boundaries.get(init_base_at - 1)
    check(
        ins is not None and ins.mnemonic == "push",
        f"INIT_POOL_BASE_AT is the imm32 of `{ins.mnemonic + ' ' + ins.op_str if ins else '?'}`",
    )
    ins = init_boundaries.get(init_count_at - 2)
    check(
        ins is not None and ins.mnemonic == "mov" and len(ins.bytes) == 10,
        f"INIT_DECAL_COUNT_AT is the address of `{ins.mnemonic + ' ' + ins.op_str if ins else '?'}`",
    )

    # 3. the call really is R_DecalUnlink
    unlink = remove + unlink_call_at + 5 + struct.unpack_from("<i", img, remove + unlink_call_at + 1)[0]
    string_rva = img.find(UNLINK_STRING)
    check(string_rva > 0, 'the string "Bad decal list" is in the image')
    pushes = [
        m.start()
        for m in re.finditer(re.escape(bytes([0x68]) + struct.pack("<I", base + string_rva)), img, re.S)
    ]
    check(len(pushes) == 1, f"it is pushed from exactly one site ({len(pushes)})")
    if pushes:
        # Walk back from the push to the nearest function prologue.
        site = pushes[0]
        start = None
        for candidate in range(site, max(site - 0x400, tlo), -1):
            if img[candidate : candidate + 3] == b"\x55\x8b\xec" and img[candidate - 1] in (0x90, 0xC3, 0xCC):
                start = candidate
                break
        check(start is not None, "its function's prologue is recognisable")
        check(
            start == unlink,
            f"the resolved call target hw+{unlink:#x} is that function"
            + ("" if start == unlink else f" (found hw+{start:#x})"),
        )

    # 6. psurface is the field R_DecalUnlink dereferences
    window = img[unlink : unlink + 0x40]
    check(
        re.search(rb"\x8b\x46" + bytes([psurface_at]), window) is not None
        or re.search(rb"\x8b\x40" + bytes([psurface_at]), window) is not None,
        f"R_DecalUnlink reads [reg+{psurface_at:#x}] -- decal_t::psurface",
    )

    # 4 + 5. the two signatures agree, and the arithmetic holds
    pool_base = u32(remove + pool_base_at)
    pool_end = u32(remove + pool_end_at)
    init_base = u32(init + init_base_at)
    cleared = u32(init + init_size_at)
    decal_count = u32(init + init_count_at)
    check(pool_base == init_base, f"both signatures name the pool at {pool_base:#x}")
    check(
        pool_end - pool_base == cleared,
        f"the loop walks {pool_end - pool_base:#x} bytes and R_DecalInit clears {cleared:#x}",
    )
    check(cleared % decal_size == 0, f"{cleared:#x} is a whole number of {decal_size}-byte decals")
    check(cleared // decal_size == 4096, f"that is {cleared // decal_size} decals (MAX_RENDER_DECALS)")
    print(f"\n  gDecalCount at {decal_count:#x}, R_DecalUnlink at hw+{unlink:#x}")

    print()
    if failures:
        print(f"{len(failures)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
