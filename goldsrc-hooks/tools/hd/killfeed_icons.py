"""Kill-feed weapon icons rendered from the weapon models (#841), drawn into
the HD `weapons*` sheets sprites_hd.py makes.

Valve's kill-feed icons are 89x19-pixel drawings; upscaled 4x (#733) they
keep their stair steps, and at `dodstudio_deathmsg scale 3` they look soft.
This renders each icon from the weapon's own model instead, at the 4x size
of its hud.txt box, and paints it into that install's 4x HD sheet over the
upscaled icon. The hook loads the sheet as it loads any HD sprite (keyed by
the stock sheet's pixels), so nothing else changes.

The kill feed draws its icon with SPR_Set(icon, 255, 255, 255) and
SPR_DrawHoles (docs/goldsrc_death_notices.md section 6): the sprite's own
colours, untinted, its transparent texels cut out, blended while
gl_spriteblend is on. The renders carry coverage alpha on every edge
(soft_finish). An alpha-test sheet's replacement is uploaded with its
alpha cut at 128 and the cut-out texels black (texture_hires.rs,
match_engine_expansion), which steps those edges again; #890 keeps the
kill-feed sheets' alpha instead, and only with it do the edges show soft.
The --review sheets simulate the cut.

hud.txt's rects overlap: the MG34's bottom four rows are the K43's top
four. Rows two kill-feed icons share belong to the lower one, and #890
draws the upper one without them; rows shared with any other sprite stay
empty (shared_area).

Per icon (ICONS):
  1. the model: the `p_` one (a bigger skin than `w_`), `w_` where there is
     no `p_`; where a weapon has variants (bipod up / down, scoped), every
     one is tried and the closest to Valve's icon wins;
  2. body 0 of each bodypart, frame 0 of sequence 0 (studiomodel.py);
  3. parts past the muzzle dropped where the table says so (the Kar's
     bayonet tip: Valve's icon has none), clip_tip;
  4. the side-on view: the model's principal axes, each muzzle / up sign
     and a roll, scored by silhouette overlap with Valve's icon, so the
     muzzle points right and the gun fills its box as Valve's does
     (choose_view); then, for guns, the model's own side view, tilted so
     the end of the barrel is level (level_view); then the table's hand
     adjustments, where the user's review asked for them: an extra turn,
     and the barrel put at Valve's height with a long magazine or bipod
     cut off at the bottom (turned, place_barrel);
  5. the skin in the HD style: the model skin models_hd.py built for this
     style when there is one, else upscaled here, else the stock skin;
  6. rendered orthographically, supersampled, two-sided Lambert with a
     bright light (ambient 0.8, diffuse 0.55) so dark metal reads over a
     dark background; a light inner edge (rim_light) inside a 3 px dark
     outline (outline), so the icon reads over a bright one too; a gap
     inside the gun (a trigger guard) keeps only a 1 px rim, so it stays
     open, also where a trigger splits it into narrow gaps (holes), and a
     guard the skin paints black on a solid plate is cut out where the
     table says so (see_through); a part thinner than half a pixel (a
     guard's bar, a wire) is kept as a 1 px line (keep_thin); the light
     edge and the outline are drawn on the supersamples and downsampled
     premultiplied, so every edge ramps over a pixel or two and a partial
     edge texel holds the outline's colour (soft_finish);
  7. in the sheet: Valve's icon and any detached part of it inside the box
     (the Kar's loose bayonet, a rocket beside a launcher) cleared, the
     render painted over; other pixels stay as sprites_hd.py made them.

Icons with no weapon model of their own (KEEP_UPSCALE: the skull, close-ups
and composites) keep the #733 upscale.

Resumable and safe to run again: before the first change to a sheet file,
the upscaled one is kept beside it as `<file>.bak`, and every run paints
over that copy; dodstudio_hd/killfeed_icons.json records which files hold
which version of the icons, so a finished sheet is skipped.

usage: python killfeed_icons.py <sprites out_dir> [--only d_kar,d_mp40]
                                [--review DIR] [--force]
  out_dir   dodstudio_hd/sprites/<style>, where sprites_hd.py put the sheets
  --only    just these icons
  --review  also write comparison sheets (stock | #733 upscale | new, at
            kill-feed scale 1, 2 and 3 on dark and bright) to DIR
  --force   repaint even if the record says the sheet is current
env: HD_STYLE (default ultrasharp), HD_GAME, HD_GAME_FOLDER, REALESRGAN
Reads the game's models and sprites; writes only in dodstudio_hd.
"""
import argparse, hashlib, json, os, re, shutil, sys, tempfile
from dataclasses import dataclass

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from studiomodel import STUDIO_NF_MASKED, MdlError, StudioModel  # noqa: E402

SS = 4  # supersampling per HD pixel
# Bump when the look changes: sheets painted by an older version are
# painted again from their .bak.
VERSION = 10  # 10: soft edges (soft_finish); shared rows belong to the lower kill-feed icon
RECORD = "killfeed_icons.json"

# The look the user picked on #850 (d_kar_recommended).
AMBIENT, DIFFUSE = 0.8, 0.55
RIM_WIDTH, RIM_STRENGTH = 2.5, 0.35  # at 4x
OUTLINE = 3  # px at 4x
OUTLINE_COLOUR = (0.06, 0.06, 0.06)
SPECK = 40  # px at 4x: loose blobs smaller than this are dropped
HOLE_MIN = 12  # px at 4x: a gap inside the icon this big stays open (outline)
HOLE_WIDTH = 5  # px at 4x: ...and at least this wide somewhere (no slits)
HOLE_OUTLINE = 1.5  # px at 4x: the rim inside such a gap (one solid pixel)
HOLE_BRIDGE = 2  # px at 4x: gaps this close (a trigger between them) count as one hole
HOLE_RIM = 1.0  # px at 4x past the edge: soft_finish's rim inside a hole (outline's HOLE_OUTLINE)
# keep_thin: a part thinner than half a pixel (a guard's bar, a wire) never
# reaches the game's alpha cut of 0.5, so it vanishes.
THIN_LO = 0.15  # coverage a thin line's pixel needs
THIN_MARGIN = 0.10  # ...above both of its neighbours across the line
THIN_MIN_RUN = 3  # px at 4x: shorter runs (a lone tip on an edge) aren't kept

ROLLS = tuple(range(-10, 11, 2))
WIDE_ROLLS = tuple(range(-40, 41, 4))  # pistols: the grip pulls the main axis
UPRIGHT_ROLLS = ROLLS + tuple(r + 90 for r in ROLLS) + tuple(r - 90 for r in ROLLS)
# Where a weapon model's muzzle points in model space: a `p_` gun lies along
# -Y (it's held in the hand, with its side up), and Valve's icons all have
# the muzzle on the right. A silhouette alone can't always tell the ends
# apart (the MG34's butt and bipod), so the view keeps the muzzle right
# whenever the model's long axis runs along this one. None leaves it free.
MUZZLE = (0.0, -1.0, 0.0)
# Which way is up in a weapon model: the top of the gun (sights, scope)
# towards +X, the grip and magazine towards -X (level_view).
MODEL_UP = (1.0, 0.0, 0.0)


@dataclass(frozen=True)
class Icon:
    """models: the `p_` variants to try (the best match wins); fallback:
    the `w_` ones, tried when none of `models` exists. clip_tip: drop the
    parts past the muzzle. rolls: the view rolls choose_view tries.
    muzzle: see MUZZLE. skip_tex: meshes whose texture name matches one of
    these regexes are left out (a bayonet with a skin of its own).

    Hand adjustments from the user's review of the levelled set:
    turn: degrees added to the view's roll after it is chosen, positive
    clockwise as seen in the icon (the right end goes down; turned).
    barrel_top: the row (stock pixels from the rect's top, as in Valve's
    icon) the top of the barrel's end is put on, instead of centring the
    whole gun: a tall bipod or magazine otherwise pushes the gun up
    (place_barrel). cut_below: with barrel_top, whatever hangs below the
    rect is cut off at its bottom edge (the outline closes the cut);
    without it the gun keeps its width and is raised until it fits.
    see_through: (texture, x, y) texels inside a hole the skin paints black
    instead of modelling (a trigger guard drawn on a solid plate): the
    near-black texels joined to each are drawn transparent (see_through)."""
    models: tuple
    fallback: tuple = ()
    clip_tip: bool = False
    rolls: tuple = ROLLS
    muzzle: tuple = MUZZLE
    skip_tex: tuple = ()
    turn: float = 0.0
    barrel_top: float | None = None
    cut_below: bool = False
    see_through: tuple = ()
    note: str = ""


