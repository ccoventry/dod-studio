"""Read-only GoldSrc studio model (.mdl, IDST version 10) reader: the bone
rest pose (sequence 0, frame 0), body 0 of every bodypart as triangles with
per-vertex positions, normals and texel UVs, and the 8-bit palettized skins
(from a `<name>T.mdl` companion when the model keeps none of its own).

Enough to render a model offline (killfeed_icons.py, #841). Every read is
bounds-checked: a truncated or hostile file raises MdlError, never an
IndexError deep in numpy or a silent garbage mesh.

Layouts (studio.h, version 10): studiohdr_t 244 bytes, mstudiobone_t 112,
mstudioseqdesc_t 176, mstudioseqgroup_t 104, mstudiobodyparts_t 76,
mstudiomodel_t 112, mstudiomesh_t 20, mstudiotexture_t 80, mstudioanim_t 12.
"""
import math, os, struct

import numpy as np

STUDIO_NF_FLATSHADE = 0x01
STUDIO_NF_CHROME = 0x02
STUDIO_NF_FULLBRIGHT = 0x04
STUDIO_NF_ADDITIVE = 0x20
STUDIO_NF_MASKED = 0x40

MAX_COUNT = 1 << 16  # far above any GoldSrc limit (MAXSTUDIOVERTS is 2048)


class MdlError(ValueError):
    pass


class _Buf:
    def __init__(self, data, what):
        self.d, self.what = data, what

    def unpack(self, fmt, at):
        size = struct.calcsize(fmt)
        if at < 0 or at + size > len(self.d):
            raise MdlError(f"{self.what}: read of {size} bytes at {at} past the end ({len(self.d)})")
        return struct.unpack_from(fmt, self.d, at)

    def bytes(self, at, n):
        if at < 0 or n < 0 or at + n > len(self.d):
            raise MdlError(f"{self.what}: {n} bytes at {at} past the end ({len(self.d)})")
        return self.d[at:at + n]

    def name(self, at, n):
        return self.bytes(at, n).split(b"\0")[0].decode("latin1")

    def array(self, dtype, at, count):
        dt = np.dtype(dtype)
        return np.frombuffer(self.bytes(at, dt.itemsize * count), dt, count)


def _count(n, what):
    if not 0 <= n <= MAX_COUNT:
        raise MdlError(f"{what} count {n} out of range")
    return n


def angle_matrix(a):
    """studio's AngleMatrix: angles are (roll x, pitch y, yaw z) in radians,
    R = Rz(yaw) Ry(pitch) Rx(roll)."""
    sr, cr = math.sin(a[0]), math.cos(a[0])
    sp, cp = math.sin(a[1]), math.cos(a[1])
    sy, cy = math.sin(a[2]), math.cos(a[2])
    return np.array([
        [cp * cy, sr * sp * cy - cr * sy, cr * sp * cy + sr * sy],
        [cp * sy, sr * sp * sy + cr * cy, cr * sp * sy - sr * cy],
        [-sp, sr * cp, cr * cp],
    ])


class Texture:
    def __init__(self, name, flags, width, height, indices, palette):
        self.name, self.flags, self.width, self.height = name, flags, width, height
        self.indices, self.palette = indices, palette  # (h, w) uint8, (256, 3) uint8

    def rgba(self):
        rgb = self.palette[self.indices]
        a = np.full(self.indices.shape, 255, np.uint8)
        if self.flags & STUDIO_NF_MASKED:
            a[self.indices == 255] = 0
        return np.dstack([rgb, a])


class Mesh:
    """Triangles of one mesh: tris (n, 3) indices into the model's vertex
    arrays; st (n, 3, 2) texel coordinates; normals (n, 3) indices."""

    def __init__(self, skin, verts, norms, st):
        self.skin, self.verts, self.norms, self.st = skin, verts, norms, st


