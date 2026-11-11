"""killfeed_composites.py without the game: its names, the butt-stroke
variants the hook reads, and the placement maths.

usage: python -m unittest test_killfeed_composites   (from this folder)
"""
import unittest

import numpy as np

import killfeed_composites as KC
import killfeed_icons as K


class TableTest(unittest.TestCase):
    def test_composed_icons_and_weapon_icons_never_overlap(self):
        self.assertFalse(set(K.ICONS) & KC.NAMES)

    def test_the_butt_variants_are_the_names_the_hook_looks_up(self):
        # goldsrc-hooks/src/deathmsg/butt.rs: VARIANT_NAMES, (rifle, head).
        self.assertEqual(KC.BUTT_VARIANTS, {
            "d_garandbutt": ("garand", "axis"),
            "d_garandbutt_us": ("garand", "us"),
            "d_k43butt_axis": ("k43", "axis"),
            "d_k43butt_us": ("k43", "us"),
        })
        self.assertIn(KC.BUTT_RECT, KC.BUTT_VARIANTS, "hud.txt's own name is one of them")
        for name in KC.BUTT_VARIANTS:
            # an m_rgszSpriteNames entry is 24 bytes (deathmsg/even.rs)
            self.assertLessEqual(len(name), 24, name)
            self.assertTrue(name.startswith("d_"), name)
        for rifle, head in KC.BUTT_VARIANTS.values():
            self.assertIn(rifle, KC.BUTT_RIFLES)
            self.assertIn(head, KC.HEADS)
            self.assertIn(KC.BUTT_RIFLES[rifle], K.ICONS, "the stock is drawn from the weapon table's view")

    def test_boxes_are_inside_the_rect(self):
        for frac in (KC.BUTT_STOCK_BOX, KC.BUTT_HEAD_BOX, KC.BUTT_HELMET_BOX, KC.SKULL_BOX):
            x0, y0, x1, y1 = frac
            self.assertTrue(0 <= x0 < x1 <= 1 and 0 <= y0 < y1 <= 1, frac)


class PlacementTest(unittest.TestCase):
    def test_canvas_boxes_keep_the_outline_margin_and_the_free_rows(self):
        c = KC.Canvas(72, 34, 4, top=2, bottom=30)
        x0, y0, x1, y1 = c.box((0.0, 0.0, 1.0, 1.0))
        self.assertEqual((x0, x1), (K.OUTLINE, 72 * 4 - K.OUTLINE))
        self.assertEqual((y0, y1), (2 * 4 + K.OUTLINE, 30 * 4 - K.OUTLINE))

    def test_fit_part_centres_or_keeps_the_right_end(self):
        pts = np.array([[0.0, 0.0, 0.0], [10.0, 5.0, 0.0]])
        k, _, ox, oy = KC.fit_part(pts, (0, 0, 100, 100))
        self.assertEqual(k, 10.0)
        self.assertEqual((ox + k * 0, ox + k * 10), (0.0, 100.0))
        self.assertEqual((oy - k * 5, oy - k * 0), (25.0, 75.0), "centred down the box; screen y runs down")
        k, _, ox, _ = KC.fit_part(pts, (0, 0, 200, 50), keep_right=True)
        self.assertEqual(k, 10.0)
        self.assertEqual(ox + k * 10, 200.0, "the right end on the box's right edge")

    def test_flipped_shows_the_other_side_with_the_muzzle_left(self):
        basis = np.eye(3)
        f = KC.flipped(basis)
        np.testing.assert_array_equal(f[0], -basis[0])
        np.testing.assert_array_equal(f[1], basis[1])
        self.assertAlmostEqual(float(np.linalg.det(f)), float(np.linalg.det(basis)), msg="a turn, not a mirror")

    def test_over_puts_an_opaque_top_over_and_leaves_clear_pixels(self):
        base = np.zeros((1, 2, 4))
        base[0, 0] = (1, 0, 0, 1)
        top = np.zeros((1, 2, 4))
        top[0, 0] = (0, 1, 0, 1)
        out = KC.over(base, top)
        np.testing.assert_array_equal(out[0, 0], (0, 1, 0, 1))
        np.testing.assert_array_equal(out[0, 1], (0, 0, 0, 0))


if __name__ == "__main__":
    unittest.main()
