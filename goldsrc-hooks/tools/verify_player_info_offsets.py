#!/usr/bin/env python3
"""Checks the `g_PlayerExtraInfo` offsets `player_info.rs` reads (#1130).

`player_info.rs` reads `client.dll`'s `g_PlayerExtraInfo[65]` (48 bytes a
player) at fixed RVAs: the table at +0xe9118, `teamnumber` at +0x08,
`health` at +0x24, `dead` at +0x2c. This proves, on real `client.dll` files:

  1. The two `health` writers store to table + 0x24 with a 48-byte index
     (`lea edx, [r + r*2]; shl edx, 4; mov [edx + table + 0x24], r32`):
     `HUD_ProcessPlayerState` at +0x31130 and `CHud::MsgFunc_HLTV` at
     +0x222c9.
  2. `CHudDoDCommon::Draw` reads `dead` at table + 0x2c (+0x2c771, the
     spectator camera's death switch), and `HUD_ProcessPlayerState` writes
     `teamnumber` at table + 0x08 (+0x31063).
  3. `MsgFunc_HLTV`'s "every player" loop steps 48 bytes from `health` of
     entry 0 and stops before entry 64: 64 entries written, inside the 65
     that `client.so`'s DWARF gives the table (3,120 bytes).

The absolute operands are rebased against the image's preferred base, so a
relocated load changes nothing here. client.dll is byte-identical in both
movie installs; both are checked anyway.

Usage:
    python goldsrc-hooks/tools/verify_player_info_offsets.py [client.dll ...]

Defaults to both movie installs. Needs `pip install pefile`.
"""

import struct
import sys
from pathlib import Path

try:
    import pefile
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install pefile`")

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
DEFAULT_DLLS = [
    STEAM / "Half-Life - PRE-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
    STEAM / "Half-Life - POST-Anniversary for Movies" / "dod" / "cl_dlls" / "client.dll",
]
# Must match goldsrc-hooks/src/player_info.rs.
TABLE_RVA = 0xE9118
STRIDE = 48
TEAM_AT, HEALTH_AT, DEAD_AT = 0x08, 0x24, 0x2C
# Entries the HLTV loop writes (`jl` before entry 64).
LOOP_ENTRIES = 64

# (rva of the instruction, its opcode bytes before the disp32, field)
SITES = [
    (0x31130, bytes([0x89, 0x8A]), HEALTH_AT, "HUD_ProcessPlayerState: mov [edx+health], ecx"),
    (0x222C9, bytes([0x89, 0x9A]), HEALTH_AT, "CHud::MsgFunc_HLTV: mov [edx+health], ebx"),
    (0x31063, bytes([0x66, 0x89, 0xB2]), TEAM_AT, "HUD_ProcessPlayerState: mov [edx+teamnumber], si"),
    (0x2C771, bytes([0x8A, 0x81]), DEAD_AT, "CHudDoDCommon::Draw: mov al, [ecx+dead]"),
]
# MsgFunc_HLTV's loop over every entry: mov eax, table+health ... cmp eax, end.
LOOP_START = 0x222D1
LOOP_END_CMP = 0x222DB


def check(path):
    pe = pefile.PE(str(path), fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    image = pe.get_memory_mapped_image()
    failures = []
    for rva, opcode, field, what in SITES:
        got = image[rva:rva + len(opcode)]
        disp = struct.unpack_from("<I", image, rva + len(opcode))[0] - base
        want = TABLE_RVA + field
        ok = got == opcode and disp == want
        print(f"  {'ok  ' if ok else 'FAIL'} +{rva:#07x} {what}: operand +{disp:#x} (want +{want:#x})")
        if not ok:
            failures.append(what)
    start = struct.unpack_from("<I", image, LOOP_START + 1)[0] - base
    end = struct.unpack_from("<I", image, LOOP_END_CMP + 1)[0] - base
    entries = (end - start) // STRIDE
    ok = image[LOOP_START] == 0xB8 and start == TABLE_RVA + HEALTH_AT and entries == LOOP_ENTRIES
    print(f"  {'ok  ' if ok else 'FAIL'} MsgFunc_HLTV loop: +{start:#x}..+{end:#x}, {entries} entries")
    if not ok:
        failures.append("loop")
    return failures


def main(argv):
    paths = [Path(a) for a in argv] or DEFAULT_DLLS
    bad = 0
    for path in paths:
        print(path)
        if not path.exists():
            print("  missing")
            bad += 1
            continue
        bad += len(check(path))
    print("all checks passed" if bad == 0 else f"{bad} check(s) failed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
