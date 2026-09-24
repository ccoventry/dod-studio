"""Independently verifies texture_hires.rs's DRAW_MIPTEX_TEXTURE signature
against the real pre-Anniversary-for-Movies hw.dll: parses the same pattern
string Rust embeds, confirms it matches EXACTLY ONCE in the whole file, and
that the match lands at Draw_MiptexTexture's own entry point (identified
independently via its "Draw_MiptexTexture" debug string).

Run after touching the pattern in texture_hires.rs, the same way the other
verify_*_offsets.py scripts in this folder are used -- this is not part of
`cargo test` because it needs the real DLL on disk, which CI does not have.
"""

import re
import struct
import sys

HW_DLL = (
    r"C:\Program Files (x86)\Steam\steamapps\common"
    r"\Half-Life - PRE-Anniversary for Movies\hw.dll"
)

# Kept in sync with texture_hires.rs's DRAW_MIPTEX_TEXTURE constant by hand;
# a mismatch here means one of the two was edited without the other.
PATTERN = (
    "55 8B EC 83 EC 28 53 56 8B 75 08 57 83 7E 18 20 74 10 8B 06 "
    "50 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 08 8B 76 18 8B 5D 0C 03 F3 B9 0A 00 00 00 8D 7D D8 "
    "6A 10 F3 A5 8D 4D D8 51 53 E8 ?? ?? ?? ?? 8B 55 E8 52 FF 15 ?? ?? ?? ?? 89 43 10 8B 45 EC "
    "50 FF 15 ?? ?? ?? ?? 83 C4 14 33 F6 89 43 14 89 73 28 89 73 24 89 73 20 89 73 30 89 73 2C "
    "8D 7B 34 8B 4C B5 F0 51 FF 15 ?? ?? ?? ?? 8B 55 08 83 C4 04"
)

STOLEN = bytes.fromhex("558BEC83EC28 5356".replace(" ", ""))


def parse_pattern(text):
    tokens = text.split()
    return [None if t == "??" else int(t, 16) for t in tokens]


def find_all(data, pattern):
    first = pattern[0]
    hits = []
    for i in range(len(data) - len(pattern) + 1):
        if data[i] != first:
            continue
        if all(want is None or data[i + j] == want for j, want in enumerate(pattern)):
            hits.append(i)
    return hits


def find_string_offset(data, needle: bytes):
    return data.find(needle)


def main():
    try:
        with open(HW_DLL, "rb") as f:
            data = f.read()
    except OSError as e:
        print(f"could not read {HW_DLL}: {e}", file=sys.stderr)
        sys.exit(1)

    pattern = parse_pattern(PATTERN)
    hits = find_all(data, pattern)
    print(f"DRAW_MIPTEX_TEXTURE matches: {len(hits)} -- {[hex(h) for h in hits]}")
    if len(hits) != 1:
        print("FAIL: expected exactly one match", file=sys.stderr)
        sys.exit(1)

    match_offset = hits[0]
    if data[match_offset : match_offset + len(STOLEN)] != STOLEN:
        print("FAIL: the match's first bytes don't equal STOLEN", file=sys.stderr)
        sys.exit(1)
    print(f"STOLEN bytes confirmed at the match's own start (+{len(STOLEN)} bytes)")

    # Cross-check: the "Draw_MiptexTexture" string's address, embedded at a
    # wildcarded offset within the match (the error-path `push <string addr>`),
    # should itself resolve to a string with that exact content elsewhere in
    # the file -- proving the wildcard really is that operand and not some
    # other coincidentally-matching span.
    # Not a bare, individually-null-terminated string -- it's the prefix of a
    # longer "Draw_MiptexTexture: Bad ..." error message, so match the prefix
    # only.
    string_offset = find_string_offset(data, b"Draw_MiptexTexture")
    if string_offset == -1:
        print("FAIL: 'Draw_MiptexTexture' string not found in the file", file=sys.stderr)
        sys.exit(1)
    # PE image base for this build, and the fact RVA == file offset in this
    # section -- both established during the original R&D session.
    image_base = 0x1D00000
    string_va = image_base + string_offset
    # The wildcarded 4-byte span at pattern index 21 (the `68 <imm32>` push).
    push_operand_offset = match_offset + 22
    embedded_va = struct.unpack_from("<I", data, push_operand_offset)[0]
    if embedded_va != string_va:
        print(
            f"FAIL: embedded string address {hex(embedded_va)} != actual "
            f"'Draw_MiptexTexture' string VA {hex(string_va)}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(
        f"cross-check OK: the match's push-string operand ({hex(embedded_va)}) "
        f"is genuinely the 'Draw_MiptexTexture' string's address"
    )

    print("\nAll checks passed.")


if __name__ == "__main__":
    main()
