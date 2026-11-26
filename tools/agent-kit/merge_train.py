"""Merge open PRs into dev one at a time, in the order given, each only once it is green.

    python -I tools/agent-kit/merge_train.py 1418 1416 1420
    python -I tools/agent-kit/merge_train.py 1418 1416 --dry-run
    python -I tools/agent-kit/merge_train.py add 1449 1452      # join the running train's queue
    python -I tools/agent-kit/merge_train.py status             # current PR, queue, done, why it stopped
    python -I tools/agent-kit/merge_train.py remove 1452        # drop a queued PR
    python -I tools/agent-kit/merge_train.py                    # no numbers: continue a stopped queue

dev's ruleset requires the branch to be up to date (CLAUDE.md, "Merging into dev"), so
every merge puts the rest behind. For each PR in turn this:
  1. checks it is open, not a draft, targets dev and has no conflicts;
  2. runs `gh pr update-branch` when it is behind dev;
  3. polls `gh pr checks` until every check the dev ruleset requires is green;
  4. merges with `gh pr merge --merge --match-head-commit <the commit that was checked>`;
  5. confirms GitHub shows it MERGED, then goes on to the next.
It stops at the first red check, conflict, timeout or failed step, names the PR and the
check, and touches nothing after it. It never passes `--admin` or `--auto` and never
pushes. The queue and a one-train lock live in the repo's `.git/merge-train/` (merge_queue.py):
the train re-reads the queue after every PR, so `add` can feed it while it runs, and a second
`merge_train.py N` while one runs joins its queue instead of starting a parallel train. When the
train stops, the queue is kept; `status` says why. The required check names come from the ruleset itself
(`repos/{owner}/{repo}/rules/branches/dev`), so a new required check needs no change here.
"""
import argparse
import json
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402
from gh_util import ToolError  # noqa: E402
import merge_queue  # noqa: E402

BASE = "dev"
POLL_INTERVAL_SECS = 30
CHECKS_TIMEOUT_SECS = 45 * 60
# How long to wait for GitHub to show a new head commit after `update-branch`, and to
# show MERGED after `gh pr merge`.
HEAD_MOVE_TIMEOUT_SECS = 120
MERGED_TIMEOUT_SECS = 60
SHORT_POLL_SECS = 5
# A stacked PR's base branch is deleted when its parent merges and GitHub then retargets
# the PR to dev; that takes a few seconds, so the train waits this long before it stops.
RETARGET_TIMEOUT_SECS = 60
# What gh/GitHub say when update-branch cannot merge dev into the PR (a 502 or a typo must not match).
CONFLICT_PHRASES = ("due to conflicts", "merge conflict between base and head")
# A PR whose base moves while its checks run is updated again, at most this often.
MAX_UPDATES_PER_PR = 3
# What `gh pr checks` prints before the new head commit has any check runs; it means
# "not started yet", not a failure (it comes right after every update-branch).
NO_CHECKS_MARKER = "no checks reported"
# `gh pr checks --json bucket`: GitHub counts a skipped required check as passed.
GREEN_BUCKETS = {"pass", "skipping"}
RED_BUCKETS = {"fail", "cancel"}

PASS, PENDING, FAIL = "pass", "pending", "fail"


# --- Decisions (pure; the tests drive these with faked `gh` output) ---------------

def parse_checks(stdout, stderr):
    """`gh pr checks --json name,bucket` as {name: bucket}. An empty dict when gh says no
    checks were reported yet. Raises ToolError for any other output it can't read."""
    if NO_CHECKS_MARKER in (stderr or "").lower() or NO_CHECKS_MARKER in (stdout or "").lower():
        return {}
    try:
        rows = json.loads(stdout or "null")
    except ValueError as e:
        raise ToolError(f"gh pr checks printed something that isn't JSON: {(stderr or stdout)[:300]}") from e
    if not isinstance(rows, list):
        raise ToolError(f"gh pr checks printed no check list: {(stderr or stdout)[:300]}")
    checks = {}
    for row in rows:
        name, bucket = row.get("name"), (row.get("bucket") or "pending").lower()
        # The same name twice (a re-run beside the old run): the worse result wins, so
        # the train never merges past a red run it can still see.
        if checks.get(name) in RED_BUCKETS or (checks.get(name) == "pending" and bucket not in RED_BUCKETS):
            continue
        checks[name] = bucket
    return checks


def checks_verdict(required, checks):
    """(PASS | PENDING | FAIL, the check names behind that verdict).

    FAIL as soon as one required check is red; PASS only when every required check is
    green; otherwise PENDING with the required checks still missing or running."""
    red = [name for name in required if checks.get(name) in RED_BUCKETS]
    if red:
        return FAIL, red
    waiting = [name for name in required if checks.get(name) not in GREEN_BUCKETS]
    if waiting:
        return PENDING, waiting
    return PASS, []


