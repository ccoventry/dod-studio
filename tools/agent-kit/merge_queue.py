"""The merge train's queue and its one-train-at-a-time lock (used by merge_train.py).

Both live in `<git common dir>/merge-train/`: inside `.git`, so no worktree tracks them,
every worktree of the repo shares them (they all report the same common dir), and a
`git clean` can't remove them. Files there:
  queue.json  {pending, done, skipped, current, stopped}: the train's state;
  lock.json   {pid, started}: the running train;
  mutex       held for a moment around every read-modify-write of the two above.

Every change takes the mutex, then writes a temp file and `os.replace`s it, so a second
process (`merge_train.py add`) never reads half a file and never loses the train's update.
A lock whose process is gone is stale and is taken over by the next train.
"""
import contextlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402
from gh_util import ToolError  # noqa: E402

MUTEX_WAIT_SECS = 10
# A mutex file older than this was left by a process that died inside it.
MUTEX_STALE_SECS = 30
REPLACE_TRIES = 20
STILL_ACTIVE = 259


def default_dir():
    """`<git common dir>/merge-train`: the same folder from the main checkout and any worktree."""
    try:
        done = subprocess.run(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
                              capture_output=True, text=True, cwd=str(kit_paths.REPO))
        common = done.stdout.strip() if done.returncode == 0 else ""
    except OSError:
        common = ""
    return Path(common or kit_paths.REPO / ".git") / "merge-train"


