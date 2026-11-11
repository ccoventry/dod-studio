"""Running `gh` and `git` for the kit's GitHub tools, and caching what they fetched.

Every call runs in the repo the kit lives in, so `gh` picks the right GitHub repo and
`{owner}/{repo}` in a `gh api` path fills itself in. A failing call raises
`ToolError` with the command and its stderr, so a tool never mistakes a failed
lookup for an empty answer (`gh` through the PowerShell tool used to fail silently
inside loops and print "none" for every issue).
"""
import json
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402


class ToolError(RuntimeError):
    pass


def run(cmd, cwd=None):
    """stdout of `cmd`, or ToolError naming the command and what it printed."""
    try:
        done = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                              cwd=str(cwd or kit_paths.REPO))
    except OSError as e:
        raise ToolError(f"{cmd[0]} could not start: {e}") from e
    if done.returncode != 0:
        raise ToolError(f"{' '.join(cmd)} failed ({done.returncode}): {done.stderr.strip()[:400]}")
    return done.stdout


def gh_json(args):
    """`gh <args>` parsed as JSON (the args must ask for JSON)."""
    return json.loads(run(["gh", *args]) or "null")


def git(args):
    return run(["git", *args])


def cached(name, max_age_secs, fetch, refresh=False, cache_dir=None):
    """`fetch()`'s result, kept as JSON in the agents cache for `max_age_secs`
    (None: for ever, for keys that name an immutable thing such as a commit)."""
    path = Path(cache_dir or kit_paths.agents_cache()) / name
    if not refresh and path.exists():
        fresh = max_age_secs is None or time.time() - path.stat().st_mtime < max_age_secs
        if fresh:
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                pass
    value = fetch()
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_name(path.name + ".part")
    partial.write_text(json.dumps(value), encoding="utf-8")
    partial.replace(path)
    return value
