#!/usr/bin/env python3
"""Checks `map_sounds.rs`'s call sites against real `hw.dll` files (#284).

`map_sounds.rs` (`ds_sound_mute_victory_music`, `ds_sound_mute_flag_capture`) redirects the three calls
the `svc_sound` and `svc_spawnstaticsound` handlers make into
`S_StartStaticSound` / `S_StartDynamicSound`. The unit tests prove each
pattern's offset lands on an `E8`; only the binaries can prove:

  1. Exactly one build's three patterns match this `hw.dll`, each exactly
     once in `.text`, and the other build's match nothing.
  2. Each call's target is the function its kind says: the one that prints
     "S_StartStaticSound: %s volume > 255" or "S_StartDynamicSound: ...".
  3. Each call sits in the handler the site names, read from the engine's own
     svc dispatch table (`survey_hw_dll.py parse`): svc_sound's two in its
     handler, svc_spawnstaticsound's in the function its handler jumps to.
  4. The caller pops 8 arguments (`add esp, 0x20` within a few bytes), the
     cdecl signature `start_static` / `start_dynamic` declare.
  5. No call site overlaps a span HLAE patches in `hw.dll`
     (`docs/goldsrc_hw_dll_survey.md` §12.3), and the sound functions' own
     entries, which HLAE detours, are left alone (we patch only the call).

Patterns are read out of `map_sounds.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_map_sounds_offsets.py [hw.dll ...]
    python goldsrc-hooks/tools/verify_map_sounds_offsets.py --anniversary

No path checks both movie installs; `--anniversary` only the 25th Anniversary
one (POST-Anniversary for Movies). Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from survey_hw_dll import Image, svc_table  # noqa: E402

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll"
POST = STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll"
RUST = Path(__file__).resolve().parent.parent / "src" / "map_sounds.rs"

# HLAE's mid-function span patches in hw.dll (survey §12.3): (rva, length).
HLAE_SPANS = [
    (0xb75b4, 5), (0xb7639, 5), (0x1d3e6, 7),  # pre-Anniversary
    (0x25d2d2, 5), (0x25d34f, 5), (0x244354, 9), (0x244492, 12),
    (0x251521, 8), (0x2516e6, 6), (0x1a7ddc, 11),  # 25th Anniversary
]


def builds():
    """[(build, [(what, kind, pattern, call_at)])] from map_sounds.rs."""
    text = RUST.read_text(encoding="utf-8")
    out = []
    for block in re.split(r"\n    Build \{", text.split("const BUILDS", 1)[1].split("];", 1)[0])[1:]:
        name = re.search(r'name: "([^"]+)"', block).group(1)
        sites = [(m.group(1), m.group(2), m.group(3), int(m.group(4))) for m in re.finditer(
            r'what: "([^"]+)",\s*kind: Kind::(\w+),.*?pattern: "([^"]+)",\s*call_at: (\d+)', block, re.S)]
        out.append((name, sites))
    if len(out) != 2 or any(len(s) != 3 for _, s in out):
        sys.exit(f"expected 2 builds of 3 sites in {RUST}, found {[(n, len(s)) for n, s in out]}")
    return out


def named(hw, message):
    """Start of the function that references the string starting `message`."""
    at = hw.data.find(message.encode())
    if at < 0:
        return None
    for ref in hw.xrefs_to(at):
        if hw.sect(ref) == ".text":
            return hw.func_start(ref, back=0x800)
    return None


def check(path, all_builds):
    hw = Image(path)
    ok = True
    table_rva, rows = svc_table(hw)
    handlers = {name: func for _op, name, func in rows}
    starts = {
        "Static": named(hw, "S_StartStaticSound: %s volume"),
        "Dynamic": named(hw, "S_StartDynamicSound: %s volume"),
    }
    if None in starts.values() or "svc_sound" not in handlers:
        print(f"  FAIL: could not name the sound functions or read the svc table ({starts})")
        return False
    spawn = handlers["svc_spawnstaticsound"]
    if hw.data[spawn] == 0xE9:  # the handler is a jmp to CL_ParseStaticSound
        spawn = spawn + 5 + struct.unpack("<i", hw.data[spawn + 1:spawn + 5])[0]
    owner = {
        "svc_sound's static call": handlers["svc_sound"],
        "svc_sound's dynamic call": handlers["svc_sound"],
        "svc_spawnstaticsound's call": spawn,
    }
    matched = []
    for build, sites in all_builds:
        hits = {what: hw.match_pattern(pattern) for what, _k, pattern, _c in sites}
        if not any(hits.values()):
            continue
        matched.append(build)
        for what, kind, pattern, call_at in sites:
            found = hits[what]
            if len(found) != 1:
                print(f"  FAIL {build} {what}: {len(found)} matches, expected one")
                ok = False
                continue
            call = found[0] + call_at
            target = call + 5 + struct.unpack("<i", hw.data[call + 1:call + 5])[0]
            problems = []
            if hw.data[call] != 0xE8:
                problems.append("not an E8 call")
            if target != starts[kind]:
                problems.append(f"calls +{target:#x}, not S_Start{kind}Sound +{starts[kind]:#x}")
            if hw.func_start(call, back=0x400) != owner[what]:
                problems.append(f"not inside its handler +{owner[what]:#x}")
            if b"\x83\xc4\x20" not in hw.data[call + 5:call + 5 + 8]:
                problems.append("no add esp, 0x20 after the call")
            for rva, length in HLAE_SPANS:
                if rva < call + 5 and call < rva + length:
                    problems.append(f"overlaps HLAE's span at +{rva:#x}")
            if problems:
                print(f"  FAIL {build} {what} at +{call:#x}: {'; '.join(problems)}")
                ok = False
            else:
                print(f"  ok   {build} {what}: call at +{call:#x} -> S_Start{kind}Sound +{target:#x}, "
                      f"in +{owner[what]:#x}, 8 args")
    if len(matched) != 1:
        print(f"  FAIL: {len(matched)} builds matched ({matched}), expected exactly one")
        ok = False
    return ok


def main():
    args = sys.argv[1:]
    if args == ["--anniversary"]:
        dlls = [POST]
    else:
        dlls = [Path(p) for p in args] or [PRE, POST]
    all_builds = builds()
    ok = True
    for path in dlls:
        print(path)
        ok &= check(path, all_builds)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
