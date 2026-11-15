import json
import tempfile
import unittest
from pathlib import Path

import hook_log as hl

H = "[dodstudio_goldsrc_hooks]"
LOG = f"""[00:04:14.868] [demo  3424.619] {H} spray: TE_PLAYERDECAL for x -> no spray
[00:05:34.695] [demo  3591.157] {H} HUD_Shutdown: the game is quitting; our cvars are no longer read
[01:30:27.365] {H} hlae_window: -dodstudio_start_behind: the window stays behind
[01:30:27.373] {H} ========== new session, 2026-11-10 ==========
[01:30:27.373] {H} goldsrc-hooks build 0.10.0+5c37559af
[01:30:30.670] {H} remote: listening on \\\\.\\pipe\\dodstudio-hl-26664 (#413)
[01:31:00.000] [demo     0.250] {H} remote: Studio ran "viewdemo a"
[01:31:10.000] [demo    10.000] {H} remote: Studio ran "mirv_recordmovie_start"
[01:31:12.000] [demo    12.000] {H} remote: Studio ran "mirv_recordmovie_stop"
[01:31:13.000] [demo     2.000] {H} seek_to: at 2.0

[01:31:14.000] [demo     3.000] {H} streaks: could not read it: no such demo
[01:32:40.921] {H} hlae_window: -dodstudio_start_behind: the window stays behind
[01:32:40.922] {H} ========== new session, 2026-11-10 ==========
[01:32:43.861] {H} remote: listening on \\\\.\\pipe\\dodstudio-hl-46176 (#413)
[01:33:00.000] [demo     1.269] {H} CRASH: access violation (0xc0000005) at core.dll+0x16d6 (0x346d16d6) -- reading 0x18
[01:33:00.000] [demo     1.269] {H} CRASH:   eip=0x346d16d6 eax=0x00000000
[01:33:00.100] [demo     1.300] {H} seek_to: at 2.0
"""


def runs():
    return hl.split_runs(hl.parse(LOG.splitlines()))


class ParseTests(unittest.TestCase):
    def test_lines_carry_wall_time_demo_clock_and_module(self):
        e = hl.parse(LOG.splitlines())[0]
        self.assertEqual((e["wall"], e["demo"], e["module"]), ("00:04:14.868", 3424.619, "spray"))

    def test_blank_lines_are_dropped(self):
        self.assertEqual(len(hl.parse(["", "  "])), 0)


class RunTests(unittest.TestCase):
    def test_the_log_splits_at_each_session_with_an_earlier_days_run_first(self):
        r = runs()
        self.assertEqual([x["index"] for x in r], [0, 1, 2])
        self.assertEqual(len(r[0]["lines"]), 2)

    def test_the_line_just_before_a_session_line_belongs_to_the_new_run(self):
        r = runs()
        self.assertIn("hlae_window", r[1]["lines"][0]["text"])
        self.assertIn("hlae_window", r[2]["lines"][0]["text"])

    def test_run_info_reads_pid_build_captures_and_crashes(self):
        r = runs()
        known = {"core.dll+0x16d6": "#207"}
        one, two = hl.run_info(r[1], known), hl.run_info(r[2], known)
        self.assertEqual((one["pid"], one["build"], one["captures"], one["crashes"]), (26664, "0.10.0+5c37559af", 1, 0))
        self.assertEqual((two["pid"], two["crashes"]), (46176, 1))
        self.assertTrue(hl.run_info(r[0])["quit"])

    def test_a_crash_is_one_entry_and_named_when_known(self):
        found = hl.crashes(runs()[2]["lines"], {"core.dll+0x16d6": "#207"})
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0][1], "#207")


class PickTests(unittest.TestCase):
    def test_runs_are_picked_by_index_time_and_pid(self):
        r = runs()
        self.assertEqual(hl.pick(r)["index"], 2)
        self.assertEqual(hl.pick(r, index=-2)["index"], 1)
        self.assertEqual(hl.pick(r, index=1)["index"], 1)
        self.assertEqual(hl.pick(r, at="01:31")["index"], 1)
        self.assertEqual(hl.pick(r, at="01:33:00")["index"], 2)
        self.assertEqual(hl.pick(r, pid=46176)["index"], 2)
        self.assertIsNone(hl.pick(r, pid=1))
        self.assertIsNone(hl.pick(r, index=9))

    def test_grep_stays_inside_one_run(self):
        r = runs()
        self.assertEqual([e["demo"] for e in hl.grep(r[1]["lines"], "seek_to: at")], [2.0])
        self.assertEqual([e["demo"] for e in hl.grep(r[2]["lines"], "seek_to: at")], [1.3])


class ContentTests(unittest.TestCase):
    def test_the_demo_clock_splits_where_it_goes_back(self):
        st = hl.stretches(runs()[1]["lines"])
        self.assertEqual([(a, b) for a, b, _, _ in st], [(0.25, 12.0), (2.0, 3.0)])

    def test_captures_errors_and_module_status(self):
        lines = runs()[1]["lines"]
        self.assertEqual([c[2] for c in hl.captures(lines)], ["mirv_recordmovie_start", "mirv_recordmovie_stop"])
        self.assertEqual([e["module"] for e in hl.errors(lines)], ["streaks"])
        mods = hl.module_status(lines)
        self.assertEqual(mods["remote"][0], 4)
        self.assertIn("mirv_recordmovie_stop", mods["remote"][1]["text"])


class FileTests(unittest.TestCase):
    def test_the_log_for_a_date_is_under_the_log_dir(self):
        path = hl.log_for("2026-11-10", {"DOD_STUDIO_LOG_DIR": "X:/logs"})
        self.assertEqual(path, Path("X:/logs") / "dodstudio_goldsrc_hooks_20261110.log")

    def test_a_game_probe_report_is_read_from_its_hook_log(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / "report.json"
            report.write_text(json.dumps({"hook_log": LOG}), encoding="utf-8")
            args = type("A", (), {"report": str(report), "log": None, "date": None})
            lines, source = hl.read_lines(args)
            self.assertEqual(len(hl.split_runs(hl.parse(lines))), 3)
            self.assertIn("hook_log", source)


if __name__ == "__main__":
    unittest.main()
