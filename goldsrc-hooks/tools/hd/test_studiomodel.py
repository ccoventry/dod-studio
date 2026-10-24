"""studiomodel.py on a tiny synthetic model: one bone, one 2x2 texture, one
mesh drawn as a strip and as a fan, and truncated copies of it.

usage: python -m unittest test_studiomodel   (from this folder)
"""
import os, struct, tempfile, unittest

import numpy as np

from studiomodel import MdlError, StudioModel

VERTS = [(0, 0, 0), (10, 0, 0), (0, 10, 0), (10, 10, 0)]


def build_mdl(fan=False, bone_pos=(1.0, 2.0, 3.0), bone_rot=(0.0, 0.0, 0.0)):
    out = bytearray(244)

    def put(data, align=4):
        while len(out) % align:
            out.append(0)
        at = len(out)
        out.extend(data)
        return at

    bone = put(b"root".ljust(32, b"\0") + struct.pack("<ii6i6f6f", -1, 0, *([-1] * 6),
                                                     *bone_pos, *bone_rot, *([1.0] * 6)))
    tex = put(bytes(80))
    pixels = put(bytes([0, 1, 2, 255]) + bytes(range(256)) * 3, align=1)
    struct.pack_into("<64siiii", out, tex, b"t.bmp", 0, 2, 2, pixels)
    skins = put(struct.pack("<h", 0))
    part = put(bytes(76))
    model = put(bytes(112))
    mesh = put(bytes(20))
    vertinfo = put(bytes(len(VERTS)))
    verts = put(struct.pack(f"<{3 * len(VERTS)}f", *[c for v in VERTS for c in v]))
    norminfo = put(bytes(1))
    norms = put(struct.pack("<3f", 0, 0, 1))
    order = [0, 1, 3, 2] if fan else [0, 1, 2, 3]
    cmds = struct.pack("<h", -4 if fan else 4)
    for v in order:
        cmds += struct.pack("<4h", v, 0, VERTS[v][0] // 5, VERTS[v][1] // 5)
    tris = put(cmds + struct.pack("<h", 0))

    struct.pack_into("<64siii", out, part, b"body", 1, 1, model)
    struct.pack_into("<64sif10i", out, model, b"m", 0, 0.0, 1, mesh, len(VERTS), vertinfo, verts,
                     1, norminfo, norms, 0, 0)
    struct.pack_into("<5i", out, mesh, 2, tris, 0, 1, 0)
    struct.pack_into("<4si64si", out, 0, b"IDST", 10, b"synthetic.mdl", len(out))
    struct.pack_into("<ii", out, 140, 1, bone)
    struct.pack_into("<ii", out, 164, 0, 0)
    struct.pack_into("<iii", out, 180, 1, tex, pixels)
    struct.pack_into("<iii", out, 192, 1, 1, skins)
    struct.pack_into("<ii", out, 204, 1, part)
    return bytes(out)


class StudioModelTest(unittest.TestCase):
    def load(self, data):
        fd, path = tempfile.mkstemp(suffix=".mdl")
        with os.fdopen(fd, "wb") as f:
            f.write(data)
        try:
            return StudioModel(path)
        finally:
            os.remove(path)

    def test_strip(self):
        m = self.load(build_mdl())
        (pos, nrm, uv, tex), = m.triangles()
        self.assertEqual(pos.shape, (2, 3, 3))
        # Bone translation applied to every vertex.
        self.assertTrue(np.allclose(pos[0], np.array([VERTS[0], VERTS[1], VERTS[2]]) + (1, 2, 3)))
        # Odd strip triangles swap their first two vertices.
        self.assertTrue(np.allclose(pos[1], np.array([VERTS[2], VERTS[1], VERTS[3]]) + (1, 2, 3)))
        self.assertTrue(np.allclose(nrm, (0, 0, 1)))
        # s, t in texels over the 2x2 skin.
        self.assertTrue(np.allclose(uv[0], [(0, 0), (1, 0), (0, 1)]))
        self.assertEqual((tex.name, tex.width, tex.height), ("t.bmp", 2, 2))
        self.assertEqual(tex.indices.tolist(), [[0, 1], [2, 255]])
        # The palette bytes are 0..255 three times over: entry 2 is bytes 6..8.
        self.assertEqual(tex.palette[2].tolist(), [6, 7, 8])

    def test_fan(self):
        m = self.load(build_mdl(fan=True))
        (pos, _, _, _), = m.triangles()
        self.assertEqual(pos.shape, (2, 3, 3))
        self.assertTrue(np.allclose(pos[:, 0], np.array(VERTS[0]) + (1, 2, 3)))

    def test_bone_rotation(self):
        # 90 degrees of yaw (z) turns +x into +y.
        m = self.load(build_mdl(bone_pos=(0, 0, 0), bone_rot=(0, 0, np.pi / 2)))
        (pos, _, _, _), = m.triangles()
        self.assertTrue(np.allclose(pos[0][1], (0, 10, 0), atol=1e-5))

    def test_truncated_raises(self):
        data = build_mdl()
        for cut in (8, 200, 300, 900, len(data) - 30, len(data) - 1):
            with self.assertRaises(MdlError, msg=f"cut at {cut}"):
                self.load(data[:cut])

    def test_bad_vertex_index_raises(self):
        data = bytearray(build_mdl())
        # Point the first tri command's vertex past the 4 vertices.
        at = data.rindex(struct.pack("<h", 4) + struct.pack("<4h", 0, 0, 0, 0))
        struct.pack_into("<h", data, at + 2, 9)
        with self.assertRaises(MdlError):
            self.load(bytes(data))

    def test_not_a_model(self):
        with self.assertRaises(MdlError):
            self.load(b"IDSP" + bytes(300))


if __name__ == "__main__":
    unittest.main()
