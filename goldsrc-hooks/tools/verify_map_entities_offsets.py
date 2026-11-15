#!/usr/bin/env python3
"""Checks `map_sounds.rs`'s `model_t` offsets against real `hw.dll` files (#284).

`map_sounds.rs` reads each map's entity lump out of the world model the engine
loaded (`model_t.entities`, `com_model.h`), and checks `model_t.type` is
`mod_brush` first. Its `BUILDS` table gives both offsets per build. Only the
binaries can prove them:

  1. `Mod_LoadBrushModel`, the function that prints "Mod_LoadBrushModel: %s
     has wrong version number", zeroes `[loadmodel + model_type_at]` (sets
     `type = mod_brush`) before it reads the header.
  2. Exactly one function it calls is `Mod_LoadEntities`: it stores 0 at
     `[loadmodel + model_entities_at]` when the lump is empty, else stores a
     call's result there (`Hunk_AllocName`), copies the lump into it
     (`memcpy`, the stored pointer pushed as the destination), and reads it
     back to look for the "wad" key.
  3. Both use the same `loadmodel` global, so they are offsets into the same
     struct.

Offsets are read out of `map_sounds.rs` rather than restated.

Usage:
    python goldsrc-hooks/tools/verify_map_entities_offsets.py [hw.dll ...]
    python goldsrc-hooks/tools/verify_map_entities_offsets.py --anniversary

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
RUST = Path(__file__).resolve().parent.parent / "src" / "map_sounds.rs"


def builds():
    """[(build, model_type_at, model_entities_at)] from map_sounds.rs."""
    text = RUST.read_text(encoding="utf-8")
    table = text.split("const BUILDS", 1)[1].split("\n];", 1)[0]
    out = []
    for block in re.split(r"\n    Build \{", table)[1:]:
        name = re.search(r'name: "([^"]+)"', block).group(1)
        type_at = int(re.search(r"model_type_at: (0x[0-9a-fA-F]+|\d+)", block).group(1), 0)
        ents_at = int(re.search(r"model_entities_at: (0x[0-9a-fA-F]+|\d+)", block).group(1), 0)
        out.append((name, type_at, ents_at))
    if len(out) != 2:
        sys.exit(f"expected 2 builds in {RUST}, found {len(out)}")
    return out


def disasm_function(hw, start, limit=0x2000):
    """Instructions from `start` up to int3 padding or the next function's
    `push ebp; mov ebp, esp` (the pre-Anniversary build pads with nothing)."""
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


def which_build(hw, all_builds):
    """The build whose sound call sites match, by `verify_map_sounds_offsets`'s rule."""
    text = RUST.read_text(encoding="utf-8")
    table = text.split("const BUILDS", 1)[1].split("\n];", 1)[0]
    for (name, *_), block in zip(all_builds, re.split(r"\n    Build \{", table)[1:]):
        patterns = re.findall(r'pattern: "([^"]+)"', block)
        if patterns and all(len(hw.match_pattern(p)) == 1 for p in patterns):
            return name
    return None


def check(path, all_builds):
    hw = Image(path)
    build = which_build(hw, all_builds)
    if build is None:
        print("  FAIL: no build's sound call sites match this hw.dll")
        return False
    _, type_at, ents_at = next(b for b in all_builds if b[0] == build)
    print(f"  {build}: model_type_at {type_at:#x}, model_entities_at {ents_at:#x}")

    at = hw.data.find(b"Mod_LoadBrushModel: %s has wrong version")
    refs = [r for r in hw.xrefs_to(at) if hw.sect(r) == ".text"] if at >= 0 else []
    if not refs:
        print("  FAIL: Mod_LoadBrushModel's message isn't referenced")
        return False
    brush = hw.func_start(refs[0], back=0x200)
    body = disasm_function(hw, brush)
    ok = True

    # 1. loadmodel->type = mod_brush, through the loadmodel global.
    loadmodel = None
    globals_in = {}  # register -> the global it was last loaded from
    zero_regs = set()
    for ins in body[:24]:
        g = re.fullmatch(r"(\w+), dword ptr \[(0x[0-9a-f]+)\]", ins.op_str)
        if ins.mnemonic == "mov" and g:
            globals_in[g.group(1)] = int(g.group(2), 16)
        x = re.fullmatch(r"(\w+), (\w+)", ins.op_str)
        if ins.mnemonic == "xor" and x and x.group(1) == x.group(2):
            zero_regs.add(x.group(1))
        m = re.fullmatch(r"dword ptr \[(\w+) \+ (0x[0-9a-f]+)\], (\w+)", ins.op_str)
        if (ins.mnemonic == "mov" and m and int(m.group(2), 16) == type_at
                and m.group(1) in globals_in and (m.group(3) in zero_regs or m.group(3) == "0")):
            loadmodel = globals_in[m.group(1)]
            break
    if loadmodel is None:
        print(f"  FAIL: Mod_LoadBrushModel +{brush:#x} doesn't zero [loadmodel + {type_at:#x}]")
        ok = False
    else:
        print(f"  ok   Mod_LoadBrushModel +{brush:#x} sets type (+{type_at:#x}) = mod_brush "
              f"through loadmodel [{loadmodel:#x}]")

    # 2. Mod_LoadEntities among its callees.
    callees = sorted({int(ins.op_str, 16) - hw.base for ins in body
                      if ins.mnemonic == "call" and ins.op_str.startswith("0x")})
    wad = [a for a in hw.find(b"wad\0") if hw.data[a - 1] == 0]
    found = []
    for callee in callees:
        if hw.sect(callee) != ".text":
            continue
        code = disasm_function(hw, callee, limit=0x200)
        ops = [(i.mnemonic, i.op_str) for i in code]
        disp = f"+ {ents_at:#x}]"
        zero_store = any(m == "mov" and op.endswith(f"{disp}, 0") for m, op in ops)
        result_store = any(
            ops[k][0] == "call" and any(m == "mov" and op.endswith(f"{disp}, eax") for m, op in ops[k + 1:k + 6])
            for k in range(len(ops))
        )
        pushed = any(m == "push" and op.endswith(disp) for m, op in ops) or any(
            m == "push" and op == "eax" for m, op in ops)
        read_back = sum(1 for m, op in ops if disp in op) >= 3
        globals_used = {int(g, 16) for _m, op in ops for g in re.findall(r"dword ptr \[(0x[0-9a-f]+)\]", op)}
        uses_wad = any(f"{hw.base + w:#x}" in op for w in wad for _m, op in ops)
        if zero_store and result_store and pushed and read_back:
            found.append((callee, loadmodel in globals_used, uses_wad))
    if len(found) != 1:
        print(f"  FAIL: {len(found)} callees look like Mod_LoadEntities at +{ents_at:#x}, expected one")
        return False
    callee, same_global, uses_wad = found[0]
    problems = []
    if not same_global:
        problems.append("doesn't use the same loadmodel global")
    if not uses_wad:
        problems.append("doesn't look for the \"wad\" key")
    if problems:
        print(f"  FAIL Mod_LoadEntities +{callee:#x}: {'; '.join(problems)}")
        return False
    print(f"  ok   Mod_LoadEntities +{callee:#x} stores its hunk copy of the lump at "
          f"[loadmodel + {ents_at:#x}], 0 when empty, and parses it for \"wad\"")
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
