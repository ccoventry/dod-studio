import contextlib
import io
import tempfile
import unittest
from pathlib import Path

import merge_queue
import merge_train as mt
from test_merge_train import GREEN, FakeGh, checks_json, pr


class QueueTrainTests(unittest.TestCase):
    """The train against a queue on disk (a temp folder), with `add` called mid-run."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.folder = tmp.name
        self.store = merge_queue.QueueStore(self.folder, pid=lambda: 100, alive=lambda pid: pid == 100)

    def other(self, pid=200):
        """A second process's view of the same folder (process 100 is alive)."""
        return merge_queue.QueueStore(self.folder, pid=lambda: pid, alive=lambda p: p in (100, pid))

    def main(self, argv, prs):
        gh = FakeGh(prs)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = mt.main(argv, store=self.store, gh=gh)
        return gh, code, out.getvalue()

    def train(self, gh):
        return mt.Train(gh, mt.Options(interval_secs=1, timeout_secs=60), out=lambda _line: None)

    def test_a_pr_added_while_the_train_runs_is_merged_after_the_current_ones(self):
        prs = {1: pr([(GREEN, "")]), 2: pr([(GREEN, "")]), 3: pr([(GREEN, "")])}
        late = self.other()
        gh = FakeGh(prs)
        gh.after_merge = lambda n: late.add([3]) if n == 1 else None
        self.store.acquire([1, 2])
        result = self.train(gh).run(self.store)
        self.assertEqual(result.merged, [1, 2, 3])
        self.assertEqual([c[1] for c in gh.calls], [1, 2, 3])

    def test_a_pr_added_while_the_last_one_merges_is_not_lost(self):
        prs = {1: pr([(GREEN, "")]), 2: pr([(GREEN, "")])}
        late = self.other()
        gh = FakeGh(prs)
        gh.after_merge = lambda n: late.add([2])
        self.store.acquire([1])
        self.assertEqual(self.train(gh).run(self.store).merged, [1, 2])

    def test_add_after_the_train_ended_reports_no_train_and_starts_nothing(self):
        self.store.acquire([1])
        self.store.take()
        self.store.merged(1)
        self.assertIsNone(self.store.take())  # empty: the lock is released in the same step
        self.assertEqual(self.other().add([5]), ([5], [], None))
        self.assertEqual(self.store.snapshot()[0]["pending"], [5])

    def test_duplicates_are_ignored(self):
        self.store.acquire([1, 2])
        added, ignored, _ = self.other().add([2, 3, 3, 4])
        self.assertEqual((added, ignored), ([3, 4], [2, 3]))
        self.assertEqual(self.store.snapshot()[0]["pending"], [1, 2, 3, 4])

    def test_an_already_merged_pr_is_not_queued_again(self):
        self.store.acquire([1])
        self.store.take()
        self.store.merged(1)
        self.assertEqual(self.other().add([1])[:2], ([], [1]))

    def test_stale_lock_from_a_dead_process_is_taken_over(self):
        dead = merge_queue.QueueStore(self.folder, pid=lambda: 999, alive=lambda p: True)
        dead.acquire([7])
        self.assertIsNone(self.store.holder(), "pid 999 is dead as far as this process can tell")
        owned, _added, _ignored = self.store.acquire([8])
        self.assertTrue(owned)
        self.assertEqual(self.store.snapshot()[0]["pending"], [7, 8], "the dead train's queue is kept")
        self.assertEqual(self.store.holder(), 100)

    def test_a_live_lock_is_not_taken_over(self):
        self.store.acquire([1])
        owned, added, _ignored = self.other().acquire([2])
        self.assertEqual((owned, added), (False, [2]))
        self.assertEqual(self.store.holder(), 100)

    def test_a_second_train_joins_the_running_one_instead_of_starting(self):
        self.store.acquire([1])
        gh = FakeGh({2: pr([(GREEN, "")])})
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = mt.main(["2"], store=self.other(), gh=gh)
        self.assertEqual(code, 0)
        self.assertIn("already running", out.getvalue())
        self.assertEqual(gh.calls, [])
        self.assertEqual(self.store.snapshot()[0]["pending"], [1, 2])
        self.assertEqual(self.store.holder(), 100, "the first train still holds the lock")

    def test_stop_keeps_the_queue_and_a_rerun_continues_from_the_stopped_pr(self):
        red = checks_json(rust="pass", clippy="fail", e2e="pass")
        prs = {1: pr([(GREEN, "")]), 2: pr([(red, "")]), 3: pr([(GREEN, "")])}
        _gh, code, _out = self.main(["1", "2", "3"], prs)
        self.assertEqual(code, 1)
        state, holder = self.store.snapshot()
        self.assertIsNone(holder, "the lock is released when the train stops")
        self.assertEqual((state["pending"], state["done"], state["stopped"]["pr"]), ([2, 3], [1], 2))
        self.assertIn("Clippy", state["stopped"]["reason"])
        self.assertIn("Stopped at #2", "\n".join(merge_queue.status_lines(state, holder)))
        prs[2]["checks"] = [(GREEN, "")]
        gh, code, _out = self.main([], prs)
        self.assertEqual(code, 0)
        self.assertEqual([c[1] for c in gh.calls], [2, 3])
        self.assertEqual(self.store.snapshot()[0]["pending"], [])

    def test_merged_and_closed_prs_are_skipped_with_a_line(self):
        prs = {1: pr([(GREEN, "")], state="MERGED"), 2: pr([(GREEN, "")], state="CLOSED"), 3: pr([(GREEN, "")])}
        gh, code, out = self.main(["1", "2", "3"], prs)
        self.assertEqual(code, 0)
        self.assertEqual([c[1] for c in gh.calls], [3])
        self.assertEqual(self.store.snapshot()[0]["skipped"], [1, 2])
        self.assertIn("Skipped", out)

    def test_dry_run_writes_no_queue_and_no_lock(self):
        gh, code, _out = self.main(["1", "--dry-run"], {1: pr([(GREEN, "")], behind=[3])})
        self.assertEqual((code, gh.calls), (0, []))
        self.assertEqual(list(Path(self.folder).iterdir()), [])

    def test_dry_run_with_no_numbers_previews_the_saved_queue(self):
        self.store.acquire([1])
        self.store.release()
        _gh, _code, out = self.main(["--dry-run"], {1: pr([(GREEN, "")])})
        self.assertIn("would merge now", out)
        self.assertEqual(self.store.snapshot()[0]["pending"], [1], "the saved queue is untouched")

    def test_remove_drops_a_queued_pr_but_not_the_one_being_merged(self):
        self.store.acquire([1, 2, 3])
        self.assertEqual(self.store.take(), 1)
        other = self.other()
        self.assertIn("removed", other.remove(2))
        self.assertIn("being merged", other.remove(1))
        self.assertIn("not in the queue", other.remove(9))
        self.assertEqual(self.store.snapshot()[0]["pending"], [1, 3])

    def test_status_names_current_queue_and_done(self):
        self.store.acquire([1, 2, 3])
        self.store.take()
        self.store.merged(1)
        self.store.take()
        lines = merge_queue.status_lines(*self.other().snapshot())
        self.assertEqual(lines[:4], ["Train: running (pid 100)", "Merging now: #2", "Queued: #3", "Merged: #1"])

    def test_add_and_status_commands_print(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            mt.main(["add", "4", "4"], store=self.store, gh=FakeGh({}))
            mt.main(["status"], store=self.store, gh=FakeGh({}))
        text = out.getvalue()
        self.assertIn("No train is running", text)
        self.assertIn("Ignored", text)
        self.assertIn("Queued: #4", text)

    def test_the_real_pid_check_knows_this_process_and_a_dead_one(self):
        import os
        self.assertTrue(merge_queue.pid_alive(os.getpid()))
        self.assertFalse(merge_queue.pid_alive(2 ** 22 - 3))


if __name__ == "__main__":
    unittest.main()
