import unittest
from datetime import date

import activity_log as al

LOG = """\
* [00:14:42] [capture] started
* [00:14:42] [HLAE] Running the batch in the game that is already open (PID 1), not starting one (#725)
* [00:14:43] warning text
  that runs on
* [00:25:43] [capture] error after 11 min 0 s: 0 of 1 clip(s) captured


========== New Session: 2026-11-07 @ 01:00 -06:00 ====================

* [01:00:01] [scan] started: 1 path(s) to read
* [01:00:02] [scan] finished after 0.1 s: 1 demo(s) read
* [01:05:00] [toast error] Capture error: boom
* [23:59:50] [render] queue_render_batch starting: 1 source dir(s)
"""


class ParseTests(unittest.TestCase):
    def setUp(self):
        self.entries = al.parse(LOG.splitlines())

    def test_each_entry_keeps_its_time_and_tag(self):
        self.assertEqual((self.entries[0]["time"], self.entries[0]["tag"]), ("00:14:42", "capture"))

    def test_an_untagged_entry_has_no_tag(self):
        self.assertIsNone(self.entries[2]["tag"])

    def test_a_continuation_line_joins_the_entry_above(self):
        self.assertIn("that runs on", self.entries[2]["text"])

    def test_entries_after_a_session_line_carry_that_session(self):
        self.assertIsNone(self.entries[0]["session"])
        self.assertTrue(self.entries[4]["session"].startswith("2026-11-07"))

    def test_a_tag_filter_matches_by_prefix(self):
        kept = [e for e in self.entries if al.keep(e, tags=["toast"])]
        self.assertEqual([e["tag"] for e in kept], ["toast error"])

    def test_since_drops_earlier_entries(self):
        self.assertEqual(len([e for e in self.entries if al.keep(e, since="01:00")]), 4)


class JobTests(unittest.TestCase):
    def test_a_start_pairs_with_the_same_tags_next_after_line(self):
        jobs = al.jobs(al.parse(LOG.splitlines()))
        capture = [j for j in jobs if j["tag"] == "capture"][0]
        self.assertEqual(capture["end"]["time"], "00:25:43")
        self.assertEqual(al.seconds_between("00:14:42", "00:25:43"), 661)

    def test_not_starting_in_a_sentence_is_not_a_start(self):
        jobs = al.jobs(al.parse(LOG.splitlines()))
        self.assertNotIn("HLAE", [j["tag"] for j in jobs])

    def test_a_start_with_no_end_comes_back_open(self):
        jobs = al.jobs(al.parse(LOG.splitlines()))
        render = [j for j in jobs if j["tag"] == "render"][0]
        self.assertIsNone(render["end"])

    def test_a_job_across_midnight_still_has_a_positive_length(self):
        self.assertEqual(al.seconds_between("23:59:50", "00:00:10"), 20)


class DateTests(unittest.TestCase):
    def test_dates_name_the_days_log(self):
        today = date(2026, 11, 10)
        self.assertEqual(al.log_name(al.parse_date("yesterday", today)), "activity_20261109.md")
        self.assertEqual(al.log_name(al.parse_date("2026-11-07", today)), "activity_20261107.md")


if __name__ == "__main__":
    unittest.main()
