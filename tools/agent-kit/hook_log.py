r"""The hook DLL's log, one game run at a time.

    hook_log.py --list                       the day's runs: start, end, PID, build, crashes, captures
    hook_log.py                              the last run: summary (demo clock, crashes, errors,
                                             capture start/stop, one status line per module)
    hook_log.py --run 3 | --run -2 | --at 14:05 | --pid 26664
                                             pick a run by index (1 = the day's first; 0 = lines
                                             from a run that began on an earlier day; negative
                                             from the end), by a wall time inside it, or by the
                                             game's PID
    hook_log.py --grep "seek_to: at" [--run ...]
                                             matching lines of THAT run only
    hook_log.py --module spray               every line one module wrote in the run
    hook_log.py --stamps                     the run's demo-clock stretches (a new one where the
                                             clock goes back: a new demo, a seek)
    hook_log.py --report <game_probe report.json>
                                             the same, on the hook-log tail a game_probe run saved

Why runs: a day's log holds every game started that day, so a plain search or a wait for a
line matches an earlier run's line (the "waitfor matched an earlier run" trap). Every query
here stays inside one run.

The log: %APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks_<YYYYMMDD>.log (DOD_STUDIO_LOG_DIR
or DS_STUDIO_APPDATA move it; --log PATH or --date YYYYMMDD|today|yesterday pick one). A run
starts at the hook's "new session" line; the few lines the DLL writes just before it (no demo
stamp, within 5 s) belong to the new run. The PID comes from the remote pipe's name
(dodstudio-hl-<pid>). The `[demo N]` stamp is client time since launch, not the viewdemo bar.
Crashes are named from goldsrc-hooks/tools/crash_report.py's KNOWN table. Only reads.
"""
import argparse
import importlib.util
import json
import os
import re
import sys
from datetime import date, datetime, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

LINE = re.compile(r"^\[(\d\d:\d\d:\d\d(?:\.\d+)?)\]\s+(?:\[demo\s+(-?[\d.]+)\]\s+)?(?:\[([\w-]+)\]\s+)?(.*)$")
SESSION = re.compile(r"=+ new session, ([\d-]+) =+")
PIPE_PID = re.compile(r"dodstudio-hl-(\d+)")
BUILD = re.compile(r"goldsrc-hooks build (\S+)")
MODULE = re.compile(r"^([A-Za-z_][\w./-]{1,40}?):\s")
CRASH = re.compile(r"\b(CRASH|PANIC|panicked)\b")
CRASH_SITE = re.compile(r"CRASH: .* at (\S+\.dll\+0x[0-9a-f]+)", re.I)
ERROR = re.compile(r"\b(error|failed|fatal|Host_Error|could not|couldn't)\b", re.I)
CAPTURE = re.compile(r"\b(mirv_recordmovie_start|mirv_recordmovie_stop|startmovie|endmovie)\b")
PRE_SESSION_SECS = 5.0


# ── Parsing (pure) ───────────────────────────────────────────────────────────

def wall_secs(stamp):
    h, m, s = stamp.split(":")
    return int(h) * 3600 + int(m) * 60 + float(s)


def parse(lines):
    """Lines as dicts {n, wall, demo, module, text, raw}; a line without a time stamp
    (a continuation, a blank) keeps the time of the line above."""
    out, wall, demo = [], None, None
    for n, raw in enumerate(lines, 1):
        raw = raw.rstrip("\r\n")
        m = LINE.match(raw)
        if m:
            wall = m.group(1)
            demo = float(m.group(2)) if m.group(2) else None
            text = m.group(4)
            mod = MODULE.match(text)
            out.append({"n": n, "wall": wall, "demo": demo, "module": mod.group(1) if mod else None,
                        "text": text, "raw": raw})
        elif raw.strip():
            out.append({"n": n, "wall": wall, "demo": None, "module": None, "text": raw, "raw": raw})
    return out


def split_runs(entries):
    """[{index, lines}]: one per game start; index 0 for lines before the day's first
    session line (a run that began on an earlier day), then 1, 2, ..."""
    starts = [i for i, e in enumerate(entries) if SESSION.search(e["text"])]
    # The DLL logs a line or two (hlae_window) just before its session line.
    for k, i in enumerate(starts):
        floor = starts[k - 1] + 1 if k else 0
        j = i
        while (j - 1 >= floor and entries[j - 1]["demo"] is None and entries[j - 1]["wall"]
               and entries[i]["wall"] and 0 <= wall_secs(entries[i]["wall"]) - wall_secs(entries[j - 1]["wall"])
               <= PRE_SESSION_SECS):
            j -= 1
        starts[k] = j
    runs = []
    if not starts or starts[0] > 0:
        head = entries[:starts[0]] if starts else entries
        if head:
            runs.append({"index": 0, "lines": head})
    for k, i in enumerate(starts):
        end = starts[k + 1] if k + 1 < len(starts) else len(entries)
        runs.append({"index": k + 1, "lines": entries[i:end]})
    return runs


