"""killfeed_icons.py without the game: the icon table, the muzzle clip, the
edge and outline passes, painting into a sheet, and when a sheet is redone.

usage: python -m unittest test_killfeed_icons   (from this folder)
"""
import json, os, tempfile, unittest

import numpy as np
from scipy import ndimage

import killfeed_icons as K
from studiomodel import Texture


def box_tris(x0, x1, y0=0.0, y1=1.0, z=0.0):
    """Two triangles making a flat rectangle, as model_triangles returns
    them (one mesh)."""
    v = np.array([[[x0, y0, z], [x1, y0, z], [x1, y1, z]],
                  [[x0, y0, z], [x1, y1, z], [x0, y1, z]]], float)
    n = np.tile([0.0, 0.0, 1.0], (2, 3, 1))
    uv = np.zeros((2, 3, 2))
    tex = Texture("t.bmp", 0, 2, 2, np.zeros((2, 2), np.uint8), np.full((256, 3), 128, np.uint8))
    return (v, n, uv, tex)


class TableTest(unittest.TestCase):
    def test_every_icon_has_a_model_and_no_icon_is_listed_twice(self):
        for name, icon in K.ICONS.items():
            self.assertTrue(name.startswith("d_"), name)
            self.assertTrue(icon.models or icon.fallback, name)
            for m in icon.models:
                self.assertTrue(m.startswith("p_") and m.endswith(".mdl"), f"{name}: {m}")
            for m in icon.fallback:
                self.assertTrue(m.startswith("w_") and m.endswith(".mdl"), f"{name}: {m}")
            self.assertTrue(icon.rolls, name)
        self.assertFalse(set(K.ICONS) & set(K.KEEP_UPSCALE))

    def test_covers_every_stock_kill_feed_icon(self):
        # The d_ entries of DoD 1.3's hud.txt, both engine builds.
        stock = {"d_scopedfg42", "d_thompson", "d_bar", "d_garand", "d_spring", "d_30cal", "d_grenade",
                 "d_amerknife", "d_colt", "d_m1carbine", "d_skull", "d_world", "d_fcarbine", "d_greasegun",
                 "d_mp40", "d_mp44", "d_kar", "d_bayonet", "d_mg42", "d_fg42", "d_spade", "d_scopedkar",
                 "d_mg34", "d_k43", "d_stick", "d_paraknife", "d_gerknife", "d_luger", "d_sten", "d_enfield",
                 "d_scopedenfield", "d_bren", "d_britgrenade", "d_britknife", "d_webley", "d_piat",
                 "d_pschreck", "d_bazooka", "d_garandbutt", "d_enfbayonet", "d_mortar", "d_germortar"}
        self.assertEqual(stock - set(K.ICONS) - set(K.KEEP_UPSCALE), set())

    def test_bipod_and_scoped_variants(self):
        self.assertEqual(set(K.ICONS["d_mg42"].models), {"p_mg42bu.mdl", "p_mg42bd.mdl"})
        self.assertIn("p_k98s.mdl", K.ICONS["d_scopedkar"].models)
        self.assertTrue(K.ICONS["d_kar"].clip_tip)

    def test_hand_adjustments(self):
        # Defaults leave the auto view alone.
        plain = K.Icon(("p_a.mdl",))
        self.assertEqual((plain.turn, plain.barrel_top, plain.cut_below), (0.0, None, False))
        for name, icon in K.ICONS.items():
            self.assertLess(abs(icon.turn), 15, name)  # a nudge, not a new view
            if icon.cut_below:
                self.assertIsNotNone(icon.barrel_top, f"{name}: cut_below needs barrel_top")
            if icon.barrel_top is not None:
                self.assertGreaterEqual(icon.barrel_top, 0, name)
        # The icons the user's review named, and only those.
        adjusted = {n for n, i in K.ICONS.items() if i.turn or i.barrel_top is not None}
        self.assertEqual(adjusted, {"d_30cal", "d_britgrenade", "d_britknife", "d_enfield", "d_grenade", "d_k43",
                                    "d_mp40", "d_mp44", "d_paraknife", "d_scopedenfield", "d_thompson"})
        for n in ("d_britgrenade", "d_britknife", "d_grenade", "d_paraknife", "d_thompson"):
            self.assertGreater(K.ICONS[n].turn, 0, f"{n}: the review asked for clockwise")
        for n in ("d_enfield", "d_scopedenfield"):
            self.assertLess(K.ICONS[n].turn, 0, f"{n}: the review asked for counter-clockwise")
        for n in ("d_30cal", "d_mp40", "d_mp44"):
            self.assertTrue(K.ICONS[n].cut_below, n)
        self.assertFalse(K.ICONS["d_k43"].cut_below)  # the stock must stay whole

    def test_candidates_prefer_p_then_w(self):
        with tempfile.TemporaryDirectory() as dod:
            os.makedirs(os.path.join(dod, "models"))
            icon = K.Icon(("p_a.mdl", "p_b.mdl"), ("w_a.mdl",))
            self.assertEqual(K.candidates(dod, icon), [])
            open(os.path.join(dod, "models", "w_a.mdl"), "wb").close()
            self.assertEqual([os.path.basename(p) for p in K.candidates(dod, icon)], ["w_a.mdl"])
            open(os.path.join(dod, "models", "p_b.mdl"), "wb").close()
            self.assertEqual([os.path.basename(p) for p in K.candidates(dod, icon)], ["p_b.mdl"])

    def test_hud_entries_and_killfeed_names(self):
        with tempfile.TemporaryDirectory() as dod:
            os.makedirs(os.path.join(dod, "sprites"))
            with open(os.path.join(dod, "sprites", "hud.txt"), "w") as f:
                f.write("3\nd_kar 320 weapons3s 0 0 40 10\nd_kar\t\t640\tweapons3\t167\t47 \t89\t19\n"
                        "// d_old 640 x 0 0 1 1\nnumber_0 640 hud 0 0 20 24\n")
            entries = K.hud_entries(dod)
            self.assertEqual(entries["d_kar"], ("weapons3", 167, 47, 89, 19))
            self.assertEqual(K.killfeed_names(entries), ["d_kar"])


