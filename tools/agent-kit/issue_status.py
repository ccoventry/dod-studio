r"""Everything about one or more issues in one go: what an issue audit used to gather by hand.

    issue_status.py 45 900 1179 [--out DIR | --combined FILE] [--json]
    issue_status.py --all-open [--out DIR]
    issue_status.py --lint [N ... | --all-open]

Per issue: title, state, labels, body, every comment (read through `gh --json`;
`gh issue view --comments` prints nothing through some shells), and then:
  - PRs that really cite it: `#N` as a whole number in a PR's title or body, or in its
    closing references, any state (`gh pr list -S "#N"` matches loosely: for #45 it
    returned 30 unrelated PRs);
  - remote branches whose name ends in `-N`;
  - dev commits whose subject or body cites `#N`, each mapped to the PR that merged it
    into dev. A PR found only this way is work shipped under another issue's number
    (#900's work merged in PR #860, titled for #841);
  - open PRs changing files the issue names (pr_files.py's cached lists).
Output: markdown per issue (stdout, one file per issue with --out, or one file with
--combined) and a one-line-per-issue table at the end.

--lint lists label and title problems: `on-dev` with no merged PR citing it (by PR text
or by dev commits), `ready` with `needs-decision`, no `area:` label, an `[Area]` title
prefix other than [R&D] / [Chore] / [Spec], old console prefixes (`dodtools_`,
`dodstudio_hide_`) in the title. Exit 1 when it finds any.

Needs `gh`, signed in, and git. Fetches origin/dev first (--no-fetch to skip). Only
reads GitHub; the full PR list is cached 10 minutes (--refresh to re-read).
"""
import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gh_util  # noqa: E402
import pr_files  # noqa: E402

ISSUE_FIELDS = "number,title,state,labels,body,comments,url,closedByPullRequestsReferences"
PR_FIELDS = "number,title,body,state,headRefName,baseRefName,mergedAt,closingIssuesReferences"
ALLOWED_PREFIXES = {"[R&D]", "[Chore]", "[Spec]"}
OLD_CONSOLE_PREFIXES = ("dodtools_", "dodstudio_hide_")
# A repo path or a file name with an extension the repo has, as an issue writes it.
FILE_TOKEN = re.compile(r"(?<![\w/.-])((?:[\w.-]+/)*[\w-]+\.(?:rs|js|cjs|mjs|py|ps1|md|toml|json|cfg|res|txt|"
                        r"yml|yaml|css|html|svg|wgsl|glsl))\b")
MERGE_SUBJECT = re.compile(r"^Merge pull request #(\d+)")
COMMIT_SEP, FIELD_SEP = "\x1e", "\x1f"
CITE = re.compile(r"(?<![\w&])#(\d+)(?!\d)")


# ── Pure parts ───────────────────────────────────────────────────────────────

def cites(text, number):
    """Whether `text` names `#number` as a whole number (not #9001 for #900, not `&#900;`)."""
    return re.search(rf"(?<![\w&])#{number}(?!\d)", text or "") is not None


def citing_prs(number, prs):
    """PRs whose title, body or closing references name the issue, newest first."""
    hits = []
    for pr in prs:
        closes = any(ref.get("number") == number for ref in pr.get("closingIssuesReferences") or [])
        if closes or cites(pr.get("title"), number) or cites(pr.get("body"), number):
            hits.append(dict(pr, closes=closes))
    return sorted(hits, key=lambda pr: -pr["number"])


def branches_for(number, branches):
    return sorted(b for b in branches if re.search(rf"-{number}$", b))


def parse_log(text):
    """`git log --format=%H<US>%s<US>%b<RS>` output as [{sha, subject, body}]."""
    commits = []
    for chunk in text.split(COMMIT_SEP):
        parts = chunk.strip("\n").split(FIELD_SEP)
        if len(parts) >= 2 and parts[0].strip():
            subject, body = parts[1], parts[2] if len(parts) > 2 else ""
            commits.append({"sha": parts[0].strip(), "subject": subject, "body": body,
                            "refs": cited_numbers(subject) | cited_numbers(body)})
    return commits


def cited_numbers(text):
    """Every whole `#N` in `text`, as ints."""
    return {int(n) for n in CITE.findall(text or "")}


def citing_commits(number, commits):
    return [c for c in commits if number in c["refs"]]


def merge_pr_number(subject):
    m = MERGE_SUBJECT.match(subject or "")
    return int(m.group(1)) if m else None


