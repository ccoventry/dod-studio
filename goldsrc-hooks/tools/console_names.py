#!/usr/bin/env python3
"""Writes `studio/src/console_commands_data.js`: every console name Studio's
Commands tab suggests as you type (#215).

Three families, each read from where it is defined rather than typed in:

    game       GoldSrc's and DoD's own cvars and commands, read out of the
               user's own `hw.dll` and `dod/cl_dlls/client.dll`:
               - cvars: the engine's static `cvar_t` records (name, default
                 string, flags, value, next) in `hw.dll`'s .data, and the
                 client's `pfnRegisterVariable(name, default, flags)` calls;
               - commands: the `Cmd_AddCommand(name, function)` calls in
                 both, found as the call site most of those pushes lead to.
               Run against both installs, so each name knows which builds
               have it.
    hlae       `mirv_*` command names out of the user's own
               `AfxHookGoldSrc.dll` (names only; HLAE's `__mirv_*` debug
               commands are left out).
    dodstudio  this repo's own: every `console_name!("...")` in
               `goldsrc-hooks/src`, with the first sentence of its entry in
               `docs/dodstudio_commands.md` as the hint.

Nothing of the game's or HLAE's is copied beyond the names themselves.

Usage:
    python goldsrc-hooks/tools/console_names.py --pre <install> --post <install> --hlae <AfxHookGoldSrc.dll>

`command_suggest.test.js` fails when a `console_name!` is missing from the
output, so a new `dodstudio_` name is a re-run away from being suggested.
"""

import argparse
import collections
import json
import re
import struct
from pathlib import Path

import capstone
import pefile

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "studio" / "src" / "console_commands_data.js"

ID = re.compile(r"^[a-z_+\-][a-z0-9_]{1,40}$")
NUM = re.compile(r"^-?[0-9.]+$")
# A registration call site must register at least this many names to count.
MIN_HITS = 10


def load(path):
    pe = pefile.PE(str(path), fast_load=True)
    data = pe.get_memory_mapped_image()
    base = pe.OPTIONAL_HEADER.ImageBase
    secs = {
        s.Name.rstrip(b"\0").decode(): (s.VirtualAddress, s.VirtualAddress + max(s.Misc_VirtualSize, s.SizeOfRawData))
        for s in pe.sections
    }
    return data, base, secs


def cstr(data, base, va, limit=64):
    off = va - base
    if off < 0 or off >= len(data):
        return None
    end = data.find(b"\0", off, off + limit)
    if end < 0:
        return None
    try:
        return data[off:end].decode("ascii")
    except UnicodeDecodeError:
        return None


def calls_with_args(path):
    """Every call in .text with the (up to three) immediates pushed before it,
    first argument first, and a key naming what it calls."""
    data, base, secs = load(path)
    lo, hi = secs[".text"]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.skipdata = True
    pushes, out = [], []
    for ins in md.disasm(data[lo:hi], base + lo):
        mn, op = ins.mnemonic, ins.op_str
        if mn == "push":
            pushes.append(int(op, 16) if op.startswith("0x") else int(op) if op.isdigit() else None)
            pushes = pushes[-4:]
        elif mn == "call":
            key = ("rel", int(op, 16)) if op.startswith("0x") else ("mem", op)
            out.append((key, list(reversed(pushes[-3:]))))
            pushes = []
        elif mn in ("ret", "jmp"):
            pushes = []
    return data, base, secs, out


def registered(path):
    """(cvars, commands) a module registers through call sites."""
    data, base, secs, calls = calls_with_args(path)
    tlo, thi = secs[".text"]
    cvars, cmds = collections.defaultdict(set), collections.defaultdict(set)
    for key, args in calls:
        if len(args) < 2 or args[0] is None or args[1] is None:
            continue
        name = cstr(data, base, args[0])
        if not name or not ID.match(name):
            continue
        if tlo <= args[1] - base < thi:
            cmds[key].add(name)
        elif len(args) >= 3 and args[2] is not None and args[2] < 0x10000:
            default = cstr(data, base, args[1], 40)
            if default is not None and re.match(r"^[ -~]{0,32}$", default):
                cvars[key].add(name)
    keep = lambda groups: {n for names in groups.values() if len(names) >= MIN_HITS for n in names}
    return keep(cvars), keep(cmds)


def static_cvars(path):
    """`cvar_t` records in .data: name, default, flags, value, next."""
    data, base, secs = load(path)
    lo, hi = secs[".data"]
    found = set()
    for off in range(lo, hi - 20, 4):
        name_va, str_va, flags, _value, nxt = struct.unpack_from("<IIIII", data, off)
        if flags > 0x100000:
            continue
        name = cstr(data, base, name_va)
        if not name or not ID.match(name):
            continue
        default = cstr(data, base, str_va, 40)
        if default is None or not re.match(r"^[ -~]{0,32}$", default):
            continue
        if nxt and not (lo <= nxt - base < hi):
            continue
        found.add(name)
    return found