class GeometryTest(unittest.TestCase):
    def test_clip_tip_drops_what_is_past_the_gap(self):
        gun = box_tris(0, 10)
        tip = box_tris(10.5, 11)  # a bayonet tip past a gap near the front
        tris, cut, dropped = K.clip_tip([gun, tip], np.array([1.0, 0, 0]))
        self.assertTrue(10 < cut < 10.5)
        self.assertEqual(dropped, 2)
        self.assertEqual(len(tris), 1)

    def test_clip_tip_without_a_gap_in_front_keeps_the_body(self):
        gun = box_tris(0, 10)
        tris, _, dropped = K.clip_tip([gun], np.array([1.0, 0, 0]))
        self.assertEqual(dropped, 0)

    def test_muzzle_signs(self):
        self.assertEqual(K.muzzle_signs(np.array([0.3, -0.95, 0]), K.MUZZLE), (1,))
        self.assertEqual(K.muzzle_signs(np.array([0.3, 0.95, 0]), K.MUZZLE), (-1,))
        # long axis across Y (a knife lying along X): free
        self.assertEqual(K.muzzle_signs(np.array([1.0, 0, 0]), K.MUZZLE), (1, -1))
        self.assertEqual(K.muzzle_signs(np.array([0, 1.0, 0]), None), (1, -1))

    def test_fit_keeps_the_aspect_and_centres(self):
        tris = [box_tris(0, 10, 0, 2)]
        basis = np.eye(3)
        kx, ky, ox, oy = K.fit_transform(tris, basis, (0, 0, 100, 40))
        self.assertEqual(kx, ky)
        self.assertAlmostEqual(kx, 10)
        self.assertAlmostEqual(ox + kx * 5, 50)
        self.assertAlmostEqual(oy - ky * 1, 20)

    def test_refit_fills_the_box_after_a_speck_is_dropped(self):
        fit = (2.0, 2.0, 10.0, 30.0)
        alpha = np.zeros((40, 100))
        alpha[15:20, 10:50] = 1  # the gun, half the box wide
        kx, ky, ox, oy = K.refit_transform(fit, alpha, (0, 0, 100, 40))
        self.assertAlmostEqual(kx, 5.0)  # 100 / 40 wide
        # the gun's left edge (x = 10 on screen) now lands on the box's
        self.assertAlmostEqual(ox + kx * ((10 - 10.0) / 2.0), 0.0)
        alpha[:] = 0
        alpha[0:40, 0:99] = 1
        self.assertIsNone(K.refit_transform(fit, alpha, (0, 0, 100, 40)))

    def test_choose_view_finds_the_matching_silhouette(self):
        tris = [box_tris(0, 10, 0, 1), box_tris(0, 2, -3, 0)]  # a barrel and a grip at the left
        target = np.zeros((16, 40))
        target[0:4, 0:40] = 1
        target[4:16, 0:8] = 1
        _, score, (sx, sy, _) = K.choose_view(tris, target, (0, 0, 40, 16), rolls=tuple(range(-30, 31, 2)), muzzle=None)
        self.assertGreater(score, 0.85)
        # the barrel along +x on the right, the grip below
        self.assertEqual((sx, sy), (1, 1))

    def gun(self, tilt_deg):
        """A gun in the DoD model layout: a thick body at the rear and a thin
        barrel towards -Y (the muzzle), the top towards +X, the side along
        Z, tilted in the X-Y plane by `tilt_deg` as a held `p_` model is."""
        def slab(y0, y1, x0, x1):
            v, n, uv, tex = box_tris(y0, y1, x0, x1)
            return (v[..., [1, 0, 2]], n, uv, tex)  # length along Y, height X
        tris = [slab(-20, -8, -2, 2), slab(-40, -20, 0.5, 1.0)]  # body, barrel
        a = np.radians(tilt_deg)
        rot = np.array([[np.cos(a), -np.sin(a), 0], [np.sin(a), np.cos(a), 0], [0, 0, 1]])
        return [(v @ rot.T, n, uv, tex) for v, n, uv, tex in tris]

    def test_the_barrel_ends_up_level_with_the_muzzle_right_and_top_up(self):
        for tilt in (0, 8, 22, -15):
            tris = self.gun(tilt)
            basis, (_, _, angle) = K.level_view(tris, (1, 1, 0.0))
            # the view tilts with the barrel's line, which is the model's tilt
            self.assertAlmostEqual(angle, tilt, delta=0.3)
            pts = np.concatenate([t[0].reshape(-1, 3) for t in tris])
            xy = np.stack([pts @ basis[0], pts @ basis[1]], 1)
            # the muzzle (the barrel's far end, most negative model Y) on the right
            muzzle = np.array([0.0, -40.0, 0.75]) @ np.array([[np.cos(np.radians(tilt)), np.sin(np.radians(tilt)), 0], [-np.sin(np.radians(tilt)), np.cos(np.radians(tilt)), 0], [0, 0, 1]])
            self.assertGreater(muzzle @ basis[0], xy[:, 0].mean(), tilt)
            # the model's +X (the top of the gun) still points up
            self.assertGreater(basis[1][0] * np.cos(np.radians(tilt)) + basis[1][1] * np.sin(np.radians(tilt)), 0, tilt)


    def test_a_positive_turn_is_clockwise(self):
        basis = K.turned(np.eye(3), 10)
        right_end = np.array([1.0, 0, 0])
        # screen y = oy - ky * up: the right end's `up` drops, so it goes down
        self.assertLess(right_end @ basis[1], 0)
        self.assertGreater(right_end @ basis[0], 0.98)
        np.testing.assert_allclose(K.turned(K.turned(np.eye(3), 7), -7), np.eye(3), atol=1e-12)

    def test_barrel_top_row_ignores_a_front_sight(self):
        a = np.zeros((40, 100))
        a[20:24, 10:100] = 1  # barrel, top at row 20
        a[14:20, 96:98] = 1  # front sight post
        a[24:38, 30:40] = 1  # magazine
        self.assertEqual(K.barrel_top_row(a), 20.0)
        self.assertIsNone(K.barrel_top_row(np.zeros((4, 4))))

    def magazine_gun(self):
        # a barrel along x at up 0..1 and a magazine hanging 6 below
        return [box_tris(0, 20, 0, 1), box_tris(8, 10, -6, 0)]

    def test_place_barrel_puts_the_barrel_on_the_row(self):
        tris, basis, box = self.magazine_gun(), np.eye(3), (0, 0, 100, 40)
        fit = K.fit_transform(tris, basis, box)
        alpha = K.silhouette(tris, basis, *fit, 100, 40, ss=1)
        # cut: as wide as the box, the barrel top on row 10, the magazine past the bottom
        kx, ky, ox, oy = K.place_barrel(tris, basis, fit, alpha, box, 10, cut=True)
        self.assertAlmostEqual(kx, 5.0)  # 100 / 20
        self.assertAlmostEqual(oy - ky * 1.0, 10, delta=0.6)
        self.assertGreater(oy + ky * 6, 40)
        # no cut: the same size, raised until the magazine ends inside the box
        kx, ky, ox, oy = K.place_barrel(tris, basis, fit, alpha, box, 15, cut=False)
        self.assertAlmostEqual(kx, 5.0)
        self.assertLessEqual(oy + ky * 6, 40 + 1e-6)
        self.assertAlmostEqual(oy - ky * 1.0, 5, delta=0.6)  # 15 asked, 5 is as low as it fits (7 tall x 5)
        self.assertAlmostEqual(ox + kx * 10, 50)  # still centred across
        # ...and left on the row when it already fits
        _, _, _, oy = K.place_barrel(tris, basis, fit, alpha, box, 3, cut=False)
        self.assertAlmostEqual(oy - 5.0, 3, delta=0.6)

    def test_place_barrel_shrink_keeps_the_row_and_fits_the_magazine(self):
        tris, basis, box = self.magazine_gun(), np.eye(3), (0, 0, 100, 40)
        fit = K.fit_transform(tris, basis, box)
        alpha = K.silhouette(tris, basis, *fit, 100, 40, ss=1)
        kx, ky, ox, oy = K.place_barrel(tris, basis, fit, alpha, box, 10, shrink=True)
        self.assertAlmostEqual(oy - ky * 1.0, 10, delta=0.6)  # the barrel stays on its row
        self.assertLessEqual(oy + ky * 6, 40 + 1e-6)  # the whole magazine inside
        self.assertLess(kx, 5.0)  # ...by drawing the gun smaller
        self.assertAlmostEqual(ox + kx * 10, 50)  # still centred across
        # shrink wins over cut
        self.assertEqual(K.place_barrel(tris, basis, fit, alpha, box, 10, cut=True, shrink=True), (kx, ky, ox, oy))
        # a gun that already fits keeps the box's width
        kx2, *_ = K.place_barrel(tris, basis, fit, alpha, (0, 0, 100, 400), 10, shrink=True)
        self.assertAlmostEqual(kx2, 5.0)

    def test_place_barrel_at_a_given_scale(self):
        tris, basis, box = self.magazine_gun(), np.eye(3), (0, 0, 100, 40)
        fit = K.fit_transform(tris, basis, box)
        alpha = K.silhouette(tris, basis, *fit, 100, 40, ss=1)
        kx, ky, ox, oy = K.place_barrel(tris, basis, fit, alpha, box, 10, shrink=True, k=3.0)
        self.assertAlmostEqual(kx, 3.0)  # fits (7 x 3 = 21 rows): the scale is kept
        self.assertAlmostEqual(oy - ky * 1.0, 10, delta=0.6)
        kx, *_ = K.place_barrel(tris, basis, fit, alpha, box, 10, shrink=True, k=8.0)
        self.assertLess(kx, 8.0)  # 6 below at 8 runs past the bottom: shrunk

    def test_layout_shrinks_moves_or_cuts_a_gun_that_does_not_fit(self):
        tris, basis = self.magazine_gun(), np.eye(3)
        ref = K.fit_transform(tris, basis, (0, 0, 100, 40))
        alpha = K.silhouette(tris, basis, *ref, 100, 40, ss=1)
        box = (0, 0, 100, 30)
        row = 6
        k = 5.0  # 7 units tall x 5 = 35 rows: too tall for 30
        shrunk = K.laid_out_fit(tris, basis, ref, alpha, box, K.Layout(barrel_row=row), k, True)
        self.assertLess(shrunk[0], k)
        self.assertAlmostEqual(shrunk[3] - shrunk[1] * 1.0, row, delta=0.6)  # barrel on the row
        moved = K.laid_out_fit(tris, basis, ref, alpha, box, K.Layout(barrel_row=row, below="move"), 4.0, True)
        self.assertAlmostEqual(moved[0], 4.0)  # 28 rows fit: same size...
        self.assertLessEqual(moved[3] + moved[1] * 6, 30 + 1e-6)  # ...raised until the magazine ends inside
        cut = K.laid_out_fit(tris, basis, ref, alpha, box, K.Layout(barrel_row=row, below="cut"), k, True)
        self.assertAlmostEqual(cut[0], k)  # kept; what hangs below is cut later
        self.assertAlmostEqual(cut[3] - cut[1] * 1.0, row, delta=0.6)

    def test_layout_centres_a_blade_at_its_own_size(self):
        tris, basis = [box_tris(0, 10, 0, 2)], np.eye(3)
        ref = K.fit_transform(tris, basis, (0, 0, 40, 10))
        alpha = K.silhouette(tris, basis, *ref, 40, 10, ss=1)
        new = K.laid_out_fit(tris, basis, ref, alpha, (0, 0, 40, 26), K.Layout(barrel_row=8), ref[0], False)
        self.assertAlmostEqual(new[0], ref[0])  # no barrel line for a blade, no growth into the tall box
        self.assertAlmostEqual(new[3] - new[1] * 1.0, 13, delta=0.6)  # centred on the rect's height

    def test_remap_moves_hand_set_places_with_the_gun(self):
        ref, new = (2.0, 2.0, 10.0, 20.0), (1.0, 1.0, 4.0, 6.0)
        self.assertEqual(K.remap(ref, new, 10.0, 20.0), (4.0, 6.0))  # the origin goes to the origin
        self.assertEqual(K.remap(ref, new, 14.0, 22.0), (6.0, 7.0))  # offsets at half the size
        icon = K.Icon(("x.mdl",), v_part=K.VPart("v.mdl", (2, 6), (2, 4, 4, 6)), cut_part=(2, 4, 6),
                      skin_art=(K.SkinArt("t", (0, 0, 1, 1), (0, 0, 1, 1), (2.0, -4.0)),))
        vp, arts, cut = K.remap_table(icon, (2.0, 2.0, 0.0, 0.0), (1.0, 1.0, 0.0, 0.0), scale=1)
        self.assertEqual(vp.window, (1.0, 3.0))
        self.assertEqual(vp.box, (1.0, 2.0, 2.0, 3.0))
        self.assertEqual(cut, (1.0, 2.0, 3.0))
        self.assertEqual(arts[0].offset, (1.0, -2.0))

    def test_no_icon_shrinks_by_default(self):
        # the MP44's shrink is one of the two options the user picks between
        self.assertEqual({n for n, i in K.ICONS.items() if i.shrink_below}, set())
        for name, icon in K.ICONS.items():
            if icon.shrink_below:
                self.assertIsNotNone(icon.barrel_top, f"{name}: shrink_below needs barrel_top")

    def test_see_through_mask_takes_the_dark_region_round_the_seed(self):
        ind = np.ones((8, 8), np.uint8)  # index 1: grey metal
        ind[2:5, 2:5] = 0  # a black hole
        ind[6, 6] = 0  # a black speck elsewhere
        pal = np.full((256, 3), 128, np.uint8)
        pal[0] = (0, 0, 0)
        tex = Texture("gun.bmp", 0, 8, 8, ind, pal)
        m = K.see_through_mask(tex, [(3, 3)])
        self.assertEqual(int(m.sum()), 9)
        self.assertFalse(m[6, 6])
        self.assertFalse(K.see_through_mask(tex, [(0, 0)]).any())  # seed on metal: no hole
        skins, cut = K.apply_see_through({}, [tex], (("GUN.bmp", 3, 3),))
        self.assertEqual(cut, {"gun.bmp"})
        self.assertEqual(skins["gun.bmp"].shape, (8, 8, 4))
        self.assertEqual(skins["gun.bmp"][3, 3, 3], 0)
        self.assertEqual(skins["gun.bmp"][0, 0, 3], 1)
        self.assertEqual(K.apply_see_through({}, [tex], ()), ({}, frozenset()))

    def test_see_through_seeds_are_on_the_icons_models(self):
        for name, icon in K.ICONS.items():
            for tex, x, y in icon.see_through:
                self.assertTrue(tex.endswith(".bmp"), name)
                self.assertTrue(x >= 0 and y >= 0, name)

    def test_cut_under_clears_below_the_row(self):
        img = np.ones((10, 4, 4))
        out = K.cut_under(img, 6.5)
        self.assertTrue((out[:7, :, 3] == 1).all())
        self.assertTrue((out[7:, :, 3] == 0).all())
        self.assertTrue((img[..., 3] == 1).all())  # the input is left alone


