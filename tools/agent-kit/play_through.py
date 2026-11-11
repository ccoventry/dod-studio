r"""Plays demos to their end in the game and says how far each got.

For each demo and each install (PRE, POST or both), one game_probe.py run:
launch behind the user's windows, `ds_capture_batch away`, play the demo
(`playdemo` or `viewdemo`), fast-forward at `host_framerate 0.05` until the
demo ends (an end line, a crash or the time budget), then read the run's
report for:

- the last demo-clock stamp the hook logged (`[demo N]`: where it stopped);
- clock jumps between stamps (forward more than 8 s or back more than 1 s);
- the end line: Host_EndGame, Host_Error or End of demo, else "end of file"
  when only the engine's routine next-demo line follows (a clean end);
- crashes (the hook's CRASH: lines) and known-bad console lines (svc_bad,
  "without model", aaatrigger, Fatal Error; add more with --bad).

Takes the game lock (game_lock.py) before every launch and releases it once
that game has exited. Demo `snapshot` commands recorded into a demo write .bmp
files into the install's game folder; new ones are moved into the run's folder.

    play_through.py <demo>... [--install pre|post|both] [--play viewdemo] [--dll DLL]
                    [--out DIR] [--budget SECONDS] [--dry-run]

<demo> is a name in --demo-dir (default: PRE's dod folder) or a path to a .dem.
POST plays the same file through a ../ path, so nothing is copied into an
install. Writes <out>\play_through.json and prints one line per run.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

GAME_PROBE = kit_paths.REPO / "goldsrc-hooks" / "tools" / "game_probe.py"
LAUNCH_ARGS = "-dodstudio_start_behind +ds_capture_run_in_background 1 +ds_capture_stay_behind 1 +developer 1"
STAMP = re.compile(r"\[demo\s+(\d+(?:\.\d+)?)\]")
# The engine looks for the next demo when one ends: on a clean end that line is all it prints.
NEXT_DEMO = "No demos listed with startdemos"
END_LINE = re.compile(r"Host_EndGame[^\r\n]*|Host_Error[^\r\n]*|End of demo[^\r\n]*|" + NEXT_DEMO)
END_WAIT = r"Host_EndGame|Host_Error|End of demo|CRASH:|" + NEXT_DEMO
KNOWN_BAD = ["Fatal Error", "svc_bad", "without model", "aaatrigger"]
JUMP_FORWARD_S = 8.0
JUMP_BACK_S = 1.0
CREATE_NO_WINDOW = 0x08000000
LOCK_OWNER = "play-through"


# ── Plan (pure) ──────────────────────────────────────────────────────────────

def resolve_demo(arg, demo_dir):
    """The .dem file a command-line demo names: a path, or a name in `demo_dir`."""
    path = Path(arg)
    if path.suffix.lower() != ".dem":
        path = path.with_name(path.name + ".dem")
    return path if path.is_absolute() or path.parent != Path(".") else Path(demo_dir) / path


def demo_arg(demo_file, install, game="dod"):
    """What `playdemo` takes for `demo_file` in `install`: relative to its game
    folder, forward slashes, no `.dem`. A file on another drive can't be reached."""
    game_dir = Path(install) / game
    try:
        rel = os.path.relpath(Path(demo_file).with_suffix(""), game_dir)
    except ValueError:
        raise ValueError(f"{demo_file} is on another drive than {game_dir}; copy it into dod_addon") from None
    return rel.replace("\\", "/")


def probe_steps(play, demo, budget_s):
    """game_probe steps for one run. The demo is started by a step, not --demo,
    so `ds_capture_batch away` goes first and a ../ path keeps its quotes."""
    return [
        "wait 8",
        "cmd ds_capture_batch away",
        f'cmd {play} "{demo}"',
        "waitfor Playing.demo|Demo file completely loaded 180",
        "cmd host_framerate 0.05",
        f"waitfor {END_WAIT} {budget_s}",
        "cmd host_framerate 0",
        "wait 2",
    ]


