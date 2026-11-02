#!/usr/bin/env python3
"""Checks `map_light/world.rs`'s world-model offsets against real `hw.dll` files (#1312).

`ds_map_apply_lighting` reads and writes the engine's own `msurface_t`s
(`com_model.h`) through `model_t.surfaces`, with no code patched and no
`hw.dll` address used. The layout is the SDK's; only the binaries can prove
both builds keep it, so for each `hw.dll` this finds, by what they do rather
than where they are:

  1. `R_BuildLightMap`, the function that prints "Error: lightmap for texture
     %s too large": it reads `extents` (two `movsx` words), `samples`,
     `texinfo` (then `texinfo->texture`), `styles` and `cached_light`, and
     compares the luxel count with 0x144 (18 x 18), `luxels::MAX_LUXELS`.
  2. `R_RenderDynamicLightmaps`, the caller of (1) that skips
     `SURF_DRAWSKY | SURF_DRAWTURB` (`test byte [s + flags], 0x14`) and
     compares each style's value with `cached_light`: the comparison a stale
     `cached_light` trips, so the lightmap is rebuilt.
  3. The caller of (1) that skips a `SURF_DRAWTILED` surface whose
     `texinfo->flags` has `TEX_SPECIAL` (`test byte [t + 0x28], 1`).
  4. `R_AddDynamicLights`, the callee of (1) that reads the surface's plane
     (+0x04, then `normal` and `dist` +0x0c) and `texturemins` (two words).
  5. `GL_BuildLightmaps` (or the Anniversary engine's rebuild of every
     model's lightmaps): a function that reads `model_t.numsurfaces` and
     `model_t.surfaces` and steps through the array by `sizeof(msurface_t)`.

`model_t.type` (+0x44) is the one `map_sounds.rs` uses, checked by
`verify_map_entities_offsets.py`; this checks the two files agree.

Offsets are read out of `world.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_map_light_offsets.py [hw.dll ...]
    python goldsrc-hooks/tools/verify_map_light_offsets.py --anniversary

No path checks both movie installs; `--anniversary` only the 25th Anniversary
one (POST-Anniversary for Movies). Needs `pip install pefile capstone`.
"""

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from survey_hw_dll import Image  # noqa: E402

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
PRE = STEAM / "Half-Life - PRE-Anniversary for Movies" / "hw.dll"
POST = STEAM / "Half-Life - POST-Anniversary for Movies" / "hw.dll"
SRC = Path(__file__).resolve().parent.parent / "src"
WORLD = SRC / "map_light" / "world.rs"
LUXELS = SRC / "map_light" / "luxels.rs"
MAP_SOUNDS = SRC / "map_sounds.rs"
MESSAGE = b"Error: lightmap for texture %s too large"


def layout():
    """{name: offset} from world.rs's consts and its layout test."""
    text = WORLD.read_text(encoding="utf-8")
    out = {}
    for name in ("MODEL_TYPE_AT", "MODEL_NUMSURFACES_AT", "MODEL_SURFACES_AT"):
        out[name] = int(re.search(rf"const {name}: usize = (0x[0-9a-fA-F]+);", text).group(1), 16)
    for struct, field, value in re.findall(
            r"offset_of!\((\w+), (\w+)\), (0x[0-9a-fA-F]+)\)", text):
        out[f"{struct}.{field}"] = int(value, 16)
    out["mirror_size"] = int(re.search(r"size_of::<MSurface>\(\) == (0x[0-9a-fA-F]+)", text).group(1), 16)
    table = text.split("const BUILDS", 1)[1].split("\n];", 1)[0]
    out["builds"] = [
        (re.search(r'name: "([^"]+)"', b).group(1),
         int(re.search(r"time_date_stamp: (0x[0-9a-fA-F_]+)", b).group(1).replace("_", ""), 16),
         int(re.search(r"size_of_image: (0x[0-9a-fA-F_]+)", b).group(1).replace("_", ""), 16),
         int(re.search(r"surface_size: (0x[0-9a-fA-F_]+)", b).group(1).replace("_", ""), 16))
        for b in re.split(r"\n    Build \{", table)[1:]
    ]
    if len(out["builds"]) != 2:
        sys.exit(f"expected 2 builds in {WORLD}, found {len(out['builds'])}")
    out["max_luxels"] = int(re.search(r"const MAX_LUXELS: usize = (0x[0-9a-fA-F]+);",
                                      LUXELS.read_text(encoding="utf-8")).group(1), 16)
    sounds = MAP_SOUNDS.read_text(encoding="utf-8")
    out["map_sounds.model_type_at"] = {int(v, 0) for v in re.findall(r"model_type_at: (0x[0-9a-fA-F]+|\d+)", sounds)}
    return out


