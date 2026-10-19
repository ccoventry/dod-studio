"""R&D (#841): kill-feed icons rendered from the weapon models instead of
upscaled from Valve's tiny sprites. Prototype for d_kar only.

The kill feed draws its icon with SPR_Set(icon, 255, 255, 255) and
SPR_DrawHoles (docs/goldsrc_death_notices.md section 6): the sprite's own
colours, untinted, its index-255 texels cut out -- and with gl_spriteblend 1
(the default) alpha-blended, so soft edges fade. So a rendered icon is a
full-colour RGBA image, transparent texels bled with their nearest opaque
colour (the engine's mip chain averages RGBA naively, see sprites_hd.py).

Steps:
  1. read the model (studiomodel.py), body 0 of each bodypart, frame 0 of
     sequence 0;
  2. pick the side-on view: the model's principal axes, every muzzle/up
     sign and a small roll, scored by silhouette overlap with Valve's icon
     (so the icon faces and fills its box as the stock one does);
  3. render orthographically at 4x the hud.txt box, supersampled, z-buffered,
     textured (stock skin, or the skin in an HD style), two-sided Lambert;
  4. optionally a thin dark outline;
  5. a comparison sheet: stock, the #733 upscale and each render, simulated
     as the engine draws them at kill-feed scale 1, 2 and 3 (bilinear for
     the stock 1x sheet; trilinear mips for a 4x HD sheet, optionally with
     a LOD bias) over a dark and a bright background.

usage: python killfeed_icons.py <out_dir> [--game <Half-Life folder>]
                                [--hd <Half-Life folder>] [--no-ai]
  --game  the install whose models and sprites are read (default: HD_GAME /
          DoD Studio's / the only Steam one, as the HD scripts pick)
  --hd    the install whose dod_addon/dodstudio_hd holds the #733 upscale
  --no-ai skip the Real-ESRGAN skin upscale
env: REALESRGAN (else %APPDATA%/dod-studio/hd_tools/realesrgan), HD_STYLE
Reads game files only; writes PNGs to <out_dir>.
"""
import argparse, os, re, shutil, sys, tempfile

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from studiomodel import STUDIO_NF_MASKED, StudioModel  # noqa: E402

UP = 4  # HD sheets are 4x
SS = 4  # supersampling per HD pixel

# ── hud.txt and the stock icon ───────────────────────────────────────────────


def hud_rect(dod, name):
    """(sheet, x, y, w, h) of the 640 entry `name` in sprites/hud.txt."""
    for line in open(os.path.join(dod, "sprites", "hud.txt"), encoding="latin1"):
        parts = line.split()
        if len(parts) == 7 and parts[0] == name and parts[1] == "640":
            x, y, w, h = (int(p) for p in parts[3:7])
            return parts[2], x, y, w, h
    raise KeyError(f"{name} not in hud.txt")


def stock_icon(dod, sheet, x, y, w, h):
    """The stock icon as RGBA (h, w, 4) float 0..1, plus the sheet's frame
    pixels and palette (for the HD key)."""
    from goldsrc import Sprite
    s = Sprite(os.path.join(dod, "sprites", sheet + ".spr"))
    _, fw, fh, idx = s.frames[0]
    if x + w > fw or y + h > fh:
        raise ValueError(f"{sheet}: rect past the {fw}x{fh} frame")
    ind = np.frombuffer(idx, np.uint8).reshape(fh, fw)[y:y + h, x:x + w]
    pal = np.frombuffer(s.palette, np.uint8).reshape(256, 3)
    rgba = np.dstack([pal[ind], np.where(ind == 255, 0, 255)]).astype(float) / 255
    return bleed(rgba), idx, s.palette


def main_component(alpha):
    """Mask of the biggest connected opaque blob (drops stray bits of a
    neighbouring icon that fall inside the rect)."""
    lab, n = ndimage.label(alpha > 0.5)
    if n == 0:
        return alpha > 0.5
    sizes = ndimage.sum(np.ones_like(lab), lab, range(1, n + 1))
    return lab == 1 + int(np.argmax(sizes))


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