ICONS = {
    # Allies
    "d_thompson": Icon(("p_tommy.mdl",), ("w_tommy.mdl",), turn=1.0, see_through=(("p_tommy.bmp", 129, 228),),
                       note="turned: the forend level; trigger guard see-through"),
    "d_greasegun": Icon(("p_grease.mdl",), ("w_greasegun.mdl",)),
    "d_bar": Icon(("p_barbu.mdl", "p_barbd.mdl"), ("w_bar.mdl",), see_through=(("p_bar.bmp", 125, 224),),
                  note="bipod up or down, closest wins; trigger guard see-through"),
    "d_garand": Icon(("p_garand.mdl",), ("w_garand.mdl",)),
    "d_spring": Icon(("p_spring.mdl",), ("w_spring.mdl",)),
    "d_m1carbine": Icon(("p_m1carb.mdl",), ("w_m1carb.mdl",), see_through=(("p_m1carbine.bmp", 102, 47),),
                        note="trigger guard see-through"),
    "d_fcarbine": Icon(("p_fcarb.mdl",), ("w_fcarb.mdl",), note="folding-stock carbine"),
    "d_30cal": Icon(("p_30cal.mdl",), ("w_30cal.mdl",), barrel_top=10, cut_below=True,
                    note="barrel at Valve's height; the taller bipod runs off the bottom"),
    "d_colt": Icon(("p_colt.mdl",), ("w_colt.mdl",), rolls=WIDE_ROLLS, see_through=(("p_colt.bmp", 73, 49),),
                   note="trigger guard see-through"),
    "d_bazooka": Icon(("p_bazooka.mdl",), ("w_bazooka.mdl",), note="no rocket beside it"),
    "d_grenade": Icon(("p_grenade.mdl",), ("w_grenade.mdl",), rolls=UPRIGHT_ROLLS, turn=6.0,
                      note="turned: the grooves level"),
    "d_amerknife": Icon(("p_amerk.mdl",), ("w_amerk.mdl",)),
    # Axis
    "d_kar": Icon(("p_k98.mdl",), ("w_98k.mdl",), clip_tip=True, note="bayonet tip clipped, as Valve's icon"),
    "d_scopedkar": Icon(("p_k98s.mdl",), ("w_scoped98k.mdl",)),
    "d_k43": Icon(("p_k43.mdl",), ("w_k43.mdl",), barrel_top=7,
                  note="barrel top towards the middle line, as Valve's, raised so the stock fits"),
    "d_mp40": Icon(("p_mp40.mdl",), ("w_mp40.mdl",), barrel_top=6, cut_below=True,
                   note="barrel at Valve's height; the longer magazine runs off the bottom"),
    "d_mp44": Icon(("p_stg44.mdl",), ("w_mp44.mdl",), barrel_top=7, cut_below=True,
                   note="barrel at Valve's height; the longer magazine runs off the bottom"),
    # p_fg42.mdl and p_bren.mdl are left out: both carry p_bar.bmp, BAR leftovers.
    "d_fg42": Icon(("p_fg42bu.mdl", "p_fg42bd.mdl"), ("w_fg42.mdl",), see_through=(("p_fg42.bmp", 197, 30),),
                   note="bipod up or down, closest wins; trigger guard see-through"),
    "d_scopedfg42": Icon(("p_fg42s.mdl", "p_scopedfg42bu.mdl"), ("w_fg42s.mdl",),
                         see_through=(("p_fg42.bmp", 197, 30),), note="trigger guard see-through"),
    "d_mg42": Icon(("p_mg42bu.mdl", "p_mg42bd.mdl"), ("w_mg42.mdl",), see_through=(("p_mg42.bmp", 85, 31),),
                   note="bipod up or down, closest wins; trigger guard see-through"),
    "d_mg34": Icon(("p_mg34bu.mdl", "p_mg34bd.mdl"), ("w_mg34.mdl",), see_through=(("p_mg34b.bmp", 48, 35),),
                   note="bipod up or down, closest wins; trigger guard see-through"),
    "d_luger": Icon(("p_luger.mdl",), ("w_luger.mdl",), rolls=WIDE_ROLLS),
    "d_pschreck": Icon(("p_pschreck.mdl",), ("w_pschreck.mdl",), note="no rocket beside it"),
    "d_stick": Icon(("p_stick.mdl",), ("w_stick.mdl",)),
    "d_spade": Icon(("p_spade.mdl",), ("w_spade.mdl",)),
    "d_gerknife": Icon(("p_youthk.mdl",), (), note="the Hitler Youth knife"),
    "d_paraknife": Icon(("p_paraknife.mdl",), ("w_paraknife.mdl",), muzzle=(1.0, 0.0, 0.0),
                        turn=4.5, note="lies along X; blade kept on the right; turned: both ends level"),
    # British
    "d_enfield": Icon(("p_enfield.mdl",), ("w_enfield.mdl",), skip_tex=(r"bayonet",), turn=-4.0,
                      note="bayonet (its own mesh) left out, as Valve's icon; turned: the forend level"),
    "d_scopedenfield": Icon(("p_enfields.mdl",), ("w_enfields.mdl", "w_enfield_scoped.mdl"), turn=-3.5,
                            note="turned: the forend level"),
    "d_sten": Icon(("p_sten.mdl",), ("w_sten.mdl",), see_through=(("p_sten.bmp", 102, 44),),
                   note="trigger guard see-through"),
    "d_bren": Icon(("p_brenbu.mdl", "p_brenbd.mdl"), ("w_bren.mdl",), note="bipod up or down, closest wins"),
    "d_webley": Icon(("p_webley.mdl",), ("w_webley.mdl",), rolls=WIDE_ROLLS),
    "d_piat": Icon(("p_piat.mdl",), ("w_piat.mdl",)),
    "d_britgrenade": Icon(("p_mills.mdl",), ("w_mills.mdl",), rolls=UPRIGHT_ROLLS, turn=8.0,
                          note="turned: the base level"),
    "d_britknife": Icon(("p_fairbairn.mdl",), (), turn=2.5, note="the Fairbairn-Sykes knife; turned: both ends level"),
}

# Kill-feed icons that keep the #733 upscale, and why.
KEEP_UPSCALE = {
    "d_skull": "no weapon (suicide, fall)",
    "d_world": "no weapon (the world)",
    "d_bayonet": "a close-up of the Kar's muzzle and bayonet, not a whole weapon",
    "d_enfbayonet": "a close-up of the Enfield's front with its bayonet",
    "d_garandbutt": "a cartoon (rifle butt and a face)",
    "d_mortar": "a composite (mortar, shell and bipod)",
    "d_germortar": "a composite (mortar, shell and bipod)",
}

# ── hud.txt and the stock icon ───────────────────────────────────────────────


def hud_entries(dod):
    """{name: (sheet, x, y, w, h)} for every 640 entry of sprites/hud.txt."""
    out = {}
    with open(os.path.join(dod, "sprites", "hud.txt"), encoding="latin1") as f:
        lines = f.readlines()
    for line in lines:
        parts = line.split()
        if len(parts) >= 7 and parts[1] == "640" and not line.lstrip().startswith("//"):
            try:
                out.setdefault(parts[0], (parts[2], *(int(p) for p in parts[3:7])))
            except ValueError:
                continue
    return out


def hud_rect(dod, name):
    """(sheet, x, y, w, h) of the 640 entry `name` in sprites/hud.txt."""
    try:
        return hud_entries(dod)[name]
    except KeyError:
        raise KeyError(f"{name} not in hud.txt") from None


def killfeed_names(entries):
    """The kill-feed icons of a hud.txt: its `d_` entries."""
    return sorted(n for n in entries if n.startswith("d_"))


def sheet_frame(dod, sheet):
    """(width, height, indices bytes, palette bytes, format) of a sheet's frame 0."""
    from goldsrc import Sprite
    s = Sprite(os.path.join(dod, "sprites", sheet + ".spr"))
    _, fw, fh, idx = s.frames[0]
    return fw, fh, idx, s.palette, s.tex_format


def stock_icon(dod, sheet, x, y, w, h):
    """The stock icon as RGBA (h, w, 4) float 0..1, plus the sheet's frame
    pixels and palette (for the HD key)."""
    fw, fh, idx, pal, _ = sheet_frame(dod, sheet)
    if x + w > fw or y + h > fh:
        raise ValueError(f"{sheet}: rect past the {fw}x{fh} frame")
    ind = np.frombuffer(idx, np.uint8).reshape(fh, fw)[y:y + h, x:x + w]
    p = np.frombuffer(pal, np.uint8).reshape(256, 3)
    rgba = np.dstack([p[ind], np.where(ind == 255, 0, 255)]).astype(float) / 255
    return bleed(rgba), idx, pal


def main_component(alpha):
    """Mask of the biggest connected opaque blob (drops stray bits of a
    neighbouring icon that fall inside the rect)."""
    lab, n = ndimage.label(alpha > 0.5)
    if n == 0:
        return alpha > 0.5
    sizes = ndimage.sum(np.ones_like(lab), lab, range(1, n + 1))
    return lab == 1 + int(np.argmax(sizes))