def named_files(texts):
    """File paths and names the issue's text mentions, in first-seen order."""
    seen = []
    for text in texts:
        for token in FILE_TOKEN.findall(text or ""):
            token = token.strip("./")
            if token not in seen and not re.fullmatch(r"\d+(\.\d+)+\w*", token):
                seen.append(token)
    return seen


def lint(issue, merged_evidence):
    """Label and title problems for one issue. `merged_evidence`: merged PR numbers
    citing it by text or by dev commits."""
    labels = {label["name"] for label in issue.get("labels") or []}
    title = issue.get("title") or ""
    problems = []
    if "on-dev" in labels and not merged_evidence:
        problems.append("on-dev, but no merged PR cites it and no dev commit does")
    if "ready" in labels and "needs-decision" in labels:
        problems.append("both ready and needs-decision")
    if not any(name.startswith("area:") for name in labels):
        problems.append("no area: label")
    prefix = re.match(r"^\s*(\[[^\]]*\])", title)
    if prefix and prefix.group(1) not in ALLOWED_PREFIXES:
        problems.append(f"title prefix {prefix.group(1)} (only [R&D], [Chore], [Spec]; the area is a label)")
    for old in OLD_CONSOLE_PREFIXES:
        if old in title:
            problems.append(f"old console prefix {old} in the title")
    return problems


def issues_pr_is_for(pr):
    """The issues a PR says it is for: its closing references and the numbers in its title
    (a body only mentions; `(part of #NN)` in the title counts)."""
    numbers = {ref.get("number") for ref in pr.get("closingIssuesReferences") or []}
    return (numbers | cited_numbers(pr.get("title"))) - {None}


def for_note(made_for, number):
    """", for #841" when a PR is for other issues than `number`; empty otherwise."""
    if not made_for or number in made_for:
        return ""
    return ", for " + ", ".join(f"#{n}" for n in made_for)


def pr_state(pr):
    if pr.get("mergedAt"):
        return f"merged into {pr.get('baseRefName')}"
    return pr.get("state", "").lower()


def status_record(issue, prs, branches, commits, merge_of, open_files, open_titles):
    """Everything the report says about one issue. `merge_of`: commit sha -> (merge PR
    number or None, merge sha or None); `open_files`: {open pr: files}."""
    number = issue["number"]
    cited = citing_prs(number, prs)
    cited_numbers = {pr["number"] for pr in cited}
    by_pr = {}
    for c in citing_commits(number, commits):
        pr_number, _ = merge_of.get(c["sha"], (None, None))
        by_pr.setdefault(pr_number, []).append(c)
    pr_by_number = {pr["number"]: pr for pr in prs}
    via_commits = []
    for pr_number, cs in sorted(by_pr.items(), key=lambda kv: (kv[0] is None, kv[0] or 0)):
        pr = pr_by_number.get(pr_number) or {}
        made_for = sorted(issues_pr_is_for(pr)) if pr else []
        via_commits.append({
            "pr": pr_number, "title": pr.get("title"), "cites_issue": pr_number in cited_numbers,
            "made_for": made_for, "for_this": number in made_for,
            "commits": [{"sha": c["sha"][:10], "subject": c["subject"]} for c in cs],
        })
    texts = [issue.get("body")] + [c.get("body") for c in issue.get("comments") or []]
    files = named_files(texts)
    touching = {}
    for query in files:
        for pr_number, hits in pr_files.touching(query, open_files).items():
            touching.setdefault(pr_number, set()).update(hits)
    merged = {pr["number"] for pr in cited if pr.get("mergedAt")}
    merged |= {v["pr"] for v in via_commits if v["pr"] is not None}
    if any(v["pr"] is None for v in via_commits):
        merged.add(0)  # commits straight on dev count as shipped
    return {
        "number": number, "title": issue.get("title"), "state": issue.get("state"), "url": issue.get("url"),
        "labels": sorted(label["name"] for label in issue.get("labels") or []),
        "body": issue.get("body") or "",
        "comments": [{"author": (c.get("author") or {}).get("login"), "at": c.get("createdAt"), "body": c.get("body")}
                     for c in issue.get("comments") or []],
        "prs": [{"number": pr["number"], "title": pr["title"], "state": pr_state(pr), "closes": pr["closes"],
                 "made_for": sorted(issues_pr_is_for(pr)), "branch": pr.get("headRefName")} for pr in cited],
        "branches": branches_for(number, branches),
        "via_commits": via_commits,
        # Merged PRs carrying commits for this issue while titled for, or closing, others:
        # the audit's "shipped under another issue's number" case (#900 in #860, for #841).
        "shipped_under_other": [{"pr": v["pr"], "made_for": v["made_for"]} for v in via_commits
                                if v["pr"] is not None and not v["for_this"]],
        "named_files": files,
        "open_prs_touching": {n: sorted(f) for n, f in sorted(touching.items())},
        "open_titles": {n: open_titles.get(n) for n in touching},
        "lint": lint(issue, merged),
    }


