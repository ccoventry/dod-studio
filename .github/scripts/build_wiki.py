"""Builds the GitHub wiki's pages from reviewed files in this repo (#355).

Usage: python .github/scripts/build_wiki.py <out-dir>

- Every page in docs/wiki/ is copied as-is, except README.md, which explains
  the folder to people reading the repo.
- Commands.md is generated from docs/dodstudio_commands.md, the source of
  truth for the hook DLL's console surface, so the two can't drift. Its
  relative links (to other docs and source files) become absolute links into
  the repo on main, since the wiki can't resolve them.

The sync workflow (.github/workflows/sync_wiki.yml) runs this on every push to
main that touches either source, then commits the result to the wiki repo.
"""

import posixpath
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
WIKI_SOURCE = REPO_ROOT / "docs" / "wiki"
COMMANDS_SOURCE = REPO_ROOT / "docs" / "dodstudio_commands.md"
BLOB_BASE = "https://github.com/ccoventry/dod-studio/blob/main/"

# A markdown link's target: `](target)` or `](target "title")`.
LINK = re.compile(r"\]\(([^)\s]+)((?:\s+\"[^\"]*\")?)\)")


def absolutize_links(markdown: str, source_dir: str) -> str:
    """Rewrites relative link targets (resolved against `source_dir`, a
    repo-relative folder) to absolute blob URLs on main. Absolute URLs,
    in-page anchors and mail links are left alone."""

    def rewrite(match: re.Match) -> str:
        target, title = match.group(1), match.group(2)
        if re.match(r"^(?:[a-z][a-z0-9+.-]*:|#)", target, re.IGNORECASE):
            return match.group(0)
        path, _, anchor = target.partition("#")
        resolved = posixpath.normpath(posixpath.join(source_dir, path))
        url = BLOB_BASE + resolved + (f"#{anchor}" if anchor else "")
        return f"]({url}{title})"

    return LINK.sub(rewrite, markdown)


def commands_page() -> str:
    body = COMMANDS_SOURCE.read_text(encoding="utf-8")
    note = (
        "> Generated from [`docs/dodstudio_commands.md`]"
        f"({BLOB_BASE}docs/dodstudio_commands.md) on every push to `main`. "
        "Edit that file, not this page.\n\n"
    )
    return note + absolutize_links(body, "docs")


def build(out_dir: Path) -> list[str]:
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for page in sorted(WIKI_SOURCE.glob("*.md")):
        if page.name.lower() == "readme.md":
            continue
        (out_dir / page.name).write_text(page.read_text(encoding="utf-8"), encoding="utf-8")
        written.append(page.name)
    (out_dir / "Commands.md").write_text(commands_page(), encoding="utf-8")
    written.append("Commands.md")
    return written


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    for name in build(Path(sys.argv[1])):
        print(name)