def _kernel32():
    import ctypes
    from ctypes import wintypes
    k = ctypes.WinDLL("kernel32", use_last_error=True)
    k.OpenProcess.restype, k.OpenProcess.argtypes = wintypes.HANDLE, [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    k.GetExitCodeProcess.restype = wintypes.BOOL
    k.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    k.GetProcessTimes.restype = wintypes.BOOL
    k.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    k.CloseHandle.restype, k.CloseHandle.argtypes = wintypes.BOOL, [wintypes.HANDLE]
    return k, ctypes, wintypes


def _open_process(pid):
    k, ctypes, wintypes = _kernel32()
    handle = k.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    return (k, ctypes, wintypes, handle) if handle else None


def pid_alive(pid):
    """Whether a process with this PID exists. (`os.kill(pid, 0)` would terminate it on Windows.)"""
    if os.name == "nt":
        opened = _open_process(pid)
        if not opened:
            return False
        k, ctypes, wintypes, handle = opened
        try:
            code = wintypes.DWORD()
            return bool(k.GetExitCodeProcess(handle, ctypes.byref(code))) and code.value == STILL_ACTIVE
        finally:
            k.CloseHandle(handle)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def process_start(pid):
    """When the process was created (Windows FILETIME ticks), or None when unknown. A PID
    is reused after a process dies; the same PID with another start time is another process."""
    if os.name != "nt":
        return None
    opened = _open_process(pid)
    if not opened:
        return None
    k, ctypes, wintypes, handle = opened
    try:
        created, ex, kern, user = (wintypes.FILETIME() for _ in range(4))
        if not k.GetProcessTimes(handle, *(ctypes.byref(t) for t in (created, ex, kern, user))):
            return None
        return (created.dwHighDateTime << 32) | created.dwLowDateTime
    finally:
        k.CloseHandle(handle)


def empty_state():
    return {"pending": [], "done": [], "skipped": [], "current": None, "stopped": None}


class MemoryQueue:
    """A queue that lives in this process only: `--dry-run`, and the tests of the train."""

    def __init__(self, prs):
        self.state = empty_state()
        self.state["pending"] = list(prs)

    def take(self):
        pending = self.state["pending"]
        self.state["current"] = pending[0] if pending else None
        return self.state["current"]

    def waiting(self):
        held = {self.state["current"], (self.state["stopped"] or {}).get("pr")}
        return [n for n in self.state["pending"] if n not in held]

    def merged(self, pr):
        self._finish(pr, "done")

    def skipped(self, pr):
        self._finish(pr, "skipped")

    def stopped(self, pr, reason):
        self.state["current"] = None
        self.state["stopped"] = {"pr": pr, "reason": reason}

    def _finish(self, pr, bucket):
        if pr in self.state["pending"]:
            self.state["pending"].remove(pr)
        self.state[bucket].append(pr)
        self.state["current"] = None


class QueueStore(MemoryQueue):
    """The queue and lock on disk. Same take/merged/skipped/stopped calls as MemoryQueue,
    each one a locked read-modify-write of the file."""

    def __init__(self, folder, pid=os.getpid, alive=pid_alive, clock=time.time, sleep=time.sleep,
                 start=process_start):
        self.dir, self.pid, self.alive, self.clock, self.sleep = Path(folder), pid, alive, clock, sleep
        self.start = start
        self.queue_file, self.lock_file, self.mutex_file = (self.dir / n for n in ("queue.json", "lock.json", "mutex"))

    # -- files ---------------------------------------------------------------

    @contextlib.contextmanager
    def mutex(self):
        self.dir.mkdir(parents=True, exist_ok=True)
        deadline = self.clock() + MUTEX_WAIT_SECS
        while True:
            try:
                os.close(os.open(self.mutex_file, os.O_CREAT | os.O_EXCL | os.O_WRONLY))
                break
            # PermissionError: Windows says so for a file another process just deleted
            # (delete pending), which is "held" too.
            except (FileExistsError, PermissionError):
                self._clear_stale_mutex()
                if self.clock() >= deadline:
                    raise ToolError(f"the queue mutex {self.mutex_file} stayed held for {MUTEX_WAIT_SECS} s")
                self.sleep(0.05)
        try:
            yield
        finally:
            with contextlib.suppress(OSError):
                self.mutex_file.unlink()

    def _clear_stale_mutex(self):
        """Remove a mutex left by a process that died inside it. The file is renamed away first:
        of two waiters that both see it stale only one rename succeeds, so they can't both
        delete it and then both enter. A file found fresh after the rename (a new holder got
        in between) is put back."""
        mine = self.mutex_file.with_name(f"mutex.stale.{self.pid()}")
        try:
            if self.clock() - self.mutex_file.stat().st_mtime <= MUTEX_STALE_SECS:
                return
            os.replace(self.mutex_file, mine)
            if self.clock() - mine.stat().st_mtime <= MUTEX_STALE_SECS:
                with contextlib.suppress(OSError):
                    os.link(mine, self.mutex_file)
            mine.unlink()
        except OSError:
            with contextlib.suppress(OSError):
                mine.unlink()

    def _read_json(self, path, default):
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return default
        except ValueError as e:
            raise ToolError(f"{path} is not valid JSON ({e}); fix or delete it") from e

    def _write_json(self, path, data):
        tmp = path.with_name(f"{path.name}.{self.pid()}.tmp")
        tmp.write_text(json.dumps(data, indent=2), encoding="utf-8")
        for attempt in range(REPLACE_TRIES):
            try:
                os.replace(tmp, path)
                return
            except PermissionError:  # a reader has it open for a moment (Windows)
                if attempt == REPLACE_TRIES - 1:
                    raise
                self.sleep(0.05)

    def _load(self):
        return {**empty_state(), **self._read_json(self.queue_file, {})}

    def _save(self):
        self._write_json(self.queue_file, self.state)

    # -- the lock ------------------------------------------------------------

    def _holder(self):
        """The live train's PID, or None (no lock, or a stale one from a dead process)."""
        lock = self._read_json(self.lock_file, None)
        if not (lock and lock.get("pid") and self.alive(lock["pid"])):
            return None
        started = lock.get("start")
        if started is not None and self.start(lock["pid"]) not in (None, started):
            return None  # the PID was reused by another process: the train died
        return lock["pid"]

    def holder(self):
        with self.mutex():
            return self._holder()

    def _release_locked(self):
        lock = self._read_json(self.lock_file, None)
        if lock and lock.get("pid") == self.pid():
            with contextlib.suppress(OSError):
                self.lock_file.unlink()

    def release(self):
        with self.mutex():
            self._release_locked()

    # -- what the commands do ------------------------------------------------

    def acquire(self, prs):
        """Queue `prs` and become the running train. If another train is alive they are queued
        for it instead. Returns (owned, added, ignored)."""
        with self.mutex():
            holder = self._holder()
            owned = holder is None or holder == self.pid()
            self.state = self._load()
            if owned:
                if not self.state["pending"]:
                    self.state = {**empty_state()}
                self.state["current"] = None
                self.state["stopped"] = None
                self._write_json(self.lock_file, {"pid": self.pid(), "start": self.start(self.pid()), "started": self.clock()})
            added, ignored = self._append(prs)
            self._save()
        return owned, added, ignored

    def add(self, prs):
        """Append to the queue. Returns (added, ignored, pid of the running train or None)."""
        with self.mutex():
            self.state = self._load()
            added, ignored = self._append(prs)
            self._save()
            return added, ignored, self._holder()

    def _append(self, prs):
        added, ignored = [], []
        for n in prs:
            known = self.state["pending"] + self.state["done"]
            if n in known or n in added:
                ignored.append(n)
            else:
                added.append(n)
        self.state["pending"] += added
        return added, ignored

    def remove(self, pr):
        """Drop a queued PR. Returns a line saying what happened."""
        with self.mutex():
            self.state = self._load()
            pending = self.state["pending"]
            if pr not in pending:
                return f"#{pr} is not in the queue"
            if pr == self.state["current"] and self._holder():
                return f"#{pr} is being merged right now; it can't be removed"
            pending.remove(pr)
            if (self.state["stopped"] or {}).get("pr") == pr:
                self.state["stopped"] = None
            self._save()
            return f"#{pr} removed from the queue"

    def peek(self):
        """The saved state, read without the mutex and without creating anything (`--dry-run`)."""
        return {**empty_state(), **self._read_json(self.queue_file, {})}

    def snapshot(self):
        """(state, the running train's PID or None), for `status`."""
        with self.mutex():
            return self._load(), self._holder()

    def take(self):
        """The next PR to merge, re-reading the file so PRs added meanwhile are seen. When the
        queue is empty the lock is released in the same step, so a PR added right after this
        starts a new train instead of sitting in a queue nobody reads."""
        with self.mutex():
            self.state = self._load()
            pending = self.state["pending"]
            self.state["current"] = pending[0] if pending else None
            self.state["stopped"] = None
            self._save()
            if not pending:
                self._release_locked()
            return self.state["current"]

    def waiting(self):
        with self.mutex():
            self.state = self._load()
            return super().waiting()

    def merged(self, pr):
        self._update(lambda: super(QueueStore, self).merged(pr))

    def skipped(self, pr):
        self._update(lambda: super(QueueStore, self).skipped(pr))

    def stopped(self, pr, reason):
        self._update(lambda: super(QueueStore, self).stopped(pr, reason))

    def _update(self, change):
        with self.mutex():
            self.state = self._load()
            change()
            self._save()


def status_lines(state, holder):
    """`status` output for a loaded state and the live train's PID (or None)."""
    def nums(items):
        return ", ".join(f"#{n}" for n in items) or "none"

    stopped = (state["stopped"] or {}).get("pr")
    # With no live train, `current` is a PR whose train died mid-way; it is retried first.
    interrupted = state["current"] if not holder else None
    held = {state["current"], stopped}
    lines = [f"Train: running (pid {holder})" if holder else "Train: not running"]
    if holder and state["current"]:
        lines.append(f"Merging now: #{state['current']}")
    if interrupted:
        lines.append(f"Interrupted at #{interrupted} (the train died mid-PR); it goes first on the next run.")
    lines.append(f"Queued: {nums([n for n in state['pending'] if n not in held])}")
    lines.append(f"Merged: {nums(state['done'])}")
    if state["skipped"]:
        lines.append(f"Skipped (already merged or closed): {nums(state['skipped'])}")
    if state["stopped"]:
        s = state["stopped"]
        lines.append(f"Stopped at #{s['pr']}: {s['reason']}")
        lines.append("Run `merge_train.py` with no PR numbers to continue from there.")
    return lines
