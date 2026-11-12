r"""One game at a time for agents that share the PC: an exclusive lock plus the safety checks.

    game_lock.py acquire <owner> [--wait MINUTES]   exit 0 = you hold it; go
    game_lock.py release <owner> [--force]
    game_lock.py status

`acquire` refuses (exit 2, REFUSED) unless Steam is signed into the one account
in-game tests may use, read from the same local file game_probe.py reads
(%APPDATA%\dod-studio\game_probe.json, kept out of the repo). It is BUSY (exit 1)
while another owner holds the lock or any hl.exe runs, including one the caller
started itself: a helper that re-acquires while its own game still runs is how
two games overlapped on 11-03. `--wait` retries every 30 s for that long.

`release` refuses while hl.exe runs (release only after your game has exited),
unless `--force`, and never releases another owner's lock.

The lock file is plain text holding the owner's name. Default:
%LOCALAPPDATA%\dod-studio-agents\game.lock, outside every checkout so all
worktrees share it; `--lock FILE` or env DS_GAME_LOCK points elsewhere (point it
at an older lock file to share the game with agents still using that one).
Run `acquire` again right before every launch: the account can change.
"""
import argparse
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

OK, BUSY, REFUSED = 0, 1, 2
RETRY_S = 30


def default_lock_file(env=os.environ):
    """DS_GAME_LOCK, else a file under %LOCALAPPDATA% that every worktree shares."""
    if env.get("DS_GAME_LOCK"):
        return Path(env["DS_GAME_LOCK"])
    base = env.get("LOCALAPPDATA") or str(Path.home())
    return Path(base) / "dod-studio-agents" / "game.lock"


# ── Decisions (pure) ─────────────────────────────────────────────────────────

def decide_acquire(holder, owner, hl_running, account_problem):
    """(exit code, message) for `acquire`. `account_problem` is None when the
    allowed account is signed in, else why not."""
    if account_problem:
        return REFUSED, f"REFUSED: {account_problem}. Do not launch; report it."
    if holder and holder != owner:
        return BUSY, f"BUSY: held by {holder}. Wait and retry."
    if hl_running:
        whose = "your own earlier game" if holder == owner else "a game nobody here holds (maybe the user's)"
        return BUSY, f"BUSY: hl.exe is running ({whose}). Wait for it to exit."
    return OK, f"OK: {owner} holds the game lock"


def decide_release(holder, owner, hl_running, force):
    """(exit code, message) for `release`; code OK means delete the lock file."""
    if holder != owner:
        return BUSY, f"not yours (holder={holder}); nothing released"
    if hl_running and not force:
        return BUSY, "hl.exe is still running: release after your game exits (or --force if it is not yours)"
    return OK, "released"


# ── The machine ──────────────────────────────────────────────────────────────

def hl_running():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq hl.exe", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    return '"hl.exe"' in out.lower()


def account_problem():
    """None when Steam is signed into the allowed test account, else why not (game_probe's check)."""
    sys.path.insert(0, str(kit_paths.REPO / "goldsrc-hooks" / "tools"))
    import game_probe
    try:
        game_probe.check_account()
    except game_probe.Refused as why:
        return str(why)
    return None


def read_holder(lock):
    try:
        return lock.read_text(encoding="utf-8").strip().splitlines()[0] or None
    except (OSError, IndexError):
        return None


def take_lock(lock, owner):
    """Creates the lock file exclusively; False when someone else made it first."""
    lock.parent.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError:
        return False
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(owner)
    return True


def try_acquire(lock, owner):
    holder = read_holder(lock)
    code, message = decide_acquire(holder, owner, hl_running(), account_problem())
    if code == OK and holder is None and not take_lock(lock, owner):
        return BUSY, f"BUSY: held by {read_holder(lock)}"
    if code == OK:
        message += f" ({time.strftime('%H:%M:%S')}, {lock})"
    return code, message


def acquire(lock, owner, wait_min=0):
    """Takes the lock for `owner`, retrying for `wait_min` minutes while BUSY. Returns (code, message)."""
    end = time.time() + wait_min * 60
    while True:
        code, message = try_acquire(lock, owner)
        if code != BUSY or time.time() + RETRY_S > end:
            return code, message
        time.sleep(RETRY_S)


def release(lock, owner, force=False):
    code, message = decide_release(read_holder(lock), owner, hl_running(), force)
    if code == OK:
        lock.unlink(missing_ok=True)
    return code, message


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--lock", type=Path, default=None, help="lock file (default: DS_GAME_LOCK or %%LOCALAPPDATA%%)")
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("acquire")
    s.add_argument("owner")
    s.add_argument("--wait", type=float, default=0, metavar="MINUTES")
    s = sub.add_parser("release")
    s.add_argument("owner")
    s.add_argument("--force", action="store_true")
    sub.add_parser("status")
    args = p.parse_args(argv)
    lock = args.lock or default_lock_file()
    if args.cmd == "status":
        problem = account_problem()
        print(f"holder={read_holder(lock)} hl_running={hl_running()} "
              f"account={'ok' if problem is None else problem} lock={lock}")
        return OK
    if args.cmd == "acquire":
        code, message = acquire(lock, args.owner, args.wait)
    else:
        code, message = release(lock, args.owner, args.force)
    print(message)
    return code


if __name__ == "__main__":
    sys.exit(main())
