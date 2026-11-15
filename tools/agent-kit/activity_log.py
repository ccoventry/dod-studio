r"""Reads Studio's activity log (logs\activity_YYYYMMDD.md): filter it, and pair each job's
start and end lines with how long it took.

    activity_log.py [--date 2026-11-10|today|yesterday] [--tag capture --tag HLAE] [--grep REGEX]
                    [--since 21:00] [--last-session]
        The entries, filtered. A tag is the word in brackets after the time:
        [capture], [render], [HLAE], [scan], [toast error] ...
    activity_log.py --jobs [--tag capture]
        Every "<tag> started" line paired with that tag's next "... after <duration>"
        line: capture batches, scans, splits, with start, end and duration.
    activity_log.py --list
        The log files there are, newest first.

The logs folder is Studio's (%APPDATA%\dod-studio\logs; DS_STUDIO_APPDATA or
--logs for another, such as a throwaway Studio's live folder). Only read.
"""
import argparse
import re
import sys
from datetime import date, datetime, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

ENTRY = re.compile(r"^\* \[(\d\d:\d\d:\d\d)\] (?:\[([^\]]+)\] )?(.*)$")
SESSION = re.compile(r"^=+ New Session: (.*?) =+\s*$")
# "[capture] started", "[scan] started: 1 path(s)", "[render] queue_render_batch starting: ...";
# not "[HLAE] Running the batch ..., not starting one".
JOB_START = re.compile(r"^(started\b|\w+ starting\b)")
JOB_END = re.compile(r"\bafter\b")


# ── Parsing (pure) ───────────────────────────────────────────────────────────

def parse(lines):
    """Entries as dicts {time, tag, text, session}; continuation lines join the entry above."""
    entries, session = [], None
    for raw in lines:
        line = raw.rstrip("\r\n")
        if m := SESSION.match(line):
            session = m.group(1)
            continue
        if m := ENTRY.match(line):
            entries.append({"time": m.group(1), "tag": m.group(2), "text": m.group(3), "session": session})
        elif line.strip() and entries:
            entries[-1]["text"] += "\n" + line
    return entries


def keep(entry, tags=(), pattern=None, since=None):
    """Whether an entry passes the filters (tags match case-insensitively, by prefix:
    `toast` keeps `toast error`)."""
    if tags and not any((entry["tag"] or "").lower().startswith(t.lower()) for t in tags):
        return False
    if pattern and not re.search(pattern, entry["text"]):
        return False
    return not (since and entry["time"] < since)


def seconds_between(start, end):
    """Seconds from one HH:MM:SS to the next, across midnight when the end reads earlier."""
    fmt = "%H:%M:%S"
    delta = datetime.strptime(end, fmt) - datetime.strptime(start, fmt)
    return delta.total_seconds() % 86400


def jobs(entries):
    """Each tag's "started" entry paired with that tag's next entry that says "after";
    an unmatched start comes back with end None (still running, or the app closed)."""
    open_jobs, done = {}, []
    for entry in entries:
        tag, text = entry["tag"], entry["text"]
        if not tag:
            continue
        if JOB_START.match(text):
            if tag in open_jobs:
                done.append({"tag": tag, "start": open_jobs[tag], "end": None})
            open_jobs[tag] = entry
        elif tag in open_jobs and JOB_END.search(text):
            done.append({"tag": tag, "start": open_jobs.pop(tag), "end": entry})
    done += [{"tag": tag, "start": start, "end": None} for tag, start in open_jobs.items()]
    return sorted(done, key=lambda j: j["start"]["time"])


def log_name(day):
    return f"activity_{day:%Y%m%d}.md"


def parse_date(text, today=None):
    today = today or date.today()
    if text in (None, "today"):
        return today
    if text == "yesterday":
        return today - timedelta(days=1)
    return datetime.strptime(text.replace("-", ""), "%Y%m%d").date()


def format_entry(entry):
    tag = f"[{entry['tag']}] " if entry["tag"] else ""
    return f"{entry['time']} {tag}{entry['text']}"


def format_job(job):
    start, end = job["start"], job["end"]
    if end is None:
        return f"{start['time']} -> (no end line)  [{job['tag']}] {start['text'][:100]}"
    secs = seconds_between(start["time"], end["time"])
    return f"{start['time']} -> {end['time']} ({secs / 60:5.1f} min)  [{job['tag']}] {end['text'][:140]}"


# ── Command ──────────────────────────────────────────────────────────────────

def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--logs", type=Path, help="logs folder (default: Studio's)")
    p.add_argument("--date", help="YYYY-MM-DD, today (default) or yesterday")
    p.add_argument("--tag", action="append", default=[])
    p.add_argument("--grep")
    p.add_argument("--since", help="HH:MM or HH:MM:SS")
    p.add_argument("--last-session", action="store_true", help="only the last Studio session in the file")
    p.add_argument("--jobs", action="store_true")
    p.add_argument("--list", action="store_true")
    args = p.parse_args(argv)
    logs = args.logs or kit_paths.studio_appdata() / "logs"
    if args.list:
        for f in sorted(logs.glob("activity_*.md"), reverse=True):
            print(f"{f.name}  {f.stat().st_size:>9} bytes")
        return 0
    path = logs / log_name(parse_date(args.date))
    if not path.is_file():
        print(f"no activity log at {path}")
        return 1
    entries = parse(path.read_text(encoding="utf-8", errors="replace").splitlines())
    if args.last_session and entries:
        entries = [e for e in entries if e["session"] == entries[-1]["session"]]
    if args.jobs:
        for job in jobs(entries):
            if keep(job["start"], args.tag, args.grep, args.since):
                print(format_job(job))
        return 0
    for entry in entries:
        if keep(entry, args.tag, args.grep, args.since):
            print(format_entry(entry))
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main())
