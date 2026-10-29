"""Kill-feed icons with no weapon model to render whole (#1088): the
suicide skull, the two bayonet close-ups, the butt stroke and the mortars,
composed from model renders in killfeed_icons.py's look (the HD skin,
the same light, light inner edge and dark outline), so they sit beside the
rendered weapon icons instead of the #733 upscale of Valve's drawing.

Each icon is drawn at `scale`x its hud.txt rect, as render_icon draws a
weapon, and goes where render_icon's would: painted into the HD sheet
(build) and, through fit_box, into the even layout's atlas (build_even).

  d_skull, d_world   a skull and crossbones from Half-Life's own bone gibs
                     (valve/models/bonegibs.mdl: the skull body without its
                     spine, over two of its long bones crossed): Valve's
                     drawing, rendered. d_world is the same drawing.
  d_bayonet          the front of the K98 render with its bayonet on, cut
  d_enfbayonet       to Valve's close-up; the Enfield's likewise
  d_garandbutt       the stock of the killer's rifle meeting the victim's
                     head, turned 60 degrees and chin up 35, his helmet
                     knocked off and flying back. DeathMsg sends one weapon
                     (42) for both rifles' butt strokes, so the even layout
                     carries four variants (BUTT_VARIANTS) and the hook
                     picks one per line from the killer's held rifle and
                     the victim's team (deathmsg/butt.rs); the HD sheet's
                     one rect holds the commonest, a Garand on an Axis head.
                     British heads are #1089.
  d_mortar           the mortar's world model (hk_mortar.mdl) side-on, as
  d_germortar        Valve's composite lays it out (baseplate left, tube,
                     bipod), without the separate shell; both teams' icons
                     are one drawing in Valve's sheets too.
"""
import os, re
from dataclasses import dataclass

import numpy as np

import killfeed_icons as K
from studiomodel import MdlError, StudioModel

# Bump when any composite's look changes (killfeed_icons.VERSION and
# EVEN_VERSION include it).
COMPOSITES_VERSION = 1

# The atlas names of the butt-stroke variants: (killer's rifle, victim's
# head). d_garandbutt is hud.txt's own name, the rest are the atlas's, all
# drawn in d_garandbutt's rect and trimmed to one box so any of them fits
# the line the game laid out for d_garandbutt (deathmsg/even.rs).
BUTT_VARIANTS = {
    "d_garandbutt": ("garand", "axis"),
    "d_garandbutt_us": ("garand", "us"),
    "d_k43butt_axis": ("k43", "axis"),
    "d_k43butt_us": ("k43", "us"),
}
BUTT_RECT = "d_garandbutt"

# The rifles whose stock is drawn: the weapon table's icon (for the view
# its render uses) and that icon's hud.txt name (Valve's silhouette).
BUTT_RIFLES = {"garand": "d_garand", "k43": "d_k43"}
# The victim's head: the player model and the `helmet` bodypart's body that
# shows his own hair (what a head shot that knocks the helmet off leaves),
# and the helmet prop flying off.
HEADS = {
    "axis": ("axis-inf", 2, "helmet_axis.mdl"),
    "us": ("us-inf", 3, "helmet_us.mdl"),
}
HEAD_TURN, HEAD_CHIN_UP = 60.0, 35.0  # degrees: the face towards the viewer, as Valve's cartoon
HELMET_VIEW = (20.0, 10.0, -40.0)  # turn, tip back, roll: "tipped, flying back"
# Where each piece goes, as fractions of the rect (x0, y0, x1, y1): the
# stock's back end runs into the head's cheek, the helmet is behind the
# head, level with its top (the R&D sheet's "flying back", #1088).
BUTT_STOCK_BOX = (0.03, 0.40, 0.54, 0.90)
BUTT_HEAD_BOX = (0.47, 0.20, 0.77, 1.00)
BUTT_HELMET_BOX = (0.73, 0.14, 1.00, 0.66)
BUTT_STOCK_LENGTH = 0.26  # of the rifle, from the butt plate

# The bayonet close-ups: how much of the gun's length, from the bayonet's
# tip back, the rect shows (Valve's shows the forend, the bands and the
# bayonet).
BAYONETS = {
    "d_bayonet": (K.Icon(("p_k98.mdl",), ("w_98k.mdl",)), "d_kar", 0.50),
    "d_enfbayonet": (K.Icon(("p_enfield.mdl",), ("w_enfield.mdl",), turn=-4.0), "d_enfield", 0.50),
}

