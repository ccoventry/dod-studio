"""Read-only GoldSrc file readers: map textures (BSP + WAD3), studio model
skins (.mdl) and sprites (.spr). Each mirrors what the pre-Anniversary
hw.dll hands to GL_LoadTexture2, because the replacement key is a hash of
exactly those bytes.
"""
import os, re, struct

# ── Map textures ─────────────────────────────────────────────────────────────


def read_miptex(buf, at):
    """Mirrors Mod_LoadTextures: mip0 at +40, palette at +40 + pix/64*85 + 2."""
    name = buf[at:at + 16].split(b"\0")[0].decode("latin1")
    w, h = struct.unpack_from("<II", buf, at + 16)
    pix = w * h
    idx = buf[at + 40: at + 40 + pix]
    pal_at = at + 40 + (pix >> 6) * 85 + 2
    pal = buf[pal_at: pal_at + 768]
    return name, w, h, bytes(idx), bytes(pal)


_wads = {}


def wad_lookup(path):
    """{lowercased name: (offset, size)} for a WAD3 file."""
    if path not in _wads:
        entries = {}
        with open(path, "rb") as f:
            hdr = f.read(12)
            if hdr[:4] == b"WAD3":
                n, diro = struct.unpack_from("<ii", hdr, 4)
                f.seek(diro)
                d = f.read(32 * n)
                for i in range(n):
                    off, disk = struct.unpack_from("<ii", d, 32 * i)
                    nm = d[32 * i + 16: 32 * i + 32].split(b"\0")[0].decode("latin1").lower()
                    entries[nm] = (off, disk)
        _wads[path] = entries
    return _wads[path]


def bsp_path(game, m):
    for g in ("dod", "dod_downloads"):
        p = os.path.join(game, g, "maps", m + ".bsp")
        if os.path.exists(p):
            return p
    raise FileNotFoundError(f"no {m}.bsp under {game}")


def map_textures(game, m, warn=print):
    """[(name, w, h, indices, palette)] for every texture map `m` uses:
    embedded in the BSP, or from the wads its worldspawn lists (searched in
    dod/, then valve/)."""
    d = open(bsp_path(game, m), "rb").read()
    lumps = [struct.unpack_from("<ii", d, 4 + 8 * i) for i in range(15)]
    ents = d[lumps[0][0]: sum(lumps[0])].decode("latin1")
    wm = re.search(r'"wad"\s+"([^"]*)"', ents)
    wads = [os.path.basename(w.replace("\\", "/")) for w in (wm.group(1).split(";") if wm else []) if w]
    to = lumps[2][0]
    n = struct.unpack_from("<i", d, to)[0]
    out = []
    for i in range(n):
        off = struct.unpack_from("<i", d, to + 4 + 4 * i)[0]
        if off < 0:
            continue
        at = to + off
        name = d[at:at + 16].split(b"\0")[0].decode("latin1")
        if struct.unpack_from("<I", d, at + 24)[0]:
            out.append(read_miptex(d, at))
            continue
        for w in wads:
            p = next((os.path.join(game, g, w) for g in ("dod", "valve")
                      if os.path.exists(os.path.join(game, g, w))), None)
            if p and name.lower() in wad_lookup(p):
                o, disk = wad_lookup(p)[name.lower()]
                with open(p, "rb") as f:
                    f.seek(o)
                    out.append(read_miptex(f.read(disk), 0))
                break
        else:
            warn(f"  {m}: {name} not found in any wad, skipped")
    return out


def skyname(game, m):
    """The map's worldspawn skyname (GoldSrc's default is desert)."""
    d = open(bsp_path(game, m), "rb").read()
    o, l = struct.unpack_from("<ii", d, 4)
    hit = re.search(r'"skyname"\s+"([^"]*)"', d[o:o + l].decode("latin1"))
    return hit.group(1).lower() if hit else "desert"


# ── Studio models ────────────────────────────────────────────────────────────

STUDIO_NF_MASKED = 0x40


def mdl_textures(path):
    """[(texture name, flags, w, h, indices, palette)] for a .mdl (version
    10), following a `<name>T.mdl` companion when the model keeps none of its
    own. Mirrors Mod_LoadStudioModel: mstudiotexture_t is 80 bytes -- name[64],
    flags, width, height, index -- and the palette follows the pixels."""
    d = open(path, "rb").read()
    if d[:4] != b"IDST":
        return []
    count, index = struct.unpack_from("<ii", d, 0xB4)
    if count == 0 or index == 0:
        t = path[:-4] + "T.mdl"
        return mdl_textures(t) if os.path.exists(t) and t != path else []
    out = []
    for i in range(count):
        at = index + 80 * i
        name = d[at:at + 64].split(b"\0")[0].decode("latin1")
        flags, w, h, off = struct.unpack_from("<iiii", d, at + 64)
        if w <= 0 or h <= 0 or off + w * h + 768 > len(d):
            continue
        out.append((name, flags, w, h, bytes(d[off:off + w * h]), bytes(d[off + w * h:off + w * h + 768])))
    return out


# ── Sprites ──────────────────────────────────────────────────────────────────

SPR_NORMAL, SPR_ADDITIVE, SPR_INDEXALPHA, SPR_ALPHTEST = 0, 1, 2, 3
SPR_FORMATS = {0: "normal", 1: "additive", 2: "indexalpha", 3: "alphtest"}


class Sprite:
    """A .spr (IDSP version 2). Every frame is uploaded as `<model>_<n>`, n
    being the frame's index -- or index * 100 + j for frame j of a group --
    with a fresh 768-byte copy of the sprite's palette.

    texFormat -> the engine's iType: normal/additive -> 0 (opaque),
    indexalpha -> 3 (RGB = palette[255], alpha = index), alphtest -> 1
    (index 255 transparent)."""

    def __init__(self, path):
        b = open(path, "rb").read()
        if b[:4] != b"IDSP":
            raise ValueError("not a sprite")
        version, self.type, self.tex_format = struct.unpack_from("<3i", b, 4)
        if version != 2:
            raise ValueError(f"sprite version {version}")
        self.width, self.height, self.numframes = struct.unpack_from("<3i", b, 0x14)
        (self.numcolors,) = struct.unpack_from("<h", b, 0x28)
        self.palette = b[0x2A:0x2A + self.numcolors * 3].ljust(768, b"\0")[:768]
        at = 0x2A + self.numcolors * 3
        self.frames = []  # (n, width, height, indices)

        def frame(n, at):
            _, _, w, h = struct.unpack_from("<4i", b, at)
            at += 16
            if w <= 0 or h <= 0 or at + w * h > len(b):
                raise ValueError(f"bad frame {n}")
            self.frames.append((n, w, h, b[at:at + w * h]))
            return at + w * h

        for i in range(self.numframes):
            (kind,) = struct.unpack_from("<i", b, at)
            at += 4
            if kind == 0:
                at = frame(i, at)
            else:
                (count,) = struct.unpack_from("<i", b, at)
                at += 4 + 4 * count
                for j in range(count):
                    at = frame(i * 100 + j, at)