def pr_problem(view):
    """Why the train can't take this PR (from `gh pr view --json`), or None."""
    if view["state"] != "OPEN":
        return f"is {view['state'].lower()}, not open"
    if view["isDraft"]:
        return "is a draft"
    if view["baseRefName"] != BASE:
        return f"targets {view['baseRefName']}, not {BASE}"
    if view.get("mergeable") == "CONFLICTING":
        return f"has conflicts with {BASE}"
    return None


# --- gh ------------------------------------------------------------------------

class Gh:
    """Every `gh` call the train makes, so the tests can swap in a fake."""

    def _run(self, args):
        try:
            done = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8",
                                  errors="replace", cwd=str(kit_paths.REPO))
        except OSError as e:
            raise ToolError(f"gh could not start: {e}") from e
        return done.returncode, done.stdout, done.stderr

    def _ok(self, args):
        code, out, err = self._run(args)
        if code != 0:
            raise ToolError(f"gh {' '.join(args)} failed ({code}): {err.strip()[:400]}")
        return out

    def required_checks(self):
        rules = json.loads(self._ok(["api", f"repos/{{owner}}/{{repo}}/rules/branches/{BASE}"]) or "[]")
        names = [c["context"] for r in rules if r.get("type") == "required_status_checks"
                 for c in r["parameters"]["required_status_checks"]]
        if not names:
            raise ToolError(f"the {BASE} ruleset lists no required checks; refusing to merge on nothing")
        return names

    def view(self, pr):
        return json.loads(self._ok(["pr", "view", str(pr), "--json",
                                    "number,title,state,isDraft,baseRefName,headRefName,mergeable,headRefOid,autoMergeRequest"]))

    def behind_by(self, head_sha):
        out = self._ok(["api", f"repos/{{owner}}/{{repo}}/compare/{BASE}...{head_sha}", "--jq", ".behind_by"])
        return int(out.strip() or 0)

    def checks(self, pr):
        # Exit codes: 0 all passed, 8 some pending, 1 some failed or none reported. The
        # JSON says which, so the code is not trusted on its own.
        _code, out, err = self._run(["pr", "checks", str(pr), "--json", "name,bucket"])
        return parse_checks(out, err)

    def update_branch(self, pr):
        self._ok(["pr", "update-branch", str(pr)])

    def merge(self, pr, head_sha):
        self._ok(["pr", "merge", str(pr), "--merge", "--match-head-commit", head_sha])

    def disable_auto(self, pr):
        self._ok(["pr", "merge", str(pr), "--disable-auto"])


# --- The train -------------------------------------------------------------------

@dataclass
class Options:
    interval_secs: float = POLL_INTERVAL_SECS
    timeout_secs: float = CHECKS_TIMEOUT_SECS
    dry_run: bool = False


@dataclass
class Result:
    merged: list = field(default_factory=list)
    stopped_at: int = None
    reason: str = ""
    untouched: list = field(default_factory=list)
    skipped: list = field(default_factory=list)


class Skip(Exception):
    """Nothing to do for this PR (already merged or closed); the message says why."""


class Stop(Exception):
    """The train stops at this PR; the message says why."""