def own_mask(alpha):
    """The icon's own opaque pixels in its rect: the biggest blob, plus
    every blob that doesn't touch the rect's border (a part drawn loose,
    like the bayonet beside Valve's Kar). A blob that touches the border
    and isn't the biggest is a neighbouring icon's."""
    lab, n = ndimage.label(alpha > 0.5)
    if n == 0:
        return np.zeros(alpha.shape, bool)
    main = main_component(alpha)
    border = set(np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]]))) - {0}
    keep = main.copy()
    for i in range(1, n + 1):
        if i not in border:
            keep |= lab == i
    return keep


def bleed(rgba):
    """Transparent texels take their nearest opaque texel's RGB."""
    mask = rgba[..., 3] <= 0
    if mask.all() or not mask.any():
        return rgba
    _, (iy, ix) = ndimage.distance_transform_edt(mask, return_indices=True)
    out = rgba.copy()
    out[..., :3] = rgba[iy, ix, :3]
    return out


# ── geometry and view ────────────────────────────────────────────────────────


def model_triangles(path):
    """[(positions, normals, uv, Texture)] of body 0 of every bodypart."""
    return StudioModel(path).triangles()


def principal_axes(tris):
    pts = np.concatenate([t[0].reshape(-1, 3) for t in tris])
    c = pts - pts.mean(0)
    _, _, vt = np.linalg.svd(c, full_matrices=False)
    return vt  # rows: longest, middle, shortest


def clip_tip(tris, axis, front=0.2):
    """Drops the triangles past the muzzle: the biggest gap between vertex
    positions along `axis` in the front fifth of the length is where the
    barrel ends and the bayonet tip begins. Returns (tris, cut, dropped)."""
    xs = np.unique(np.round(np.concatenate([t[0].reshape(-1, 3) @ axis for t in tris]), 3))
    head = xs[xs > xs.max() - front * (xs.max() - xs.min())]
    if len(head) < 2:
        return tris, float(xs.max()), 0
    gaps = np.diff(head)
    i = int(np.argmax(gaps))
    cut = float(head[i] + gaps[i] / 2)
    out = []
    for v, n, uv, tex in tris:
        keep = ((v @ axis) <= cut).all(axis=1)
        out.append((v[keep], n[keep], uv[keep], tex))
    dropped = sum(len(t[0]) for t in tris) - sum(len(t[0]) for t in out)
    return [t for t in out if len(t[0])], cut, dropped


def view_basis(a1, a2, sx, sy, roll_deg):
    r = np.radians(roll_deg)
    right = sx * a1
    up = sy * a2
    right, up = np.cos(r) * right + np.sin(r) * up, -np.sin(r) * right + np.cos(r) * up
    toward = np.cross(right, up)
    return np.array([right, up, toward])


def fit_transform(tris, basis, box):
    """(kx, ky, ox, oy): the scale and offset that fit the projected model
    inside `box` (x0, y0, x1, y1 in output pixels), centred, same scale on
    both axes."""
    pts = np.concatenate([t[0].reshape(-1, 3) for t in tris]) @ basis.T
    lo, hi = pts[:, :2].min(0), pts[:, :2].max(0)
    bw, bh = box[2] - box[0], box[3] - box[1]
    k = min(bw / max(hi[0] - lo[0], 1e-6), bh / max(hi[1] - lo[1], 1e-6))
    cx, cy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    mx, my = (lo + hi) / 2
    return k, k, cx - k * mx, cy + k * my  # screen y = oy - ky * up


def refit_transform(fit, alpha, box, slack=0.05):
    """A transform that makes what's left after drop_specks fill `box`,
    or None when it already does: a stray vertex far from the gun (a knife
    model's) shrinks the first fit, and dropping its speck leaves the gun
    small."""
    ys, xs = np.nonzero(alpha >= 0.5)
    if not len(xs):
        return None
    x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    bw, bh = box[2] - box[0], box[3] - box[1]
    s = min(bw / (x1 - x0), bh / (y1 - y0))
    if s < 1 + slack:
        return None
    kx, ky, ox, oy = fit
    # Screen point p -> centre + s * (p - bbox centre).
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    bx, by = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    return kx * s, ky * s, bx + s * (ox - cx), by + s * (oy - cy)


def silhouette(tris, basis, kx, ky, ox, oy, w, h, ss=4):
    img = Image.new("L", (w * ss, h * ss), 0)
    d = ImageDraw.Draw(img)
    for pos, _, _, _ in tris:
        p = pos @ basis.T
        xs, ys = (ox + kx * p[..., 0]) * ss, (oy - ky * p[..., 1]) * ss
        for tx, ty in zip(xs, ys):
            d.polygon(list(zip(tx.tolist(), ty.tolist())), fill=255)
    return np.asarray(img.resize((w, h), Image.BOX), float) / 255


def iou(a, b):
    return (a & b).sum() / max((a | b).sum(), 1)


def muzzle_signs(a1, muzzle):
    """The `right` signs choose_view may use: only the one that puts
    `muzzle` on the right when the long axis `a1` runs along it."""
    if muzzle is not None:
        d = float(np.dot(a1, muzzle))
        if abs(d) > 0.7:
            return (1 if d > 0 else -1,)
    return (1, -1)


def choose_view(tris, target_alpha, box, rolls=ROLLS, muzzle=MUZZLE):
    """(basis, IoU, (sx, sy, roll)): the view whose silhouette best overlaps
    the stock icon, at stock resolution."""
    h, w = target_alpha.shape
    a1, a2, _ = principal_axes(tris)
    target = target_alpha > 0.5

    def score(sx, sy, roll):
        basis = view_basis(a1, a2, sx, sy, roll)
        return iou(silhouette(tris, basis, *fit_transform(tris, basis, box), w, h) > 0.5, target)

    best = max(((score(sx, sy, r), sx, sy, r) for sx in muzzle_signs(a1, muzzle) for sy in (1, -1) for r in rolls),
               key=lambda t: t[0])
    s, sx, sy, roll = best
    step = (rolls[1] - rolls[0]) if len(rolls) > 1 else 2
    for r in np.arange(roll - step * 0.75, roll + step * 0.76, step / 4):
        t = score(sx, sy, r)
        if t > s:
            s, roll = t, float(r)
    return view_basis(a1, a2, sx, sy, roll), s, (sx, sy, roll)


# Icons drawn at the roll that matches Valve's silhouette rather than with
# a level barrel: blades and grenades have no barrel to level.
NOT_LEVELLED = re.compile(r"knife|spade|grenade|stick", re.I)


def line_slope(cx, cy, eps):
    """The slope of the straight line most of the points (`cx`, `cy`) lie
    on, within `eps`: a step or a spike off that line doesn't count."""
    best = (-1, np.inf, 0.0, 0.0)
    for i in range(len(cx)):
        for j in range(i + 1, len(cx)):
            k = (cy[j] - cy[i]) / (cx[j] - cx[i])
            c = cy[i] - k * cx[i]
            resid = np.abs(cy - (k * cx + c))
            n = int((resid <= eps).sum())
            err = float(resid[resid <= eps].sum())
            if n > best[0] or (n == best[0] and err < best[1]):
                best = (n, err, k, c)
    _, _, k, c = best
    inl = np.abs(cy - (k * cx + c)) <= eps
    if inl.sum() >= 2:
        k = np.polyfit(cx[inl], cy[inl], 1)[0]
    return float(k)


def runs(col):
    """(top, bottom) of each opaque run in a silhouette column, top first."""
    d = np.diff(np.concatenate([[0], col.astype(np.int8), [0]]))
    return list(zip(np.nonzero(d == 1)[0], np.nonzero(d == -1)[0] - 1))


