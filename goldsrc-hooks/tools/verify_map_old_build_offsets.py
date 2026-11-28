#!/usr/bin/env python3
"""Checks `map_old_build/world.rs`'s `model_t` offsets against real `hw.dll` files (#1540).

`map_old_build` reads the loaded map's pieces out of the world model
(`model_t.numsubmodels`, `model_t.submodels`, `com_model.h`) and checks each
`*N` model's bounds (`model_t.mins`, `model_t.maxs`) against them. Only the
binaries can prove the offsets, and that the engine pads each piece's bounds
by one unit, which the matching relies on:

  1. `Mod_LoadSubmodels` (its own function on the pre-Anniversary build,
     inlined into `Mod_LoadBrushModel` on the 25th Anniversary one): after
     the lump's size check against 64 (`test <reg>, 0x3f`) it stores the
     array it allocates at `[loadmodel + MODEL_SUBMODELS_AT]` and the count
     at `[loadmodel + MODEL_NUMSUBMODELS_AT]`, then subtracts 1.0 from each
     `mins` and adds 1.0 to each `maxs` it reads.
  2. `Mod_LoadBrushModel`'s loop that names each piece `"*%i"` stores a
     submodel's bounds into `[mod + MODEL_MINS_AT]` .. `+8` and
     `[mod + MODEL_MAXS_AT]` .. `+8` before it, reading the submodel array
     from `[mod + MODEL_SUBMODELS_AT]`.

`MODEL_TYPE_AT` and `MODEL_ENTITIES_AT` are the offsets
`verify_map_entities_offsets.py` checks for `map_sounds`, and
`MODEL_NEEDLOAD_AT` the one `verify_map_light_offsets.py` checks for
`map_light`; this checks they match those.

Offsets are read out of `world.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_map_old_build_offsets.py [hw.dll ...]
    python goldsrc-hooks/tools/verify_map_old_build_offsets.py --anniversary

No path checks both movie installs; `--anniversary` only the 25th Anniversary
one (POST-Anniversary for Movies). Needs `pip install pefile capstone`.
"""

import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from survey_hw_dll import Image  # noqa: E402

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll"
POST = STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll"
SRC = Path(__file__).resolve().parent.parent / "src"
RUST = SRC / "map_old_build" / "world.rs"
MAP_SOUNDS = SRC / "map_sounds.rs"
MAP_LIGHT = SRC / "map_light" / "world.rs"


def offsets():
    """{name: value} for every `MODEL_*_AT` constant in world.rs."""
    text = RUST.read_text(encoding="utf-8")
    found = {m.group(1): int(m.group(2), 0)
             for m in re.finditer(r"const (MODEL_\w+_AT): usize = (0x[0-9a-fA-F]+|\d+);", text)}
    want = {"MODEL_NEEDLOAD_AT", "MODEL_TYPE_AT", "MODEL_MINS_AT", "MODEL_MAXS_AT",
            "MODEL_NUMSUBMODELS_AT", "MODEL_SUBMODELS_AT", "MODEL_ENTITIES_AT"}
    if set(found) != want:
        sys.exit(f"expected {sorted(want)} in {RUST}, found {sorted(found)}")
    return found


def aligned(hw, at, before, after):
    """Instructions from some start up to `before` bytes ahead of `at` through
    `after` bytes past it, decoded so that one of them starts at `at`."""
    for back in range(before, 0, -1):
        ins = list(hw.disasm(at - back, at + after))
        if any(i.address - hw.base == at for i in ins):
            return ins
    return list(hw.disasm(at, at + after))


def is_one(hw, op_str):
    """Whether an operand is a float constant 1.0 in the image."""
    m = re.search(r"\[(0x[0-9a-f]+)\]", op_str)
    if not m:
        return False
    rva = int(m.group(1), 16) - hw.base
    return 0 <= rva < len(hw.data) - 4 and struct.unpack_from("<f", hw.data, rva)[0] == 1.0


def check_submodels(hw, o):
    """Rule 1. Returns a description of the site, or None."""
    # Each loader has its own copy of the message on the pre-Anniversary build.
    refs = [r for at in hw.find(b"MOD_LoadBmodel: funny lump size in %s\0")
            for r in hw.xrefs_to(at) if hw.sect(r) == ".text"]
    for ref in refs:
        # The message's push is `ref - 1`; the size check that skips it sits
        # a few instructions before, and the loop within 0x140 bytes after.
        ins = aligned(hw, ref - 1, 0x40, 0x140)
        ops = [(i.mnemonic, i.op_str) for i in ins]
        push = next((k for k, i in enumerate(ins) if i.address - hw.base == ref - 1), None)
        if push is None:
            continue
        size_check = any(m == "test" and op.endswith(", 0x3f") for m, op in ops[max(0, push - 8):push])
        stores = {disp for m, op in ops if m == "mov"
                  for disp in re.findall(r"^dword ptr \[\w+ \+ (0x[0-9a-f]+)\], \w+$", op)}
        arrays = (f"{o['MODEL_SUBMODELS_AT']:#x}" in stores
                  and f"{o['MODEL_NUMSUBMODELS_AT']:#x}" in stores)
        # fsub [1.0] / fadd [1.0], or fld1 then fsubp / faddp.
        def by_one(direct, popping):
            return any(m == direct and is_one(hw, op) for m, op in ops) or any(
                ops[k][0] == "fld1" and any(m == popping for m, _ in ops[k + 1:k + 4])
                for k in range(len(ops)))

        minus = by_one("fsub", "fsubp")
        plus = by_one("fadd", "faddp")
        if size_check and arrays and minus and plus:
            return f"+{ref - 1:#x}"
    return None