class PassesTest(unittest.TestCase):
    def square(self):
        img = np.zeros((20, 20, 4))
        img[5:15, 5:15] = (0.1, 0.1, 0.1, 1)
        return img

    def test_rim_light_lightens_only_the_edge(self):
        out = K.rim_light(self.square(), 2.5, 0.35)
        self.assertGreater(out[5, 10, 0], 0.3)  # edge
        self.assertAlmostEqual(out[10, 10, 0], 0.1)  # middle
        self.assertTrue((out[..., 3] == self.square()[..., 3]).all())

    def test_outline_adds_a_dark_ring(self):
        out = K.outline(self.square(), 3)
        self.assertEqual(out[10, 3, 3], 1.0)  # 2 px out
        self.assertEqual(out[10, 1, 3], 0.0)  # 4 px out
        self.assertLess(out[10, 3, :3].max(), 0.1)
        self.assertAlmostEqual(out[10, 10, 0], 0.1)

    def ring(self):
        """A 30 px square with a 10 px hole and a 2 px slit inside."""
        img = np.zeros((40, 40, 4))
        img[5:35, 5:35] = (0.5, 0.5, 0.5, 1)
        img[10:20, 10:20, 3] = 0  # the hole (a trigger guard's inside)
        img[25:27, 10:30, 3] = 0  # a slit
        return img

    def test_holes_finds_a_gap_but_not_a_slit_or_the_outside(self):
        h = K.holes(self.ring()[..., 3])
        self.assertTrue(h[10:20, 10:20].all())
        self.assertFalse(h[25:27].any())
        self.assertFalse(h[0].any())

    def test_outline_keeps_a_hole_open(self):
        out = K.outline(self.ring(), 3)
        self.assertEqual(out[15, 10, 3], 1.0)  # 1 px rim inside the hole
        self.assertEqual(out[15, 12, 3], 0.0)  # the rest still clear
        self.assertEqual(out[15, 15, 3], 0.0)
        self.assertEqual(out[26, 20, 3], 1.0)  # the slit is filled as before
        self.assertEqual(out[20, 3, 3], 1.0)  # outside: the full 3 px rim (2 px out)

    def test_holes_keeps_a_trigger_s_gaps_open_together(self):
        img = np.zeros((40, 40, 4))
        img[5:35, 5:35, 3] = 1
        img[10:20, 10:13, 3] = 0  # 3 px gap behind the trigger
        img[10:20, 14:17, 3] = 0  # 3 px gap in front of it (trigger: column 13)
        img[25:27, 10:30, 3] = 0  # a lone slit
        h = K.holes(img[..., 3])
        self.assertTrue(h[10:20, 10:13].all() and h[10:20, 14:17].all())
        self.assertFalse(h[10:20, 13].any())  # the trigger itself stays
        self.assertFalse(h[25:27].any())
        self.assertFalse(K.holes(img[..., 3], bridge=0)[10:20, 10:17].any())  # each alone is a slit

    def test_keep_thin_keeps_a_faint_line_but_not_a_soft_edge_or_a_corner(self):
        img = self.square()
        img[5:15, 15, 3] = 0.4  # the square's soft right edge
        img[18, 2:12] = (0.5, 0.5, 0.5, 0.3)  # a wire under it, a third of a pixel thick
        img[2, 2] = (0.5, 0.5, 0.5, 0.3)  # a lone faint corner pixel
        out = K.keep_thin(img)
        self.assertTrue((out[18, 2:12, 3] == 1).all())
        self.assertTrue((out[5:15, 15, 3] == 0.4).all())
        self.assertEqual(out[2, 2, 3], 0.3)
        self.assertTrue((out[5:15, 5:15, 3] == 1).all())

    def test_keep_thin_joins_a_dotted_wire(self):
        img = np.zeros((10, 20, 4))
        img[5, 2:14, 3] = [0.3, 0.6] * 6  # every other pixel already over the cut
        out = K.keep_thin(img)
        self.assertTrue((out[5, 2:14, 3] >= 0.6).all())

    def test_drop_specks_keeps_a_bar_joined_at_a_corner(self):
        img = self.square()
        for i in range(4):
            img[15 + i, 15 + i] = (0.5, 0.5, 0.5, 1)  # a 1 px diagonal bar off the corner
        self.assertEqual(K.drop_specks(img, 40)[17, 17, 3], 1)
        self.assertEqual(K.drop_specks(img, 40, corners=False)[17, 17, 3], 0)

    def test_shared_area_finds_a_neighbour_s_rows(self):
        entries = {"d_mg34": ("weapons3", 167, 173, 89, 23), "d_k43": ("weapons3", 167, 192, 89, 14),
                   "d_other": ("weapons4", 167, 180, 89, 23), "weapon_x": ("weapons3", 200, 170, 20, 5)}
        mask, top, bottom = K.shared_area(entries, "d_mg34")
        self.assertTrue(mask[19:23].all() and not mask[5:19, 0:30].any())
        self.assertEqual((top, bottom), (0, 19))  # the K43's four rows off the bottom
        self.assertTrue(mask[0:2, 33:53].all())  # a narrow sprite: masked, rows kept
        # the K43 is the lower icon: the rows it shares with the MG34 are its own
        mask, top, bottom = K.shared_area(entries, "d_k43")
        self.assertEqual((top, bottom), (0, 14))
        self.assertFalse(mask.any())

    def test_shared_rows_belong_to_the_lower_kill_feed_icon_only(self):
        # weapons1: the weapon menu's weapon_luger over the scoped FG42; a
        # kill-feed icon over a menu sprite (the scoped Enfield over
        # weapon_britgrenade)
        entries = {"weapon_luger": ("weapons1", 160, 115, 80, 35), "d_scopedfg42": ("weapons1", 167, 147, 89, 26),
                   "d_scopedenfield": ("weapons4", 167, 75, 89, 19), "weapon_britgrenade": ("weapons4", 167, 92, 80, 46),
                   "d_enfield": ("weapons4", 168, 57, 88, 19)}
        mask, top, bottom = K.shared_area(entries, "d_scopedfg42")
        self.assertEqual((top, bottom), (3, 26))  # a menu sprite above: still kept out
        self.assertTrue(mask[0:3].any())
        mask, top, bottom = K.shared_area(entries, "d_scopedenfield")
        self.assertEqual((top, bottom), (0, 17))  # its top row (the Enfield's) its own; the menu sprite's not
        self.assertFalse(mask[0].any())
        mask, top, bottom = K.shared_area(entries, "d_enfield")
        self.assertEqual((top, bottom), (0, 18))  # the upper icon keeps out of its bottom row
        self.assertTrue(mask[18, 1:].all())

    def test_drop_specks(self):
        img = self.square()
        img[0, 0] = (1, 1, 1, 1)
        out = K.drop_specks(img, 40)
        self.assertEqual(out[0, 0, 3], 0)
        self.assertEqual(out[10, 10, 3], 1)

    def test_own_mask_keeps_loose_parts_but_not_a_neighbours(self):
        a = np.zeros((10, 30))
        a[2:5, 2:25] = 1  # the gun
        a[7:9, 20:26] = 1  # a loose bayonet inside the rect
        a[0, 0:3] = 1  # a neighbour's pixels on the border
        own = K.own_mask(a)
        self.assertTrue(own[3, 10] and own[8, 22])
        self.assertFalse(own[0, 1])

    def test_paint_clears_the_stock_icon_and_keeps_the_rest(self):
        sheet = np.zeros((40, 80, 4))
        sheet[..., :3] = 0.5
        sheet[8:12, 8:40, 3] = 1  # the upscaled stock icon, rect (2, 2, 10, 4) at 4x
        sheet[0:4, 60:64, 3] = 1  # another icon
        stock_alpha = np.zeros((4, 10))
        stock_alpha[0:1, 0:8] = 1
        icon4 = np.zeros((16, 40, 4))
        icon4[8:12, 20:30] = (1, 0, 0, 1)
        K.paint(sheet, (2, 2, 10, 4), stock_alpha, icon4)
        self.assertEqual(sheet[9, 9, 3], 0)  # stock pixels gone
        self.assertEqual(sheet[17, 30, 3], 1)  # render in
        self.assertEqual(tuple(sheet[17, 30, :3]), (1, 0, 0))
        self.assertEqual(sheet[1, 61, 3], 1)  # the other icon untouched

    def test_as_uploaded_cuts_alpha_like_the_hook(self):
        img = np.array([[[0.5, 0.5, 0.5, 0.4], [0.5, 0.5, 0.5, 0.6]]])
        out = K.as_uploaded(img)
        self.assertEqual(tuple(out[0, 0]), (0, 0, 0, 0))
        self.assertEqual(tuple(out[0, 1]), (0.5, 0.5, 0.5, 1))