def probe_command(run, args):
    """The game_probe.py command line for one planned run."""
    cmd = [sys.executable, "-I", str(GAME_PROBE), "--install", run["install"], "--out", str(run["out"]),
           "--launch-args", LAUNCH_ARGS]
    if args.dll:
        cmd += ["--dll", str(Path(args.dll).resolve())]
    for step in probe_steps(args.play, run["demo_arg"], args.budget):
        cmd += ["--step", step]
    return cmd


def plan_runs(demo_files, installs, out_root, install_dirs):
    """One run per (install, demo), installs in the outer loop so each install's runs sit together."""
    runs = []
    for install in installs:
        for demo in demo_files:
            runs.append({
                "install": install, "demo": demo.stem, "demo_file": str(demo),
                "demo_arg": demo_arg(demo, install_dirs[install]),
                "out": Path(out_root) / f"{install}-{demo.stem}",
            })
    return runs


# ── Reading a report (pure) ──────────────────────────────────────────────────

def clock_jumps(stamps):
    """Pairs of consecutive stamps where the demo clock jumps forward or steps back."""
    return [(a, b) for a, b in zip(stamps, stamps[1:]) if b - a > JUMP_FORWARD_S or b < a - JUMP_BACK_S]


def pick_end_line(ends):
    """The line that says why the demo ended: an error before the engine's routine next-demo line."""
    errors = [e.strip() for e in ends if e != NEXT_DEMO]
    if errors:
        return errors[0]
    return f"end of file ({NEXT_DEMO})" if ends else None


def summarize(report, bad=KNOWN_BAD):
    """What a game_probe report says about how far the demo played."""
    hook = report.get("hook_log", "")
    console = report.get("console_log", "")
    stamps = [float(s) for s in STAMP.findall(hook)]
    ends = END_LINE.findall(console) + END_LINE.findall(hook)
    both = console + "\n" + hook
    failed = [s for s in report.get("steps", []) if not s.get("ok")]
    return {
        "first_demo_s": stamps[0] if stamps else None,
        "last_demo_s": stamps[-1] if stamps else None,
        "jumps": clock_jumps(stamps)[:20],
        "end_line": pick_end_line(ends),
        "crashes": report.get("crashes", []),
        "bad_lines": [line.strip() for line in both.splitlines() if any(b in line for b in bad)][:20],
        "alive_at_end": report.get("alive_at_end"),
        "error": report.get("error"),
        "failed_steps": [f"{s['step']}" + (f" ({s['note']})" if s.get("note") else "") for s in failed],
    }


def table_line(run, summary):
    """One line per run for the terminal."""
    last = summary.get("last_demo_s")
    flags = []
    if summary.get("crashes"):
        flags.append("CRASH")
    if summary.get("bad_lines"):
        flags.append(f"{len(summary['bad_lines'])} bad line(s)")
    if summary.get("error"):
        flags.append(summary["error"])
    return (f"{run['install']:4} {run['demo']:40} last {'-' if last is None else f'{last:8.1f}'} s  "
            f"end: {summary.get('end_line') or '-'}" + (f"  [{', '.join(flags)}]" if flags else ""))


def new_files(before, after):
    return sorted(set(after) - set(before))


# ── Running ──────────────────────────────────────────────────────────────────

def bmps_in(folder):
    return {p.name for p in Path(folder).glob("*.bmp")}


def move_new_bmps(game_dir, before, dest):
    """Moves .bmp files a demo's recorded `snapshot` wrote into the game folder since `before`."""
    moved = new_files(before, bmps_in(game_dir))
    for name in moved:
        dest.mkdir(parents=True, exist_ok=True)
        shutil.move(str(Path(game_dir) / name), str(dest / name))
    return moved


def wait_for_no_game(timeout_s=120):
    import game_lock
    end = time.time() + timeout_s
    while game_lock.hl_running() and time.time() < end:
        time.sleep(2)