class Train:
    def __init__(self, gh, options, clock=time.monotonic, sleep=time.sleep, out=print):
        self.gh, self.opt, self.clock, self.sleep, self.out = gh, options, clock, sleep, out
        self.merged_heads = set()  # head branches of the PRs this run merged

    def log(self, pr, text):
        self.out(f"[{datetime.now():%H:%M:%S}] #{pr} {text}")

    def run(self, queue):
        """Rides every PR the queue hands out until it is empty or one stops the train.
        `queue` is a list of PR numbers or a merge_queue queue (re-read after each PR)."""
        if not hasattr(queue, "take"):
            queue = merge_queue.MemoryQueue(queue)
        result = Result()
        required = self.gh.required_checks()
        self.out(f"[{datetime.now():%H:%M:%S}] required checks on {BASE}: {', '.join(required)}")
        while (pr := queue.take()) is not None:
            try:
                self.ride(pr, required)
                queue.merged(pr)
                result.merged.append(pr)
            except Skip as e:
                self.log(pr, f"skipped: {e}")
                queue.skipped(pr)
                result.skipped.append(pr)
            except (Stop, ToolError) as e:
                queue.stopped(pr, str(e))
                result.stopped_at, result.reason, result.untouched = pr, str(e), queue.waiting()
                self.log(pr, f"STOP: {e}")
                break
        return result

    def ride(self, pr, required):
        view = self.gh.view(pr)
        stacked = self.is_stacked_on_merged(view)
        if stacked and self.opt.dry_run:
            # Its base is the only thing a retarget fixes; draft and conflicts are not.
            problem = pr_problem({**view, "baseRefName": BASE})
            if problem:
                raise Stop(problem)
            self.log(pr, f"{view['title']}: targets {view['baseRefName']}; would wait for GitHub to retarget, then merge")
            self.merged_heads.add(view["headRefName"])
            return
        if stacked:
            view = self.wait_for_retarget(pr, view)
        self.log(pr, f"{view['title']} (head {view['headRefOid'][:8]})")
        if view["state"] in ("MERGED", "CLOSED"):
            if view["state"] == "MERGED":
                # A child of a parent merged in an earlier run still gets retargeted.
                self.merged_heads.add(view["headRefName"])
            raise Skip(f"already {view['state'].lower()}")
        problem = pr_problem(view)
        if problem:
            raise Stop(problem)
        if self.opt.dry_run:
            self.dry_run(pr, view, required)
            self.merged_heads.add(view["headRefName"])
            return
        head = view["headRefOid"]
        for _update in range(MAX_UPDATES_PER_PR + 1):
            head = self.bring_up_to_date(pr, head)
            self.wait_for_green(pr, required)
            if self.gh.behind_by(head) == 0:
                break
            self.log(pr, f"{BASE} moved while the checks ran; updating again")
        else:
            raise Stop(f"{BASE} kept moving; updated {MAX_UPDATES_PER_PR} times without a green, current run")
        self.log(pr, f"merging (--merge, head {head[:8]})")
        self.gh.merge(pr, head)
        self.confirm_merged(pr)
        self.merged_heads.add(view["headRefName"])

    def is_stacked_on_merged(self, view):
        """True for an open PR whose base is the head branch of a PR this run merged: GitHub
        retargets it to dev a few seconds later. Any other base (a branch nobody here merged,
        main) is a real mistake that pr_problem reports at once."""
        return view["state"] == "OPEN" and view["baseRefName"] in self.merged_heads

    def wait_for_retarget(self, pr, view):
        """The PR's view once its base is dev, polling while GitHub retargets it."""
        self.log(pr, f"targets {view['baseRefName']}, not {BASE}; waiting for GitHub to retarget it")
        deadline = self.clock() + RETARGET_TIMEOUT_SECS
        while self.clock() < deadline:
            self.sleep(SHORT_POLL_SECS)
            view = self.gh.view(pr)
            if view["state"] != "OPEN" or view["baseRefName"] == BASE:
                break
        return view

    def dry_run(self, pr, view, required):
        behind = self.gh.behind_by(view["headRefOid"])
        if behind:
            self.log(pr, f"would update-branch ({behind} commits behind {BASE}), wait for checks, then merge")
            return
        verdict, names = checks_verdict(required, self.gh.checks(pr))
        if verdict == FAIL:
            raise Stop(f"red required check: {', '.join(names)}")
        wait = "merge now" if verdict == PASS else f"wait for {', '.join(names)}, then merge"
        self.log(pr, f"up to date with {BASE}; would {wait}")

    def bring_up_to_date(self, pr, head):
        """The PR's head once it contains dev: unchanged, or the commit update-branch made."""
        behind = self.gh.behind_by(head)
        if behind == 0:
            self.log(pr, f"up to date with {BASE}")
            return head
        self.log(pr, f"{behind} commits behind {BASE}; gh pr update-branch")
        try:
            self.gh.update_branch(pr)
        except ToolError as e:
            if any(phrase in str(e).lower() for phrase in CONFLICT_PHRASES):
                raise Stop(f"has conflicts with {BASE}: resolve and rerun ({e})") from e
            raise
        deadline = self.clock() + HEAD_MOVE_TIMEOUT_SECS
        while self.clock() < deadline:
            view = self.gh.view(pr)
            if view["headRefOid"] != head:
                self.log(pr, f"updated, new head {view['headRefOid'][:8]}")
                return view["headRefOid"]
            self.sleep(SHORT_POLL_SECS)
        raise Stop(f"update-branch ran but the head did not move within {HEAD_MOVE_TIMEOUT_SECS} s")

    def wait_for_green(self, pr, required):
        deadline = self.clock() + self.opt.timeout_secs
        last = None
        while True:
            checks = self.gh.checks(pr)
            verdict, names = checks_verdict(required, checks)
            if verdict == FAIL:
                raise Stop(f"red required check: {', '.join(names)}")
            if verdict == PASS:
                self.log(pr, f"all {len(required)} required checks green")
                return
            line = ("no checks reported yet (normal right after an update)" if not checks
                    else f"{len(required) - len(names)}/{len(required)} green, waiting on {', '.join(names)}")
            if line != last:
                self.log(pr, line)
                last = line
            if self.clock() >= deadline:
                raise Stop(f"timed out after {self.opt.timeout_secs / 60:.0f} min waiting on {', '.join(names)}")
            self.sleep(self.opt.interval_secs)

    def confirm_merged(self, pr):
        deadline = self.clock() + MERGED_TIMEOUT_SECS
        while True:
            view = self.gh.view(pr)
            if view["state"] == "MERGED":
                self.log(pr, "MERGED")
                return
            if view.get("autoMergeRequest"):
                # gh turns a refused merge into auto-merge on some setups; the train never
                # leaves one behind.
                self.gh.disable_auto(pr)
                raise Stop("GitHub queued an auto-merge instead of merging; auto-merge turned back off")
            if self.clock() >= deadline:
                raise Stop(f"gh pr merge returned but the PR is still {view['state']} after {MERGED_TIMEOUT_SECS} s")
            self.sleep(SHORT_POLL_SECS)