SKULLS = ("d_skull", "d_world")
SKULL_MODEL = "bonegibs.mdl"  # in valve/models
SKULL_BODY, BONE_BODY = 4, 1  # its `parts` bodies: the skull and spine, a long bone
SKULL_DROP = r"spine"  # the skull body's spine, left out
SKULL_BOX = (0.24, 0.02, 0.76, 0.86)
BONE_ANGLE = 32.0  # degrees each bone is turned from level
SKULL_TURN = -80.0  # degrees about its up axis: the face to the viewer, a touch to one side

MORTARS = ("d_mortar", "d_germortar")
MORTAR_MODEL = "hk_mortar.mdl"

NAMES = frozenset(SKULLS) | frozenset(BAYONETS) | frozenset(BUTT_VARIANTS) | frozenset(MORTARS)


@dataclass
class Canvas:
    """The rect being drawn: its size in stock px, the scale, and the rows
    another sprite shares (left empty)."""
    w: int
    h: int
    scale: int
    top: int = 0
    bottom: int | None = None

    @property
    def size(self):
        return self.w * self.scale, self.h * self.scale

    def box(self, frac):
        """A box (x0, y0, x1, y1) in canvas px from rect fractions, kept
        inside the free rows and the outline's margin."""
        W, H = self.size
        m = K.OUTLINE
        top = self.top * self.scale + m
        bottom = (self.bottom if self.bottom is not None else self.h) * self.scale - m
        span = bottom - top
        x0, y0, x1, y1 = frac
        return (max(m, x0 * W), top + y0 * span, min(W - m, x1 * W), top + y1 * span)


def textures_of(tris):
    return sorted({id(t[3]): t[3] for t in tris}.values(), key=lambda t: t.name)


def layer(tris, basis, fit, canvas, skins, gun=False):
    """One piece rendered on the whole canvas with `fit`, finished as the
    weapon icons are (rim light, outline, soft edges)."""
    W, H = canvas.size
    lit, hi = K.render(tris, basis, *fit, W, H, skins, hires=True)
    if gun:
        lit = K.keep_thin(lit)
    lit = K.drop_specks(lit, corners=gun)
    return K.soft_finish(lit, *hi)


def over(base, top):
    """`top` (straight RGBA float) composited over `base`."""
    a = top[..., 3:4]
    out_a = a + base[..., 3:4] * (1 - a)
    rgb = (top[..., :3] * a + base[..., :3] * base[..., 3:4] * (1 - a)) / np.maximum(out_a, 1e-9)
    return np.dstack([np.where(out_a > 0, rgb, 0), out_a])


def project(tris, basis):
    return np.concatenate([t[0].reshape(-1, 3) for t in tris]) @ basis.T


def fit_part(pts, box, keep_right=False):
    """(k, k, ox, oy) fitting the projected points `pts` in `box`, centred;
    with `keep_right`, their right end on the box's right edge instead."""
    lo, hi = pts[:, :2].min(0), pts[:, :2].max(0)
    bw, bh = box[2] - box[0], box[3] - box[1]
    k = min(bw / max(hi[0] - lo[0], 1e-6), bh / max(hi[1] - lo[1], 1e-6))
    cy = (box[1] + box[3]) / 2
    my = (lo[1] + hi[1]) / 2
    if keep_right:
        return k, k, box[2] - k * hi[0], cy + k * my
    cx = (box[0] + box[2]) / 2
    return k, k, cx - k * (lo[0] + hi[0]) / 2, cy + k * my


def end_fit(tris, basis, box, fraction, right_end=True):
    """The fit that shows the last `fraction` of the gun's length (its right
    end, the muzzle in a weapon view) filling `box`, that end on the box's
    right edge; the rest runs off the canvas on the left."""
    pts = project(tris, basis)
    x0, x1 = pts[:, 0].min(), pts[:, 0].max()
    cut = x1 - fraction * (x1 - x0)
    return fit_part(pts[pts[:, 0] >= cut], box, keep_right=right_end)


def flipped(basis):
    """The same view from the gun's other side: the muzzle on the left."""
    right, up, toward = basis
    return np.array([-right, up, -toward])