class SoftEdgeTest(unittest.TestCase):
    """soft_finish: coverage alpha on the edges, colour downsampled
    premultiplied, and the hand fixes still deciding the shape."""

    def tilted(self):
        """A grey slab tilted 20 degrees, rendered as render_icon renders
        (4x4 samples per pixel)."""
        v, n, uv, tex = box_tris(-12, 12, -3, 3)
        a = np.radians(20)
        rot = np.array([[np.cos(a), -np.sin(a), 0], [np.sin(a), np.cos(a), 0], [0, 0, 1]])
        return K.render([(v @ rot.T, n, uv, tex)], np.eye(3), 1, 1, 20, 12, 40, 24, {}, hires=True)

    @staticmethod
    def from_mask(mask4, ss=K.SS, rgb=(0.5, 0.5, 0.5)):
        """`lit` and its samples for a shape whose edges lie on pixel edges."""
        cov = np.kron(mask4, np.ones((ss, ss))).astype(float)
        col = np.zeros(cov.shape + (3,))
        col[cov > 0] = rgb
        lit = np.zeros(mask4.shape + (4,))
        lit[..., :3] = rgb
        lit[..., 3] = mask4
        return lit, col, cov

    def test_render_hires_returns_the_samples_it_averaged(self):
        lit, (col, cov) = self.tilted()
        self.assertEqual(cov.shape, (24 * K.SS, 40 * K.SS))
        self.assertEqual(col.shape, (24 * K.SS, 40 * K.SS, 3))
        np.testing.assert_allclose(cov.reshape(24, K.SS, 40, K.SS).mean((1, 3)), lit[..., 3])

    def test_a_slanted_edge_gets_coverage_alpha(self):
        lit, hi = self.tilted()
        out = K.soft_finish(lit, *hi)
        a = out[..., 3]
        partial = (a > 0.02) & (a < 0.98)
        self.assertGreater(partial.sum(), 40)  # the whole slanted rim is soft
        self.assertGreater(len(np.unique(np.round(a[partial], 2))), 8)  # a ramp, not a few steps
        # going up through the top edge, alpha falls from 1 to 0 within 2 px
        for x in range(12, 28):
            col = a[:, x]
            top = int(np.argmax(col > 0.02))
            self.assertGreaterEqual(col[top + 2], 0.98, x)

    def test_partial_texels_hold_the_outline_colour(self):
        lit, hi = self.tilted()
        red = (0.9, 0.2, 0.2)
        out = K.soft_finish(lit, *hi, colour=red)
        a = out[..., 3]
        # the outer edge: partial texels with nothing but transparency outside
        outer = (a > 0.02) & (a < 0.98) & ~np.pad(a >= 0.98, 1)[2:, 1:-1] & ~np.pad(a >= 0.98, 1)[1:-1, 2:]
        self.assertGreater(outer.sum(), 10)
        # premultiplied: the colour of the covered part, not darkened by the
        # transparent samples averaged with it
        np.testing.assert_allclose(out[outer, :3], np.tile(red, (outer.sum(), 1)), atol=1e-6)

    def test_the_outline_is_three_pixels_past_the_edge(self):
        mask = np.zeros((20, 30))
        mask[6:14, 8:22] = 1
        out = K.soft_finish(*self.from_mask(mask))
        a = out[..., 3]
        self.assertTrue((a[10, 5:8] == 1).all() and (a[3:6, 15] == 1).all())
        self.assertEqual(a[10, 4], 0)
        self.assertEqual(a[2, 15], 0)
        np.testing.assert_allclose(out[10, 5:8, :3], np.tile(K.OUTLINE_COLOUR, (3, 1)))
        self.assertAlmostEqual(out[10, 15, 0], 0.5)  # the middle keeps its colour

    def test_the_rect_s_border_stays_clear(self):
        mask = np.zeros((20, 30))
        mask[3:17, 3:27] = 1  # touching the box, OUTLINE px in from every side
        a = K.soft_finish(*self.from_mask(mask))[..., 3]
        self.assertTrue((a[0] == 0).all() and (a[-1] == 0).all() and (a[:, 0] == 0).all() and (a[:, -1] == 0).all())
        self.assertTrue((a[1:3, 10] == 1).all())  # the outline thins to 2 px there

    def test_keep_thin_lines_stay_whole_and_cleared_pixels_stay_out(self):
        mask = np.zeros((30, 40))
        mask[5:12, 5:35] = 1
        lit, col, cov = self.from_mask(mask)
        lit[20, 5:20, 3] = 1  # keep_thin's line: no coverage of its own, opaque
        cov[24 * K.SS:26 * K.SS, 30 * K.SS:34 * K.SS] = 1  # a speck drop_specks cleared
        col[cov > 0] = 0.5
        out = K.soft_finish(lit, col, cov)
        a = out[..., 3]
        self.assertTrue((a[20, 5:20] == 1).all())
        self.assertTrue((a[17:24, 10] == 1).all())  # its 3 px outline
        self.assertGreater(out[20, 10, 0], 0.4)  # body colour, not the outline's
        self.assertTrue((a[24:26, 30:34] == 0).all())

    def test_a_hole_stays_open(self):
        lit, col, cov = self.from_mask(PassesTest().ring()[..., 3])
        a = K.soft_finish(lit, col, cov)[..., 3]
        self.assertEqual(a[15, 10], 1.0)  # 1 px rim inside the hole
        self.assertEqual(a[15, 11], 0.0)
        self.assertEqual(a[15, 15], 0.0)
        self.assertEqual(a[26, 20], 1.0)  # the slit is filled

    def test_the_shape_stays_where_outline_put_it(self):
        lit, hi = self.tilted()
        old = K.as_uploaded(K.outline(K.rim_light(lit)))[..., 3] > 0
        new = K.soft_finish(lit, *hi)[..., 3] >= 0.5
        edge = old ^ ndimage.binary_erosion(old)
        near_edge = ndimage.binary_dilation(edge | (~old & ndimage.binary_dilation(old)))
        self.assertTrue(((old ^ new) <= near_edge).all())  # only the edge row moves
        self.assertLess((old ^ new).sum(), 0.15 * old.sum())