def to_markdown(rec):
    out = [f"# #{rec['number']} {rec['title']}", "",
           f"{rec['state']} · labels: {', '.join(rec['labels']) or 'none'} · {rec['url'] or ''}", ""]
    if rec["lint"]:
        out += ["**Lint:** " + "; ".join(rec["lint"]), ""]
    out += ["## PRs citing it", ""]
    n = rec["number"]
    out += [f"- #{p['number']} ({p['state']}{', closes it' if p['closes'] else ''}{for_note(p['made_for'], n)}) "
            f"{p['title']}" for p in rec["prs"]] or ["- none"]
    out += ["", "## Dev commits citing it, by the PR that merged them", ""]
    if not rec["via_commits"]:
        out.append("- none")
    for v in rec["via_commits"]:
        where = f"PR #{v['pr']} {v['title'] or ''}" if v["pr"] else "straight on dev (no merge PR)"
        note = ""
        if v["pr"] and not v["for_this"]:
            others = ", ".join(f"#{m}" for m in v["made_for"]) or "no issue"
            note = f"  **(shipped under another number: the PR is for {others})**"
        out.append(f"- {where}{note}")
        out += [f"  - {c['sha']} {c['subject']}" for c in v["commits"]]
    out += ["", "## Remote branches", ""] + ([f"- {b}" for b in rec["branches"]] or ["- none"])
    out += ["", "## Open PRs changing files it names", ""]
    if rec["named_files"]:
        out.append("Files named: " + ", ".join(f"`{f}`" for f in rec["named_files"][:30]))
        out.append("")
    out += [f"- #{n} {rec['open_titles'].get(n) or ''}: {', '.join(f)}" for n, f in rec["open_prs_touching"].items()] or ["- none"]
    out += ["", "## Body", "", rec["body"].strip() or "(empty)", ""]
    out += [f"## Comments ({len(rec['comments'])})", ""]
    for c in rec["comments"]:
        out += [f"### {c['author']} · {c['at']}", "", (c["body"] or "").strip(), ""]
    return "\n".join(out)


def table_row(rec):
    prs = ",".join(f"#{p['number']}{'' if p['state'].startswith('merged') else '(' + p['state'] + ')'}"
                   for p in rec["prs"][:6]) or "-"
    shipped = ",".join(f"#{s['pr']}(for {'/'.join(f'#{m}' for m in s['made_for']) or 'none'})"
                       for s in rec["shipped_under_other"]) or "-"
    touch = ",".join(f"#{n}" for n in rec["open_prs_touching"]) or "-"
    lint_flag = f" LINT({len(rec['lint'])})" if rec["lint"] else ""
    return (f"| #{rec['number']} | {rec['state'].lower()} | {rec['title'][:60]} | {prs} | {shipped} | "
            f"{','.join(rec['branches']) or '-'} | {touch} |{lint_flag}")


TABLE_HEAD = ["| issue | state | title | PRs citing | its commits merged under another issue | branches | "
              "open PRs on its files |",
              "| --- | --- | --- | --- | --- | --- | --- |"]


# ── Fetching ─────────────────────────────────────────────────────────────────

def fetch_issues(numbers, all_open):
    if all_open:
        return gh_util.gh_json(["issue", "list", "--state", "open", "--limit", "1000", "--json", ISSUE_FIELDS.replace(
            ",closedByPullRequestsReferences", "")])
    return [gh_util.gh_json(["issue", "view", str(n), "--json", ISSUE_FIELDS]) for n in numbers]


def all_prs(refresh):
    return gh_util.cached("all_prs.json", 600, lambda: gh_util.gh_json(
        ["pr", "list", "--state", "all", "--limit", "5000", "--json", PR_FIELDS]), refresh)


def remote_branches():
    out = gh_util.git(["ls-remote", "--heads", "origin"])
    return [line.split("refs/heads/", 1)[1] for line in out.splitlines() if "refs/heads/" in line]