def model_triangles(path, skip_tex=(), skip_parts=()):
    m = StudioModel(path)
    keep = [t for name, model in m.parts if name not in skip_parts
            for t in _model_tris(m, model) if not any(re.search(p, t[3].name, re.I) for p in skip_tex)]
    return keep


def _model_tris(m, model):
    out = []
    for mesh in model["meshes"]:
        tex = m.texture_for(mesh.skin)
        if tex is None or not len(mesh.verts):
            continue
        uv = mesh.st / np.array([tex.width, tex.height], float)
        out.append((model["verts"][mesh.verts], model["norms"][mesh.norms], uv, tex))
    return out


def principal_axes(tris):
    pts = np.concatenate([t[0].reshape(-1, 3) for t in tris])
    c = pts - pts.mean(0)
    _, _, vt = np.linalg.svd(c, full_matrices=False)
    return vt  # rows: longest, middle, shortest


def view_basis(a1, a2, sx, sy, roll_deg):
    r = np.radians(roll_deg)
    right = sx * a1
    up = sy * a2
    right, up = np.cos(r) * right + np.sin(r) * up, -np.sin(r) * right + np.cos(r) * up
    toward = np.cross(right, up)
    return np.array([right, up, toward])


def fit_transform(tris, basis, box, max_stretch=1.0):
    """(kx, ky, ox, oy): scales and offset that contain the projected model
    inside `box` (x0, y0, x1, y1 in output pixels), centred. A model
    narrower than the box's aspect is stretched vertically by up to
    `max_stretch`, as Valve's icons exaggerate a rifle's depth."""
    pts = np.concatenate([t[0].reshape(-1, 3) for t in tris]) @ basis.T
    lo, hi = pts[:, :2].min(0), pts[:, :2].max(0)
    bw, bh = box[2] - box[0], box[3] - box[1]
    fit_w, fit_h = bw / max(hi[0] - lo[0], 1e-6), bh / max(hi[1] - lo[1], 1e-6)
    kx = min(fit_w, fit_h)
    ky = min(fit_h, kx * max_stretch)
    cx, cy = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    mx, my = (lo + hi) / 2
    return kx, ky, cx - kx * mx, cy + ky * my  # screen y = oy - ky * up


def silhouette(tris, basis, kx, ky, ox, oy, w, h, ss=4):
    img = Image.new("L", (w * ss, h * ss), 0)
    d = ImageDraw.Draw(img)
    for pos, _, _, _ in tris:
        p = pos @ basis.T
        xs, ys = (ox + kx * p[..., 0]) * ss, (oy - ky * p[..., 1]) * ss
        for tx, ty in zip(xs, ys):
            d.polygon(list(zip(tx.tolist(), ty.tolist())), fill=255)
    return np.asarray(img.resize((w, h), Image.BOX), float) / 255


def choose_view(tris, target_alpha, box):
    """The (basis, k, ox, oy) whose silhouette best overlaps the stock icon,
    at stock resolution."""
    h, w = target_alpha.shape
    a1, a2, _ = principal_axes(tris)
    target = target_alpha > 0.5
    best = None
    for sx in (1, -1):
        for sy in (1, -1):
            for roll in range(-10, 11, 2):
                basis = view_basis(a1, a2, sx, sy, roll)
                sil = silhouette(tris, basis, *fit_transform(tris, basis, box), w, h) > 0.5
                iou = (sil & target).sum() / max((sil | target).sum(), 1)
                if best is None or iou > best[0]:
                    best = (iou, sx, sy, roll)
    iou, sx, sy, roll = best
    # Refine the roll to half a degree.
    for r in np.arange(roll - 1.5, roll + 1.6, 0.5):
        basis = view_basis(a1, a2, sx, sy, r)
        sil = silhouette(tris, basis, *fit_transform(tris, basis, box), w, h) > 0.5
        s = (sil & target).sum() / max((sil | target).sum(), 1)
        if s > iou:
            iou, roll = s, r
    return view_basis(a1, a2, sx, sy, roll), iou, (sx, sy, roll)


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


def render(tris, basis, kx, ky, ox, oy, w, h, skins, light=None, ambient=0.5, diffuse=0.7, ss=SS):
    """RGBA (h, w, 4) float 0..1. skins: {texture name: RGBA float array}
    to override a texture (an HD skin); others use the model's own."""
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
        masked = bool(tex.flags & STUDIO_NF_MASKED)
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
    return bleed(np.dstack([rgb, a]))