def summary(prs, result, dry_run=False):
    lines = [("Would merge" if dry_run else "Merged") + f" ({len(result.merged)}/{len(prs)}): " + (", ".join(f"#{n}" for n in result.merged) or "none")]
    if result.skipped:
        lines.append("Skipped (already merged or closed): " + ", ".join(f"#{n}" for n in result.skipped))
    if result.stopped_at is not None:
        lines.append(f"Stopped at #{result.stopped_at}: {result.reason}")
        if result.untouched:
            lines.append("Not touched: " + ", ".join(f"#{n}" for n in result.untouched))
        if not dry_run:
            lines.append("The queue is kept: run merge_train.py with no PR numbers to continue; `status` shows it.")
    return lines


def everything(result):
    """Every PR the run saw, for the summary's count."""
    stopped = [] if result.stopped_at is None else [result.stopped_at]
    return result.merged + result.skipped + stopped + result.untouched


def listed(numbers):
    return ", ".join(f"#{n}" for n in numbers)


def queue_command(argv, store):
    """`add`, `remove` and `status`: they only touch the queue files, never GitHub."""
    ap = argparse.ArgumentParser(prog="merge_train.py", description="Manage the running merge train's queue")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("add", help="queue PRs; a running train picks them up after its current PR").add_argument(
        "prs", nargs="+", type=int)
    sub.add_parser("remove", help="drop a queued PR").add_argument("pr", type=int)
    sub.add_parser("status", help="the running train's current PR, queue, what is done and why it stopped")
    args = ap.parse_args(argv)
    if args.cmd == "add":
        added, ignored, holder = store.add(args.prs)
        if added:
            print("Queued: " + listed(added))
        if ignored:
            print("Ignored (already queued or merged): " + listed(ignored))
        print(f"The train (pid {holder}) picks them up after its current PR." if holder
              else "No train is running, so nothing started. Run `merge_train.py` with no PR numbers to start it.")
    elif args.cmd == "remove":
        print(store.remove(args.pr))
    else:
        for line in merge_queue.status_lines(*store.snapshot()):
            print(line)
    return 0


def main(argv=None, store=None, gh=None):
    argv = sys.argv[1:] if argv is None else argv
    store = store or merge_queue.QueueStore(merge_queue.default_dir())
    if argv and argv[0] in ("add", "remove", "status"):
        return queue_command(argv, store)
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("prs", nargs="*", type=int, help="PR numbers, merged in this order (none: continue the queue)")
    ap.add_argument("--dry-run", action="store_true", help="show what it would do; change nothing")
    ap.add_argument("--interval", type=float, default=POLL_INTERVAL_SECS, help="seconds between check polls")
    ap.add_argument("--timeout", type=float, default=CHECKS_TIMEOUT_SECS / 60,
                    help="minutes to wait for one PR's checks")
    args = ap.parse_args(argv)
    options = Options(interval_secs=args.interval, timeout_secs=args.timeout * 60, dry_run=args.dry_run)
    owned = False
    try:
        if args.dry_run:
            # Nothing is written: the PRs given, or else the saved queue, run through a copy.
            queue = merge_queue.MemoryQueue(args.prs or store.peek()["pending"])
        else:
            owned, added, ignored = store.acquire(args.prs)
            if ignored:
                print("Ignored (already queued or merged): " + listed(ignored))
            if not owned:
                print("A train is already running; " + ("queued " + listed(added) if added else "nothing new to queue")
                      + ". It picks them up after its current PR (`merge_train.py status`).")
                return 0
            queue = store
        result = Train(gh or Gh(), options).run(queue)
    except ToolError as e:
        print(f"merge_train: {e}")
        return 2
    finally:
        if owned:
            store.release()
    print("--- summary" + (" (dry run, nothing changed)" if args.dry_run else ""))
    for line in summary(everything(result), result, dry_run=args.dry_run):
        print(line)
    return 0 if result.stopped_at is None else 1


if __name__ == "__main__":
    sys.exit(main())
