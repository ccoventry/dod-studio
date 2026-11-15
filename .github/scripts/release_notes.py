#!/usr/bin/env python3
"""Build a release's notes from the `Release note:` lines of its merged PRs.

Both release workflows run this before tauri-action and publish its output as
the release body (see docs/versioning_and_releases.md). It also runs locally:

    python .github/scripts/release_notes.py --from v0.1.4 --target origin/dev

What it does:
1. Lists the PRs merged between the previous tag and the target commit: every
   `Merge pull request #N from owner/branch` merge commit in `prev..target`
   (all of them, not just first-parent, so a stable release on `main` reaches
   the PRs that landed on `dev`, and a PR merged into another PR's branch is
   counted too), plus squash merges (`... (#N)`) on the target's first-parent
   history. Release plumbing PRs (`dev` -> `main` cutovers, `main` -> `dev`
   syncs) are left out.
2. Fetches each PR's title, body and labels with `gh api graphql` (the `gh`
   CLI reads GH_TOKEN in CI and the logged-in account locally).
3. Takes the first `Release note:` line of each body (case-insensitive;
   "none" or empty means internal work), and groups the notes into New /
   Fixed / Changed. An explicit `New:` / `Fixed:` / `Changed:` prefix wins;
   otherwise a `bug` label means Fixed, `enhancement` means New, and anything
   else is Changed.
4. Drops the PRs a later PR in the same range supersedes: a body line
   `Supersedes release note: #710, #731` says that PR's own note replaces the
   notes of the PRs it renames or changes, so the release never names a
   feature that no longer exists. Every drop is logged to stderr.
5. Prints Markdown: an optional hand-written summary, the grouped notes, a
   collapsed list of every merged PR, and a compare link.

Hand-written summary: when `.github/release_notes/<version>.md` exists for the
version being released (`--version`; a leading `v` and any `-N` experimental
suffix are dropped, so an experimental `0.2.0-57` build and the stable 0.2.0
release both read `0.2.0.md`), its content comes first, under "What's new".

Previous tag: `--from` names it; without it, the highest stable `vX.Y.Z` tag
on the remote (`--remote`, default origin) is used, below `--before` when that
is given. Only tags on the remote count, because a local clone can also carry
the upstream dod-tools tags (v0.2.0 .. v0.10.0), which are not this project's
releases. The experimental workflow compares from the latest stable tag, never
from `experimental-latest`; the stable workflow passes `--before <new version>`.

Python 3 standard library only, plus `git` and `gh` on PATH.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

GROUPS = ("New", "Fixed", "Changed")

# `Release note: ...` / `**Release note:** ...` / `- Release notes: ...`
_NOTE_RE = re.compile(
    r"^[ \t]*(?:[-*][ \t]+)?(?:\*\*|__)?release[ \t]+notes?"
    r"(?:[ \t]*:[ \t]*(?:\*\*|__)?|(?:\*\*|__)[ \t]*:)[ \t]*(.*)$",
    re.IGNORECASE | re.MULTILINE,
)
# `Supersedes release note: #710, #731` / `**Supersedes release notes:** 710 731`
_SUPERSEDES_RE = re.compile(
    r"^[ \t]*(?:[-*][ \t]+)?(?:\*\*|__)?supersedes[ \t]+release[ \t]+notes?"
    r"(?:[ \t]*:[ \t]*(?:\*\*|__)?|(?:\*\*|__)[ \t]*:)[ \t]*(.*)$",
    re.IGNORECASE | re.MULTILINE,
)
_PR_TOKEN_RE = re.compile(r"^#?(\d+)$")
_HTML_COMMENT_RE = re.compile(r"<!--.*?-->", re.DOTALL)
_PREFIX_RE = re.compile(
    r"^(new|added|fixed|fix|changed|change)[ \t]*:[ \t]*(.*)$", re.IGNORECASE | re.DOTALL
)
_PREFIX_GROUP = {
    "new": "New",
    "added": "New",
    "fixed": "Fixed",
    "fix": "Fixed",
    "changed": "Changed",
    "change": "Changed",
}
_NONE_VALUES = {"none", "n/a", "na", "-", "no", "internal"}
_MERGE_RE = re.compile(r"^Merge pull request #(\d+) from ([^/\s]+)/(\S+)")
_SQUASH_RE = re.compile(r"\(#(\d+)\)\s*$")
_STABLE_TAG_RE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")


@dataclass
class PullRequest:
    number: int
    title: str
    body: str = ""
    labels: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Parsing and grouping (pure; covered by test_release_notes.py)
# ---------------------------------------------------------------------------


def extract_release_note(body: str | None) -> str | None:
    """The first `Release note:` line's text, or None for none/empty/missing.

    HTML comments are stripped first, so the template's guidance comment never
    counts as a note, and an unfilled `<placeholder>` counts as empty.
    """
    if not body:
        return None
    text = _HTML_COMMENT_RE.sub("", body.replace("\r\n", "\n"))
    match = _NOTE_RE.search(text)
    if not match:
        return None
    note = match.group(1).strip().strip("`").strip()
    if not note or (note.startswith("<") and note.endswith(">")):
        return None
    if note.rstrip(".").strip().lower() in _NONE_VALUES:
        return None
    return note


def classify(note: str, labels: list[str] | None = None) -> tuple[str, str]:
    """(group, text without its prefix). A prefix wins over labels."""
    match = _PREFIX_RE.match(note)
    if match:
        text = match.group(2).strip()
        if text.rstrip(".").strip().lower() in _NONE_VALUES:
            text = ""
        return _PREFIX_GROUP[match.group(1).lower()], text
    names = {label.lower() for label in labels or []}
    if "bug" in names:
        return "Fixed", note
    if "enhancement" in names:
        return "New", note
    return "Changed", note


def group_notes(prs: list[PullRequest]) -> dict[str, list[tuple[str, int]]]:
    """{group: [(note, pr number)]}, each list in PR-number order."""
    groups: dict[str, list[tuple[str, int]]] = {name: [] for name in GROUPS}
    for pr in sorted(prs, key=lambda p: p.number):
        note = extract_release_note(pr.body)
        if note is None:
            continue
        group, text = classify(note, pr.labels)
        if text:
            groups[group].append((text, pr.number))
    return groups


def extract_superseded(body: str | None) -> list[int]:
    """PR numbers on every `Supersedes release note:` line, in order, unique.

    Numbers may carry a `#` and be separated by commas or spaces; other words
    on the line ("and") are ignored. HTML comments are stripped first, like
    the `Release note:` line, so the template's hint never counts.
    """
    if not body:
        return []
    text = _HTML_COMMENT_RE.sub("", body.replace("\r\n", "\n"))
    numbers: list[int] = []
    for match in _SUPERSEDES_RE.finditer(text):
        for token in re.split(r"[,\s]+", match.group(1)):
            token_match = _PR_TOKEN_RE.match(token.strip("`*_.;"))
            if token_match:
                number = int(token_match.group(1))
                if number not in numbers:
                    numbers.append(number)
    return numbers


def _list(numbers: list[int]) -> str:
    return ", ".join(f"#{n}" for n in numbers)


def apply_supersedes(prs: list[PullRequest]) -> tuple[list[PullRequest], list[str]]:
    """(the PRs that stay, log lines) after dropping superseded PRs.

    A PR named on a `Supersedes release note:` line of another PR in the same
    range is dropped from the notes and from the full list; the superseding
    PR's own note stands for it. Chains collapse (A supersedes B supersedes C
    leaves A). A number outside the range shipped in an earlier release, so
    its note is already out: that gets a notice and nothing else. A cycle
    keeps its highest-numbered PR, the latest one.
    """
    by_number = {pr.number: pr for pr in prs}
    log: list[str] = []
    superseded_by: dict[int, int] = {}
    for pr in sorted(prs, key=lambda p: p.number):
        for number in extract_superseded(pr.body):
            if number == pr.number:
                continue
            if number not in by_number:
                log.append(
                    f"notice: #{pr.number} supersedes #{number}, which is not in this release "
                    f"(shipped earlier?); its note is already out and cannot be dropped"
                )
                continue
            # Two PRs superseding the same one: the later PR wins.
            superseded_by[number] = max(superseded_by.get(number, 0), pr.number)

    while True:
        cycle = _find_cycle(superseded_by)
        if not cycle:
            break
        keep = max(cycle)
        del superseded_by[keep]
        log.append(
            f"warning: {_list(sorted(cycle))} supersede each other; #{keep} (the latest) is kept"
        )

    def head(number: int) -> int:
        while number in superseded_by:
            number = superseded_by[number]
        return number

    lost: dict[int, list[int]] = {}
    for number in sorted(superseded_by):
        by, top = superseded_by[number], head(number)
        log.append(
            f"#{number} dropped: superseded by #{by}"
            + (f" (in turn superseded by #{top})" if top != by else "")
        )
        if extract_release_note(by_number[number].body) is not None:
            lost.setdefault(top, []).append(number)
    for top, numbers in sorted(lost.items()):
        if extract_release_note(by_number[top].body) is None:
            log.append(
                f"warning: #{top} supersedes {_list(numbers)} but has no release note of its own; "
                f"their notes are dropped with nothing in their place"
            )

    kept = [pr for pr in prs if pr.number not in superseded_by]
    return kept, log


def _find_cycle(edges: dict[int, int]) -> list[int] | None:
    """The PR numbers of one cycle in `edges` (dropped -> superseder), or None."""
    for start in sorted(edges):
        path: list[int] = []
        number = start
        while number in edges and number not in path:
            path.append(number)
            number = edges[number]
        if number in path:
            return path[path.index(number):]
    return None


def summary_version(version: str | None) -> str | None:
    """`v0.2.0-57` -> `0.2.0`: the name of the hand-written summary file."""
    if not version:
        return None
    version = version.strip()
    if version.startswith("v"):
        version = version[1:]
    return version.split("-", 1)[0] or None


def pick_previous_tag(tags: list[str], before: str | None = None) -> str | None:
    """The highest stable `vX.Y.Z` tag, below `before` when it is given."""
    limit = None
    if before:
        limit_match = _STABLE_TAG_RE.match("v" + summary_version(before))
        if not limit_match:
            raise ValueError(f"--before is not a plain X.Y.Z version: {before}")
        limit = tuple(int(x) for x in limit_match.groups())
    best: tuple[tuple[int, int, int], str] | None = None
    for tag in tags:
        match = _STABLE_TAG_RE.match(tag)
        if not match:
            continue
        key = tuple(int(x) for x in match.groups())
        if limit is not None and key >= limit:
            continue
        if best is None or key > best[0]:
            best = (key, tag)
    return best[1] if best else None


def find_pr_numbers(merges: list[str], first_parent: list[str]) -> list[int]:
    """PR numbers from merge-commit subjects and first-parent squash subjects.

    `merges` holds the subjects of every merge commit in the range;
    `first_parent` the subjects of the non-merge commits on the target's
    first-parent history (where a squash merge lands).
    """
    numbers: set[int] = set()
    for subject in merges:
        match = _MERGE_RE.match(subject)
        if match:
            numbers.add(int(match.group(1)))
    for subject in first_parent:
        match = _SQUASH_RE.search(subject)
        if match:
            numbers.add(int(match.group(1)))
    return sorted(numbers)


def _escape_title(title: str) -> str:
    return title.replace("<", "&lt;").replace(">", "&gt;").strip()


def render(
    prs: list[PullRequest],
    repo: str,
    previous_tag: str,
    target_sha: str,
    summary: str | None = None,
    header: str | None = None,
) -> str:
    out: list[str] = []
    if header:
        out += [header, ""]
    if summary and summary.strip():
        out += ["## What's new", "", summary.strip(), ""]
    groups = group_notes(prs)
    noted = sum(len(items) for items in groups.values())
    for name in GROUPS:
        if not groups[name]:
            continue
        out += [f"## {name}", ""]
        out += [f"- {text} (#{number})" for text, number in groups[name]]
        out.append("")
    if noted == 0 and not (summary and summary.strip()):
        out += ["No release notes were written for these changes; the full list is below.", ""]
    ordered = sorted(prs, key=lambda p: p.number)
    out.append(f"<details><summary>All {len(ordered)} pull requests</summary>")
    out.append("")
    out += [f"- {_escape_title(pr.title)} (#{pr.number})" for pr in ordered]
    out += ["", "</details>", ""]
    out.append(
        f"**Full diff:** https://github.com/{repo}/compare/{previous_tag}...{target_sha}"
    )
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------------------
# git / GitHub
# ---------------------------------------------------------------------------


def _run(args: list[str], check: bool = True) -> subprocess.CompletedProcess:
    result = subprocess.run(
        args, capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if check and result.returncode != 0:
        raise SystemExit(f"{' '.join(args[:3])} failed: {result.stderr.strip()}")
    return result


def git(*args: str) -> str:
    return _run(["git", *args]).stdout


def remote_tags(remote: str) -> list[str]:
    tags = []
    for line in git("ls-remote", "--tags", "--refs", remote).splitlines():
        ref = line.split("\t", 1)[-1]
        if ref.startswith("refs/tags/"):
            tags.append(ref[len("refs/tags/"):])
    return tags


def default_repo(remote: str) -> str:
    env = os.environ.get("GITHUB_REPOSITORY")
    if env:
        return env
    url = git("remote", "get-url", remote).strip()
    match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?/?$", url)
    if not match:
        raise SystemExit(f"cannot tell the GitHub repo from {remote}'s URL {url}; pass --repo")
    return match.group(1)


def range_subjects(previous_tag: str, target: str) -> tuple[list[str], list[str]]:
    span = f"{previous_tag}..{target}"
    merges = git("log", "--merges", "--format=%s", span).splitlines()
    first_parent = git("log", "--first-parent", "--no-merges", "--format=%s", span).splitlines()
    return merges, first_parent


_PR_FIELDS = (
    "number title body state isCrossRepository headRefName baseRefName "
    "labels(first: 30) { nodes { name } }"
)


def fetch_prs(repo: str, numbers: list[int], chunk: int = 50) -> list[PullRequest]:
    """Merged PRs among `numbers`; release plumbing (dev<->main) is dropped."""
    owner, name = repo.split("/", 1)
    prs: list[PullRequest] = []
    for start in range(0, len(numbers), chunk):
        part = numbers[start:start + chunk]
        fields = " ".join(f"pr{n}: pullRequest(number: {n}) {{ {_PR_FIELDS} }}" for n in part)
        query = f"query($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) {{ {fields} }} }}"
        result = _run(
            ["gh", "api", "graphql", "-f", f"query={query}", "-f", f"owner={owner}", "-f", f"name={name}"],
            check=False,
        )
        try:
            data = json.loads(result.stdout)
        except json.JSONDecodeError:
            raise SystemExit(f"gh api graphql failed: {result.stderr.strip() or result.stdout.strip()}")
        repository = (data.get("data") or {}).get("repository")
        if repository is None:
            raise SystemExit(f"gh api graphql returned no repository data: {result.stderr.strip()}")
        for n in part:
            node = repository.get(f"pr{n}")
            if node is None:
                # A squash subject's `(#N)` can name an issue, not a PR.
                print(f"note: #{n} is not a pull request; skipped", file=sys.stderr)
                continue
            if node["state"] != "MERGED":
                continue
            plumbing = (
                not node["isCrossRepository"]
                and node["headRefName"] in ("dev", "main")
                and node["baseRefName"] in ("dev", "main")
            )
            if plumbing:
                continue
            prs.append(
                PullRequest(
                    number=node["number"],
                    title=node["title"],
                    body=node.get("body") or "",
                    labels=[label["name"] for label in node["labels"]["nodes"]],
                )
            )
    return prs


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument("--from", dest="previous", help="previous tag (default: auto, see --before)")
    parser.add_argument("--before", help="auto-pick the highest stable tag below this version")
    parser.add_argument("--target", default="HEAD", help="commit being released (default HEAD)")
    parser.add_argument("--version", help="version being released, for .github/release_notes/<version>.md")
    parser.add_argument("--header", help="line printed above the notes")
    parser.add_argument("--repo", help="owner/name (default: GITHUB_REPOSITORY, else the remote's URL)")
    parser.add_argument("--remote", default="origin", help="remote whose tags count (default origin)")
    parser.add_argument("--output", help="write here (UTF-8) instead of stdout")
    args = parser.parse_args(argv)

    previous = args.previous
    if not previous:
        previous = pick_previous_tag(remote_tags(args.remote), args.before)
        if not previous:
            raise SystemExit(f"no stable vX.Y.Z tag on {args.remote} to compare from; pass --from")
    target_sha = git("rev-parse", "--verify", f"{args.target}^{{commit}}").strip()
    repo = args.repo or default_repo(args.remote)

    merges, first_parent = range_subjects(previous, target_sha)
    prs, supersede_log = apply_supersedes(fetch_prs(repo, find_pr_numbers(merges, first_parent)))
    for line in supersede_log:
        print(f"release notes: {line}", file=sys.stderr)

    summary = None
    version = summary_version(args.version)
    if version:
        path = Path(__file__).resolve().parent.parent / "release_notes" / f"{version}.md"
        if path.is_file():
            summary = path.read_text(encoding="utf-8")

    text = render(prs, repo, previous, target_sha, summary=summary, header=args.header)
    noted = sum(len(v) for v in group_notes(prs).values())
    dropped = sum(" dropped: " in line for line in supersede_log)
    print(
        f"release notes: {previous}..{target_sha[:10]}: {len(prs)} PRs, {noted} with a release note"
        + (f", summary {version}.md" if summary else "")
        + (f", {dropped} superseded" if dropped else ""),
        file=sys.stderr,
    )
    if args.output:
        Path(args.output).write_text(text, encoding="utf-8", newline="\n")
    else:
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