def disasm_function(hw, start, limit=0x1000):
    """Instructions from `start` up to int3 padding or the next prologue."""
    out = []
    base = hw.base + start
    for ins in hw.disasm(start, start + limit):
        if ins.mnemonic == "int3":
            break
        if out and ins.address != base and ins.mnemonic == "mov" and ins.op_str == "ebp, esp" \
                and out[-1].mnemonic == "push" and out[-1].op_str == "ebp" and out[-1].address != base:
            out.pop()
            break
        out.append(ins)
    return out


def text_of(body):
    return [f"{i.mnemonic} {i.op_str}" for i in body]


def has(lines, rx):
    return any(re.search(rx, line) for line in lines)


def number(value):
    """`value` as capstone prints a displacement: decimal below 10, else hex."""
    return str(value) if value < 10 else f"{value:#x}"


def disp(value):
    return rf"\+ {number(value)}\]"


def field(value):
    """`[reg + value]`."""
    return rf"\[\w+ \+ {number(value)}\]"


def field2(value):
    """`[reg + reg + value]`."""
    return rf"\[\w+ \+ \w+ \+ {number(value)}\]"


def callees(hw, body):
    return sorted({int(i.op_str, 16) - hw.base for i in body
                   if i.mnemonic == "call" and re.fullmatch(r"0x[0-9a-f]+", i.op_str)})


