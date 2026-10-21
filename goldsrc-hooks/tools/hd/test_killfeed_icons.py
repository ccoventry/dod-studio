"""killfeed_icons.py without the game: the icon table, the muzzle clip, the
edge and outline passes, painting into a sheet, and when a sheet is redone.

usage: python -m unittest test_killfeed_icons   (from this folder)
"""
import json, os, tempfile, unittest

import numpy as np

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
        # no cut: shrunk so the magazine still ends inside the box
        kx, ky, ox, oy = K.place_barrel(tris, basis, fit, alpha, box, 10, cut=False)
        self.assertLessEqual(oy + ky * 6, 40 + 1e-6)
        self.assertAlmostEqual(oy - ky * 1.0, 10, delta=0.6)
        self.assertAlmostEqual(ox + kx * 10, 50)  # still centred across

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


if __name__ == "__main__":
    unittest.main()