def first(lines, pattern):
    for e in lines:
        m = pattern.search(e["text"])
        if m:
            return m.group(1)
    return None


def stretches(lines):
    """Demo-clock stretches [(first, last, line count, start wall)]: a new one starts
    where the clock goes back (a new demo or a seek back)."""
    out = []
    for e in lines:
        if e["demo"] is None:
            continue
        if out and e["demo"] >= out[-1][1]:
            out[-1] = (out[-1][0], e["demo"], out[-1][2] + 1, out[-1][3])
        else:
            out.append((e["demo"], e["demo"], 1, e["wall"]))
    return out


def captures(lines):
    """[(wall, demo, command)] for each capture start/stop the run saw."""
    out = []
    for e in lines:
        m = CAPTURE.search(e["text"])
        if m:
            out.append((e["wall"], e["demo"], m.group(1)))
    return out


def crashes(lines, known=None):
    """[(line, what it is or None)] for CRASH/PANIC lines; a crash's register and stack
    lines are folded into its first line."""
    known = known or {}
    out = []
    for e in lines:
        if not CRASH.search(e["text"]):
            continue
        if e["text"].startswith("CRASH:   "):
            continue  # registers and stack of the crash above
        site = CRASH_SITE.search(e["text"])
        out.append((e, known.get(site.group(1).lower()) if site else None))
    return out


def errors(lines):
    return [e for e in lines if ERROR.search(e["text"]) and not CRASH.search(e["text"])]


def module_status(lines):
    """{module: (line count, last line)} in first-seen order."""
    out = {}
    for e in lines:
        if e["module"]:
            count, _ = out.get(e["module"], (0, None))
            out[e["module"]] = (count + 1, e)
    return out


def run_info(run, known=None):
    lines = run["lines"]
    walls = [e["wall"] for e in lines if e["wall"]]
    pid = first(lines, PIPE_PID)
    return {
        "index": run["index"], "start": walls[0] if walls else None, "end": walls[-1] if walls else None,
        "pid": int(pid) if pid else None, "build": first(lines, BUILD), "lines": len(lines),
        "crashes": len(crashes(lines, known)), "captures": sum(1 for c in captures(lines) if c[2].endswith("start")
                                                            or c[2] == "startmovie"),
        "quit": any("HUD_Shutdown" in e["text"] for e in lines),
    }


def pick(runs, index=None, at=None, pid=None):
    """The run asked for (the last when nothing is); None when none matches."""
    if pid is not None:
        return next((r for r in runs if first(r["lines"], PIPE_PID) == str(pid)), None)
    if at is not None:
        target = wall_secs(at if at.count(":") == 2 else at + ":00")
        best = None
        for r in runs:
            walls = [e["wall"] for e in r["lines"] if e["wall"]]
            if walls and wall_secs(walls[0]) <= target:
                best = r
        return best
    if index is None:
        return runs[-1] if runs else None
    if index < 0:
        return runs[index] if -index <= len(runs) else None
    return next((r for r in runs if r["index"] == index), None)


def grep(lines, pattern):
    rx = re.compile(pattern)
    return [e for e in lines if rx.search(e["raw"])]


# ── Files ────────────────────────────────────────────────────────────────────

def logs_dir(env=os.environ):
    override = env.get("DOD_STUDIO_LOG_DIR")
    return Path(override) if override else kit_paths.studio_appdata(env) / "logs"


def log_for(day, env=os.environ):
    if day in (None, "today"):
        d = date.today()
    elif day == "yesterday":
        d = date.today() - timedelta(days=1)
    else:
        d = datetime.strptime(day.replace("-", ""), "%Y%m%d").date()
    return logs_dir(env) / f"dodstudio_goldsrc_hooks_{d:%Y%m%d}.log"


def read_lines(args):
    if args.report:
        report = json.loads(Path(args.report).read_text(encoding="utf-8"))
        return (report.get("hook_log") or "").splitlines(), f"{args.report} (hook_log)"
    path = Path(args.log) if args.log else log_for(args.date)
    if not path.exists():
        raise SystemExit(f"no hook log at {path}")
    return path.read_text(encoding="utf-8", errors="replace").splitlines(), str(path)