def game_names(install):
    install = Path(install)
    cvars, cmds = set(), set()
    for dll in (install / "hw.dll", install / "dod" / "cl_dlls" / "client.dll"):
        c, m = registered(dll)
        cvars |= c
        cmds |= m
    cvars |= static_cvars(install / "hw.dll")
    return cvars, cmds - cvars


def hlae_names(dll):
    data = Path(dll).read_bytes()
    found = re.findall(rb"(?<![A-Za-z0-9_])(mirv_[a-z0-9_]+)\x00", data)
    return sorted({m.decode() for m in found})


def first_sentence(text):
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = text.replace("`", "").strip()
    text = re.split(r"(?<=[.;])\s|;\s", text)[0].rstrip(".;")
    return text[:117] + "..." if len(text) > 120 else text


def section_text(doc, start):
    """The first prose paragraph under a heading, joined into one line,
    without a leading "No arguments." or a usage line."""
    paras, cur = [], []
    for line in doc[start:]:
        if line.startswith("#"):
            break
        if line.strip():
            cur.append(line.strip())
        elif cur:
            paras.append(" ".join(cur))
            cur = []
    if cur:
        paras.append(" ".join(cur))
    for para in paras:
        para = re.sub(r"^No arguments\.\s*", "", para)
        if not para or para.startswith(("|", "```", "-")):
            continue
        # A bare usage line is skipped; prose that opens with the command
        # ("`dodstudio_x <n>` puts the camera...") is the description.
        if para.startswith("`") and len(para.split("`", 2)[-1].strip()) < 4:
            continue
        return para
    return ""


def dodstudio_names():
    src = REPO / "goldsrc-hooks" / "src"
    names = set()
    for f in src.rglob("*.rs"):
        # Code only: a doc comment showing how to call the macro (names.rs)
        # is not a registered name.
        code = "\n".join(
            line for line in f.read_text(encoding="utf-8").splitlines() if not line.lstrip().startswith("//")
        )
        names |= set(re.findall(r'console_name!\("([a-z0-9_]+)"\)', code))
    hints = {}
    doc = (REPO / "docs" / "dodstudio_commands.md").read_text(encoding="utf-8").splitlines()
    for i, line in enumerate(doc):
        row = re.match(r"^\| `dodstudio_([a-z0-9_]+)` \|[^|]*\| (.*?) \|", line)
        if row:
            hints.setdefault(row.group(1), first_sentence(row.group(2)))
        if line.startswith("### "):
            # A heading may name several commands ("`dodstudio_seek_to` / `dodstudio_seek_by`").
            for name in re.findall(r"`dodstudio_([a-z0-9_]+)", line):
                hints.setdefault(name, first_sentence(section_text(doc, i + 1)))
    return [["dodstudio_" + n, hints.get(n, "")] for n in sorted(names)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pre", required=True, help="the pre-Anniversary Half-Life folder")
    ap.add_argument("--post", required=True, help="the 25th Anniversary Half-Life folder")
    ap.add_argument("--hlae", required=True, help="AfxHookGoldSrc.dll")
    args = ap.parse_args()

    pre_c, pre_m = game_names(args.pre)
    post_c, post_m = game_names(args.post)
    game = []
    for name in sorted(pre_c | pre_m | post_c | post_m):
        kind = "cvar" if name in pre_c or name in post_c else "cmd"
        builds = "both" if (name in pre_c | pre_m) and (name in post_c | post_m) else "pre" if name in pre_c | pre_m else "post"
        game.append([name, kind, builds])

    js = (
        "// console_commands_data.js — GENERATED by goldsrc-hooks/tools/console_names.py.\n"
        "// Do not edit by hand: re-run it (see its docstring) after a new console name.\n"
        "// The names Studio's Commands tab suggests as you type (#215).\n\n"
        "/** [name, 'cvar' | 'cmd', 'both' | 'pre' | 'post'] from the game's own DLLs. */\n"
        f"export const GAME_NAMES = {json.dumps(game, separators=(',', ':'))};\n\n"
        "/** HLAE's commands. */\n"
        f"export const HLAE_NAMES = {json.dumps(hlae_names(args.hlae), separators=(',', ':'))};\n\n"
        "/** [name, hint] for DoD Studio's own (goldsrc-hooks). */\n"
        f"export const DODSTUDIO_NAMES = {json.dumps(dodstudio_names(), separators=(',', ':'), ensure_ascii=False)};\n"
    )
    OUT.write_text(js, encoding="utf-8", newline="\n")
    print(f"{OUT}: {len(game)} game, {len(hlae_names(args.hlae))} HLAE, {len(dodstudio_names())} dodstudio")


if __name__ == "__main__":
    main()
