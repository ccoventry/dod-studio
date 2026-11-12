import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

import pr_board as pb

BODY = """Release note: none

Part of #1085.

## Test done
- `npm run test:unit`: 1527 passed.
- Not tested: narrow windows.

## For the user
Pick a colour for the chip.
"""


def run(name, conclusion="SUCCESS", status="COMPLETED"):
    return {"__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion}


class CiTests(unittest.TestCase):
    def test_all_passing_checks_pass(self):
        self.assertEqual(pb.ci_state([run("a"), run("b", "SKIPPED")]), "pass")

    def test_a_failed_check_is_named(self):
        self.assertEqual(pb.ci_state([run("a"), run("Clippy", "FAILURE")]), "FAIL(Clippy)")

    def test_a_running_check_is_running(self):
        self.assertEqual(pb.ci_state([run("a"), run("b", "", "IN_PROGRESS")]), "running")

    def test_no_checks_is_none(self):
        self.assertEqual(pb.ci_state([]), "none")


class FlagTests(unittest.TestCase):
    def test_behind_conflicts_and_stacked_are_flagged(self):
        pr = {"mergeStateStatus": "DIRTY", "mergeable": "CONFLICTING", "baseRefName": "feat/x-1"}
        self.assertEqual(pb.merge_flags(pr), ["conflicts", "stacked on feat/x-1"])
        self.assertEqual(pb.merge_flags({"mergeStateStatus": "BEHIND", "baseRefName": "dev"}), ["behind"])


class BodyTests(unittest.TestCase):
    def test_the_first_test_done_line_is_read(self):
        test_done, _ = pb.body_notes(BODY)
        self.assertEqual(test_done, "- `npm run test:unit`: 1527 passed.")

    def test_lines_and_sections_for_the_user_are_read(self):
        _, needs = pb.body_notes(BODY)
        self.assertEqual(needs, ["- Not tested: narrow windows.", "Pick a colour for the chip."])

    def test_an_empty_body_has_nothing(self):
        self.assertEqual(pb.body_notes(None), (None, []))


class DllTests(unittest.TestCase):
    def test_a_test_folder_for_the_pr_or_its_issue_with_a_dll_is_found(self):
        with tempfile.TemporaryDirectory() as tmp:
            for name, dll in (("test-1337", True), ("test-1363", False), ("test-13370", True)):
                (Path(tmp) / name).mkdir()
                if dll:
                    (Path(tmp) / name / "dodstudio_goldsrc_hooks.dll").write_bytes(b"")
            pr = {"number": 1363, "title": "Black band on top (#1337)", "closingIssuesReferences": []}
            self.assertEqual(pb.test_dll_folders(pb.related_numbers(pr), tmp), ["test-1337"])


class RowTests(unittest.TestCase):
    def test_a_row_carries_age_and_title(self):
        pr = {"number": 7, "title": "T", "createdAt": "2026-11-01T00:00:00Z", "body": BODY,
              "statusCheckRollup": [], "baseRefName": "dev"}
        row = pb.board_row(pr, "/nonexistent", now=datetime(2026, 11, 11, tzinfo=timezone.utc))
        self.assertEqual((row["age_days"], row["ci"]), (10, "none"))
        self.assertIn("#7", pb.format_row(row))


if __name__ == "__main__":
    unittest.main()