def barrel_axis_slope(cov, reach=0.35, grow=1.6, samples=60):
    """The slope of the bore's line at the end of the barrel, from a side
    view's silhouette (`cov`: coverage 0..1, muzzle towards the right, y
    down; the slope returned is y-up). Walking back from the muzzle, the
    barrel is the run each column shares with the one before; while it
    stays about as thick as the barrel its middle (to a fraction of a pixel,
    from the edge pixels' coverage) is on the bore's line. Where a sight
    post or gas tube sits on top (or a lug, rod or bipod hangs under) only
    one edge moves, and the other edge, half a barrel away, still is; where
    both move the forend or receiver has started and the walk stops."""
    sil = cov > 0.5
    xs = np.nonzero(sil.any(0))[0]
    if len(xs) < 8:
        return 0.0
    x0, x1 = xs.min(), xs.max()
    span = x1 - x0
    start = x1 - max(1, span // 200)  # off the muzzle's anti-aliased edge
    first = runs(sil[:, start])
    if not first:
        return 0.0
    top, bot = max(first, key=lambda r: r[1] - r[0])
    thick = max(bot - top, 1)
    step = max(1, int(reach * span) // (samples * 3))
    clean, guessed = [], []
    misses = 0
    for x in range(start, int(x1 - reach * span), -step):
        over = [r for r in runs(sil[:, x]) if r[0] <= bot + 1 and r[1] >= top - 1]
        if not over:
            misses += 1
            if misses > 3:
                break
            continue
        t, b = max(over, key=lambda r: min(r[1], bot) - max(r[0], top))
        if b - t <= grow * thick:
            rows = np.arange(max(t - 1, 0), min(b + 2, len(cov)))
            c = cov[rows, x]
            clean.append((x, float((rows * c).sum() / c.sum())))
            top, bot, thick = t, b, max(b - t, 1)
            misses = 0
        elif abs(b - bot) <= max(2, thick * 0.25):  # something on top
            guessed.append((x, b - thick / 2))
            top, bot = b - thick, b
            misses = 0
        elif abs(t - top) <= max(2, thick * 0.25):  # something underneath
            guessed.append((x, t + thick / 2))
            top, bot = t, t + thick
            misses = 0
        else:
            misses += 1
            if misses > 3:
                break
    # The clean run alone when it's long enough: an edge drifts with a
    # tapered barrel, the middle doesn't. A blast shield or a muzzle cup
    # off the bore's line is a step in the middles, left out by line_slope.
    if len(clean) >= 8 and clean[0][0] - clean[-1][0] >= 0.06 * span:
        cx, cy = np.array(clean, float).T
        return -line_slope(cx, cy, 0.75)
    pts = clean + guessed
    if len(pts) < 3:
        return 0.0
    cx, cy = np.array(pts, float).T
    return -line_slope(cx, cy, max(1.0, 0.0015 * span))


# The silhouette level_view measures in: big enough that a pixel is a
# small fraction of a degree along the barrel's end.
LEVEL_SIZE = (2048, 1024)


def side_view(angle):
    """The basis of the side view tilted by `angle` degrees: every DoD
    weapon model shares one layout (built for the hand bone), the muzzle
    along -Y, the top of the gun towards +X, the side along Z, tilted in
    the X-Y plane as it's held (rifles ~20 degrees)."""
    right, up = np.array(MUZZLE, float), np.array(MODEL_UP, float)
    r = np.radians(angle)
    rr, uu = np.cos(r) * right + np.sin(r) * up, -np.sin(r) * right + np.cos(r) * up
    return np.array([rr, uu, np.cross(rr, uu)])


def level_view(tris, pick):
    """A side view with the end of the barrel level, as in Valve's icons:
    the silhouette match alone tilts a gun towards Valve's drawing, and a
    gun's overall outline (stock drop, tapered barrel, a gas tube) isn't
    its bore, so the tilt is measured at the muzzle. Returns (basis, pick)."""
    sx, sy, _ = pick
    w, h = LEVEL_SIZE
    angle = 0.0
    for _ in range(3):
        basis = side_view(angle)
        cov = silhouette(tris, basis, *fit_transform(tris, basis, (4, 4, w - 4, h - 4)), w, h)
        angle += float(np.degrees(np.arctan(barrel_axis_slope(cov))))
    return side_view(angle), (sx, sy, angle)


def turned(basis, degrees):
    """`basis` rolled by `degrees`, clockwise as seen in the icon (screen
    y runs down, so the right end goes down for a positive turn)."""
    r = np.radians(degrees)
    right, up, toward = basis
    return np.array([np.cos(r) * right + np.sin(r) * up, -np.sin(r) * right + np.cos(r) * up, toward])


def barrel_top_row(alpha, front=0.15):
    """The row of the barrel's top edge at its end: the most common top
    row of the silhouette's columns in the front `front` of its length (a
    front sight post is a few columns, the barrel most of them)."""
    sil = alpha >= 0.5
    xs = np.nonzero(sil.any(0))[0]
    if not len(xs):
        return None
    x1 = xs.max()
    cols = range(max(xs.min(), int(x1 - front * (x1 - xs.min()))), x1 + 1)
    tops = [int(np.argmax(sil[:, x])) for x in cols if sil[:, x].any()]
    vals, counts = np.unique(tops, return_counts=True)
    return float(vals[np.argmax(counts)])


def place_barrel(tris, basis, fit, alpha, box, row, cut=False):
    """A transform that puts the top of the barrel's end (measured in
    `alpha`, rendered with `fit`) on screen row `row`, as wide as `box`
    allows. With `cut`, shrunk only so nothing pokes out above the box
    (what hangs below is cut later). Without, the gun keeps that size and
    is raised until its bottom fits, the barrel as near `row` as that
    allows (shrunk only when it is taller than the box). Centred across as
    fit_transform is."""
    kx, ky, ox, oy = fit
    r0 = barrel_top_row(alpha)
    if r0 is None:
        return fit
    pts = np.concatenate([t[0].reshape(-1, 3) for t in tris]) @ basis.T
    lo, hi = pts[:, :2].min(0), pts[:, :2].max(0)
    ub = (oy - r0) / ky  # the barrel top, model units up
    k = (box[2] - box[0]) / max(hi[0] - lo[0], 1e-6)
    if cut:
        if hi[1] > ub:
            k = min(k, (row - box[1]) / (hi[1] - ub))
    else:
        k = min(k, (box[3] - box[1]) / max(hi[1] - lo[1], 1e-6))
        row = min(row, box[3] - k * (ub - lo[1]))  # raised until the bottom fits
        row = max(row, box[1] + k * (hi[1] - ub))  # never past the top
    cx = (box[0] + box[2]) / 2
    return k, k, cx - k * (lo[0] + hi[0]) / 2, row + k * ub


HOLE_DARK = 16  # a see_through texel: every channel at most this (of 255)


def see_through_mask(tex, seeds):
    """The texels of `tex` drawn transparent: the near-black ones joined to
    each (x, y) of `seeds` (a hole painted black on a solid plate, as some
    DoD models draw their trigger guards). A seed off a dark texel is
    ignored rather than cutting a hole in the gun."""
    dark = (tex.palette[tex.indices] <= HOLE_DARK).all(2)
    lab, _ = ndimage.label(dark)
    out = np.zeros(dark.shape, bool)
    for x, y in seeds:
        if 0 <= y < dark.shape[0] and 0 <= x < dark.shape[1] and dark[y, x]:
            out |= lab == lab[y, x]
    return out


def apply_see_through(skins, textures, see_through):
    """`skins` (as render takes them) with each see_through hole's alpha 0;
    returns (skins, names of the textures to draw as masked)."""
    cut = set()
    for t in textures:
        seeds = [(x, y) for name, x, y in see_through if name.lower() == t.name.lower()]
        if not seeds:
            continue
        m = see_through_mask(t, seeds)
        if not m.any():
            continue
        base = skins.get(t.name)
        base = (t.rgba().astype(float) / 255 if base is None else base).copy()
        f = base.shape[0] // t.height
        base[np.kron(m, np.ones((f, f), bool)), 3] = 0
        skins = {**skins, t.name: base}
        cut.add(t.name)
    return skins, frozenset(cut)


def cut_under(rgba, bottom):
    """Clears every pixel from row `bottom` down (a magazine or bipod cut
    at the rect's edge, `bottom` leaving room for the outline)."""
    out = rgba.copy()
    out[int(np.ceil(bottom)):, :, 3] = 0
    return out


# ── rasterizer ───────────────────────────────────────────────────────────────


def sample_bilinear(tex, u, v):
    """tex (h, w, c) float; u, v in 0..1, wrapped."""
    h, w = tex.shape[:2]
    x = (u * w - 0.5) % w
    y = (v * h - 0.5) % h
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = (x - x0)[:, None], (y - y0)[:, None]
    x1, y1 = (x0 + 1) % w, (y0 + 1) % h
    return ((tex[y0, x0] * (1 - fx) + tex[y0, x1] * fx) * (1 - fy)
            + (tex[y1, x0] * (1 - fx) + tex[y1, x1] * fx) * fy)


def render(tris, basis, kx, ky, ox, oy, w, h, skins, light=None, ambient=AMBIENT, diffuse=DIFFUSE, ss=SS,
           masked_names=frozenset(), hires=False):
    """RGBA (h, w, 4) float 0..1. skins: {texture name: RGBA float array}
    to override a texture (an HD skin); others use the model's own.
    masked_names: textures drawn as masked (their alpha cut at 0.5) though
    the model doesn't flag them (apply_see_through). With `hires`, returns
    (rgba, (colour, coverage)): also the samples before the downsample,
    (h*ss, w*ss, 3) and (h*ss, w*ss), for soft_finish."""
    W, H = w * ss, h * ss
    col = np.zeros((H, W, 3))
    cov = np.zeros((H, W))
    zbuf = np.full((H, W), -np.inf)
    L = np.array(light if light is not None else (-0.35, 0.75, 0.55), float)
    L /= np.linalg.norm(L)
    for pos, nrm, uv, tex in tris:
        rgba = skins.get(tex.name)
        if rgba is None:
            rgba = tex.rgba().astype(float) / 255
        masked = bool(tex.flags & STUDIO_NF_MASKED) or tex.name in masked_names
        p = pos @ basis.T
        # Normals of the stretched model: the inverse-transpose scales y by kx/ky.
        n = (nrm @ basis.T) * np.array([1.0, kx / ky, 1.0])
        sx = (ox + kx * p[..., 0]) * ss
        sy = (oy - ky * p[..., 1]) * ss
        for t in range(len(p)):
            x, y = sx[t], sy[t]
            x0, x1 = max(int(np.floor(x.min())), 0), min(int(np.ceil(x.max())), W - 1)
            y0, y1 = max(int(np.floor(y.min())), 0), min(int(np.ceil(y.max())), H - 1)
            if x0 > x1 or y0 > y1:
                continue
            area = (x[1] - x[0]) * (y[2] - y[0]) - (x[2] - x[0]) * (y[1] - y[0])
            if abs(area) < 1e-9:
                continue
            gx, gy = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            w0 = ((x[1] - gx) * (y[2] - gy) - (x[2] - gx) * (y[1] - gy)) / area
            w1 = ((x[2] - gx) * (y[0] - gy) - (x[0] - gx) * (y[2] - gy)) / area
            w2 = 1 - w0 - w1
            inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
            if not inside.any():
                continue
            iy, ix = np.nonzero(inside)
            b = np.stack([w0[inside], w1[inside], w2[inside]], 1)
            z = b @ p[t, :, 2]
            py, px = iy + y0, ix + x0
            closer = z > zbuf[py, px]
            if not closer.any():
                continue
            b, z, py, px = b[closer], z[closer], py[closer], px[closer]
            texel = sample_bilinear(rgba, b @ uv[t, :, 0], b @ uv[t, :, 1])
            if masked:
                keep = texel[:, 3] > 0.5
                b, z, py, px, texel = b[keep], z[keep], py[keep], px[keep], texel[keep]
            nn = b @ n[t]
            nn /= np.maximum(np.linalg.norm(nn, axis=1, keepdims=True), 1e-9)
            nn[nn[:, 2] < 0] *= -1  # two-sided
            shade = ambient + diffuse * np.clip(nn @ L, 0, None)
            zbuf[py, px] = z
            col[py, px] = np.clip(texel[:, :3] * shade[:, None], 0, 1)
            cov[py, px] = 1
    # Box-downsample, colour weighted by coverage.
    c4 = col.reshape(h, ss, w, ss, 3) * cov.reshape(h, ss, w, ss, 1)
    a = cov.reshape(h, ss, w, ss).mean((1, 3))
    rgb = c4.mean((1, 3)) / np.maximum(a, 1e-9)[..., None]
    out = bleed(np.dstack([rgb, a]))
    return (out, (col, cov)) if hires else out


def drop_specks(rgba, min_px=SPECK, corners=True):
    """Clears loose blobs smaller than `min_px` pixels: a sling swivel or
    a stray vertex far from the gun renders as a dot that reads as dirt at
    icon size. With `corners`, pixels touching at a corner are one blob: a
    1 px guard bar running diagonally into the ring is not a speck (the
    Bren's)."""
    lab, n = ndimage.label(rgba[..., 3] >= 0.5, structure=np.ones((3, 3)) if corners else None)
    if n <= 1:
        return rgba
    sizes = ndimage.sum(np.ones_like(lab), lab, range(1, n + 1))
    out = rgba.copy()
    for i, s in enumerate(sizes, 1):
        if s < min_px:
            out[ndimage.binary_dilation(lab == i, iterations=1), 3] = 0
    return out


def keep_thin(rgba, lo=THIN_LO, margin=THIN_MARGIN, min_run=THIN_MIN_RUN):
    """Makes a part thinner than half a pixel opaque, so the game's alpha
    cut keeps it as a 1 px line rather than dropping it (a trigger guard's
    lower bar, a launcher's wire guard). Such a part is a ridge in the
    coverage: a pixel brighter than both neighbours across it (along a row,
    a column or a diagonal), where a thick part's soft edge only rises
    inwards. A run is joined through its pixels already over the cut (a
    dotted wire), and is kept only when at least `min_run` of its pixels
    are under it: a sharp corner has one or two, which stay as they are."""
    a = rgba[..., 3]
    h, w = a.shape
    p = np.pad(a, 1)
    ridge = np.zeros(a.shape, bool)
    for dy, dx in ((0, 1), (1, 0), (1, 1), (1, -1)):
        n1 = p[1 + dy:1 + dy + h, 1 + dx:1 + dx + w]
        n2 = p[1 - dy:1 - dy + h, 1 - dx:1 - dx + w]
        ridge |= a - np.maximum(n1, n2) > margin
    line = ridge & (a >= lo)
    lab, n = ndimage.label(line, structure=np.ones((3, 3)))
    if n == 0:
        return rgba
    faint = a < 0.5
    sizes = ndimage.sum(faint, lab, range(1, n + 1))  # the run's pixels the cut would drop
    keep = np.isin(lab, [i for i, s in enumerate(sizes, 1) if s >= min_run]) & faint
    out = rgba.copy()
    out[keep, 3] = 1.0
    return out


def rim_light(rgba, width=RIM_WIDTH, strength=RIM_STRENGTH):
    """Lightens the icon's own edge, `width` px in, so dark metal still has
    an edge over a dark background."""
    a = rgba[..., 3]
    inside = ndimage.distance_transform_edt(a >= 0.5)
    f = np.clip(1 - (inside - 0.5) / width, 0, 1) * strength * (a >= 0.5)
    rgb = rgba[..., :3] + (1 - rgba[..., :3]) * f[..., None]
    return np.dstack([rgb, a])


def _big_regions(region, inner, border_ok, min_px, min_width):
    """Mask of the components of `region` that are at least `min_px`
    pixels and `min_width` across (in `inner`'s distance), and, unless
    `border_ok`, don't reach the image's border."""
    lab, n = ndimage.label(region)
    if n == 0:
        return np.zeros(region.shape, bool)
    border = set() if border_ok else set(np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]])))
    ids = range(1, n + 1)
    sizes = ndimage.sum(np.ones_like(lab), lab, ids)
    # the widest point: twice the distance to the nearest pixel outside
    widths = 2 * np.asarray(ndimage.maximum(ndimage.distance_transform_edt(inner), lab, ids))
    keep = [i for i, s, wd in zip(ids, sizes, widths) if i not in border and s >= min_px and wd >= min_width]
    return np.isin(lab, keep)