def check_brush_loop(hw, o):
    """Rule 2. Returns a description of the site, or None."""
    for star in hw.find(b"*%i\0"):
        for ref in (r for r in hw.xrefs_to(star) if hw.sect(r) == ".text"):
            # Back to the top of the loop body: 0x120 bytes covers both builds.
            ins = aligned(hw, ref - 1, 0x120, 0x10)
            ops = [(i.mnemonic, i.op_str) for i in ins]
            stored = {int(d, 16) for m, op in ops if m in ("mov", "movss")
                      for d in re.findall(r"^dword ptr \[\w+ \+ (0x[0-9a-f]+)\], \w+$", op)}
            # The pre-Anniversary build stores `mins[0]` and `maxs[0]` through
            # a register it points there first (`lea ecx, [esi + 0x54]`,
            # `mov dword ptr [ecx], edx`), the address it then passes on.
            stored |= {int(d, 16) for m, op in ops if m == "lea"
                       for d in re.findall(r"^\w+, \[\w+ \+ (0x[0-9a-f]+)\]$", op)}
            bounds = {o["MODEL_MINS_AT"] + 4 * k for k in range(3)} | {
                o["MODEL_MAXS_AT"] + 4 * k for k in range(3)}
            reads_array = any(f"+ {o['MODEL_SUBMODELS_AT']:#x}]" in op for _m, op in ops)
            if bounds <= stored and reads_array:
                return f"+{ref - 1:#x}"
    return None


def check_shared(o):
    """`type` and `entities` are the offsets map_sounds reads, `needload` the
    one map_light reads (`verify_map_light_offsets.py` checks it)."""
    text = MAP_SOUNDS.read_text(encoding="utf-8")
    types = {int(v, 0) for v in re.findall(r"model_type_at: (0x[0-9a-fA-F]+|\d+)", text)}
    ents = {int(v, 0) for v in re.findall(r"model_entities_at: (0x[0-9a-fA-F]+|\d+)", text)}
    light = MAP_LIGHT.read_text(encoding="utf-8")
    needload = {int(v, 0) for v in
                re.findall(r"const MODEL_NEEDLOAD_AT: usize = (0x[0-9a-fA-F]+|\d+);", light)}
    return (types == {o["MODEL_TYPE_AT"]} and ents == {o["MODEL_ENTITIES_AT"]}
            and needload == {o["MODEL_NEEDLOAD_AT"]})


def check(path, o):
    hw = Image(path)
    ok = True
    site = check_submodels(hw, o)
    if site:
        print(f"  ok   Mod_LoadSubmodels {site}: array at +{o['MODEL_SUBMODELS_AT']:#x}, count at "
              f"+{o['MODEL_NUMSUBMODELS_AT']:#x}, bounds padded by 1.0")
    else:
        print("  FAIL: no Mod_LoadSubmodels site stores the array and count at those offsets "
              "and pads the bounds by 1.0")
        ok = False
    site = check_brush_loop(hw, o)
    if site:
        print(f"  ok   Mod_LoadBrushModel's \"*%i\" loop {site}: mins at +{o['MODEL_MINS_AT']:#x}, "
              f"maxs at +{o['MODEL_MAXS_AT']:#x}")
    else:
        print("  FAIL: Mod_LoadBrushModel's \"*%i\" loop doesn't store the bounds at those offsets")
        ok = False
    return ok


def main():
    args = sys.argv[1:]
    if args == ["--anniversary"]:
        dlls = [POST]
    else:
        dlls = [Path(p) for p in args] or [PRE, POST]
    o = offsets()
    ok = check_shared(o)
    print(("ok   " if ok else "FAIL ") + f"type +{o['MODEL_TYPE_AT']:#x} and entities "
          f"+{o['MODEL_ENTITIES_AT']:#x} are map_sounds' offsets, needload "
          f"+{o['MODEL_NEEDLOAD_AT']:#x} map_light's")
    for path in dlls:
        print(path)
        ok &= check(path, o)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