class StudioModel:
    def __init__(self, path, bodies=None):
        with open(path, "rb") as f:
            b = _Buf(f.read(), os.path.basename(path))
        self.path = path
        if b.unpack("<4s", 0)[0] != b"IDST":
            raise MdlError(f"{b.what}: not a studio model")
        (version,) = b.unpack("<i", 4)
        if version != 10:
            raise MdlError(f"{b.what}: version {version}, only 10 is read")
        self.name = b.name(8, 64)
        (self.numbones, self.boneindex) = b.unpack("<ii", 140)
        (self.numseq, self.seqindex) = b.unpack("<ii", 164)
        (numtex, texindex) = b.unpack("<ii", 180)
        (numskinref, numskinfam, skinindex) = b.unpack("<iii", 192)
        (numbodyparts, bodypartindex) = b.unpack("<ii", 204)
        _count(self.numbones, "bone")

        self.bones = self._bones(b)
        self.bone_mats = self._pose(b)
        self.textures, self.skins = self._textures(path, b, numtex, texindex, numskinref, numskinfam, skinindex)
        self.parts = self._bodyparts(b, _count(numbodyparts, "bodypart"), bodypartindex, bodies or {})

    # ── bones and rest pose ──────────────────────────────────────────────────

    def _bones(self, b):
        bones = []
        for i in range(self.numbones):
            at = self.boneindex + 112 * i
            name = b.name(at, 32)
            (parent,) = b.unpack("<i", at + 32)
            value = b.unpack("<6f", at + 64)
            scale = b.unpack("<6f", at + 88)
            if parent >= i or parent < -1:
                raise MdlError(f"{b.what}: bone {i} has parent {parent}")
            bones.append((name, parent, value, scale))
        return bones

    def _frame0(self, b):
        """Per bone, the 6 values (pos xyz, rot xyz) of sequence 0, frame 0,
        when sequence 0's animation lives in this file (seqgroup 0);
        otherwise the bones' default values."""
        vals = [list(v) for _, _, v, _ in self.bones]
        if self.numseq <= 0:
            return vals
        (seqgroup,) = b.unpack("<i", self.seqindex + 156)
        (numblends, animindex) = b.unpack("<ii", self.seqindex + 120)
        if seqgroup != 0 or numblends < 1:
            return vals
        for i, (_, _, value, scale) in enumerate(self.bones):
            at = animindex + 12 * i
            offsets = b.unpack("<6H", at)
            for j in range(6):
                if offsets[j] == 0:
                    continue
                # mstudioanimvalue_t run: {valid, total} bytes then `valid`
                # shorts; frame 0 is the first one.
                valid, total = b.unpack("<BB", at + offsets[j])
                if valid == 0 or total == 0:
                    continue
                (raw,) = b.unpack("<h", at + offsets[j] + 2)
                vals[i][j] = value[j] + raw * scale[j]
        return vals

    def _pose(self, b):
        vals = self._frame0(b)
        mats = []
        for i, (_, parent, _, _) in enumerate(self.bones):
            m = np.eye(4)
            m[:3, :3] = angle_matrix(vals[i][3:6])
            m[:3, 3] = vals[i][0:3]
            mats.append(m if parent < 0 else mats[parent] @ m)
        return mats

    # ── textures ─────────────────────────────────────────────────────────────

    def _textures(self, path, b, numtex, texindex, numskinref, numskinfam, skinindex):
        if numtex == 0:
            companion = path[:-4] + "T.mdl"
            if not os.path.exists(companion) or os.path.normcase(companion) == os.path.normcase(path):
                return [], []
            with open(companion, "rb") as f:
                b = _Buf(f.read(), os.path.basename(companion))
            if b.unpack("<4s", 0)[0] != b"IDST":
                raise MdlError(f"{b.what}: not a studio model")
            (numtex, texindex) = b.unpack("<ii", 180)
            (numskinref, numskinfam, skinindex) = b.unpack("<iii", 192)
        textures = []
        for i in range(_count(numtex, "texture")):
            at = texindex + 80 * i
            name = b.name(at, 64)
            flags, w, h, off = b.unpack("<iiii", at + 64)
            if not (0 < w <= 4096 and 0 < h <= 4096):
                raise MdlError(f"{b.what}: texture {name} is {w}x{h}")
            idx = b.array(np.uint8, off, w * h).reshape(h, w)
            pal = b.array(np.uint8, off + w * h, 768).reshape(256, 3)
            textures.append(Texture(name, flags, w, h, idx, pal))
        _count(numskinref, "skinref")
        skins = []
        if _count(numskinfam, "skin family") and numskinref:
            skins = b.array("<i2", skinindex, numskinref * numskinfam).reshape(numskinfam, numskinref).tolist()
        return textures, skins

    def texture_for(self, skinref, family=0):
        if self.skins and 0 <= family < len(self.skins) and 0 <= skinref < len(self.skins[family]):
            skinref = self.skins[family][skinref]
        return self.textures[skinref] if 0 <= skinref < len(self.textures) else None

    # ── geometry ─────────────────────────────────────────────────────────────

    def _bodyparts(self, b, n, index, bodies):
        parts = []
        for i in range(n):
            at = index + 76 * i
            name = b.name(at, 64)
            nummodels, _base, modelindex = b.unpack("<iii", at + 64)
            _count(nummodels, "model")
            pick = bodies.get(name, 0)
            if not 0 <= pick < nummodels:
                continue
            parts.append((name, self._model(b, modelindex + 112 * pick)))
        return parts

    def _model(self, b, at):
        name = b.name(at, 64)
        (nummesh, meshindex, numverts, vertinfo, vertindex,
         numnorms, norminfo, normindex) = b.unpack("<8i", at + 72)
        _count(nummesh, "mesh"), _count(numverts, "vertex"), _count(numnorms, "normal")
        vbone = b.array(np.uint8, vertinfo, numverts).astype(int)
        nbone = b.array(np.uint8, norminfo, numnorms).astype(int)
        if (vbone >= max(self.numbones, 1)).any() or (nbone >= max(self.numbones, 1)).any():
            raise MdlError(f"{b.what}: {name} references a bone past {self.numbones}")
        verts = b.array("<f4", vertindex, numverts * 3).reshape(-1, 3).astype(float)
        norms = b.array("<f4", normindex, numnorms * 3).reshape(-1, 3).astype(float)
        # To model space through each vertex's bone.
        mats = np.array(self.bone_mats) if self.bone_mats else np.eye(4)[None]
        verts = np.einsum("nij,nj->ni", mats[vbone, :3, :3], verts) + mats[vbone, :3, 3]
        norms = np.einsum("nij,nj->ni", mats[nbone, :3, :3], norms)
        meshes = []
        for m in range(nummesh):
            numtris, triindex, skinref, _, _ = b.unpack("<5i", meshindex + 20 * m)
            meshes.append(self._mesh(b, triindex, skinref, numverts, numnorms))
        return {"name": name, "verts": verts, "norms": norms, "meshes": meshes}

    def _mesh(self, b, at, skinref, numverts, numnorms):
        """Tri commands: a short count, negative for a fan, zero to end;
        then |count| x (vertex, normal, s, t) shorts."""
        tv, tn, ts = [], [], []
        for _ in range(MAX_COUNT):
            (n,) = b.unpack("<h", at)
            at += 2
            if n == 0:
                break
            fan, n = n < 0, abs(n)
            cmd = np.array(b.unpack(f"<{4 * n}h", at)).reshape(n, 4)
            at += 8 * n
            if (cmd[:, 0] < 0).any() or (cmd[:, 0] >= numverts).any() or (cmd[:, 1] < 0).any() or (cmd[:, 1] >= numnorms).any():
                raise MdlError(f"{b.what}: tri command index out of range")
            for k in range(n - 2):
                if fan:
                    tri = (0, k + 1, k + 2)
                else:
                    tri = (k, k + 1, k + 2) if k % 2 == 0 else (k + 1, k, k + 2)
                tv.append(cmd[list(tri), 0])
                tn.append(cmd[list(tri), 1])
                ts.append(cmd[list(tri), 2:4])
        else:
            raise MdlError(f"{b.what}: tri commands never end")
        return Mesh(skinref, np.array(tv, int).reshape(-1, 3), np.array(tn, int).reshape(-1, 3),
                    np.array(ts, float).reshape(-1, 3, 2))

    def triangles(self, family=0):
        """[(positions (n,3,3), normals (n,3,3), uv (n,3,2) in 0..1, Texture)]
        per mesh of every bodypart's chosen body."""
        out = []
        for _, model in self.parts:
            for mesh in model["meshes"]:
                tex = self.texture_for(mesh.skin, family)
                if tex is None or not len(mesh.verts):
                    continue
                uv = mesh.st / np.array([tex.width, tex.height], float)
                out.append((model["verts"][mesh.verts], model["norms"][mesh.norms], uv, tex))
        return out
