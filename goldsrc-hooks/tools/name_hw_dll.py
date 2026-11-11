#!/usr/bin/env python3
"""Names `hw.dll`'s functions from the symbols in Valve's Linux engine builds.

The Half-Life Dedicated Server's Linux depot (SteamCMD, anonymous login; see
`docs/goldsrc_linux_symbols.md`) ships the engine unstripped:

    sw.so            the software-renderer client engine, 25th Anniversary
                     only: CL_*, demo playback, V_*, SCR_*, Key_*, S_*, R_*
                     (the software renderer's, not GL's) and all the shared code
    engine_i486.so   the dedicated server's engine, both builds: Host_*, SV_*,
                     Cmd_*, Cbuf_*, COM_*, MSG_*, NET_*, Mod_*, PM_*, DELTA_*

Same source as `hw.dll`, other compiler, so this reuses
`name_client_dll.py`'s matcher (vtables where the DLL has RTTI, strings,
constants, callbacks registered beside a string, then call-graph gaps), adds
the engine's dispatch tables (`table`), and drops the `cl_enginefunc_t` slots,
which only `client.dll` calls through. It also reads the other engine modules
against their `.so`: `DemoPlayer.dll` with `demoplayer.so` (it has RTTI).

`--check` measures the result against functions this project located in
`hw.dll` by hand (`KNOWN`, from `docs/goldsrc_hw_dll_survey.md` and the
`goldsrc-hooks` modules' per-build tables): a hit is a known function the map
names correctly, a miss one it names wrongly.

Usage:
    python goldsrc-hooks/tools/name_hw_dll.py --hw <hw.dll> --so <sw.so|engine_i486.so>
        [--out FILE] [--check pre|anniversary] [--find NAME ...]

`--find` prints the `hw.dll` rva the map gives each named Linux function.
Requires `pyelftools`, `pefile` and `capstone`.
"""

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import name_client_dll  # noqa: E402
from name_client_dll import LinuxLibrary, WindowsDll, write_map  # noqa: E402

# Functions located in `hw.dll` by hand, with where each was found. Names are
# the Linux build's (`CL_Parse_Time` is `svc_time`'s handler).
KNOWN = {
    "pre": {
        "Cbuf_Init": 0x272B0,  # survey §3.1
        "CL_Disconnect": 0x17850,  # survey §2
        "CL_CheckGameDirectory": 0x1C2A0,  # game_dir_check.rs
        "CL_ParseServerMessage": 0x1D300,  # survey §7
        "CL_ParsePacketEntities": 0x12D90,  # survey §8
        "CL_PlayDemo_f": 0x105FD,  # survey §14.9 landmarks
        "CL_ViewDemo_f": 0x1093D,
        "CL_TimeDemo_f": 0x10B1D,
        "CL_ParseStaticSound": 0x1CA90,  # map_sounds.rs
        "CL_HudMessage": 0x19AB0,  # survey §7 (#677)
        "CL_Parse_Time": 0x1D930,  # svc table, survey §7
        "CL_Parse_Version": 0x1DBE0,
        "CL_Parse_Disconnect": 0x1DA30,
        "CL_Parse_StuffText": 0x1E1A0,
        "CL_Parse_LightStyle": 0x1E280,
        "ClientDLL_Key_Event": 0xB960,  # demo_console_keys.rs
        "Con_ToggleConsole_f": 0x2C690,  # demo_console_keys.rs
        "SCR_CenterPrint": 0x4BA90,  # survey §7 (#677)
        "V_ScreenShake": 0xC24E0,  # hltv_fov / view modules
        "V_CheckGamma": 0xC19B0,  # lightmap_gamma.rs
        "R_DecalInit": 0x49DA0,  # decals.rs
        "R_DecalUnlink": 0x49E80,  # decals.rs
        "R_BuildLightMap": 0x47390,  # lightmap_gamma.rs
        "Mod_LoadBrushModel": 0x41BB0,  # map_entities.rs
        "Mod_LoadEntities": 0x40A20,  # map_entities.rs
        "PM_RecursiveHullCheck": 0x6C830,  # hull_trace_guard.rs
        "PM_PlayerTraceEx": 0x6BF00,  # hull_trace_guard.rs
        "PM_HullPointContents": 0x6B810,  # hull_trace_guard.rs
    },
    "anniversary": {
        "CL_PlayDemo_f": 0x199890,  # survey §14.9 landmarks
        "CL_ViewDemo_f": 0x19C590,
        "CL_TimeDemo_f": 0x19C530,
        "CL_ParseStaticSound": 0x1A8410,
        "CL_HudMessage": 0x1A30A0,
        "ClientDLL_Key_Event": 0x1971B0,
        "Con_ToggleConsole_f": 0x1BC360,
        "SCR_CenterPrint": 0x24CDE0,
        "V_ScreenShake": 0x2322C0,
        "R_DecalInit": 0x2484D0,
        "R_DecalUnlink": 0x2492A0,
        "R_BuildLightMap": 0x247B50,
        "Mod_LoadBrushModel": 0x23F060,
        "Mod_LoadEntities": 0x23FF90,
        "PM_RecursiveHullCheck": 0x1E2540,
        "SV_RecursiveHullCheck": 0x237540,
    },
}