def outline(rgba, radius, colour=(0.06, 0.06, 0.06)):
    """A dark rim `radius` px wide (soft-edged) under the icon."""
    a = rgba[..., 3]
    dist = ndimage.distance_transform_edt(a < 0.5)
    rim = np.clip(radius + 0.5 - dist, 0, 1)
    out_a = a + rim * (1 - a)
    rgb = (rgba[..., :3] * a[..., None] + np.array(colour) * (rim * (1 - a))[..., None]) / np.maximum(out_a, 1e-9)[..., None]
    return bleed(np.dstack([rgb, out_a]))


# ── HD skins ─────────────────────────────────────────────────────────────────


def esrgan_path():
    env = os.environ.get("REALESRGAN")
    if env:
        return env
    appdata = os.environ.get("APPDATA", "")
    return os.path.join(appdata, "dod-studio", "hd_tools", "realesrgan", "realesrgan-ncnn-vulkan.exe")


def hd_skins(textures, style):
    """{name: 4x RGBA float} through styles.upscale, padded as models_hd.py
    pads skins (reflect, half a side); None when the upscaler is missing."""
    exe = esrgan_path()
    if not os.path.exists(exe):
        return None
    os.environ["REALESRGAN"] = exe
    import styles as S
    S.ESRGAN = exe
    work = tempfile.mkdtemp(prefix="killfeed_hd_")
    try:
        src, dst = os.path.join(work, "in"), os.path.join(work, "out")
        os.makedirs(src)
        for i, t in enumerate(textures):
            rgb = t.rgba()[..., :3]
            pad = np.pad(rgb, ((t.height // 2,) * 2, (t.width // 2,) * 2, (0, 0)), mode="reflect")
            Image.fromarray(pad).save(os.path.join(src, f"{i}.png"))
        S.upscale(src, dst, style)
        out = {}
        for i, t in enumerate(textures):
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


# ── engine simulation and the sheet ──────────────────────────────────────────


def gl_bilinear(img, out_w, out_h, u0=0.0, u1=1.0, v0=0.0, v1=1.0):
    """GL_LINEAR sampling (2x2 texels, clamped) of img at out_w x out_h pixel
    centres, the quad mapping u0..u1, v0..v1."""
    h, w = img.shape[:2]
    u = u0 + (np.arange(out_w) + 0.5) / out_w * (u1 - u0)
    v = v0 + (np.arange(out_h) + 0.5) / out_h * (v1 - v0)
    x = np.clip(u * w - 0.5, 0, w - 1)
    y = np.clip(v * h - 0.5, 0, h - 1)
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    x1, y1 = np.minimum(x0 + 1, w - 1), np.minimum(y0 + 1, h - 1)
    fx, fy = (x - x0)[None, :, None], (y - y0)[:, None, None]
    a = img[y0][:, x0] * (1 - fx) + img[y0][:, x1] * fx
    b = img[y1][:, x0] * (1 - fx) + img[y1][:, x1] * fx
    return a * (1 - fy) + b * fy


def mips(img):
    levels = [img]
    while min(levels[-1].shape[:2]) >= 2:
        l = levels[-1]
        h, w = l.shape[0] // 2 * 2, l.shape[1] // 2 * 2
        levels.append(l[:h, :w].reshape(h // 2, 2, w // 2, 2, -1).mean((1, 3)))
    return levels


def draw_stock(icon, scale):
    h, w = icon.shape[:2]
    return gl_bilinear(icon, w * scale, h * scale)


def draw_hd(icon4, base_w, base_h, scale, bias=0.0):
    """Trilinear: LOD = log2(texels per screen pixel) + bias."""
    ow, oh = base_w * scale, base_h * scale
    lod = max(np.log2(icon4.shape[1] / ow) + bias, 0.0)
    levels = mips(icon4)
    lo = min(int(np.floor(lod)), len(levels) - 1)
    hi = min(lo + 1, len(levels) - 1)
    f = lod - np.floor(lod)
    a = gl_bilinear(levels[lo], ow, oh)
    return a if f == 0 or hi == lo else a * (1 - f) + gl_bilinear(levels[hi], ow, oh) * f


def over(fg, bg_rgb):
    a = fg[..., 3:4]
    return fg[..., :3] * a + np.array(bg_rgb, float) * (1 - a)


BACKGROUNDS = (("dark", (0.10, 0.11, 0.12)), ("bright", (0.82, 0.80, 0.72)))


def sheet(rows, base_w, base_h, path):
    """rows: [(label, kind, image)] kind 'stock' (1x) or 'hd' (4x) or
    ('hd', bias)."""
    pad, label_w = 10, 330
    col_w = [base_w * s for s in (1, 2, 3)]
    row_h = base_h * 3 + pad
    W = label_w + 2 * (sum(col_w) + 3 * pad) + pad
    H = pad + 24 + len(rows) * (row_h + pad)
    canvas = Image.new("RGB", (W, H), (60, 60, 64))
    d = ImageDraw.Draw(canvas)
    try:
        font = ImageFont.load_default(size=13)
    except TypeError:
        font = ImageFont.load_default()
    x = label_w
    for bg_name, bg in BACKGROUNDS:
        for s, cw in zip((1, 2, 3), col_w):
            d.text((x, pad), f"scale {s}, {bg_name}", fill=(230, 230, 230), font=font)
            x += cw + pad
        x += pad
    y = pad + 24
    for label, kind, img in rows:
        d.multiline_text((pad, y + 4), label, fill=(235, 235, 235), font=font, spacing=3)
        x = label_w
        for _, bg in BACKGROUNDS:
            d.rectangle([x - pad // 2, y - pad // 2, x + sum(col_w) + 2 * pad + pad // 2, y + row_h - pad // 2],
                        fill=tuple(int(c * 255) for c in bg))
            for s, cw in zip((1, 2, 3), col_w):
                if kind == "stock":
                    drawn = draw_stock(img, s)
                else:
                    bias = kind[1] if isinstance(kind, tuple) else 0.0
                    drawn = draw_hd(img, base_w, base_h, s, bias)
                rgb = (np.clip(over(drawn, bg), 0, 1) * 255 + 0.5).astype(np.uint8)
                canvas.paste(Image.fromarray(rgb), (x, y))
                x += cw + pad
            x += pad
        y += row_h + pad
    canvas.save(path)


def save_rgba(img, path):
    Image.fromarray((np.clip(img, 0, 1) * 255 + 0.5).astype(np.uint8), "RGBA").save(path)


# ── d_kar prototype ──────────────────────────────────────────────────────────

# Lighting variants: "" is the default; "_bright" lifts the dark metal so it
# still reads over a dark background.
LIGHTING = {"": {}, "_bright": {"ambient": 0.8, "diffuse": 0.55}}
# Which variants go on the sheet, per model prefix: (variant, LOD bias).
# STYLE is the HD style's skin (or the stock skin without an upscaler).
SHEET_ROWS = {
    "w_": [("stock", 0.0), ("STYLE", 0.0), ("STYLE_outline", 0.0)],
    "p_": [("STYLE_outline", 0.0), ("STYLE_bright_outline", 0.0), ("STYLE_bright_outline", -1.0)],
    "v_": [("STYLE", 0.0)],
}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out_dir")
    ap.add_argument("--game")
    ap.add_argument("--hd")
    ap.add_argument("--no-ai", action="store_true")
    ap.add_argument("--icon", default="d_kar")
    ap.add_argument("--models", default="w_98k.mdl,p_k98.mdl,v_98k.mdl")
    args = ap.parse_args()
    if args.game:
        os.environ["HD_GAME"] = args.game
    import hdcommon as C
    import styles as S
    game = args.game or C.game_root()
    dod = os.path.join(game, "dod")
    os.makedirs(args.out_dir, exist_ok=True)
    style = S.style_from_env()

    sheet_name, x, y, w, h = hud_rect(dod, args.icon)
    print(f"{args.icon}: {sheet_name} {x},{y} {w}x{h}")
    stock, frame_idx, frame_pal = stock_icon(dod, sheet_name, x, y, w, h)
    target = main_component(stock[..., 3])
    ys, xs = np.nonzero(target)
    box1 = (xs.min(), ys.min(), xs.max() + 1, ys.max() + 1)
    save_rgba(stock, os.path.join(args.out_dir, f"{args.icon}_stock_1x.png"))
    rows = [(f"stock {sheet_name}.spr\n{w}x{h}, bilinear", "stock", stock)]

    # The #733 upscale, if built.
    hd_root = os.path.join(args.hd or game, "dod_addon", "dodstudio_hd", "sprites", style)
    key = f"{sheet_name}_0_{C.fnv1a32(frame_idx, frame_pal):08x}.tga"
    up_path = os.path.join(hd_root, key)
    if os.path.exists(up_path):
        big = np.asarray(Image.open(up_path).convert("RGBA"), float) / 255
        fw = int(round(big.shape[1] / 256))  # sheet scale
        up = big[y * fw:(y + h) * fw, x * fw:(x + w) * fw]
        if fw != UP:
            up = np.asarray(Image.fromarray((up * 255).astype(np.uint8), "RGBA").resize((w * UP, h * UP), Image.LANCZOS), float) / 255
        save_rgba(up, os.path.join(args.out_dir, f"{args.icon}_upscale733_{style}_4x.png"))
        rows.append((f"#733 upscale ({style})\nmipmapped", "hd", up))
    else:
        print(f"no #733 upscale at {up_path}")

    W, H = w * UP, h * UP
    for model_name in args.models.split(","):
        path = os.path.join(dod, "models", model_name)
        if not os.path.exists(path):
            print(f"{model_name}: not found, skipped")
            continue
        is_v = model_name.lower().startswith("v_")
        tris = model_triangles(path, skip_tex=(r"hand", r"sleeve", r"glove") if is_v else (),
                               skip_parts=("sleeve", "hands", "arms") if is_v else ())
        ntri = sum(len(t[0]) for t in tris)
        # Leave room for a 3 px outline (at 4x) inside the box.
        margin = 3
        box = (box1[0] * UP + margin, box1[1] * UP + margin, box1[2] * UP - margin, box1[3] * UP - margin)
        basis, iou, pick = choose_view(tris, target, tuple(v / UP for v in box))
        print(f"{model_name}: {ntri} tris, view muzzle{'+' if pick[0] > 0 else '-'} up{'+' if pick[1] > 0 else '-'} "
              f"roll {pick[2]:+.1f}, silhouette IoU {iou:.2f}")
        stem = os.path.splitext(model_name)[0]
        textures = sorted({id(t[3]): t[3] for t in tris}.values(), key=lambda t: t.name)
        skins = None if args.no_ai else hd_skins(textures, style)
        if skins is None and not args.no_ai:
            print(f"Real-ESRGAN not found at {esrgan_path()}: no {style} skin")
        variants = {}
        fit = fit_transform(tris, basis, box)
        for light, kw in LIGHTING.items():
            variants["stock" + light] = render(tris, basis, *fit, W, H, {}, **kw)
            if skins:
                variants[style + light] = render(tris, basis, *fit, W, H, skins, **kw)
        for name in list(variants):
            variants[name + "_outline"] = outline(variants[name], 3)
        for name, img in variants.items():
            save_rgba(img, os.path.join(args.out_dir, f"{args.icon}_{stem}_{name}_4x.png"))
        best = style if skins else "stock"
        picks = SHEET_ROWS.get(stem[:2], [("stock", 0.0)])
        for name, bias in picks:
            name = name.replace("STYLE", best)
            if name not in variants:
                continue
            label = name.replace("_", " + ").replace("+ bright", "+ brighter light")
            if name.startswith("stock"):
                label = label.replace("stock", "stock skin", 1)
            else:
                label = label.replace(best, f"{best} skin", 1)
            label += ", LOD bias -1" if bias else ""
            rows.append((f"{model_name} ({ntri} tris)\n{label}", ("hd", bias), variants[name]))
    sheet(rows, w, h, os.path.join(args.out_dir, f"{args.icon}_comparison.png"))
    print(f"wrote {os.path.join(args.out_dir, args.icon + '_comparison.png')}")


if __name__ == "__main__":
    main()
