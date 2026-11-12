"""Merge open PRs into dev one at a time, in the order given, each only once it is green.

    python -I tools/agent-kit/merge_train.py 1418 1416 1420
    python -I tools/agent-kit/merge_train.py 1418 1416 --dry-run

dev's ruleset requires the branch to be up to date (CLAUDE.md, "Merging into dev"), so
every merge puts the rest behind. For each PR in turn this:
  1. checks it is open, not a draft, targets dev and has no conflicts;
  2. runs `gh pr update-branch` when it is behind dev;
  3. polls `gh pr checks` until every check the dev ruleset requires is green;
  4. merges with `gh pr merge --merge --match-head-commit <the commit that was checked>`;
  5. confirms GitHub shows it MERGED, then goes on to the next.
It stops at the first red check, conflict, timeout or failed step, names the PR and the
check, and touches nothing after it. It never passes `--admin` or `--auto` and never
pushes. The required check names come from the ruleset itself
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

BASE = "dev"
POLL_INTERVAL_SECS = 30
CHECKS_TIMEOUT_SECS = 45 * 60
# How long to wait for GitHub to show a new head commit after `update-branch`, and to
# show MERGED after `gh pr merge`.
HEAD_MOVE_TIMEOUT_SECS = 120
MERGED_TIMEOUT_SECS = 60
SHORT_POLL_SECS = 5
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
                                    "number,title,state,isDraft,baseRefName,mergeable,headRefOid,autoMergeRequest"]))

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


class Stop(Exception):
    """The train stops at this PR; the message says why."""


class Train:
    def __init__(self, gh, options, clock=time.monotonic, sleep=time.sleep, out=print):
        self.gh, self.opt, self.clock, self.sleep, self.out = gh, options, clock, sleep, out

    def log(self, pr, text):
        self.out(f"[{datetime.now():%H:%M:%S}] #{pr} {text}")

    def run(self, prs):
        result = Result()
        required = self.gh.required_checks()
        self.out(f"[{datetime.now():%H:%M:%S}] required checks on {BASE}: {', '.join(required)}")
        for i, pr in enumerate(prs):
            try:
                self.ride(pr, required)
                result.merged.append(pr)
            except (Stop, ToolError) as e:
                result.stopped_at, result.reason, result.untouched = pr, str(e), list(prs[i + 1:])
                self.log(pr, f"STOP: {e}")
                break
        return result

    def ride(self, pr, required):
        view = self.gh.view(pr)
        self.log(pr, f"{view['title']} (head {view['headRefOid'][:8]})")
        problem = pr_problem(view)
        if problem:
            raise Stop(problem)
        if self.opt.dry_run:
            self.dry_run(pr, view, required)
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
        self.gh.update_branch(pr)
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
    if result.stopped_at is not None:
        lines.append(f"Stopped at #{result.stopped_at}: {result.reason}")
        if result.untouched:
            lines.append("Not touched: " + ", ".join(f"#{n}" for n in result.untouched))
    return lines


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("prs", nargs="+", type=int, help="PR numbers, merged in this order")
    ap.add_argument("--dry-run", action="store_true", help="show what it would do; change nothing")
    ap.add_argument("--interval", type=float, default=POLL_INTERVAL_SECS, help="seconds between check polls")
    ap.add_argument("--timeout", type=float, default=CHECKS_TIMEOUT_SECS / 60,
                    help="minutes to wait for one PR's checks")
    args = ap.parse_args(argv)
    options = Options(interval_secs=args.interval, timeout_secs=args.timeout * 60, dry_run=args.dry_run)
    try:
        result = Train(Gh(), options).run(args.prs)
    except ToolError as e:
        print(f"merge_train: {e}")
        return 2
    print("--- summary" + (" (dry run, nothing changed)" if args.dry_run else ""))
    for line in summary(args.prs, result, dry_run=args.dry_run):
        print(line)
    return 0 if result.stopped_at is None else 1


if __name__ == "__main__":
    sys.exit(main())
