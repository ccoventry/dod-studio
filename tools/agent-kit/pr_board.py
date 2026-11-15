r"""Open PRs, oldest first, as one table: what each is waiting on.

    pr_board.py [--limit 200] [--json] [--repo OWNER/NAME]

Per PR: number, age, draft, CI (pass / FAIL / running / none), whether it is behind
dev or has conflicts, a stacked base (any base other than dev or main: no CI runs
on it), whether a test DLL folder `local\test-<n>*\` with a .dll exists in the main
checkout (for the PR's own number or any issue its title or closing references
name), and the PR body's "Test done" first line plus any line that says the user
is needed. Needs `gh`, signed in. Only reads.
"""
import argparse
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

FIELDS = ("number,title,createdAt,isDraft,baseRefName,headRefName,mergeStateStatus,mergeable,"
          "statusCheckRollup,closingIssuesReferences,body")
TRUNK = {"dev", "main"}
NEEDS_USER = re.compile(r"needs? the user|for the user|user to (test|run|check|decide)|not tested|untested", re.I)
ISSUE_REF = re.compile(r"#(\d+)")
PASSING = {"SUCCESS", "NEUTRAL", "SKIPPED"}
FAILING = {"FAILURE", "ERROR", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE"}


# ── Pure parts ───────────────────────────────────────────────────────────────

def check_result(check):
    """One rollup entry (a CheckRun or a StatusContext) as pass / fail / running."""
    if check.get("__typename") == "StatusContext":
        state = check.get("state") or ""
        return "pass" if state == "SUCCESS" else "fail" if state in FAILING | {"FAILURE"} else "running"
    if check.get("status") != "COMPLETED":
        return "running"
    conclusion = check.get("conclusion") or ""
    return "pass" if conclusion in PASSING else "fail" if conclusion in FAILING else "running"


def ci_state(rollup):
    """The PR's CI in one word: FAIL beats running beats pass; none without checks."""
    results = [check_result(c) for c in rollup or []]
    if not results:
        return "none"
    if "fail" in results:
        failed = [c.get("name") or c.get("context") for c in rollup if check_result(c) == "fail"]
        return "FAIL(" + ",".join(failed)[:40] + ")"
    return "running" if "running" in results else "pass"


def merge_flags(pr):
    """behind / conflicts / stacked-on-<base>, as GitHub reports them."""
    flags = []
    if pr.get("mergeStateStatus") == "BEHIND":
        flags.append("behind")
    if pr.get("mergeable") == "CONFLICTING" or pr.get("mergeStateStatus") == "DIRTY":
        flags.append("conflicts")
    if pr.get("baseRefName") not in TRUNK:
        flags.append(f"stacked on {pr.get('baseRefName')}")
    return flags


def section(body, heading):
    """The lines under a `## <heading>` (any level), up to the next heading."""
    lines, inside = [], False
    for line in (body or "").splitlines():
        if re.match(r"^#{1,6}\s", line):
            if inside:
                break
            inside = line.lstrip("#").strip().lower().startswith(heading.lower())
            continue
        if inside and line.strip():
            lines.append(line.strip())
    return lines


def body_notes(body):
    """(first "Test done" line, lines that say the user is needed)."""
    test_done = section(body, "Test done")
    needs = []
    for line in (body or "").splitlines():
        if not NEEDS_USER.search(line):
            continue
        heading = re.match(r"^#{1,6}\s+(.*)", line)
        if heading:
            # A "## For the user" heading: its first line says what.
            under = section(body, heading.group(1).strip())
            if under:
                needs.append(under[0])
        else:
            needs.append(line.strip())
    return (test_done[0] if test_done else None), needs[:3]


def related_numbers(pr):
    """The PR's number and every issue its title or closing references name."""
    numbers = {pr["number"]}
    numbers.update(int(n) for n in ISSUE_REF.findall(pr.get("title", "")))
    numbers.update(ref["number"] for ref in pr.get("closingIssuesReferences") or [])
    return numbers


def test_dll_folders(numbers, local_dir):
    """Folders `local\\test-<n>` or `test-<n>-*` holding a .dll, for any of `numbers`."""
    found = []
    for folder in sorted(Path(local_dir).glob("test-*")):
        m = re.match(r"test-(\d+)(?:\D|$)", folder.name)
        if m and int(m.group(1)) in numbers and any(folder.glob("*.dll")):
            found.append(folder.name)
    return found


def age_days(created_at, now=None):
    now = now or datetime.now(timezone.utc)
    return (now - datetime.fromisoformat(created_at.replace("Z", "+00:00"))).days


def board_row(pr, local_dir, now=None):
    """Everything the table shows for one PR."""
    test_done, needs = body_notes(pr.get("body"))
    return {
        "number": pr["number"], "title": pr["title"], "age_days": age_days(pr["createdAt"], now),
        "draft": pr.get("isDraft", False), "ci": ci_state(pr.get("statusCheckRollup")),
        "flags": merge_flags(pr), "test_dlls": test_dll_folders(related_numbers(pr), local_dir),
        "test_done": test_done, "needs_user": needs,
    }


def format_row(row):
    head = (f"#{row['number']:<5} {row['age_days']:>3}d {'draft ' if row['draft'] else ''}{row['ci']:<8} "
            f"{' '.join(row['flags']):<14} dll:{','.join(row['test_dlls']) or '-':<12} {row['title'][:80]}")
    lines = [head]
    if row["test_done"]:
        lines.append(f"        test done: {row['test_done'][:140]}")
    lines += [f"        user: {line[:140]}" for line in row["needs_user"]]
    return "\n".join(lines)


# ── Command ──────────────────────────────────────────────────────────────────

def fetch(limit, repo=None):
    cmd = ["gh", "pr", "list", "--state", "open", "--limit", str(limit), "--json", FIELDS]
    if repo:
        cmd += ["--repo", repo]
    out = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8",
                         cwd=str(kit_paths.REPO), check=True).stdout
    return sorted(json.loads(out), key=lambda pr: pr["createdAt"])


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--limit", type=int, default=200)
    p.add_argument("--repo")
    p.add_argument("--json", action="store_true")
    args = p.parse_args(argv)
    local_dir = kit_paths.main_checkout() / "local"
    rows = [board_row(pr, local_dir) for pr in fetch(args.limit, args.repo)]
    if args.json:
        print(json.dumps(rows, indent=1))
    else:
        print(f"{len(rows)} open PR(s), oldest first")
        for row in rows:
            print(format_row(row))
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main())