def holes(alpha, min_px=HOLE_MIN, min_width=HOLE_WIDTH, bridge=HOLE_BRIDGE):
    """The see-through gaps inside an icon (a trigger guard's inside): the
    clear regions that don't reach the image's border, of at least `min_px`
    pixels and `min_width` px across at their widest (a slit, such as a gas
    tube's gap under a barrel, stays closed). Enclosed gaps less than
    2 x `bridge` px apart count as one: a trigger inside its guard splits
    the inside into gaps too narrow on their own, and they all stay open."""
    clear = alpha < 0.5
    out = _big_regions(clear, clear, False, min_px, min_width)
    if bridge <= 0:
        return out
    enclosed = clear & ndimage.binary_fill_holes(~clear)
    merged = ndimage.binary_closing(enclosed, structure=np.ones((3, 3)), iterations=bridge) & ~(clear & ~enclosed)
    lab, n = ndimage.label(merged)
    if n == 0:
        return out
    # a group of gaps counts when the gaps themselves, not the merge, are
    # the hole's size: at least two of them, together min_px, and the merged
    # region min_width across
    big = _big_regions(merged, merged, True, min_px, min_width)
    for i in np.unique(lab[big]):
        comp = lab == i
        parts, k = ndimage.label(enclosed & comp)
        if k >= 2 and (enclosed & comp).sum() >= min_px:
            out |= enclosed & comp
    return out


def outline(rgba, radius=OUTLINE, colour=OUTLINE_COLOUR, hole_radius=HOLE_OUTLINE):
    """A dark rim `radius` px wide (soft-edged) under the icon; inside a
    hole only `hole_radius` px, so the full rim doesn't fill a trigger
    guard back in."""
    a = rgba[..., 3]
    dist = ndimage.distance_transform_edt(a < 0.5)
    r = np.where(holes(a), hole_radius, radius)
    rim = np.clip(r + 0.5 - dist, 0, 1)
    out_a = a + rim * (1 - a)
    rgb = (rgba[..., :3] * a[..., None] + np.array(colour) * (rim * (1 - a))[..., None]) / np.maximum(out_a, 1e-9)[..., None]
    return bleed(np.dstack([rgb, out_a]))


def _up(mask, ss):
    return np.kron(mask, np.ones((ss, ss), bool))