class WireTest(unittest.TestCase):
    """wire_lines: a part thinner than a pixel becomes a 1 px line (#900)."""

    def test_model_parts_splits_pieces_that_share_no_vertex(self):
        a, b = box_tris(0, 4), box_tris(6, 9)
        v = np.concatenate([a[0], b[0]])
        parts = K.model_parts([(v, np.concatenate([a[1], b[1]]), np.concatenate([a[2], b[2]]), a[3])])
        self.assertEqual(sorted(sum(len(p[0]) for p in part) for part in parts), [2, 2])

    def test_wire_size_and_widen(self):
        part = [box_tris(2, 20, 5.0, 5.3)]  # 18 px long, 0.3 px thick
        m, x0, y0 = K.part_mask(part, np.eye(3), (1, 1, 0, 10), pad=2)
        w, length = K.wire_size(m)
        self.assertLess(w, 0.5)
        self.assertGreater(length, 17)
        thick = K.widen(m, w)
        self.assertAlmostEqual(K.wire_size(thick)[0], 1.0, delta=0.15)

    def test_wire_lines_widens_a_wire_but_not_a_bar(self):
        wire, bar = box_tris(2, 20, 5.0, 5.3), box_tris(2, 20, 0.0, 3.0)
        for part, widened in ((wire, 1), (bar, 0)):
            lit, hi = K.render([part], np.eye(3), 1, 1, 0, 12, 24, 16, {}, hires=True)
            out, (col, cov), n = K.wire_lines(hi, [part], np.eye(3), (1, 1, 0, 12), {})
            self.assertEqual(n, widened)
            if widened:
                self.assertLess(lit[..., 3].max(), 0.5)  # lost at the game's cut before
                col_a = out[:, 10, 3]
                self.assertGreaterEqual(col_a.max(), 0.5)
                # one pixel's worth across, to a sample (SS per pixel) either way
                self.assertAlmostEqual(col_a.sum(), 1.0, delta=1.01 / K.SS)
                np.testing.assert_allclose(out[col_a.argmax(), 10, :3], lit[col_a.argmax(), 10, :3], atol=0.1)
            else:
                np.testing.assert_array_equal(out, lit)