def check(path, lay):
    hw = Image(path)
    ok = True

    def result(good, what):
        nonlocal ok
        ok &= good
        print(f"  {'ok  ' if good else 'FAIL'} {what}")

    stamp, image = hw.pe.FILE_HEADER.TimeDateStamp, hw.pe.OPTIONAL_HEADER.SizeOfImage
    entry = next((b for b in lay["builds"] if (b[1], b[2]) == (stamp, image)), None)
    if entry is None:
        result(False, f"no BUILDS entry has TimeDateStamp {stamp:#x} and SizeOfImage {image:#x}")
        return False
    name, _, _, size = entry
    result(size >= lay["mirror_size"], f"{name}: TimeDateStamp {stamp:#x}, SizeOfImage {image:#x}, "
                                       f"msurface_t {size:#x} bytes (the mirror reads {lay['mirror_size']:#x})")
    at = hw.data.find(MESSAGE)
    refs = [r for r in hw.xrefs_to(at) if hw.sect(r) == ".text"] if at >= 0 else []
    if not refs:
        result(False, "R_BuildLightMap's message isn't referenced")
        return False
    build = hw.func_start(refs[0], back=0x400)
    body = text_of(disasm_function(hw, build))
    s = lambda f: lay[f"MSurface.{f}"]  # noqa: E731
    ext = s("extents")
    result(has(body, rf"movsx \w+, word ptr {field(ext)}") and
           has(body, rf"movsx \w+, word ptr {field(ext + 2)}"),
           f"R_BuildLightMap +{build:#x} reads extents at +{ext:#x}/+{ext + 2:#x}")
    result(has(body, rf"mov \w+, dword ptr {field(s('samples'))}"),
           f"  ... samples at +{s('samples'):#x}")
    result(has(body, rf"{field(s('texinfo'))}"), f"  ... texinfo at +{s('texinfo'):#x}")
    result(has(body, rf"lea \w+, {field(s('styles'))}") or has(body, rf"{field2(s('styles'))}"),
           f"  ... styles at +{s('styles'):#x}")
    result(has(body, rf"lea \w+, {field(s('cached_light'))}"),
           f"  ... cached_light at +{s('cached_light'):#x}")
    result(has(body, rf"cmp \w+, {lay['max_luxels']:#x}$"), f"  ... caps a lightmap at {lay['max_luxels']:#x} luxels")

    callers = sorted({hw.func_start(c, back=0x800) for c in hw.calls_to(build)} - {None})
    bodies = {c: text_of(disasm_function(hw, c)) for c in callers}
    dynamic = [c for c, b in bodies.items()
               if has(b, rf"test byte ptr {field(s('flags'))}, 0x14")
               and has(b, rf"lea \w+, {field(s('cached_light'))}")
               and has(b, r"cmp eax, dword ptr \[\w+\]")]
    result(len(dynamic) == 1,
           f"R_RenderDynamicLightmaps {', '.join(f'+{c:#x}' for c in dynamic) or '(none)'}: skips sky/water "
           f"(flags +{s('flags'):#x} & 0x14), compares each style with cached_light")
    tiled = [c for c, b in bodies.items() if has(b, r"test \w+, 0x20$") and has(b, rf"test byte ptr {field(lay['MTexInfo.flags'])}, 1")]
    result(len(tiled) >= 1, f"a lightmap builder +{tiled[0]:#x} skips SURF_DRAWTILED with TEX_SPECIAL "
                            f"(texinfo flags +{lay['MTexInfo.flags']:#x})" if tiled else
           "no caller skips SURF_DRAWTILED + TEX_SPECIAL")

    lights = []
    for callee in callees(hw, disasm_function(hw, build)):
        b = text_of(disasm_function(hw, callee, limit=0x600))
        if (has(b, rf"mov \w+, dword ptr {field(s('plane'))}")
                and has(b, rf"movsx \w+, word ptr {field(s('texturemins'))}")
                and has(b, rf"movsx \w+, word ptr {field(s('texturemins') + 2)}")
                and has(b, rf"{field(lay['MPlane.dist'])}")
                # texinfo->vecs[0][3] and vecs[1][3], the texture offsets
                # (x87 on the pre-Anniversary build, SSE on the Anniversary one)
                and has(b, rf"(fadd|addss \w+,) dword ptr {field(0x0c)}")
                and has(b, rf"(fadd|addss \w+,) dword ptr {field(0x1c)}")):
            lights.append(callee)
    result(len(lights) == 1, f"R_AddDynamicLights {', '.join(f'+{c:#x}' for c in lights) or '(none)'}: plane at "
                             f"+{s('plane'):#x} (dist +{lay['MPlane.dist']:#x}), texturemins at +{s('texturemins'):#x}, "
                             f"texinfo vecs at +0")

    # Any function reading both model_t fields and stepping by sizeof(msurface_t).
    num, arr = lay["MODEL_NUMSURFACES_AT"], lay["MODEL_SURFACES_AT"]
    walkers = []
    seen = set()
    for caller in callers + [c2 for c in callers for c2 in
                             sorted({hw.func_start(x, back=0x800) for x in hw.calls_to(c)} - {None})]:
        if caller in seen:
            continue
        seen.add(caller)
        b = text_of(disasm_function(hw, caller, limit=0x800))
        if has(b, disp(num)) and has(b, rf"mov \w+, dword ptr {field(arr)}") and \
                (has(b, rf"add \w+, {size:#x}$") or has(b, rf"lea \w+, {field(size)}")):
            walkers.append(caller)
    result(len(walkers) >= 1, f"lightmap builder(s) {', '.join(f'+{c:#x}' for c in walkers) or '(none)'} walk "
                              f"model_t.surfaces (+{arr:#x}, count +{num:#x}) {size:#x} bytes a surface")
    result(lay["map_sounds.model_type_at"] == {lay["MODEL_TYPE_AT"]},
           f"model_t.type +{lay['MODEL_TYPE_AT']:#x} matches map_sounds.rs")
    return ok


def main():
    args = sys.argv[1:]
    paths = [POST] if args == ["--anniversary"] else [Path(a) for a in args] or [PRE, POST]
    lay = layout()
    ok = True
    for path in paths:
        print(path)
        ok &= check(path, lay)
    print("all ok" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
