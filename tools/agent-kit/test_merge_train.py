import json
import unittest

import merge_train as mt

REQUIRED = ["Rust workspace tests", "Clippy", "Frontend e2e tests"]


def checks_json(**buckets):
    names = {"rust": "Rust workspace tests", "clippy": "Clippy", "e2e": "Frontend e2e tests",
             "shard": "Frontend e2e shard 1"}
    return json.dumps([{"name": names[k], "bucket": v} for k, v in buckets.items()])


GREEN = checks_json(rust="pass", clippy="pass", e2e="pass")
NO_CHECKS = ("", "no checks reported on the 'feat/x' branch")


class FakeGh:
    """Scripted `gh`: each PR has a queue of check results (stdout, stderr) and how far
    behind dev it is; every mutating call is recorded."""

    def __init__(self, prs, required=REQUIRED):
        self.prs = prs
        for number, p in prs.items():
            p.setdefault("head", f"{number:040d}")
        self.required = required
        self.calls = []

    def required_checks(self):
        return self.required

    def view(self, pr):
        p = self.prs[pr]
        return {"number": pr, "title": f"PR {pr}", "state": p.get("state", "OPEN"),
                "isDraft": p.get("draft", False), "baseRefName": p.get("base", "dev"),
                "mergeable": p.get("mergeable", "MERGEABLE"), "headRefOid": p["head"],
                "autoMergeRequest": None}

    def behind_by(self, head_sha):
        """Pops the PR's next `behind` value; the last one repeats."""
        p = next(p for p in self.prs.values() if p["head"] == head_sha)
        behind = p.setdefault("behind", [0])
        return behind.pop(0) if len(behind) > 1 else behind[0]

    def checks(self, pr):
        queue = self.prs[pr]["checks"]
        out, err = queue.pop(0) if len(queue) > 1 else queue[0]
        return mt.parse_checks(out, err)

    def update_branch(self, pr):
        self.calls.append(("update-branch", pr))
        p = self.prs[pr]
        p["head"] = "f" * 32 + f"{pr:08d}"

    def merge(self, pr, head_sha):
        self.calls.append(("merge", pr, head_sha))
        self.prs[pr]["state"] = "MERGED"

    def disable_auto(self, pr):
        self.calls.append(("disable-auto", pr))


class FakeClock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def sleep(self, secs):
        self.now += secs


def pr(checks, **extra):
    return {"checks": checks, **extra}


def run_train(prs, order, **options):
    gh = FakeGh(prs)
    clock = FakeClock()
    lines = []
    result = mt.Train(gh, mt.Options(interval_secs=30, timeout_secs=600, **options),
                      clock=clock, sleep=clock.sleep, out=lines.append).run(order)
    return gh, result, lines


class ParseChecksTests(unittest.TestCase):
    def test_no_checks_reported_is_an_empty_list_not_an_error(self):
        self.assertEqual(mt.parse_checks(*NO_CHECKS), {})

    def test_buckets_by_name(self):
        self.assertEqual(mt.parse_checks(checks_json(rust="pass", clippy="pending"), ""),
                         {"Rust workspace tests": "pass", "Clippy": "pending"})

    def test_a_red_run_beside_a_green_rerun_stays_red(self):
        rows = json.dumps([{"name": "Clippy", "bucket": "fail"}, {"name": "Clippy", "bucket": "pass"}])
        self.assertEqual(mt.parse_checks(rows, ""), {"Clippy": "fail"})

    def test_output_that_is_not_json_raises(self):
        with self.assertRaises(mt.ToolError):
            mt.parse_checks("HTTP 502", "")


class VerdictTests(unittest.TestCase):
    def test_all_required_green_passes_and_other_checks_do_not_matter(self):
        checks = mt.parse_checks(checks_json(rust="pass", clippy="pass", e2e="pass", shard="pending"), "")
        self.assertEqual(mt.checks_verdict(REQUIRED, checks), (mt.PASS, []))

    def test_a_skipped_required_check_counts_as_green(self):
        checks = mt.parse_checks(checks_json(rust="skipping", clippy="pass", e2e="pass"), "")
        self.assertEqual(mt.checks_verdict(REQUIRED, checks)[0], mt.PASS)

    def test_red_wins_over_pending(self):
        checks = mt.parse_checks(checks_json(rust="pending", clippy="fail"), "")
        self.assertEqual(mt.checks_verdict(REQUIRED, checks), (mt.FAIL, ["Clippy"]))

    def test_cancelled_is_red(self):
        checks = mt.parse_checks(checks_json(rust="cancel", clippy="pass", e2e="pass"), "")
        self.assertEqual(mt.checks_verdict(REQUIRED, checks), (mt.FAIL, ["Rust workspace tests"]))

    def test_a_missing_required_check_is_pending(self):
        checks = mt.parse_checks(checks_json(rust="pass", clippy="pass"), "")
        self.assertEqual(mt.checks_verdict(REQUIRED, checks), (mt.PENDING, ["Frontend e2e tests"]))


