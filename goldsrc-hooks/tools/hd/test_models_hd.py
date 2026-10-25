"""models_hd.py's speck fill on small synthetic index grids.

usage: python -m unittest test_models_hd   (from this folder)
"""
import unittest

import numpy as np

from models_hd import SPECK_PX, is_held, solid, specks


def grid(h=20, w=20):
    return np.zeros((h, w), np.uint8)


class SpeckTest(unittest.TestCase):
    def test_tiny_holes_are_specks(self):
        ind = grid()
        ind[3, 3] = 255                    # 1 px
        ind[10, 10:13] = 255               # 3 px in a row
        s = specks(ind == 255)
        self.assertTrue(s[3, 3])
        self.assertTrue(s[10, 10:13].all())
        self.assertEqual(int(s.sum()), 4)

    def test_threshold(self):
        ind = grid()
        ind[2, 2:2 + SPECK_PX] = 255       # exactly SPECK_PX: a speck
        ind[8, 2:3 + SPECK_PX] = 255       # one more: a hole
        s = specks(ind == 255)
        self.assertTrue(s[2].any())
        self.assertFalse(s[8].any())

    def test_real_hole_kept(self):
        # A peep-sight-sized hole (the stock Garand's is 137 px).
        ind = grid(30, 30)
        ind[5:17, 5:17] = 255
        self.assertFalse(specks(ind == 255).any())

    def test_corner_neighbours_are_one_blob(self):
        # Nine pixels touching only at corners: one 9 px blob, not nine specks.
        ind = grid()
        for i in range(9):
            ind[2 + i, 2 + i] = 255
        self.assertFalse(specks(ind == 255).any())

    def test_edge_counts_as_solid(self):
        ind = grid()
        ind[0, 0] = 255
        ind[19, 7:9] = 255
        self.assertEqual(int(specks(ind == 255).sum()), 3)

    def test_solid_only_changes_held_skins(self):
        ind = grid(30, 30)
        ind[1, 1] = 255                    # speck
        ind[5:17, 5:17] = 255              # hole
        held, other = solid(ind, True), solid(ind, False)
        self.assertTrue(held[1, 1])
        self.assertFalse(other[1, 1])
        self.assertFalse(held[5:17, 5:17].any())
        self.assertTrue(((ind != 255) <= held).all())
        self.assertTrue(((ind != 255) == other).all())

    def test_is_held(self):
        self.assertTrue(is_held(r"C:\game\dod\models\v_garand.mdl"))
        self.assertTrue(is_held("models/P_Luger.mdl"))
        self.assertFalse(is_held("models/w_garand.mdl"))
        self.assertFalse(is_held("models/mapmodels/tol_oak1.mdl"))


if __name__ == "__main__":
    unittest.main()
