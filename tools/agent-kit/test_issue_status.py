import unittest

import issue_status as st

US, RS = st.FIELD_SEP, st.COMMIT_SEP


def issue(number=900, title="Kill-feed icons", labels=("area:hd",), body="", comments=()):
    return {"number": number, "title": title, "state": "OPEN", "url": None,
            "labels": [{"name": n} for n in labels], "body": body,
            "comments": [{"author": {"login": "a"}, "createdAt": "t", "body": b} for b in comments]}


def pr(number, title="", body="", merged=True, closes=()):
    return {"number": number, "title": title, "body": body, "state": "MERGED" if merged else "OPEN",
            "mergedAt": "2026-10-24" if merged else None, "baseRefName": "dev", "headRefName": f"b-{number}",
            "closingIssuesReferences": [{"number": n} for n in closes]}


class CiteTests(unittest.TestCase):
    def test_a_whole_number_is_a_cite(self):
        self.assertTrue(st.cites("Round 4 (#900; VERSION 11)", 900))
        self.assertTrue(st.cites("#900", 900))

    def test_a_longer_number_or_an_html_entity_is_not(self):
        self.assertFalse(st.cites("#9001 and #1900", 900))
        self.assertFalse(st.cites("&#900;", 900))
        self.assertFalse(st.cites("issue900", 900))

    def test_prs_cite_by_title_body_or_closing_reference(self):
        prs = [pr(1, title="x (#45)"), pr(2, body="Part of #45."), pr(3, closes=[45]), pr(4, body="#450")]
        self.assertEqual([p["number"] for p in st.citing_prs(45, prs)], [3, 2, 1])
        self.assertTrue(st.citing_prs(45, prs)[0]["closes"])


class LogTests(unittest.TestCase):
    LOG = (f"aaa{US}Kill-feed icons: wire guards (VERSION 11){US}Part of #900.\n{RS}\n"
           f"bbb{US}Other (#841){US}{RS}\n")

    def test_the_log_is_split_into_commits_with_their_cites(self):
        commits = st.parse_log(self.LOG)
        self.assertEqual([c["sha"] for c in commits], ["aaa", "bbb"])
        self.assertEqual(commits[0]["refs"], {900})
        self.assertEqual([c["sha"] for c in st.citing_commits(841, commits)], ["bbb"])

    def test_a_merge_subject_names_its_pr(self):
        self.assertEqual(st.merge_pr_number("Merge pull request #860 from ccoventry/feat/x"), 860)
        self.assertIsNone(st.merge_pr_number("Merge branch 'dev' into feat/x"))


class RecordTests(unittest.TestCase):
    def record(self, prs, merge_of, open_files=None, the_issue=None):
        commits = st.parse_log(f"aaa{US}Kill-feed icons: wire guards{US}Part of #900.{RS}")
        return st.status_record(the_issue or issue(body="See `goldsrc-hooks/tools/hd/killfeed_icons.py`."),
                                prs, ["feat/icons-900", "feat/icons-9000"], commits, merge_of,
                                open_files or {}, {})

    def test_work_merged_in_a_pr_made_for_another_issue_is_flagged(self):
        prs = [pr(860, title="Kill-feed icons from the models (#841)", body="Round 4 (#900)", closes=[841])]
        rec = self.record(prs, {"aaa": (860, "m1")})
        self.assertEqual(rec["shipped_under_other"], [{"pr": 860, "made_for": [841]}])
        self.assertIn("shipped under another number: the PR is for #841", st.to_markdown(rec))

    def test_work_merged_in_its_own_pr_is_not_flagged(self):
        prs = [pr(901, title="Guards (#900)", closes=[900])]
        rec = self.record(prs, {"aaa": (901, "m1")})
        self.assertEqual(rec["shipped_under_other"], [])

    def test_branches_ending_in_the_number_and_open_prs_on_its_files_are_listed(self):
        rec = self.record([], {"aaa": (None, None)},
                          {1294: ["goldsrc-hooks/tools/hd/killfeed_icons.py"], 7: ["other.py"]})
        self.assertEqual(rec["branches"], ["feat/icons-900"])
        self.assertEqual(rec["open_prs_touching"], {1294: ["goldsrc-hooks/tools/hd/killfeed_icons.py"]})

    def test_the_table_row_names_the_other_issue(self):
        prs = [pr(860, title="Icons (#841)", body="#900", closes=[841])]
        row = st.table_row(self.record(prs, {"aaa": (860, "m1")}))
        self.assertIn("#860(for #841)", row)


class NamedFileTests(unittest.TestCase):
    def test_paths_and_file_names_are_found_once_and_versions_are_not(self):
        texts = ["Edit `studio/src/strings.js` and strings.js, cfg_scan.rs too; version 0.10.0 ships.",
                 "Also docs/coding_standards.md."]
        self.assertEqual(st.named_files(texts),
                         ["studio/src/strings.js", "strings.js", "cfg_scan.rs", "docs/coding_standards.md"])


class LintTests(unittest.TestCase):
    def test_a_clean_issue_has_no_problems(self):
        self.assertEqual(st.lint(issue(), set()), [])

    def test_on_dev_needs_a_merged_pr_or_commit(self):
        self.assertEqual(st.lint(issue(labels=("area:hd", "on-dev")), set()),
                         ["on-dev, but no merged PR cites it and no dev commit does"])
        self.assertEqual(st.lint(issue(labels=("area:hd", "on-dev")), {860}), [])

    def test_ready_with_needs_decision_and_a_missing_area_are_flagged(self):
        problems = st.lint(issue(labels=("ready", "needs-decision")), set())
        self.assertEqual(problems, ["both ready and needs-decision", "no area: label"])

    def test_area_prefixes_and_old_console_names_in_titles_are_flagged(self):
        problems = st.lint(issue(title="[Capture Studio] dodtools_seek and dodstudio_hide_hud"), set())
        self.assertEqual(len(problems), 3)
        self.assertIn("[Capture Studio]", problems[0])
        for ok in ("[R&D] x", "[Chore] x", "[Spec] x"):
            self.assertEqual(st.lint(issue(title=ok), set()), [])


class ForTests(unittest.TestCase):
    def test_a_pr_is_for_its_closing_refs_and_title_numbers(self):
        self.assertEqual(st.issues_pr_is_for(pr(1, title="x (part of #1085)", body="#900", closes=[841])),
                         {841, 1085})
        self.assertEqual(st.for_note([841], 900), ", for #841")
        self.assertEqual(st.for_note([900], 900), "")


if __name__ == "__main__":
    unittest.main()