def gun_view(dod, icon_name, icon, log):
    """(tris, basis, model) the weapon table's view of `icon_name`'s gun,
    `icon` standing in for its table entry, or None."""
    sheet, x, y, w, h = K.hud_rect(dod, icon_name)
    stock, _, _ = K.stock_icon(dod, sheet, x, y, w, h)
    target = K.main_component(stock[..., 3])
    if not target.any():
        return None
    ys, xs = np.nonzero(target)
    m = K.OUTLINE / 4
    best = K.best_view(dod, icon_name, icon, target, (xs.min() + m, m, xs.max() + 1 - m, h - m), log)
    if best is None:
        return None
    tris, _, _, basis, path, _ = best
    return tris, basis, os.path.basename(path)


# ── the pieces ───────────────────────────────────────────────────────────────


def bayonet(dod, name, canvas, style, models_dir, log):
    icon, view_of, fraction = BAYONETS[name]
    view = gun_view(dod, view_of, icon, log)
    if view is None:
        return None, "no model in this install"
    tris, basis, model = view
    skins = K.hd_skins(textures_of(tris), style, models_dir)
    box = canvas.box((0.0, 0.0, 1.0, 1.0))
    box = (0.0, box[1], box[2], box[3])  # the cut end runs to the left edge
    img = layer(tris, basis, end_fit(tris, basis, box, fraction), canvas, skins, gun=True)
    return img, {"model": model, "shows": f"the front {fraction:.0%} of the gun"}


def head_tris(dod, team):
    """The victim's head with his own hair (no helmet), and the helmet prop."""
    model, hair, prop = HEADS[team]
    path = os.path.join(dod, "models", "player", model, model + ".mdl")
    m = StudioModel(path, bodies={"helmet": hair})
    m.parts = [p for p in m.parts if p[0] in ("head", "helmet")]
    tris = m.triangles()
    hair_part = StudioModel(path, bodies={"helmet": hair})
    hair_part.parts = [p for p in hair_part.parts if p[0] == "helmet"]
    hp = np.concatenate([t[0].reshape(-1, 3) for t in hair_part.triangles()])
    # The face skin also covers the hands, posed far from the head: keep
    # what lies near the hair, which only the head does.
    centre = hp.mean(0)
    reach = max(np.linalg.norm(hp.max(0) - hp.min(0)) * 0.85, 9.0)
    kept = []
    for pos, nrm, uv, tex in tris:
        near = np.linalg.norm(pos.mean(1) - centre, axis=1) < reach
        if near.any():
            kept.append((pos[near], nrm[near], uv[near], tex))
    helmet = StudioModel(os.path.join(dod, "models", prop)).triangles()
    return kept, helmet


def rz(d):
    r = np.radians(d)
    return np.array([[np.cos(r), -np.sin(r), 0], [np.sin(r), np.cos(r), 0], [0, 0, 1]])


def ry(d):
    r = np.radians(d)
    return np.array([[np.cos(r), 0, np.sin(r)], [0, 1, 0], [-np.sin(r), 0, np.cos(r)]])


def rx(d):
    r = np.radians(d)
    return np.array([[1, 0, 0], [0, np.cos(r), -np.sin(r)], [0, np.sin(r), np.cos(r)]])


def model_view(turn, tip, roll=0.0):
    """A view of a player-model-space prop: face on (+X towards the viewer,
    +Z up), turned `turn` degrees, tipped `tip` degrees (positive: chin or
    front up) and rolled `roll`."""
    X, _, Z = np.eye(3)
    right, up = -X, Z
    base = np.array([right, up, np.cross(right, up)])
    return base @ (rz(turn) @ ry(-tip) @ rx(roll))