class ProblemTests(unittest.TestCase):
    def base(self, **over):
        return {"state": "OPEN", "isDraft": False, "baseRefName": "dev", "mergeable": "MERGEABLE", **over}

    def test_open_ready_pr_into_dev_is_fine(self):
        self.assertIsNone(mt.pr_problem(self.base()))

    def test_refusals(self):
        self.assertIn("draft", mt.pr_problem(self.base(isDraft=True)))
        self.assertIn("main", mt.pr_problem(self.base(baseRefName="main")))
        self.assertIn("closed", mt.pr_problem(self.base(state="CLOSED")))
        self.assertIn("conflicts", mt.pr_problem(self.base(mergeable="CONFLICTING")))

    def test_unknown_mergeability_is_not_a_conflict(self):
        self.assertIsNone(mt.pr_problem(self.base(mergeable="UNKNOWN")))


class TrainTests(unittest.TestCase):
    def test_green_prs_merge_in_order(self):
        gh, result, _ = run_train({1: pr([(GREEN, "")]), 2: pr([(GREEN, "")])}, [1, 2])
        self.assertEqual(result.merged, [1, 2])
        self.assertIsNone(result.stopped_at)
        self.assertEqual([c[:2] for c in gh.calls], [("merge", 1), ("merge", 2)])

    def test_merge_pins_the_checked_head_commit(self):
        gh, _, _ = run_train({1: pr([(GREEN, "")])}, [1])
        self.assertEqual(gh.calls, [("merge", 1, f"{1:040d}")])

    def test_red_check_stops_the_train_and_later_prs_are_untouched(self):
        red = checks_json(rust="pass", clippy="fail", e2e="pass")
        gh, result, lines = run_train({1: pr([(GREEN, "")]), 2: pr([(red, "")]), 3: pr([(GREEN, "")])},
                                      [1, 2, 3])
        self.assertEqual(result.merged, [1])
        self.assertEqual((result.stopped_at, result.untouched), (2, [3]))
        self.assertIn("Clippy", result.reason)
        self.assertNotIn(3, [c[1] for c in gh.calls])
        self.assertTrue(any("#2 STOP" in line and "Clippy" in line for line in lines))

    def test_behind_pr_is_updated_and_waits_through_no_checks_reported(self):
        pending = checks_json(rust="pending", clippy="pass", e2e="pending")
        prs = {1: pr([NO_CHECKS, NO_CHECKS, (pending, ""), (GREEN, "")], behind=[4, 0])}
        gh, result, lines = run_train(prs, [1])
        self.assertEqual(result.merged, [1])
        self.assertEqual(gh.calls[0], ("update-branch", 1))
        self.assertEqual(gh.calls[1][:2], ("merge", 1))
        self.assertTrue(gh.calls[1][2].startswith("f"), "merges the updated head, not the old one")
        self.assertTrue(any("no checks reported yet" in line for line in lines))

    def test_timeout_stops_without_merging(self):
        pending = checks_json(rust="pass", clippy="pending", e2e="pass")
        gh, result, _ = run_train({1: pr([(pending, "")]), 2: pr([(GREEN, "")])}, [1, 2])
        self.assertEqual((result.stopped_at, result.untouched), (1, [2]))
        self.assertIn("timed out", result.reason)
        self.assertIn("Clippy", result.reason)
        self.assertEqual(gh.calls, [])

    def test_draft_and_conflict_stop_before_anything_runs(self):
        for extra, word in (({"draft": True}, "draft"), ({"mergeable": "CONFLICTING"}, "conflicts"),
                            ({"base": "main"}, "main")):
            gh, result, _ = run_train({1: pr([(GREEN, "")], **extra)}, [1])
            self.assertEqual(result.stopped_at, 1)
            self.assertIn(word, result.reason)
            self.assertEqual(gh.calls, [])

    def test_base_moving_during_the_checks_updates_again(self):
        prs = {1: pr([(GREEN, "")], behind=[0, 2, 2, 0])}
        gh, result, _ = run_train(prs, [1])
        self.assertEqual(result.merged, [1])
        self.assertEqual([c[0] for c in gh.calls], ["update-branch", "merge"])

    def test_dry_run_changes_nothing(self):
        prs = {1: pr([(GREEN, "")], behind=[3]), 2: pr([(GREEN, "")])}
        gh, result, lines = run_train(prs, [1, 2], dry_run=True)
        self.assertEqual(gh.calls, [])
        self.assertTrue(any("would update-branch" in line for line in lines))
        self.assertTrue(any("would merge now" in line for line in lines))

    def test_summary_names_the_stop(self):
        result = mt.Result(merged=[1], stopped_at=2, reason="red required check: Clippy", untouched=[3])
        self.assertEqual(mt.summary([1, 2, 3], result),
                         ["Merged (1/3): #1", "Stopped at #2: red required check: Clippy", "Not touched: #3"])


if __name__ == "__main__":
    unittest.main()