def soft_finish(lit, col_hi, cov_hi, ss=SS, radius=OUTLINE, colour=OUTLINE_COLOUR, hole_radius=HOLE_RIM,
                rim_width=RIM_WIDTH, rim_strength=RIM_STRENGTH, edge_gap=1.0):
    """rim_light and outline drawn on the supersamples, then box-downsampled
    with the colour premultiplied: the icon's edges get coverage alpha (a
    ramp over a pixel or two) and partial texels hold the outline's colour,
    not the body's or the background's (#841; #890 keeps that alpha on
    screen). Interior lines, the body's edge against the outline and the
    rim light are smoothed the same way.

    `lit` is the pixel-size render after the hand fixes (keep_thin,
    drop_specks, cut_under), and those fixes decide the shape: the body is
    the supersamples of render's own coverage inside its kept pixels (those
    of alpha 0.5 or more and their neighbours that kept any alpha), plus
    the whole of a pixel keep_thin made opaque. `radius` and `hole_radius`
    are measured from the body's edge (outline's are from pixel centres,
    so its 3 px rim is about 2.5 px past the edge; the cut at 128 made it 3).
    The holes are holes() of `lit`, as outline finds them. The outline
    keeps `edge_gap` px off the rect's border, so the last pixel row and
    column stay clear (the quad's edge never cuts a soft edge off)."""
    a = lit[..., 3]
    h, w = a.shape
    cov = cov_hi > 0
    cov4 = cov.reshape(h, ss, w, ss).mean((1, 3))
    body4 = a >= 0.5
    forced = body4 & (cov4 < 0.5)  # keep_thin's lines: the whole pixel
    near = ndimage.binary_dilation(body4, structure=np.ones((3, 3))) & (a > 0)
    body = (cov & _up(near, ss)) | _up(forced, ss)
    # distances in pixels from a sample's centre to the body's edge
    out_d = ndimage.distance_transform_edt(~body) / ss - 0.5 / ss
    in_d = ndimage.distance_transform_edt(body) / ss - 0.5 / ss
    r = np.where(_up(holes(a), ss), hole_radius, radius)
    # the outline stops short of the rect's last pixel (2 px wide where the
    # gun touches its box), so the quad's edge never cuts through it
    H, W = body.shape
    yy, xx = np.mgrid[0:H, 0:W]
    to_border = np.minimum(np.minimum(yy, H - 1 - yy), np.minimum(xx, W - 1 - xx)) / ss
    rim = ~body & (out_d <= r) & (to_border >= edge_gap)
    rgb = np.where(cov[..., None], col_hi, np.repeat(np.repeat(lit[..., :3], ss, 0), ss, 1))
    f = np.clip(1 - in_d / rim_width, 0, 1) * rim_strength
    rgb = rgb + (1 - rgb) * f[..., None]
    rgb = np.where(rim[..., None], np.array(colour, float), np.where(body[..., None], rgb, 0.0))
    occ = (body | rim).astype(float)
    pre = (rgb * occ[..., None]).reshape(h, ss, w, ss, 3).mean((1, 3))
    alpha = occ.reshape(h, ss, w, ss).mean((1, 3))
    out = np.dstack([pre / np.maximum(alpha, 1e-9)[..., None], alpha])
    return bleed(out)


# ── skins ────────────────────────────────────────────────────────────────────


def esrgan_path():
    env = os.environ.get("REALESRGAN")
    if env:
        return env
    local = os.path.join(HERE, "realesrgan", "realesrgan-ncnn-vulkan.exe")
    if os.path.exists(local):
        return local
    appdata = os.environ.get("APPDATA", "")
    return os.path.join(appdata, "dod-studio", "hd_tools", "realesrgan", "realesrgan-ncnn-vulkan.exe")


def skin_key(tex):
    """models_hd.py's file name for a skin (without .tga)."""
    import hdcommon as C
    return f"{C.file_stem_name(tex.name)}_{C.fnv1a32(tex.indices.tobytes(), tex.palette.tobytes()):08x}"


def _skin_from_file(path, tex):
    img = Image.open(path).convert("RGBA")
    if img.size != (tex.width * 4, tex.height * 4):
        img = img.resize((tex.width * 4, tex.height * 4), Image.LANCZOS)
    big = np.asarray(img, float) / 255
    if not tex.flags & STUDIO_NF_MASKED:
        big[..., 3] = 1
    return big