def run_one(run, args):
    """Lock, play, release, read. Returns the run's summary."""
    import game_lock
    code, message = game_lock.acquire(args.lock, LOCK_OWNER, args.lock_wait)
    print(f"  lock: {message}", flush=True)
    if code != game_lock.OK:
        return {"error": f"no game lock: {message}"}
    try:
        game_dir = kit_paths.install_dir(run["install"]) / "dod"
        before = bmps_in(game_dir)
        run["out"].mkdir(parents=True, exist_ok=True)
        with open(run["out"] / "probe.log", "wb") as log:
            subprocess.run(probe_command(run, args), stdout=log, stderr=subprocess.STDOUT,
                           stdin=subprocess.DEVNULL, creationflags=CREATE_NO_WINDOW)
        wait_for_no_game()
        moved = move_new_bmps(game_dir, before, run["out"] / "demo-snapshot-bmps")
    finally:
        _, released = game_lock.release(args.lock, LOCK_OWNER)
        print(f"  lock: {released}", flush=True)
    report_file = run["out"] / "report.json"
    if not report_file.is_file():
        return {"error": f"no report at {report_file} (see probe.log)"}
    summary = summarize(json.loads(report_file.read_text(encoding="utf-8", errors="replace")), args.bad)
    summary["moved_bmps"] = moved
    return summary


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("demos", nargs="+")
    p.add_argument("--install", choices=["pre", "post", "both"], default="both")
    p.add_argument("--play", choices=["playdemo", "viewdemo"], default="playdemo")
    p.add_argument("--demo-dir", type=Path, help="where demo names are looked up (default: PRE's dod folder)")
    p.add_argument("--dll", help="hook DLL (default: game_probe's: Studio's setting, then the repo build)")
    p.add_argument("--out", type=Path, help="output folder (default: DS_TEST_OUT or <main checkout>/local/play-through)")
    p.add_argument("--budget", type=int, default=1500, help="wall seconds to wait for the end of each demo")
    p.add_argument("--bad", action="append", default=list(KNOWN_BAD), help="another known-bad console line")
    p.add_argument("--lock", type=Path, help="game lock file (default: game_lock.py's)")
    p.add_argument("--lock-wait", type=float, default=10, help="minutes to wait for the game lock")
    p.add_argument("--dry-run", action="store_true", help="print the plan and the game_probe commands only")
    args = p.parse_args(argv)
    import game_lock
    args.lock = args.lock or game_lock.default_lock_file()
    installs = ["pre", "post"] if args.install == "both" else [args.install]
    install_dirs = {name: kit_paths.install_dir(name) for name in installs}
    demo_dir = args.demo_dir or kit_paths.install_dir("pre") / "dod"
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    base = args.out or Path(os.environ.get("DS_TEST_OUT") or kit_paths.main_checkout() / "local" / "play-through")
    out_root = base / stamp if not args.out else base
    demo_files = [resolve_demo(d, demo_dir) for d in args.demos]
    missing = [str(d) for d in demo_files if not d.is_file()]
    if missing:
        print("demo not found: " + ", ".join(missing))
        return 2
    try:
        runs = plan_runs(demo_files, installs, out_root, install_dirs)
    except ValueError as why:
        print(why)
        return 2
    if args.dry_run:
        for run in runs:
            print(f"{run['install']} {run['demo']} -> {run['out']}")
            print("   " + subprocess.list2cmdline(probe_command(run, args)))
        return 0
    results = []
    for run in runs:
        print(f"{run['install']} {run['demo']}: playing", flush=True)
        summary = run_one(run, args)
        results.append({**{k: str(v) for k, v in run.items()}, **summary})
        print(table_line(run, summary), flush=True)
        out_root.mkdir(parents=True, exist_ok=True)
        (out_root / "play_through.json").write_text(json.dumps(results, indent=1), encoding="utf-8")
    print(f"\n{len(results)} run(s); {out_root / 'play_through.json'}")
    for run, result in zip(runs, results):
        print(table_line(run, result))
    return 1 if any(r.get("crashes") or r.get("error") for r in results) else 0


if __name__ == "__main__":
    sys.exit(main())