class SkinArtTest(unittest.TestCase):
    """skin_art: an unused drawing in the skin put beside the part whose
    art it sits next to (the MP40's guard, #900)."""

    @staticmethod
    def textured_box():
        """A 16x8 box on screen at (4..20, 4..12) whose UVs cover the left
        half (texels 0..16 x 0..8) of a 32x8 skin; the right half holds a
        dark 4x4 square on light grey, which no triangle uses."""
        idx = np.zeros((8, 32), np.uint8)
        idx[2:6, 22:26] = 1
        pal = np.zeros((256, 3), np.uint8)
        pal[0], pal[1] = 190, 20
        tex = Texture("s.bmp", 0, 32, 8, idx, pal)
        v = np.array([[[4, -4, 0], [20, -4, 0], [20, -12, 0]], [[4, -4, 0], [20, -12, 0], [4, -12, 0]]], float)
        uv = np.array([[[0, 0], [16, 0], [16, 8]], [[0, 0], [16, 8], [0, 8]]], float) / np.array([32, 8])
        return [(v, np.tile([0.0, 0.0, 1.0], (2, 3, 1)), uv, tex)]

    def test_texel_to_screen_is_the_parts_own_map(self):
        M = K.texel_to_screen(self.textured_box(), np.eye(3), (1, 1, 0, 0), "s.bmp", (0, 0, 16, 8))
        np.testing.assert_allclose(np.array([16, 8, 1.0]) @ M, [20, 12], atol=1e-6)
        np.testing.assert_allclose(np.array([0, 0, 1.0]) @ M, [4, 4], atol=1e-6)
        self.assertIsNone(K.texel_to_screen(self.textured_box(), np.eye(3), (1, 1, 0, 0), "s.bmp", (20, 0, 30, 8)))

    def test_skin_art_draws_the_dark_drawing_behind_the_render(self):
        tris = self.textured_box()
        lit, hi = K.render(tris, np.eye(3), 1, 1, 0, 0, 40, 16, {}, hires=True)
        art = K.SkinArt("s.bmp", (20, 0, 28, 8), (0, 0, 16, 8), offset=(2, 0))
        out, (col, cov), n = K.skin_art(hi, tris, np.eye(3), (1, 1, 0, 0), {}, (art,), scale=1)
        self.assertEqual(n, 1)
        # texels 22..26 x 2..6 land at x 26..30 (the box's map) + 2, y 6..10
        a = out[..., 3]
        self.assertTrue((a[6:10, 28:32] == 1).all())
        self.assertEqual(a[6:10, 26].max(), 0)  # the light background isn't drawn (bilinear: half a texel's ramp)
        self.assertEqual(a[:, 33:].max(), 0)
        np.testing.assert_allclose(out[8, 30, :3], 20 / 255 * (K.AMBIENT + K.DIFFUSE * 0.55 / np.linalg.norm((-0.35, 0.75, 0.55))), atol=1e-3)
        np.testing.assert_array_equal(out[4:12, 4:20], lit[4:12, 4:20])  # the render stays in front

    def test_mp40_art_is_in_its_skin(self):
        (art,) = K.ICONS["d_mp40"].skin_art
        self.assertEqual(art.texture, "p_mp40.bmp")
        self.assertTrue(all(0 <= c <= 256 for c in art.art + art.anchor))