def known_crashes():
    """crash_report.py's KNOWN table (module+offset -> what it is), or {} if it moved."""
    path = kit_paths.REPO / "goldsrc-hooks" / "tools" / "crash_report.py"
    try:
        spec = importlib.util.spec_from_file_location("crash_report", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return {k.lower(): v for k, v in module.KNOWN.items()}
    except (OSError, AttributeError, ImportError, SyntaxError):
        return {}


# ── Command ──────────────────────────────────────────────────────────────────

def stamp(e):
    demo = f"[demo {e['demo']:9.3f}] " if e["demo"] is not None else ""
    return f"{e['n']:>6} {e['wall']} {demo}{e['text']}"


def summary(run, info, known, max_lines):
    lines = run["lines"]
    out = [f"run {info['index']}: {info['start']} - {info['end']}, PID {info['pid'] or '?'}, build {info['build'] or '?'}, "
           f"{info['lines']} lines{', HUD_Shutdown seen' if info['quit'] else ''}"]
    st = stretches(lines)
    out.append(f"demo clock ([demo N], client time since launch): {len(st)} stretch(es)")
    out += [f"  {a:9.3f} -> {b:9.3f}  ({n} lines, from {w})" for a, b, n, w in st[:max_lines]]
    cr = crashes(lines, known)
    out.append(f"crashes: {len(cr)}")
    out += [f"  {stamp(e)}" + (f"\n         known: {what}" if what else "") for e, what in cr[:max_lines]]
    errs = errors(lines)
    out.append(f"error-like lines: {len(errs)}")
    out += [f"  {stamp(e)}" for e in errs[:max_lines]]
    caps = captures(lines)
    out.append(f"capture start/stop: {len(caps)}")
    out += [f"  {w}  demo {d if d is not None else '?'}  {c}" for w, d, c in caps[:max_lines * 2]]
    mods = module_status(lines)
    out.append(f"modules: {len(mods)} (lines, last line)")
    out += [f"  {name:<24} {count:>5}  {e['text'][:150]}" for name, (count, e) in mods.items()]
    return "\n".join(out)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    src = p.add_mutually_exclusive_group()
    src.add_argument("--log")
    src.add_argument("--date")
    src.add_argument("--report", help="a game_probe report.json: its hook_log tail")
    which = p.add_mutually_exclusive_group()
    which.add_argument("--run", type=int)
    which.add_argument("--at", help="HH:MM[:SS] inside the run")
    which.add_argument("--pid", type=int)
    p.add_argument("--list", action="store_true")
    p.add_argument("--grep")
    p.add_argument("--module")
    p.add_argument("--stamps", action="store_true")
    p.add_argument("--max", type=int, default=15, help="lines per section in the summary")
    p.add_argument("--json", action="store_true")
    args = p.parse_args(argv)
    raw, source = read_lines(args)
    runs = split_runs(parse(raw))
    known = known_crashes()
    if args.list:
        infos = [run_info(r, known) for r in runs]
        if args.json:
            print(json.dumps(infos, indent=1))
            return 0
        print(f"{source}: {len(runs)} run(s)")
        print(f"{'run':>4} {'start':<12} {'end':<12} {'pid':>6} {'lines':>6} {'crash':>5} {'caps':>4} shut build")
        for i in infos:
            print(f"{i['index']:>4} {i['start'] or '?':<12} {i['end'] or '?':<12} {i['pid'] or '?':>6} {i['lines']:>6} "
                  f"{i['crashes']:>5} {i['captures']:>4} {'yes' if i['quit'] else 'no ':<4} {i['build'] or '?'}")
        return 0
    run = pick(runs, args.run, args.at, args.pid)
    if run is None:
        print(f"{source}: no such run ({len(runs)} run(s); --list shows them)", file=sys.stderr)
        return 1
    info = run_info(run, known)
    if args.grep or args.module:
        hits = run["lines"]
        if args.module:
            hits = [e for e in hits if e["module"] == args.module]
        if args.grep:
            hits = grep(hits, args.grep)
        if args.json:
            print(json.dumps(hits, indent=1))
        else:
            print(f"run {info['index']} (PID {info['pid'] or '?'}, {info['start']} - {info['end']}): {len(hits)} line(s)")
            for e in hits:
                print(stamp(e))
        return 0 if hits else 1
    if args.stamps:
        st = stretches(run["lines"])
        if args.json:
            print(json.dumps(st))
        else:
            for a, b, n, w in st:
                print(f"{w}  {a:9.3f} -> {b:9.3f}  {n} lines")
        return 0
    if args.json:
        lines = run["lines"]
        print(json.dumps(dict(info, stretches=stretches(lines),
                              crashes=[dict(e, known=what) for e, what in crashes(lines, known)],
                              errors=errors(lines), captures=captures(lines),
                              modules={k: {"lines": c, "last": e["text"]} for k, (c, e) in module_status(lines).items()}),
                         indent=1))
        return 0
    print(f"{source}: {len(runs)} run(s)")
    print(summary(run, info, known, args.max))
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main())