def hd_skins(textures, style, models_dir):
    """{texture name: 4x RGBA float} in `style`: models_hd.py's file from
    `models_dir` where it built one, the rest upscaled here (padded as
    models_hd.py pads skins). Textures left out draw with the stock skin."""
    import styles as S
    out, todo = {}, []
    for t in textures:
        p = os.path.join(models_dir, skin_key(t) + ".tga") if models_dir else None
        if p and os.path.exists(p):
            try:
                out[t.name] = _skin_from_file(p, t)
                continue
            except OSError:
                pass
        todo.append(t)
    kind = S.DEFS[style][0]
    if not todo or kind == "blend":
        return out
    if kind == "ai":
        exe = esrgan_path()
        if not os.path.exists(exe):
            return out
        S.ESRGAN = exe
    work = tempfile.mkdtemp(prefix="killfeed_hd_")
    try:
        src, dst = os.path.join(work, "in"), os.path.join(work, "out")
        os.makedirs(src)
        for i, t in enumerate(todo):
            rgb = t.rgba()[..., :3]
            pad = np.pad(rgb, ((t.height // 2,) * 2, (t.width // 2,) * 2, (0, 0)), mode="reflect")
            Image.fromarray(pad).save(os.path.join(src, f"{i}.png"))
        S.upscale(src, dst, style)
        for i, t in enumerate(todo):
            p = os.path.join(dst, f"{i}.png")
            if not os.path.exists(p):
                continue
            big = np.asarray(Image.open(p).convert("RGB"), float) / 255
            oy, ox = t.height // 2 * 4, t.width // 2 * 4
            big = big[oy:oy + t.height * 4, ox:ox + t.width * 4]
            a = np.asarray(Image.fromarray(t.rgba()[..., 3]).resize((t.width * 4, t.height * 4), Image.NEAREST), float) / 255
            out[t.name] = np.dstack([big, a])
        return out
    finally:
        shutil.rmtree(work, ignore_errors=True)


# ── one icon ─────────────────────────────────────────────────────────────────


def candidates(dod, icon):
    """The model paths to try: the `p_` ones that exist, else the `w_` ones."""
    models = os.path.join(dod, "models")
    found = [os.path.join(models, m) for m in icon.models if os.path.exists(os.path.join(models, m))]
    return found or [os.path.join(models, m) for m in icon.fallback if os.path.exists(os.path.join(models, m))]


def shared_area(entries, name):
    """(mask, top, bottom): the part of `name`'s rect that another hud.txt
    entry of the same sheet also covers (h, w bool), and the free rows
    [top, bottom) left once the overlaps that span at least half the
    width are taken off its top and bottom. Whatever is drawn there shows
    in both sprites: the MG34's bottom four rows are the K43's top four,
    so a render reaching into them draws a line under the other gun.

    Rows two kill-feed icons share belong to the lower one: the hook draws
    the upper icon without them on an HD sheet (#890), so they are left out
    here only for the icon above. Rows shared with any other sprite (the
    weapon menu's weapon_luger over the scoped FG42) stay empty, as Valve
    keeps them."""
    sheet, x, y, w, h = entries[name]
    mask = np.zeros((h, w), bool)
    top, bottom = 0, h
    for other, (s2, x2, y2, w2, h2) in entries.items():
        if other == name or s2 != sheet:
            continue
        if name.startswith("d_") and other.startswith("d_") and y2 < y:
            continue  # a kill-feed icon above: the shared rows are this one's

        c0, c1 = max(x, x2) - x, min(x + w, x2 + w2) - x
        r0, r1 = max(y, y2) - y, min(y + h, y2 + h2) - y
        if c0 >= c1 or r0 >= r1:
            continue
        mask[r0:r1, c0:c1] = True
        if c1 - c0 >= w / 2:
            if r0 == 0 and r1 < h:
                top = max(top, r1)
            elif r1 == h and r0 > 0:
                bottom = min(bottom, r0)
    if top >= bottom:
        top, bottom = 0, h
    return mask, top, bottom


def render_icon(dod, name, icon, style, models_dir, scale=4, log=print, _free_rows=False):
    """(rgba at `scale`x the rect, info dict) or (None, reason)."""
    sheet, x, y, w, h = hud_rect(dod, name)
    stock, _, _ = stock_icon(dod, sheet, x, y, w, h)
    target = main_component(stock[..., 3])
    if not target.any():
        return None, "blank in this install's sheet"
    ys, xs = np.nonzero(target)
    shared, top, bottom = shared_area(hud_entries(dod), name)
    # Valve's width, the rect's whole height: a real gun is stubbier than
    # Valve's drawing (a knife most of all), so the height Valve's thin
    # line leaves free lets it grow to the width. When the gun then
    # reaches into rows another sprite shares, it is drawn again in the
    # free rows only (shared_area).
    box1 = (xs.min(), top, xs.max() + 1, bottom) if _free_rows else (xs.min(), 0, xs.max() + 1, h)
    m = OUTLINE
    box = (box1[0] * scale + m, box1[1] * scale + m, box1[2] * scale - m, box1[3] * scale - m)
    best = None
    for path in candidates(dod, icon):
        try:
            tris = model_triangles(path)
            tris = [t for t in tris if not any(re.search(r, t[3].name, re.I) for r in icon.skip_tex)]
        except (OSError, MdlError) as e:
            log(f"  {name}: {os.path.basename(path)} unreadable ({e}), skipped")
            continue
        if not tris:
            continue
        cut = None
        if icon.clip_tip:
            tris, _, dropped = clip_tip(tris, principal_axes(tris)[0])
            cut = dropped
        basis, score, pick = choose_view(tris, target, tuple(v / scale for v in box), icon.rolls, icon.muzzle)
        if best is None or score > best[1]:
            best = (tris, score, pick, basis, path, cut)
    if best is None:
        return None, "no model in this install"
    tris, score, pick, basis, path, cut = best
    if not NOT_LEVELLED.search(name):
        basis, pick = level_view(tris, pick)
    if icon.turn:
        basis = turned(basis, icon.turn)
    textures = sorted({id(t[3]): t[3] for t in tris}.values(), key=lambda t: t.name)
    skins = hd_skins(textures, style, models_dir)
    n_hd = len(skins)
    skins, cutouts = apply_see_through(skins, textures, icon.see_through)
    # Thin parts are kept on guns only: a blade's or grenade's thin bit is
    # the end of a crossguard or a lever, which reads as a stray tip.
    gun = not NOT_LEVELLED.search(name)
    samples = {}  # the last draw's supersamples, for soft_finish

    def draw(f):
        lit, samples["last"] = render(tris, basis, *f, w * scale, h * scale, skins, masked_names=cutouts,
                                      hires=True)
        return keep_thin(lit) if gun else lit

    def specks(lit):
        return drop_specks(lit, corners=gun)

    fit = fit_transform(tris, basis, box)
    lit = specks(draw(fit))
    if icon.barrel_top is not None:
        fit = place_barrel(tris, basis, fit, lit[..., 3], box, icon.barrel_top * scale, icon.cut_below)
        lit = draw(fit)
        if icon.cut_below:
            lit = cut_under(lit, box[3])
        lit = specks(lit)
    else:
        refit = refit_transform(fit, lit[..., 3], box)
        if refit is not None:
            lit = specks(draw(refit))
    img = soft_finish(lit, *samples["last"])
    if shared.any():  # nothing of the render in another sprite's area
        shared4 = np.kron(shared, np.ones((scale, scale), bool))
        # A gun reaching more than a pixel row in (the K43's stock, the
        # MG34's bipod foot) is drawn again in the free rows; a single
        # edge row (a sight's tip) is just cut there.
        deep = ((lit[..., 3] >= 0.5) & shared4).any(1).sum() > 1
        if not _free_rows and (top > 0 or bottom < h) and deep:
            return render_icon(dod, name, icon, style, models_dir, scale, log, _free_rows=True)
        img[shared4, 3] = 0
    info = {"model": os.path.basename(path), "iou": round(float(score), 2),
            "roll": round(float(pick[2] + icon.turn), 1), "clipped_tris": cut,
            "skin": f"{style} ({n_hd} of {len(textures)})" if n_hd else "stock"}
    if cutouts:
        info["see_through"] = sorted(cutouts)
    if icon.turn:
        info["turn"] = icon.turn
    if icon.barrel_top is not None:
        info["barrel_top"] = icon.barrel_top
        info["cut_below"] = icon.cut_below
    return img, info


# ── the sheet ────────────────────────────────────────────────────────────────


def paint(sheet4, rect, stock_alpha, icon4):
    """Paints `icon4` (the rect at the sheet's scale) into `sheet4` (RGBA
    float, modified): the stock icon's own pixels in the rect, dilated a
    pixel, cleared first, so no stair step of Valve's icon shows round the
    render; pixels outside the rect and other icons' bits untouched."""
    x, y, w, h = rect
    f = icon4.shape[1] // w
    own = ndimage.binary_dilation(own_mask(stock_alpha), iterations=1)
    clear = np.kron(own, np.ones((f, f), bool))
    region = sheet4[y * f:(y + h) * f, x * f:(x + w) * f]
    region[clear, 3] = 0
    a = icon4[..., 3:4]
    base_a = region[..., 3:4]
    out_a = a + base_a * (1 - a)
    rgb = (icon4[..., :3] * a + region[..., :3] * base_a * (1 - a)) / np.maximum(out_a, 1e-9)
    region[..., :3] = np.where(out_a > 0, rgb, region[..., :3])
    region[..., 3:4] = out_a
    sheet4[y * f:(y + h) * f, x * f:(x + w) * f] = bleed(region)
    return sheet4


def load_rgba(path):
    return np.asarray(Image.open(path).convert("RGBA"), float) / 255


def to_image(rgba):
    return Image.fromarray((np.clip(rgba, 0, 1) * 255 + 0.5).astype(np.uint8), "RGBA")


def file_sha(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def load_record(path):
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def source_for(tga, entry, version=VERSION):
    """What to paint over, and whether the sheet is done already:
    ("skip", None) when `tga` is this version's output; ("bak", path) when
    it is an older output, painted again from the upscale kept beside it;
    ("new", path) when `tga` is not ours (a fresh upscale): back it up and
    paint over it. With no record of the file but a .bak beside it, the
    .bak is taken as the upscale, never overwritten."""
    bak = tga + ".bak"
    if not entry:
        return ("bak", bak) if os.path.exists(bak) else ("new", tga)
    if os.path.exists(tga) and file_sha(tga) == entry.get("sha"):
        if entry.get("version") == version:
            return "skip", None
        if os.path.exists(bak):
            return "bak", bak
    return "new", tga


def build(out_dir, only=None, review=None, force=False, log=print):
    """Paints every kill-feed icon of the game into the HD sheets in
    `out_dir` (dodstudio_hd/sprites/<style>). Returns how many icons were
    painted."""
    import hdcommon as C
    import styles as S
    style = S.style_from_env()
    dod = C.dod_dir()
    hd = os.path.dirname(os.path.dirname(os.path.abspath(out_dir)))
    models_dir = os.path.join(hd, "models", style)
    entries = hud_entries(dod)
    names = [n for n in killfeed_names(entries) if n in ICONS and (not only or n in only)]
    unknown = [n for n in killfeed_names(entries) if n not in ICONS and n not in KEEP_UPSCALE]
    if unknown:
        log(f"kill-feed icons with no model in the table, left as upscaled: {', '.join(unknown)}")
    by_sheet = {}
    for n in names:
        by_sheet.setdefault(entries[n][0], []).append(n)
    record_path = os.path.join(hd, RECORD)
    record = load_record(record_path)
    done_icons, total, step = 0, len(names), 0
    report = []
    for sheet, icons in sorted(by_sheet.items()):
        # top to bottom: an icon's paint clears Valve's icon round it, so
        # the lower of two icons sharing rows (which owns them, shared_area)
        # is painted after the upper one
        icons = sorted(icons, key=lambda n: (entries[n][2], n))
        try:
            fw, fh, idx, pal, _ = sheet_frame(dod, sheet)
        except (OSError, ValueError) as e:
            log(f"{sheet}.spr unreadable ({e}): its icons left as they are")
            step += len(icons)
            continue
        key = f"{C.file_stem_name(sheet)}_0_{C.fnv1a32(idx, pal):08x}"
        tga = os.path.join(out_dir, key + ".tga")
        if not os.path.exists(tga):
            log(f"{key}.tga not built (run sprites_hd.py first): {len(icons)} icon(s) skipped")
            step += len(icons)
            print(f"@@progress {step} of {total} kill-feed icons", flush=True)
            continue
        entry = record.get(style, {}).get(key)
        if force:
            what, src = ("bak", tga + ".bak") if os.path.exists(tga + ".bak") else ("new", tga)
        else:
            what, src = source_for(tga, entry)
        if what == "skip":
            log(f"{key}.tga: kill-feed icons already painted (v{VERSION})")
            step += len(icons)
            print(f"@@progress {step} of {total} kill-feed icons", flush=True)
            continue
        if what == "new":
            shutil.copy2(tga, tga + ".bak")
        base = load_rgba(src)
        f = base.shape[1] // fw
        if f < 1 or base.shape[0] // fh != f:
            log(f"{key}.tga is {base.shape[1]}x{base.shape[0]}, not a whole multiple of {fw}x{fh}: skipped")
            step += len(icons)
            continue
        sheet4 = base.copy()
        stock_ind = np.frombuffer(idx, np.uint8).reshape(fh, fw)
        painted = []
        for n in icons:
            step += 1
            _, x, y, w, h = entries[n]
            img, info = render_icon(dod, n, ICONS[n], style, models_dir, scale=f, log=log)
            if img is None:
                log(f"  {n}: {info}, left as upscaled")
            else:
                stock_alpha = (stock_ind[y:y + h, x:x + w] != 255).astype(float)
                paint(sheet4, (x, y, w, h), stock_alpha, img)
                painted.append(n)
                report.append((n, sheet, info))
                log(f"  {n}: {info['model']}, IoU {info['iou']}, roll {info['roll']}, skin {info['skin']}")
                if review:
                    os.makedirs(review, exist_ok=True)
                    to_image(img).save(os.path.join(review, f"{n}_new_{f}x.png"))
                    to_image(base[y * f:(y + h) * f, x * f:(x + w) * f]).save(os.path.join(review, f"{n}_upscale733_{f}x.png"))
            print(f"@@progress {step} of {total} kill-feed icons", flush=True)
        if painted:
            C.save_output(to_image(sheet4), tga)
            record.setdefault(style, {})[key] = {"version": VERSION, "sha": file_sha(tga), "icons": painted}
            tmp = record_path + ".part"
            with open(tmp, "w", encoding="utf-8") as fh_:
                json.dump(record, fh_, indent=1, sort_keys=True)
            os.replace(tmp, record_path)
            done_icons += len(painted)
            log(f"{key}.tga: {len(painted)} icon(s) painted ({'from ' + os.path.basename(src) if what == 'bak' else 'upscale kept as .bak'})")
    if review:
        with open(os.path.join(review, "icons.json"), "w", encoding="utf-8") as fh_:
            json.dump([{"icon": n, "sheet": s, **i} for n, s, i in report], fh_, indent=1)
    return done_icons


# ── engine simulation, for the review sheets ─────────────────────────────────


def gl_bilinear(img, out_w, out_h):
    """GL_LINEAR sampling (2x2 texels, clamped) of img at out_w x out_h pixel
    centres over the whole image."""
    h, w = img.shape[:2]
    u = (np.arange(out_w) + 0.5) / out_w
    v = (np.arange(out_h) + 0.5) / out_h
    x = np.clip(u * w - 0.5, 0, w - 1)
    y = np.clip(v * h - 0.5, 0, h - 1)
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    x1, y1 = np.minimum(x0 + 1, w - 1), np.minimum(y0 + 1, h - 1)
    fx, fy = (x - x0)[None, :, None], (y - y0)[:, None, None]
    a = img[y0][:, x0] * (1 - fx) + img[y0][:, x1] * fx
    b = img[y1][:, x0] * (1 - fx) + img[y1][:, x1] * fx
    return a * (1 - fy) + b * fy


def as_uploaded(rgba):
    """What the hook uploads for an alpha-test sheet: alpha cut at 128,
    cut-out texels black (match_engine_expansion)."""
    keep = rgba[..., 3] >= 0.5
    out = np.zeros_like(rgba)
    out[keep, :3] = rgba[keep, :3]
    out[keep, 3] = 1
    return out


def mips(img):
    levels = [img]
    while min(levels[-1].shape[:2]) >= 2:
        l = levels[-1]
        h, w = l.shape[0] // 2 * 2, l.shape[1] // 2 * 2
        levels.append(l[:h, :w].reshape(h // 2, 2, w // 2, 2, -1).mean((1, 3)))
    return levels


def draw_stock(icon, scale):
    h, w = icon.shape[:2]
    return gl_bilinear(as_uploaded(icon), w * scale, h * scale)


def draw_hd(icon4, base_w, base_h, scale, bias=0.0):
    """Trilinear: LOD = log2(texels per screen pixel) + bias."""
    ow, oh = base_w * scale, base_h * scale
    lod = max(np.log2(icon4.shape[1] / ow) + bias, 0.0)
    levels = mips(as_uploaded(icon4))
    lo = min(int(np.floor(lod)), len(levels) - 1)
    hi = min(lo + 1, len(levels) - 1)
    f = lod - np.floor(lod)
    a = gl_bilinear(levels[lo], ow, oh)
    return a if f == 0 or hi == lo else a * (1 - f) + gl_bilinear(levels[hi], ow, oh) * f


def over(fg, bg_rgb):
    a = fg[..., 3:4]
    return fg[..., :3] * a + np.array(bg_rgb, float) * (1 - a)


BACKGROUNDS = (("dark", (0.10, 0.11, 0.12)), ("bright", (0.82, 0.80, 0.72)))


def _font(size=13):
    try:
        return ImageFont.load_default(size=size)
    except TypeError:
        return ImageFont.load_default()


def drawn(kind, img, base_w, base_h, scale):
    if kind == "stock":
        return draw_stock(img, scale)
    bias = kind[1] if isinstance(kind, tuple) else 0.0
    return draw_hd(img, base_w, base_h, scale, bias)


def sheet(groups, path, title=""):
    """groups: [(icon name, base_w, base_h, [(label, kind, image)])]; kind
    'stock' (1x), 'hd' (4x, no bias) or ('hd', bias). One block per icon,
    one row per image, scale 1 / 2 / 3 on dark then bright."""
    pad, label_w = 8, 230
    max_w = max(g[1] for g in groups)
    col_w = [max_w * s for s in (1, 2, 3)]
    W = label_w + 2 * (sum(col_w) + 3 * pad) + pad
    H = pad + 30 + sum(len(rows) * (g[2] * 3 + pad) + 22 for g in groups for rows in [g[3]])
    canvas = Image.new("RGB", (W, H), (60, 60, 64))
    d = ImageDraw.Draw(canvas)
    font, big = _font(13), _font(15)
    d.text((pad, pad), title, fill=(255, 255, 255), font=big)
    x = label_w
    for bg_name, _ in BACKGROUNDS:
        for s, cw in zip((1, 2, 3), col_w):
            d.text((x, pad + 14), f"scale {s}, {bg_name}", fill=(230, 230, 230), font=font)
            x += cw + pad
        x += pad
    y = pad + 34
    for name, bw, bh, rows in groups:
        d.text((pad, y), name, fill=(255, 220, 120), font=big)
        y += 20
        row_h = bh * 3 + pad
        for label, kind, img in rows:
            d.text((pad + 10, y + 2), label, fill=(235, 235, 235), font=font)
            x = label_w
            for _, bg in BACKGROUNDS:
                d.rectangle([x - pad // 2, y - pad // 2, x + sum(col_w) + 2 * pad + pad // 2, y + row_h - pad // 2],
                            fill=tuple(int(c * 255) for c in bg))
                for s, cw in zip((1, 2, 3), col_w):
                    rgb = (np.clip(over(drawn(kind, img, bw, bh, s), bg), 0, 1) * 255 + 0.5).astype(np.uint8)
                    canvas.paste(Image.fromarray(rgb), (x, y))
                    x += cw + pad
                x += pad
            y += row_h
        y += 2
    canvas.save(path)


def zoom_sheet(groups, path, title="", scale=3, factor=2):
    """Scale `scale` only, every pixel `factor`x, dark and bright side by side."""
    pad, label_w = 8, 230
    max_w = max(g[1] for g in groups) * scale * factor
    W = label_w + 2 * (max_w + pad) + pad
    H = pad + 26 + sum(len(g[3]) * (g[2] * scale * factor + pad) + 22 for g in groups)
    canvas = Image.new("RGB", (W, H), (60, 60, 64))
    d = ImageDraw.Draw(canvas)
    font, big = _font(13), _font(15)
    d.text((pad, pad), title, fill=(255, 255, 255), font=big)
    y = pad + 26
    for name, bw, bh, rows in groups:
        d.text((pad, y), name, fill=(255, 220, 120), font=big)
        y += 20
        for label, kind, img in rows:
            d.text((pad + 10, y + 2), label, fill=(235, 235, 235), font=font)
            x = label_w
            for _, bg in BACKGROUNDS:
                rgb = (np.clip(over(drawn(kind, img, bw, bh, scale), bg), 0, 1) * 255 + 0.5).astype(np.uint8)
                im = Image.fromarray(rgb)
                canvas.paste(im.resize((im.width * factor, im.height * factor), Image.NEAREST), (x, y))
                x += max_w + pad
            y += bh * scale * factor + pad
        y += 2
    canvas.save(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out_dir")
    ap.add_argument("--only", default="")
    ap.add_argument("--review")
    ap.add_argument("--force", action="store_true")
    args = ap.parse_args()
    only = {n.strip() for n in args.only.split(",") if n.strip()} or None
    n = build(args.out_dir, only=only, review=args.review, force=args.force)
    print(f"painted {n} kill-feed icon(s) into {args.out_dir}")


if __name__ == "__main__":
    main()