class CutPartTest(unittest.TestCase):
    def test_cut_columns_clears_only_its_columns_below_the_row(self):
        rgba = np.ones((10, 12, 4))
        out = K.cut_columns(rgba, 4, 8, 6)
        self.assertEqual(out[6:, 4:8, 3].max(), 0)
        self.assertTrue((out[:6, :, 3] == 1).all() and (out[:, :4, 3] == 1).all() and (out[:, 8:, 3] == 1).all())

    def test_no_icon_cuts_a_part_by_default(self):
        # the MP44's shorter magazine is a preview option, not applied
        self.assertEqual({n for n, i in K.ICONS.items() if i.cut_part}, set())


class VPartTest(unittest.TestCase):
    """v_part: the guard and trigger from the first-person model (#900)."""

    @staticmethod
    def gun(k=1.0):
        """A receiver with a grip under it, `k` times the size."""
        a, b = box_tris(0, 20 * k, 0, 4 * k), box_tris(4 * k, 8 * k, -6 * k, 0)
        return [a, b]

    def test_match_v_model_finds_the_scale_and_place(self):
        target = K.silhouette(self.gun(), np.eye(3), 4, 4, 10, 30, 120, 60) >= 0.5
        fit, score = K.match_v_model(self.gun(2.0), np.eye(3), target, (0, 120), 4, scales=(0.3, 1.0))
        self.assertGreater(score, 0.9)
        self.assertAlmostEqual(fit[0] / 2, 1.0, delta=0.04)  # 2 px per unit of the bigger model
        self.assertAlmostEqual(fit[2], 10, delta=2)
        self.assertAlmostEqual(fit[3], 30, delta=2)

    def test_v_side_view_puts_the_muzzle_right_and_the_top_up(self):
        right, up, _ = K.v_side_view(0)
        np.testing.assert_allclose(right, K.V_MUZZLE)
        np.testing.assert_allclose(up, K.V_UP)

    def test_a_missing_v_model_changes_nothing(self):
        tris = self.gun()
        lit, hi = K.render(tris, np.eye(3), 4, 4, 10, 40, 120, 60, {}, hires=True)
        vp = K.VPart("v_none.mdl", (0, 30), (0, 0, 30, 15))
        with tempfile.TemporaryDirectory() as d:
            out, hi2, info = K.v_part(hi, tris, np.eye(3), (4, 4, 10, 40), vp, d, "ultrasharp", None, 4)
        self.assertIsNone(info)
        np.testing.assert_array_equal(hi2[1], hi[1])

    def test_v_parts_in_the_table(self):
        with_v = {n for n, i in K.ICONS.items() if i.v_part is not None}
        self.assertEqual(with_v, {"d_garand", "d_kar", "d_scopedkar", "d_k43", "d_spring", "d_mp44"})
        for n in with_v:
            vp = K.ICONS[n].v_part
            self.assertTrue(vp.model.startswith("v_") and vp.model.endswith(".mdl"), n)
            x0, y0, x1, y1 = vp.box
            self.assertTrue(vp.window[0] <= x0 < x1 <= vp.window[1] and 0 <= y0 < y1, n)


