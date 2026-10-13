"""Tests for release_notes.py's parsing and grouping. No network, no git.

    python -m unittest discover -s .github/scripts -p "test_*.py"
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from release_notes import (  # noqa: E402
    PullRequest,
    classify,
    extract_release_note,
    find_pr_numbers,
    group_notes,
    pick_previous_tag,
    render,
    summary_version,
)

TEMPLATE_UNFILLED = """<!--
Release note: one plain-language line for players, or "none" for internal work.
-->
Release note: <one plain-language line for players, or "none">

## Summary
"""


class ExtractReleaseNote(unittest.TestCase):
    def test_plain_line(self):
        body = "Release note: Fixed: The game no longer crashes on the first frame.\n\n## Cause\n..."
        self.assertEqual(
            extract_release_note(body), "Fixed: The game no longer crashes on the first frame."
        )

    def test_case_insensitive_and_crlf(self):
        self.assertEqual(extract_release_note("intro\r\nRELEASE NOTE:  New: Split demos\r\n"), "New: Split demos")

    def test_bold_and_bullet_forms(self):
        self.assertEqual(extract_release_note("**Release note:** Faster scans"), "Faster scans")
        self.assertEqual(extract_release_note("**Release note**: Faster scans"), "Faster scans")
        self.assertEqual(extract_release_note("- Release notes: Faster scans"), "Faster scans")

    def test_first_match_wins(self):
        body = "Release note: first\nRelease note: second"
        self.assertEqual(extract_release_note(body), "first")

    def test_none_and_empty_are_skipped(self):
        for value in ("none", "None.", "NONE", "n/a", "", "   ", "`none`"):
            with self.subTest(value=value):
                self.assertIsNone(extract_release_note(f"Release note: {value}\nmore"))

    def test_unfilled_template_is_skipped(self):
        self.assertIsNone(extract_release_note(TEMPLATE_UNFILLED))

    def test_repo_pr_template_unfilled_is_skipped(self):
        template = Path(__file__).resolve().parent.parent / "pull_request_template.md"
        body = template.read_text(encoding="utf-8")
        self.assertIsNone(extract_release_note(body))
        filled = body.replace('<one plain-language line for players, or "none">', "New: Split demos")
        self.assertEqual(extract_release_note(filled), "New: Split demos")

    def test_comment_is_ignored_but_filled_line_counts(self):
        body = TEMPLATE_UNFILLED.replace(
            'Release note: <one plain-language line for players, or "none">',
            "Release note: Changed: Settings open faster",
        )
        self.assertEqual(extract_release_note(body), "Changed: Settings open faster")

    def test_missing_line_or_body(self):
        self.assertIsNone(extract_release_note(None))
        self.assertIsNone(extract_release_note(""))
        self.assertIsNone(extract_release_note("## Summary\nNo note here."))

    def test_mid_line_mention_and_no_colon_do_not_count(self):
        self.assertIsNone(extract_release_note("Adds a release note: section to the template"))
        self.assertIsNone(extract_release_note("Release notes are built by a script"))


class Classify(unittest.TestCase):
    def test_prefixes(self):
        self.assertEqual(classify("New: Highlights tab"), ("New", "Highlights tab"))
        self.assertEqual(classify("fixed: crash on exit"), ("Fixed", "crash on exit"))
        self.assertEqual(classify("Changed:Default FOV is 90"), ("Changed", "Default FOV is 90"))

    def test_default_is_changed(self):
        self.assertEqual(classify("Default FOV is 90"), ("Changed", "Default FOV is 90"))

    def test_labels_imply_group(self):
        self.assertEqual(classify("Crash on exit", ["bug"]), ("Fixed", "Crash on exit"))
        self.assertEqual(classify("Highlights tab", ["Enhancement"]), ("New", "Highlights tab"))

    def test_prefix_beats_label(self):
        self.assertEqual(classify("Changed: Crash dialog wording", ["bug"]), ("Changed", "Crash dialog wording"))


class Grouping(unittest.TestCase):
    def test_groups_sorted_and_skips(self):
        prs = [
            PullRequest(12, "b", "Release note: Fixed: B"),
            PullRequest(3, "a", "Release note: New: A"),
            PullRequest(7, "c", "Release note: none"),
            PullRequest(9, "d", "no note at all"),
            PullRequest(5, "e", "Release note: E", ["bug"]),
            PullRequest(4, "f", "Release note: F"),
        ]
        groups = group_notes(prs)
        self.assertEqual(groups["New"], [("A", 3)])
        self.assertEqual(groups["Fixed"], [("E", 5), ("B", 12)])
        self.assertEqual(groups["Changed"], [("F", 4)])

    def test_render(self):
        prs = [
            PullRequest(2, "Fix <crash>", "Release note: Fixed: No crash"),
            PullRequest(1, "Internal refactor", "Release note: none"),
        ]
        text = render(prs, "o/r", "v0.1.4", "abc123", summary="Big release.\n", header="Built from abc.")
        self.assertTrue(text.startswith("Built from abc.\n\n## What's new\n\nBig release.\n"))
        self.assertIn("## Fixed\n\n- No crash (#2)\n", text)
        self.assertNotIn("## New", text)
        self.assertNotIn("## Changed", text)
        self.assertIn("<details><summary>All 2 pull requests</summary>", text)
        self.assertLess(text.index("Internal refactor (#1)"), text.index("Fix &lt;crash&gt; (#2)"))
        self.assertIn("https://github.com/o/r/compare/v0.1.4...abc123", text)

    def test_render_without_notes(self):
        text = render([PullRequest(1, "x")], "o/r", "v0.1.4", "abc")
        self.assertIn("No release notes were written", text)
        self.assertNotIn("What's new", text)


class Ranges(unittest.TestCase):
    def test_find_pr_numbers(self):
        merges = [
            "Merge pull request #746 from ccoventry/fix/hook-crash-first-frame",
            "Merge remote-tracking branch 'origin/dev' into feat/x",
            "Merge branch 'dev' into feat/keep-game-behind",
            "Merge pull request #12 from someone/dev",
        ]
        first_parent = ["Add split button (#700)", "Fix typo in docs", "Mention #5 in passing"]
        self.assertEqual(find_pr_numbers(merges, first_parent), [12, 700, 746])

    def test_pick_previous_tag(self):
        tags = ["v0.1.3", "v0.1.4", "experimental-latest", "v0.1.10-rc", "v0.1.2", "v0.0.9"]
        self.assertEqual(pick_previous_tag(tags), "v0.1.4")
        self.assertEqual(pick_previous_tag(tags, before="0.1.4"), "v0.1.3")
        self.assertEqual(pick_previous_tag(tags, before="0.2.0"), "v0.1.4")
        self.assertEqual(pick_previous_tag(["v0.1.9", "v0.1.10"]), "v0.1.10")
        self.assertIsNone(pick_previous_tag(["experimental-latest"]))

    def test_summary_version(self):
        self.assertEqual(summary_version("0.2.0-57"), "0.2.0")
        self.assertEqual(summary_version("v0.2.0"), "0.2.0")
        self.assertEqual(summary_version("0.2.0"), "0.2.0")
        self.assertIsNone(summary_version(""))
        self.assertIsNone(summary_version(None))


if __name__ == "__main__":
    unittest.main()
