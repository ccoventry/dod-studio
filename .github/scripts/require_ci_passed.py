#!/usr/bin/env python3
"""Fail unless every required CI check passed on the code being released (#915).

The release workflows used to re-run `cargo test --workspace`, repeating what
CI had already run on the same code. This asks GitHub instead: it reads the
required status checks of the branch the release is cut from (the rulesets,
via `GET /repos/{repo}/rules/branches/{branch}`) and looks for a passing check
run of each one, by its exact name and app, on the commit being built.

The commit being built often has no CI of its own:

- stable builds main's `dev` -> `main` merge commit, and CI never runs on a
  push to main;
- experimental builds dev's head, whose push CI may still be running.

So the check runs of any commit with the *same tree* count too: the commit's
parents and the heads of the PRs it came from. The rulesets require a branch
to be up to date before it merges, so a merge commit's tree equals its PR
head's, and CI's pull_request run (which tests the PR merged into its base)
tested exactly that tree. A commit whose tree no CI run tested fails.

A check passes on `success`, `neutral` or `skipped`, as the rulesets count
them. When a check has no pass yet but a run of it is still going, this waits
(polling every `--poll` seconds, up to `--timeout` minutes).

Usage (CI, from the repo root):

    python .github/scripts/require_ci_passed.py --sha "$SHA" --branch main

Locally the `gh` CLI uses the logged-in account; in CI it reads GH_TOKEN.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass

PASSING = {"success", "neutral", "skipped"}


@dataclass(frozen=True)
class Required:
    name: str
    app_id: int | None


@dataclass(frozen=True)
class Run:
    name: str
    app_id: int | None
    run_id: int
    status: str  # queued / in_progress / completed / ...
    conclusion: str | None
    url: str
    sha: str


@dataclass(frozen=True)
class Verdict:
    name: str
    state: str  # pass / pending / fail / missing
    detail: str


# ---------------------------------------------------------------------------
# Pure logic (tested in test_require_ci_passed.py)
# ---------------------------------------------------------------------------


def required_checks(rules: list[dict]) -> list[Required]:
    """The required status checks in a `rules/branches/{branch}` response."""
    out: list[Required] = []
    seen = set()
    for rule in rules:
        if rule.get("type") != "required_status_checks":
            continue
        for check in (rule.get("parameters") or {}).get("required_status_checks", []):
            req = Required(check["context"], check.get("integration_id"))
            if req not in seen:
                seen.add(req)
                out.append(req)
    return out


def evaluate(required: list[Required], runs: list[Run]) -> list[Verdict]:
    """One verdict per required check, over the runs of every same-tree commit.

    Per commit, only the latest run of a check counts (a re-run replaces the
    run before it). The check passes when any commit's latest run passed.
    """
    verdicts = []
    for req in required:
        latest: dict[str, Run] = {}
        for run in runs:
            if run.name != req.name:
                continue
            if req.app_id is not None and run.app_id != req.app_id:
                continue
            prev = latest.get(run.sha)
            if prev is None or run.run_id > prev.run_id:
                latest[run.sha] = run
        if not latest:
            verdicts.append(Verdict(req.name, "missing", "no run on any same-tree commit"))
            continue
        passed = [r for r in latest.values() if r.status == "completed" and r.conclusion in PASSING]
        if passed:
            r = passed[0]
            verdicts.append(Verdict(req.name, "pass", f"{r.conclusion} on {r.sha[:10]} {r.url}"))
            continue
        pending = [r for r in latest.values() if r.status != "completed"]
        if pending:
            r = pending[0]
            verdicts.append(Verdict(req.name, "pending", f"{r.status} on {r.sha[:10]} {r.url}"))
            continue
        r = max(latest.values(), key=lambda x: x.run_id)
        verdicts.append(Verdict(req.name, "fail", f"{r.conclusion} on {r.sha[:10]} {r.url}"))
    return verdicts


def outcome(verdicts: list[Verdict]) -> str:
    """`pass` when all pass, `wait` while a run is still going, else `fail`."""
    states = {v.state for v in verdicts}
    if states <= {"pass"}:
        return "pass"
    if "pending" in states:
        return "wait"
    return "fail"


def report(verdicts: list[Verdict]) -> str:
    width = max((len(v.name) for v in verdicts), default=0)
    return "\n".join(f"  {v.state.upper():8} {v.name:{width}}  {v.detail}" for v in verdicts)


# ---------------------------------------------------------------------------
# GitHub
# ---------------------------------------------------------------------------


def gh_api(path: str):
    result = subprocess.run(
        ["gh", "api", "-H", "Accept: application/vnd.github+json", path],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if result.returncode != 0:
        raise SystemExit(f"gh api {path} failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def same_tree_commits(repo: str, sha: str) -> list[str]:
    """`sha`, then its parents and the heads of its PRs that have its tree."""
    commit = gh_api(f"repos/{repo}/git/commits/{sha}")
    tree = commit["tree"]["sha"]
    candidates = [sha]
    others = [p["sha"] for p in commit.get("parents", [])]
    for pr in gh_api(f"repos/{repo}/commits/{sha}/pulls"):
        others.append(pr["head"]["sha"])
    for other in others:
        if other in candidates:
            continue
        if gh_api(f"repos/{repo}/git/commits/{other}")["tree"]["sha"] == tree:
            candidates.append(other)
    return candidates


def check_runs(repo: str, sha: str) -> list[Run]:
    runs: list[Run] = []
    page = 1
    while True:
        data = gh_api(f"repos/{repo}/commits/{sha}/check-runs?per_page=100&page={page}")
        batch = data.get("check_runs", [])
        for cr in batch:
            runs.append(
                Run(
                    name=cr["name"],
                    app_id=(cr.get("app") or {}).get("id"),
                    run_id=cr["id"],
                    status=cr["status"],
                    conclusion=cr.get("conclusion"),
                    url=cr.get("html_url") or "",
                    sha=sha,
                )
            )
        if len(batch) < 100:
            return runs
        page += 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--sha", required=True, help="the commit being released")
    ap.add_argument("--branch", required=True, help="branch whose required checks apply")
    ap.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "ccoventry/dod-studio"))
    ap.add_argument("--timeout", type=float, default=45, help="minutes to wait for running checks")
    ap.add_argument("--poll", type=float, default=30, help="seconds between polls")
    args = ap.parse_args(argv)

    required = required_checks(gh_api(f"repos/{args.repo}/rules/branches/{args.branch}"))
    if not required:
        print(f"::error::{args.branch} has no required status checks; refusing to release untested code")
        return 1
    candidates = same_tree_commits(args.repo, args.sha)
    print(f"Commit {args.sha}, required checks of {args.branch}: {', '.join(r.name for r in required)}")
    print(f"Commits with the same tree: {', '.join(candidates)}")

    deadline = time.monotonic() + args.timeout * 60
    while True:
        runs = [run for sha in candidates for run in check_runs(args.repo, sha)]
        verdicts = evaluate(required, runs)
        result = outcome(verdicts)
        print(report(verdicts), flush=True)
        if result == "pass":
            print("All required CI checks passed on this tree.")
            return 0
        if result == "fail":
            bad = [v.name for v in verdicts if v.state in ("fail", "missing")]
            print(f"::error::Required CI checks did not pass on {args.sha[:10]}'s tree: {', '.join(bad)}")
            return 1
        if time.monotonic() >= deadline:
            waiting = [v.name for v in verdicts if v.state == "pending"]
            print(f"::error::Gave up after {args.timeout:g} min waiting for: {', '.join(waiting)}")
            return 1
        print(f"Waiting {args.poll:g} s for running checks...", flush=True)
        time.sleep(args.poll)


if __name__ == "__main__":
    sys.exit(main())
