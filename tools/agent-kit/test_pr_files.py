import unittest
from datetime import datetime, timezone

import pr_files as pf

FILES = {
    1: ["studio/src/strings.js", "studio/src/main.js"],
    2: ["native/src/messages.rs", "studio/src/main.js"],
    3: ["docs/strings.js.md"],
}


class MatchTests(unittest.TestCase):
    def test_a_full_path_a_file_name_and_a_folder_match(self):
        self.assertTrue(pf.matches("studio/src/strings.js", "studio/src/strings.js"))
        self.assertTrue(pf.matches("strings.js", "studio/src/strings.js"))
        self.assertTrue(pf.matches("src/strings.js", "studio/src/strings.js"))
        self.assertTrue(pf.matches("studio/", "studio/src/strings.js"))
        self.assertTrue(pf.matches(".\\studio\\src\\strings.js", "studio/src/strings.js"))

    def test_a_different_file_with_the_name_inside_does_not(self):
        self.assertFalse(pf.matches("strings.js", "docs/strings.js.md"))
        self.assertFalse(pf.matches("rc/strings.js", "studio/src/strings.js"))

    def test_touching_lists_each_pr_and_its_matching_files(self):
        self.assertEqual(pf.touching("main.js", FILES), {1: ["studio/src/main.js"], 2: ["studio/src/main.js"]})


class OverlapTests(unittest.TestCase):
    def test_other_prs_sharing_a_file_are_listed_with_the_files(self):
        self.assertEqual(pf.overlaps(1, FILES), {2: ["studio/src/main.js"]})
        self.assertEqual(pf.overlaps(3, FILES), {})


class StaleTests(unittest.TestCase):
    def test_a_draft_row_carries_counts_and_conflicts(self):
        pr = {"number": 882, "title": "R&D: health", "createdAt": "2026-10-19T00:00:00Z",
              "mergeable": "CONFLICTING", "baseRefName": "dev"}
        row = pf.stale_row(pr, 1606, 3, now=datetime(2026, 11, 10, tzinfo=timezone.utc))
        self.assertEqual((row["age_days"], row["behind"], row["conflicts"]), (22, 1606, True))
        self.assertIn("behind  1606 ahead   3 conflicts", pf.format_stale(row))

    def test_unknown_counts_print_as_question_marks(self):
        pr = {"number": 1, "title": "t", "createdAt": "2026-11-10T00:00:00Z", "baseRefName": "feat/x"}
        line = pf.format_stale(pf.stale_row(pr, None, 0, now=datetime(2026, 11, 10, tzinfo=timezone.utc)))
        self.assertIn("behind     ? ahead   0", line)
        self.assertIn("(on feat/x)", line)


if __name__ == "__main__":
    unittest.main()
