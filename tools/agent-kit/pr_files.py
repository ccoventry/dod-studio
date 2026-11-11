r"""Which open PRs touch a file, which open PRs overlap one PR, and how stale the drafts are.

    pr_files.py studio/src/strings.js [more files]
        Every open PR that changes each file. A name with no folder (`strings.js`)
        matches that file name in any folder; a folder (`studio/src/`) matches
        everything under it.
    pr_files.py --pr 1381
        The other open PRs that change any of PR 1381's files, and which files.
    pr_files.py --stale-drafts
        Each open draft: commits behind dev, ahead, conflicts, age.

    --json for machine output; --refresh to re-read the open-PR list (kept 10 minutes).

Each PR's file list comes from `gh pr diff N --name-only` and is cached per head commit
under the agents cache (kit_paths.agents_cache), so a second call costs one `gh pr list`.
Needs `gh`, signed in. Only reads.
"""
import argparse
import json
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gh_util  # noqa: E402

OPEN_FIELDS = "number,title,headRefOid,headRefName,baseRefName,isDraft,mergeable,mergeStateStatus,createdAt"
LIST_MAX_AGE = 600


# ── Pure parts ───────────────────────────────────────────────────────────────

def norm(path):
    """A repo path with forward slashes and no leading ./ or /."""
    path = path.strip().replace("\\", "/")
    while path.startswith("./"):
        path = path[2:]
    return path.lstrip("/")


def matches(query, path):
    """Whether repo file `path` is what `query` names: the same path, a file name with
    no folder matching the last part, or a folder (ending in /) holding it."""
    query, path = norm(query), norm(path)
    if query.endswith("/"):
        return path.startswith(query)
    if "/" not in query:
        return path.rsplit("/", 1)[-1] == query
    return path == query or path.endswith("/" + query)


def touching(query, files_by_pr):
    """{pr number: [its files that match query]} for the PRs with any."""
    hits = {}
    for number, files in files_by_pr.items():
        found = [f for f in files if matches(query, f)]
        if found:
            hits[number] = found
    return hits


def overlaps(number, files_by_pr):
    """{other pr: [files both change]} for every other PR sharing a file with `number`."""
    mine = set(files_by_pr.get(number, ()))
    out = {}
    for other, files in files_by_pr.items():
        if other != number:
            shared = sorted(mine & set(files))
            if shared:
                out[other] = shared
    return out


def stale_row(pr, behind, ahead, now=None):
    now = now or datetime.now(timezone.utc)
    created = datetime.fromisoformat(pr["createdAt"].replace("Z", "+00:00"))
    return {
        "number": pr["number"], "title": pr["title"], "age_days": (now - created).days,
        "behind": behind, "ahead": ahead,
        "conflicts": pr.get("mergeable") == "CONFLICTING" or pr.get("mergeStateStatus") == "DIRTY",
        "base": pr.get("baseRefName"),
    }


def format_stale(row):
    behind = "?" if row["behind"] is None else row["behind"]
    ahead = "?" if row["ahead"] is None else row["ahead"]
    flags = "conflicts" if row["conflicts"] else ""
    base = "" if row["base"] in ("dev", "main") else f" (on {row['base']})"
    return (f"#{row['number']:<5} {row['age_days']:>3}d behind {behind:>5} ahead {ahead:>3} {flags:<9} "
            f"{row['title'][:80]}{base}")


# ── GitHub ───────────────────────────────────────────────────────────────────

def open_prs(refresh=False):
    return gh_util.cached("open_prs.json", LIST_MAX_AGE,
                          lambda: gh_util.gh_json(["pr", "list", "--state", "open", "--limit", "300",
                                                   "--json", OPEN_FIELDS]), refresh)


def files_of(pr):
    """The PR's changed files, cached for its head commit."""
    key = f"pr_files/{pr['number']}-{pr['headRefOid'][:12]}.json"
    return gh_util.cached(key, None, lambda: sorted(
        line.strip() for line in gh_util.run(["gh", "pr", "diff", str(pr["number"]), "--name-only"]).splitlines()
        if line.strip()))


def files_by_pr(prs):
    with ThreadPoolExecutor(6) as pool:
        lists = list(pool.map(files_of, prs))
    return {pr["number"]: files for pr, files in zip(prs, lists)}


def behind_ahead(pr):
    """(commits dev has that the head lacks, commits the head adds), or (None, None)."""
    try:
        data = gh_util.gh_json(["api", f"repos/{{owner}}/{{repo}}/compare/dev...{pr['headRefOid']}"])
    except gh_util.ToolError:
        return None, None
    return data.get("behind_by"), data.get("ahead_by")


# ── Command ──────────────────────────────────────────────────────────────────

def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("files", nargs="*", help="repo paths, file names or folders/")
    p.add_argument("--pr", type=int, help="list the open PRs overlapping this one")
    p.add_argument("--stale-drafts", action="store_true")
    p.add_argument("--json", action="store_true")
    p.add_argument("--refresh", action="store_true", help="re-read the open-PR list now")
    args = p.parse_args(argv)
    if not (args.files or args.pr or args.stale_drafts):
        p.error("name a file, --pr N or --stale-drafts")
    try:
        prs = open_prs(args.refresh)
        titles = {pr["number"]: pr["title"] for pr in prs}
        out = {}
        if args.files or args.pr:
            by_pr = files_by_pr(prs)
        for query in args.files:
            out[query] = touching(query, by_pr)
            if not args.json:
                hits = out[query]
                print(f"{query}: {len(hits)} open PR(s)")
                for number, files in sorted(hits.items()):
                    print(f"  #{number:<5} {titles[number][:80]}  [{', '.join(files)}]")
        if args.pr:
            if args.pr not in by_pr:
                print(f"#{args.pr} is not an open PR", file=sys.stderr)
                return 1
            out[f"#{args.pr}"] = overlaps(args.pr, by_pr)
            if not args.json:
                hits = out[f"#{args.pr}"]
                print(f"#{args.pr} ({len(by_pr[args.pr])} files) overlaps {len(hits)} open PR(s)")
                for number, files in sorted(hits.items()):
                    print(f"  #{number:<5} {titles[number][:70]}")
                    print(f"         {', '.join(files)}")
        if args.stale_drafts:
            drafts = sorted((pr for pr in prs if pr.get("isDraft")), key=lambda pr: pr["createdAt"])
            with ThreadPoolExecutor(6) as pool:
                counts = list(pool.map(behind_ahead, drafts))
            rows = [stale_row(pr, *c) for pr, c in zip(drafts, counts)]
            out["stale_drafts"] = rows
            if not args.json:
                print(f"{len(rows)} open draft(s), oldest first")
                for row in rows:
                    print(format_stale(row))
    except gh_util.ToolError as e:
        print(e, file=sys.stderr)
        return 2
    if args.json:
        print(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main())
