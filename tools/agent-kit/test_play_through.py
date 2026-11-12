import unittest
from pathlib import Path

import play_through as pt

PRE = Path("C:/Steam/common/Half-Life - PRE-Anniversary for Movies")
POST = Path("C:/Steam/common/Half-Life - POST-Anniversary for Movies")


class PlanTests(unittest.TestCase):
    def test_a_demo_in_the_installs_own_folder_is_its_bare_name(self):
        self.assertEqual(pt.demo_arg(PRE / "dod" / "a_b-c.dem", PRE), "a_b-c")

    def test_another_installs_demo_is_reached_by_a_relative_path(self):
        self.assertEqual(pt.demo_arg(PRE / "dod" / "x.dem", POST),
                         "../../Half-Life - PRE-Anniversary for Movies/dod/x")

    def test_a_bare_name_is_looked_up_in_the_demo_folder(self):
        self.assertEqual(pt.resolve_demo("x", PRE / "dod"), PRE / "dod" / "x.dem")

    def test_installs_are_the_outer_loop(self):
        runs = pt.plan_runs([PRE / "dod" / "a.dem", PRE / "dod" / "b.dem"], ["pre", "post"], Path("out"),
                            {"pre": PRE, "post": POST})
        self.assertEqual([(r["install"], r["demo"]) for r in runs],
                         [("pre", "a"), ("pre", "b"), ("post", "a"), ("post", "b")])

    def test_the_game_is_sent_away_before_the_demo_starts(self):
        steps = pt.probe_steps("viewdemo", "../x", 60)
        self.assertLess(steps.index("cmd ds_capture_batch away"), steps.index('cmd viewdemo "../x"'))
        self.assertTrue(steps[-3].endswith(" 60"))


class SummaryTests(unittest.TestCase):
    def report(self, hook, console, **extra):
        return {"hook_log": hook, "console_log": console, "steps": [], "crashes": [], **extra}

    def test_the_last_stamp_is_where_the_demo_stopped(self):
        s = pt.summarize(self.report("[demo  12.5] a\n[demo 40.0] b\n[demo 41.0] c", ""))
        self.assertEqual((s["first_demo_s"], s["last_demo_s"]), (12.5, 41.0))
        self.assertEqual(s["jumps"], [(12.5, 40.0)])

    def test_an_engine_error_wins_over_the_routine_next_demo_line(self):
        console = "Host_Error: CL_ParseServerMessage: Illegible server message - svc_bad\n" + pt.NEXT_DEMO
        s = pt.summarize(self.report("", console))
        self.assertTrue(s["end_line"].startswith("Host_Error"))
        self.assertTrue(any("svc_bad" in line for line in s["bad_lines"]))

    def test_a_clean_end_reads_as_end_of_file(self):
        s = pt.summarize(self.report("", "some kill\n" + pt.NEXT_DEMO + "\n"))
        self.assertTrue(s["end_line"].startswith("end of file"))
        self.assertEqual(s["bad_lines"], [])

    def test_no_stamps_and_no_end_line_say_none(self):
        s = pt.summarize(self.report("", ""))
        self.assertIsNone(s["last_demo_s"])
        self.assertIsNone(s["end_line"])

    def test_a_backwards_clock_step_counts_as_a_jump(self):
        self.assertEqual(pt.clock_jumps([1956.4, 4.4, 5.0]), [(1956.4, 4.4)])

    def test_new_snapshot_files_are_the_ones_not_there_before(self):
        self.assertEqual(pt.new_files({"a.bmp"}, {"a.bmp", "b.bmp"}), ["b.bmp"])


if __name__ == "__main__":
    unittest.main()
