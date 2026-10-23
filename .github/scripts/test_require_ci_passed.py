"""Tests for require_ci_passed.py's verdicts. No network.

    python -m unittest discover -s .github/scripts -p "test_*.py"
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from require_ci_passed import (  # noqa: E402
    Required,
    Run,
    evaluate,
    outcome,
    required_checks,
)

ACTIONS = 15368
MERGE = "a" * 40  # main's merge commit: no CI of its own
HEAD = "b" * 40  # the dev tip it merged, same tree


def run(name, sha=HEAD, run_id=1, status="completed", conclusion="success", app_id=ACTIONS):
    return Run(name, app_id, run_id, status, conclusion, f"https://x/{run_id}", sha)


def states(verdicts):
    return {v.name: v.state for v in verdicts}


class RequiredChecks(unittest.TestCase):
    def test_reads_contexts_from_rules(self):
        rules = [
            {"type": "deletion"},
            {
                "type": "required_status_checks",
                "parameters": {
                    "required_status_checks": [
                        {"context": "Clippy", "integration_id": ACTIONS},
                        {"context": "Hooks 32-bit"},
                    ]
                },
            },
        ]
        self.assertEqual(
            required_checks(rules),
            [Required("Clippy", ACTIONS), Required("Hooks 32-bit", None)],
        )

    def test_two_rulesets_naming_one_check_count_once(self):
        rule = {
            "type": "required_status_checks",
            "parameters": {"required_status_checks": [{"context": "Clippy", "integration_id": ACTIONS}]},
        }
        self.assertEqual(len(required_checks([rule, rule])), 1)


class Evaluate(unittest.TestCase):
    req = [Required("Clippy", ACTIONS), Required("Rust workspace tests", ACTIONS)]

    def test_runs_on_a_same_tree_commit_count(self):
        v = evaluate(self.req, [run("Clippy"), run("Rust workspace tests", run_id=2)])
        self.assertEqual(outcome(v), "pass")

    def test_missing_check_fails(self):
        v = evaluate(self.req, [run("Clippy")])
        self.assertEqual(states(v)["Rust workspace tests"], "missing")
        self.assertEqual(outcome(v), "fail")

    def test_failed_check_fails(self):
        v = evaluate(self.req, [run("Clippy"), run("Rust workspace tests", run_id=2, conclusion="failure")])
        self.assertEqual(states(v)["Rust workspace tests"], "fail")
        self.assertEqual(outcome(v), "fail")

    def test_rerun_replaces_the_earlier_run(self):
        runs = [
            run("Clippy"),
            run("Rust workspace tests", run_id=2, conclusion="failure"),
            run("Rust workspace tests", run_id=3),
        ]
        self.assertEqual(outcome(evaluate(self.req, runs)), "pass")

    def test_later_failure_replaces_an_earlier_pass(self):
        runs = [
            run("Clippy"),
            run("Rust workspace tests", run_id=3, conclusion="failure"),
            run("Rust workspace tests", run_id=2),
        ]
        self.assertEqual(outcome(evaluate(self.req, runs)), "fail")

    def test_pass_on_pr_head_counts_while_push_run_is_going(self):
        runs = [
            run("Clippy"),
            run("Rust workspace tests", sha=HEAD, run_id=2),
            run("Rust workspace tests", sha=MERGE, run_id=5, status="in_progress", conclusion=None),
        ]
        self.assertEqual(outcome(evaluate(self.req, runs)), "pass")

    def test_running_check_waits(self):
        runs = [run("Clippy"), run("Rust workspace tests", run_id=2, status="queued", conclusion=None)]
        self.assertEqual(outcome(evaluate(self.req, runs)), "wait")

    def test_waits_for_a_missing_check_while_another_runs(self):
        runs = [run("Clippy", status="in_progress", conclusion=None)]
        self.assertEqual(outcome(evaluate(self.req, runs)), "wait")

    def test_skipped_and_neutral_pass(self):
        runs = [run("Clippy", conclusion="skipped"), run("Rust workspace tests", run_id=2, conclusion="neutral")]
        self.assertEqual(outcome(evaluate(self.req, runs)), "pass")

    def test_cancelled_fails(self):
        runs = [run("Clippy"), run("Rust workspace tests", run_id=2, conclusion="cancelled")]
        self.assertEqual(outcome(evaluate(self.req, runs)), "fail")

    def test_same_name_from_another_app_does_not_count(self):
        runs = [run("Clippy", app_id=999), run("Rust workspace tests", run_id=2)]
        self.assertEqual(states(evaluate(self.req, runs))["Clippy"], "missing")


if __name__ == "__main__":
    unittest.main()