def butt(dod, name, canvas, style, models_dir, log):
    rifle, team = BUTT_VARIANTS[name]
    icon_name = BUTT_RIFLES[rifle]
    view = gun_view(dod, icon_name, K.ICONS[icon_name], log)
    if view is None:
        return None, "no rifle model in this install"
    gun, gun_basis, gun_model = view
    try:
        head, helmet = head_tris(dod, team)
    except (OSError, MdlError) as e:
        return None, f"no {HEADS[team][0]} player model ({e})"
    # The stock with its butt plate on the right, against the head.
    stock_basis = flipped(gun_basis)
    gun_skins = K.hd_skins(textures_of(gun), style, models_dir)
    img = layer(gun, stock_basis, end_fit(gun, stock_basis, canvas.box(BUTT_STOCK_BOX), BUTT_STOCK_LENGTH),
                canvas, gun_skins, gun=True)
    head_basis = model_view(HEAD_TURN, HEAD_CHIN_UP)
    head_skins = K.hd_skins(textures_of(head), style, models_dir)
    img = over(img, layer(head, head_basis, fit_part(project(head, head_basis), canvas.box(BUTT_HEAD_BOX)),
                          canvas, head_skins))
    helmet_basis = model_view(*HELMET_VIEW)
    helmet_skins = K.hd_skins(textures_of(helmet), style, models_dir)
    img = over(img, layer(helmet, helmet_basis,
                          fit_part(project(helmet, helmet_basis), canvas.box(BUTT_HELMET_BOX)), canvas, helmet_skins))
    return img, {"model": f"{gun_model} stock, {HEADS[team][0]} head, {HEADS[team][2]}"}


def skull(dod, name, canvas, style, models_dir, log):
    path = os.path.join(os.path.dirname(dod), "valve", "models", SKULL_MODEL)
    try:
        skull_tris = StudioModel(path, bodies={"parts": SKULL_BODY}).triangles()
        bone = StudioModel(path, bodies={"parts": BONE_BODY}).triangles()
    except (OSError, MdlError) as e:
        return None, f"no {SKULL_MODEL} in this install ({e})"
    skull_tris = [t for t in skull_tris if not re.search(SKULL_DROP, t[3].name, re.I)]
    if not skull_tris or not bone:
        return None, f"{SKULL_MODEL} has no skull or bone body"
    X, Y, Z = np.eye(3)
    face_on = np.array([X, Y, np.cross(X, Y)]) @ ry(SKULL_TURN).T  # upright, face to the viewer
    skins = K.hd_skins(textures_of(skull_tris + bone), style, models_dir)
    a1, a2, _ = K.principal_axes(bone)
    img = np.zeros((canvas.size[1], canvas.size[0], 4))
    whole = canvas.box((0.0, 0.0, 1.0, 1.0))
    for turn in (BONE_ANGLE, -BONE_ANGLE):
        basis = K.turned(np.array([a1, a2, np.cross(a1, a2)]), turn)
        img = over(img, layer(bone, basis, fit_part(project(bone, basis), whole), canvas, skins))
    img = over(img, layer(skull_tris, face_on, fit_part(project(skull_tris, face_on), canvas.box(SKULL_BOX)),
                          canvas, skins))
    return img, {"model": f"{SKULL_MODEL} skull and crossbones"}


def mortar(dod, name, canvas, style, models_dir, log):
    path = os.path.join(dod, "models", MORTAR_MODEL)
    try:
        tris = StudioModel(path).triangles()
    except (OSError, MdlError) as e:
        return None, f"no {MORTAR_MODEL} in this install ({e})"
    X, Y, Z = np.eye(3)
    side = np.array([Z, X, np.cross(Z, X)])  # baseplate left, tube to the right
    skins = K.hd_skins(textures_of(tris), style, models_dir)
    img = layer(tris, side, fit_part(project(tris, side), canvas.box((0.0, 0.0, 1.0, 1.0))), canvas, skins, gun=True)
    return img, {"model": MORTAR_MODEL}


def compose(dod, name, style, models_dir, scale=4, log=print):
    """(rgba at `scale`x the icon's hud.txt rect, info) for one of NAMES
    (a butt variant in d_garandbutt's rect), or (None, why not). Rows the
    rect shares with another sprite are left empty."""
    rect_name = BUTT_RECT if name in BUTT_VARIANTS else name
    _, _, _, w, h = K.hud_rect(dod, rect_name)
    shared, top, bottom = K.shared_area(K.hud_entries(dod), rect_name)
    canvas = Canvas(w, h, scale, top, bottom)
    if name in SKULLS:
        img, info = skull(dod, name, canvas, style, models_dir, log)
    elif name in BAYONETS:
        img, info = bayonet(dod, name, canvas, style, models_dir, log)
    elif name in BUTT_VARIANTS:
        img, info = butt(dod, name, canvas, style, models_dir, log)
    elif name in MORTARS:
        img, info = mortar(dod, name, canvas, style, models_dir, log)
    else:
        raise KeyError(name)
    if img is None:
        return None, info
    if shared.any():
        img[np.kron(shared, np.ones((scale, scale), bool)), 3] = 0
    return K.bleed(img), info
