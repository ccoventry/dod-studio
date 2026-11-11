import unittest

import frames


class EdgeTests(unittest.TestCase):
    def test_dark_rows_are_counted_from_each_edge(self):
        rows = [0, 5, 200, 180, 3, 0, 0]
        cols = [255, 0]
        self.assertEqual(frames.edges_from_maxima(rows, cols, 16),
                         {"top": 2, "bottom": 3, "left": 0, "right": 1})

    def test_an_all_black_frame_is_black_from_every_edge(self):
        self.assertEqual(frames.dark_run([0, 0, 0], 16), 3)


class RegionTests(unittest.TestCase):
    def test_the_kill_feed_sits_top_right(self):
        left, top, right, bottom = frames.region_box("killfeed", 1920, 1080)
        self.assertEqual((top, right), (0, 1920))
        self.assertGreater(left, 960)

    def test_the_crosshair_box_is_a_square_at_the_centre(self):
        left, top, right, bottom = frames.region_box("crosshair", 2560, 1440)
        self.assertEqual(right - left, bottom - top)
        self.assertEqual((left + right) // 2, 1280)
        self.assertEqual((top + bottom) // 2, 720)

    def test_a_box_in_fractions_scales_to_the_frame(self):
        self.assertEqual(frames.parse_box("0.5,0,0.5,0.25", 1280, 720), (640, 0, 1280, 180))

    def test_a_box_in_pixels_is_kept(self):
        self.assertEqual(frames.parse_box("10,20,30,40", 1280, 720), (10, 20, 40, 60))


class SheetTests(unittest.TestCase):
    def test_picks_include_the_first_and_last_frame(self):
        self.assertEqual(frames.pick_evenly(101, 5), [0, 25, 50, 75, 100])

    def test_asking_for_more_frames_than_exist_takes_them_all(self):
        self.assertEqual(frames.pick_evenly(3, 10), [0, 1, 2])

    def test_the_grid_wraps_at_the_column_count(self):
        self.assertEqual(frames.grid(10, 4), (4, 3))
        self.assertEqual(frames.grid(2, 4), (2, 1))


class DiffAndClockTests(unittest.TestCase):
    def test_changed_share_counts_pixels_above_the_threshold(self):
        hist = [0] * 256
        hist[0], hist[9], hist[200] = 50, 25, 25
        self.assertEqual(frames.changed_share(hist, 8), 0.5)

    def test_the_clock_counts_from_frames_at_the_given_rate(self):
        f = frames.clock_filter(300)
        self.assertIn("n/300", f)
        self.assertIn("mod(n\\,300)*100/300", f)

    def test_the_slow_copy_is_a_quarter_speed_at_30_fps(self):
        real, slow = frames.clock_jobs("in.mp4", "out", 120)
        self.assertEqual(real[-1], "out.mp4")
        self.assertEqual(slow[-1], "out_0.25x.mp4")
        self.assertIn("setpts=4*PTS", slow[slow.index("-vf") + 1])
        self.assertEqual(slow[slow.index("-r") + 1], "30")


if __name__ == "__main__":
    unittest.main()