class RecordTest(unittest.TestCase):
    def test_source_for(self):
        with tempfile.TemporaryDirectory() as d:
            tga = os.path.join(d, "weapons3_0_12345678.tga")
            with open(tga, "wb") as f:
                f.write(b"upscale")
            self.assertEqual(K.source_for(tga, None), ("new", tga))
            with open(tga + ".bak", "wb") as f:
                f.write(b"upscale")
            # no record but a .bak: the .bak is the upscale, never overwritten
            self.assertEqual(K.source_for(tga, None), ("bak", tga + ".bak"))
            with open(tga, "wb") as f:
                f.write(b"painted")
            entry = {"version": K.VERSION, "sha": K.file_sha(tga)}
            self.assertEqual(K.source_for(tga, entry), ("skip", None))
            self.assertEqual(K.source_for(tga, {**entry, "version": K.VERSION - 1}), ("bak", tga + ".bak"))
            # the sheet was rebuilt by sprites_hd.py since: a fresh upscale
            with open(tga, "wb") as f:
                f.write(b"new upscale")
            self.assertEqual(K.source_for(tga, entry), ("new", tga))

    def test_load_record_tolerates_junk(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, K.RECORD)
            self.assertEqual(K.load_record(p), {})
            with open(p, "w") as f:
                f.write("[1, 2")
            self.assertEqual(K.load_record(p), {})
            with open(p, "w") as f:
                json.dump({"ultrasharp": {}}, f)
            self.assertEqual(K.load_record(p), {"ultrasharp": {}})


class EvenLayoutTest(unittest.TestCase):
    """The even layout's atlas (#1022): which rule each icon gets, the
    packing, and the sprite and table the hook reads."""

    def test_every_icon_gets_the_rule_its_class_has(self):
        self.assertLessEqual(K.LONG_GUNS | K.PISTOLS | K.LAUNCHERS, set(K.ICONS))
        self.assertEqual(len(K.LONG_GUNS), 21)
        for name in K.ICONS:
            lay = K.even_layout(name)
            self.assertEqual(lay.height, K.EVEN_HEIGHT, name)
            if name in K.LONG_GUNS:
                self.assertEqual((lay.barrel_row, lay.size), (K.EVEN_BARREL_ROW, "width"), name)
            elif name in K.PISTOLS or name in K.LAUNCHERS:
                self.assertEqual((lay.barrel_row, lay.size), (K.EVEN_BARREL_ROW, "natural"), name)
            else:  # knives, the spade, grenades: their own size, centred
                self.assertTrue(K.NOT_LEVELLED.search(name), name)
                self.assertEqual((lay.barrel_row, lay.size), (None, "natural"), name)
            self.assertEqual(lay.below, "move" if name in K.MOVE_TO_FIT else "shrink", name)
        self.assertEqual(K.MOVE_TO_FIT, {"d_bren", "d_bazooka", "d_pschreck", "d_piat"})

    def test_pack_keeps_boxes_apart_and_inside_their_frame(self):
        sizes = {f"d_{i}": (30 + (i * 37) % 60, 26) for i in range(40)}
        spots, count = K.pack(sizes)
        self.assertEqual(set(spots), set(sizes))
        self.assertGreater(count, 1)
        g = K.ATLAS_GAP
        for k, (f, x, y) in spots.items():
            w, h = sizes[k]
            self.assertTrue(0 <= f < count)
            self.assertTrue(x >= g and y >= g and x + w + g <= K.ATLAS_SIZE and y + h + g <= K.ATLAS_SIZE, k)
            for k2, (f2, x2, y2) in spots.items():
                if k2 == k or f2 != f:
                    continue
                w2, h2 = sizes[k2]
                apart = x + w + g <= x2 or x2 + w2 + g <= x or y + h + g <= y2 or y2 + h2 + g <= y
                self.assertTrue(apart, f"{k} and {k2} closer than the gap")
        # The same sizes always pack the same way, whatever order they come in.
        again, _ = K.pack(dict(reversed(list(sizes.items()))))
        self.assertEqual(again, spots)

    def test_pack_refuses_a_box_bigger_than_a_frame(self):
        with self.assertRaises(ValueError):
            K.pack({"d_big": (K.ATLAS_SIZE, 26)})
        with self.assertRaises(ValueError):
            K.pack({"d_none": (0, 26)})

    def test_the_sprite_reads_back_as_valves_sheets_are(self):
        from goldsrc import Sprite
        boxes = [(0, 2, 2, 71, 26), (1, 2, 2, 30, 26)]
        frames = K.placeholder_frames(boxes, 2)
        pal = K.atlas_palette()
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, K.ATLAS_NAME + ".spr")
            K.write_whole(p, K.spr_bytes(frames, pal))
            s = Sprite(p)
        self.assertEqual((s.type, s.tex_format, s.width, s.height, s.numframes), (2, 3, 256, 256, 2))
        self.assertEqual(s.palette, pal)
        self.assertEqual([f[:3] for f in s.frames], [(0, 256, 256), (1, 256, 256)])
        ind = np.frombuffer(s.frames[0][3], np.uint8).reshape(256, 256)
        self.assertTrue((ind[2:28, 2:73] == K.PLACEHOLDER).all())
        self.assertEqual(int((ind != 255).sum()), 71 * 26, "only the boxes show")
        with self.assertRaises(ValueError):
            K.spr_bytes([b"\0" * 10], pal)

    def test_the_placeholder_depends_on_the_rects_alone(self):
        # The HD key is a hash of these bytes, so any style's build with the
        # same rects names its frames the same.
        a = K.placeholder_frames([(0, 2, 2, 71, 26)], 1)
        self.assertEqual(a, K.placeholder_frames([(0, 2, 2, 71, 26)], 1))
        self.assertNotEqual(a, K.placeholder_frames([(0, 2, 2, 72, 26)], 1))

    def test_the_table_lists_each_icon_once_with_a_header(self):
        text = K.table_text({"d_mp44": (0, 2, 2, 59, 26), "d_colt": (1, 2, 30, 30, 26)}, 2)
        lines = [l for l in text.splitlines() if not l.startswith("//")]
        self.assertEqual(lines, [f"{K.ATLAS_NAME} {K.ATLAS_TABLE_VERSION} 2", "d_colt 1 2 30 30 26",
                                 "d_mp44 0 2 2 59 26"])
        text.encode("ascii")

    def test_fit_box_shrinks_only_a_taller_icon_and_centres_it(self):
        img = np.ones((34 * 4, 72 * 4, 4))
        out, (w, h) = K.fit_box(img, 72, 34, 26, 4)
        self.assertEqual((w, h), (56, 26), "72 x 26/34 = 55.06, rounded up")
        self.assertEqual(out.shape, (26 * 4, 56 * 4, 4))
        self.assertGreater(out[..., 3].sum(), 0.95 * 26 * 4 * 55 * 4)
        small = np.ones((20 * 4, 84 * 4, 4))
        out, (w, h) = K.fit_box(small, 84, 20, 26, 4)
        self.assertEqual((w, h), (84, 26))
        rows = np.nonzero(out[..., 3].any(1))[0]
        self.assertEqual((rows.min(), rows.max() + 1), (12, 92), "20 rows centred in 26, at 4x")


if __name__ == "__main__":
    unittest.main()