def dev_commits():
    fmt = f"%H{FIELD_SEP}%s{FIELD_SEP}%b{COMMIT_SEP}"
    return parse_log(gh_util.git(["log", "origin/dev", "--no-merges", f"--format={fmt}"]))


def dev_merges():
    """{merge sha: subject} for the merges on dev's first-parent line: the merges that
    landed something on dev (a PR's own `Merge branch 'dev'` commits are not on it)."""
    out = gh_util.git(["log", "origin/dev", "--first-parent", "--merges", "--format=%H%x1f%s"])
    return dict(line.split(FIELD_SEP, 1) for line in out.splitlines() if FIELD_SEP in line)


def landing_merge(sha, on_dev_line):
    """(PR number, merge sha) of the first merge on dev's line that has `sha` in it;
    (None, None) for a commit made straight on dev. `on_dev_line`: dev_merges()."""
    # --ancestry-path walks the whole graph (with --first-parent it would never reach a
    # commit that came in through a merge's second parent); oldest is last.
    out = gh_util.git(["rev-list", "--ancestry-path", "--merges", f"{sha}..origin/dev"])
    for merge in reversed(out.split()):
        if merge in on_dev_line:
            return merge_pr_number(on_dev_line[merge]), merge
    return None, None


# ── Command ──────────────────────────────────────────────────────────────────

def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("numbers", nargs="*", type=lambda s: int(s.lstrip("#")))
    p.add_argument("--all-open", action="store_true")
    p.add_argument("--lint", action="store_true", help="only the lint list")
    p.add_argument("--out", type=Path, help="write issue-<N>.md per issue into this folder")
    p.add_argument("--combined", type=Path, help="write every issue into this one markdown file")
    p.add_argument("--json", action="store_true")
    p.add_argument("--no-fetch", action="store_true", help="don't fetch origin/dev first")
    p.add_argument("--no-files", action="store_true", help="skip the open-PR file overlap (saves the gh pr diff calls)")
    p.add_argument("--refresh", action="store_true")
    args = p.parse_args(argv)
    if not args.numbers and not args.all_open:
        p.error("name issues or --all-open")
    try:
        if not args.no_fetch:
            gh_util.git(["fetch", "-q", "origin", "dev"])
        issues = fetch_issues(args.numbers, args.all_open)
        prs = all_prs(args.refresh)
        branches = remote_branches()
        commits = dev_commits()
        wanted = {i["number"] for i in issues}
        merge_of, on_dev_line = {}, dev_merges()
        for c in commits:
            if c["refs"] & wanted:
                merge_of[c["sha"]] = landing_merge(c["sha"], on_dev_line)
        open_files, open_titles = {}, {}
        if not args.no_files and not args.lint:
            open_list = pr_files.open_prs(args.refresh)
            open_files = pr_files.files_by_pr(open_list)
            open_titles = {pr["number"]: pr["title"] for pr in open_list}
    except gh_util.ToolError as e:
        print(e, file=sys.stderr)
        return 2
    records = [status_record(i, prs, branches, commits, merge_of, open_files, open_titles)
               for i in sorted(issues, key=lambda i: i["number"])]
    if args.lint:
        flagged = [r for r in records if r["lint"]]
        for r in flagged:
            print(f"#{r['number']:<5} {r['title'][:70]}")
            for problem in r["lint"]:
                print(f"        {problem}")
        print(f"{len(flagged)} of {len(records)} issue(s) with lint")
        return 1 if flagged else 0
    if args.json:
        print(json.dumps(records, indent=1, default=list))
        return 0
    docs = [to_markdown(r) for r in records]
    table = "\n".join(TABLE_HEAD + [table_row(r) for r in records])
    if args.out:
        args.out.mkdir(parents=True, exist_ok=True)
        for r, doc in zip(records, docs):
            (args.out / f"issue-{r['number']}.md").write_text(doc + "\n", encoding="utf-8")
        (args.out / "table.md").write_text(table + "\n", encoding="utf-8")
        print(f"{len(docs)} issue file(s) and table.md in {args.out.resolve()}")
    elif args.combined:
        args.combined.parent.mkdir(parents=True, exist_ok=True)
        args.combined.write_text("\n\n---\n\n".join(docs) + "\n\n" + table + "\n", encoding="utf-8")
        print(f"{len(docs)} issue(s) in {args.combined.resolve()}")
    else:
        print("\n\n---\n\n".join(docs))
        print()
    print(table)
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main())