class EngineDll(WindowsDll):
    """`hw.dll`: as `client.dll`, but with no engine table of slots to read."""

    def engine_slot(self, rva):
        return None


# A table needs this many function pointers before it is matched as one.
MIN_TABLE_POINTERS = 4


def so_tables(so):
    """{object name: [(offset, kind, value)]} for each `.so` object holding function pointers.

    `kind` is "f" (value: the function's start) or "s" (value: the string).
    """
    out = {}
    lib = so.lib
    for mangled, (addr, size) in lib.objects.items():
        layout = []
        for off in range(0, size - 3, 4):
            target = lib.abs_refs.get(addr + off)
            if target is None:
                continue
            if target in so.functions:
                layout.append((off, "f", target))
            else:
                text = so.string_at(target)
                if text is not None:
                    layout.append((off, "s", text))
        if sum(1 for _o, k, _v in layout if k == "f") >= MIN_TABLE_POINTERS:
            out[mangled] = (size, layout)
    return out


def _dll_pointer_runs(dll):
    """{run length: [start rva]} of consecutive relocated dwords that are function starts."""
    runs, at, end = {}, None, None
    for place in sorted(p for p, t in dll.relocs.items() if t in dll.functions and not dll.in_text(p)):
        if at is not None and place == end:
            end += 4
            continue
        if at is not None:
            runs.setdefault((end - at) // 4, []).append(at)
        at, end = place, place + 4
    if at is not None:
        runs.setdefault((end - at) // 4, []).append(at)
    return runs


def _dll_table_at(dll, base, layout):
    """The `hw.dll` function for each "f" slot when every "s" slot's string agrees at `base`, else None."""
    pairs = []
    for off, kind, value in layout:
        target = dll.relocs.get(base + off)
        if target is None:
            return None
        if kind == "s":
            if dll.string_at(target) != value:
                return None
        elif target in dll.functions:
            pairs.append((target, value))
    return pairs


def match_tables(names):
    """Pair the slots of the same table: dispatch tables, function-pointer structs.

    A table with strings in it (`cl_parsefuncs`: opcode, name, handler) is
    found in `hw.dll` from its first string and kept only when every string
    lands at the same offset. A table of pointers only is kept when exactly
    one run of relocated function pointers in `hw.dll` has its length.
    """
    dll = names.dll
    by_string = {}
    for place, target in dll.relocs.items():
        text = dll.string_at(target) if not dll.in_text(place) else None
        if text is not None:
            by_string.setdefault(text, []).append(place)
    runs = _dll_pointer_runs(dll)
    for _name, (size, layout) in so_tables(names.so).items():
        strings = [(off, value) for off, kind, value in layout if kind == "s"]
        if strings:
            first_off, first = strings[0]
            bases = {place - first_off for place in by_string.get(first, ())}
            found = [p for p in (_dll_table_at(dll, b, layout) for b in bases) if p]
        else:
            count = size // 4
            pointers = [v for _o, _k, v in layout]
            found = []
            if len(pointers) == count and len(runs.get(count, ())) == 1:
                base = runs[count][0]
                found = [list(zip((dll.relocs[base + 4 * i] for i in range(count)), pointers))]
        if len(found) != 1:
            continue
        for rva, so_start in found[0]:
            names.add(rva, so_start, "table")


# `callgraph` pairs by position between two matched calls, which fails on the
# engine's small helpers: MSVC inlines `Q_strcmp`, `VectorMA` and the `FS_*`
# wrappers that GCC calls, so the gaps hold different functions. Built from
# two different `.so` files, 30 of 506 names disagreed between the two maps
# of the pre-Anniversary `hw.dll`, every one a callgraph pair, nearly all
# under this size. A callgraph pair needs a function at least this big.
MIN_CALLGRAPH_BYTES = 64


class EngineNameMap(name_client_dll.NameMap):
    def add(self, rva, so_start, method, check=True):
        fn = self.so.functions.get(so_start)
        if method == "callgraph" and fn is not None and fn.size < MIN_CALLGRAPH_BYTES:
            self.rejected["callgraph (small)"] += 1
            return False
        return super().add(rva, so_start, method, check)


def build_engine_map(dll, so):
    """`name_client_dll.build_map` with the table method after the vtables."""
    names = EngineNameMap(dll, so)
    name_client_dll.match_exports(names)
    name_client_dll.match_vtables(names)
    match_tables(names)
    name_client_dll.match_callbacks(names)
    while True:
        before = len(names.by_dll)
        name_client_dll.match_features(names)
        name_client_dll.match_callgraph(names)
        if len(names.by_dll) == before:
            return names


def check(names, known):
    """[(name, known rva, mapped rva or None, verdict)] for each known function.

    Verdicts: `hit` (the map names that rva with that name), `miss` (it
    gives the name to another rva), `unnamed` (no pair for it), `absent`
    (the Linux file has no function by that name).
    """
    so_by_name = {}
    for start in names.so.functions:
        so_by_name.setdefault(names.so.name(start)[0], start)
    out = []
    for name, rva in sorted(known.items(), key=lambda kv: kv[1]):
        so_start = so_by_name.get(name)
        if so_start is None:
            out.append((name, rva, None, "absent"))
            continue
        mapped = names.by_so.get(so_start)
        verdict = "unnamed" if mapped is None else ("hit" if mapped == rva else "miss")
        out.append((name, rva, mapped, verdict))
    return out


SELF_NAMED = re.compile(r"^([A-Z][A-Za-z]*_[A-Za-z0-9_]+)\s*:")


def self_named_agreement(names):
    """(agree, disagree examples, total) over named functions whose own message names a function.

    GoldSrc's errors start with the function's name (`"Cbuf_AddText:
    overflow"`). A message can sit in a function that inlined the named one,
    so a disagreement is a lead, not proof of a wrong name.
    """
    agree, disagree = 0, []
    for rva, (so_start, _method) in names.by_dll.items():
        claimed = {m.group(1) for s in names.dll.functions[rva].strings
                   if (m := SELF_NAMED.match(s[2:]))}
        if not claimed:
            continue
        name = names.so.name(so_start)[0]
        if name in claimed:
            agree += 1
        else:
            disagree.append((rva, name, sorted(claimed)))
    return agree, disagree


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--hw", required=True, type=Path)
    parser.add_argument("--so", required=True, type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check", choices=sorted(KNOWN))
    parser.add_argument("--find", nargs="+", default=[])
    args = parser.parse_args(argv)

    dll = EngineDll(args.hw)
    so = LinuxLibrary(args.so)
    dll.scan_all()
    so.scan_all()
    names = build_engine_map(dll, so)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            write_map(names, f)
    summary = name_client_dll.summary(names).replace("client.dll", "hw.dll").replace("client.so", args.so.name)
    by_method = Counter(m for _so, m in names.by_dll.values())
    # `summary` lists `name_client_dll`'s methods; this one's differ.
    lines = [line for line in summary.splitlines() if not line.startswith("by method")]
    lines.append("by method: " + ", ".join(f"{m} {c}" for m, c in by_method.most_common()))
    print("\n".join(lines), file=sys.stderr)
    if args.check:
        rows = check(names, KNOWN[args.check])
        for name, rva, mapped, verdict in rows:
            got = f"{mapped:#x}" if mapped is not None else "-"
            print(f"{verdict:8} {name:26} known {rva:#x}  mapped {got}")
        hits = sum(1 for r in rows if r[3] == "hit")
        misses = sum(1 for r in rows if r[3] == "miss")
        print(f"{hits} hit, {misses} miss, of {len(rows)} known")
        agree, disagree = self_named_agreement(names)
        print(f"self-named messages: {agree} named functions agree, {len(disagree)} differ")
        for rva, name, claimed in disagree:
            print(f"  differs  {rva:#x} {name}: message names {', '.join(claimed)}")
    for wanted in args.find:
        found = [(rva, so_start, method) for rva, (so_start, method) in names.by_dll.items()
                 if names.so.name(so_start)[0] == wanted]
        for rva, so_start, method in found or [(None, None, None)]:
            where = f"hw.dll+{rva:#x} ({method}; Linux {so_start:#x})" if rva is not None else "not named"
            print(f"{wanted}: {where}")


if __name__ == "__main__":
    main()
