"""HD model skins: dodstudio_hd/models/<style>/<texture>_<hash>.tga.

For every skin in the given models, keyed by (texture name, FNV-1a-32 of its
8-bit pixels + 768-byte palette) exactly as texture_hires.rs computes it for
model skins:

  extract -> (masked skins: fill the cut-outs) -> reflect-pad -> 4x in the
  style -> Lanczos to the power-of-two target (capped at HD_CAP a side,
  1024 by default) ->
  (masked: alpha from the original mask, less a weapon skin's specks)

A weapon skin's specks (`specks`) are see-through blobs too small to be a
real hole: they are drawn solid, in their neighbours' colour.

Skins are UV atlases, not tiles, so the padding mirrors each edge rather
than wrapping to the opposite one.

Files are matched by content, so skins from several installs can share one
folder: stock and custom versions of the same weapon each get their own file,
and whichever the game loads finds its match.

usage: python models_hd.py <out_dir> <source> [<source> ...]
  source: a .mdl file; a folder (every .mdl under it, recursively); or a .txt
  list of .mdl paths, one per line, relative ones read from <game>/valve/models
env:    HD_STYLE (default ultrasharp), HD_GAME, HD_WORK, HD_BATCH
"""
import glob, os, sys
import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt, label

import hdcommon as C
import styles as S
from goldsrc import STUDIO_NF_MASKED, mdl_textures

# A see-through blob of this many pixels or fewer on a weapon skin is a
# speck, not a hole. Some custom skins were converted to 256 colours with
# palette entry 255 left as an ordinary near-black, so dark metal pixels
# that landed on it are cut out: PRE's custom v_garand has 1,170 such blobs
# and no real hole, and each blinks as the gun sways. The smallest real hole
# in any stock or custom v_/p_ skin in either movie install is 21 pixels (a
# 256x256 skin's sight or guard); 8 clears 99% of the Garand's specks and
# stays well under that.
SPECK_PX = 8
# Models held in hand, first person (v_) and third (p_). Map models are left
# alone: a tree's 1-3 px gaps between leaves, a torn flag's frayed edge and
# a wire mesh's gaps are small holes on purpose.
HELD = ("v_", "p_")


def is_held(f):
    return os.path.basename(f).lower().startswith(HELD)


def specks(mask, max_px=SPECK_PX):
    """The pixels of `mask` (see-through) in blobs of at most `max_px`
    pixels, corner neighbours counted as one blob: solid pixels surround
    each one. The skin's edge counts as solid, since no UV reaches past it
    (PRE's Garand has 85 specks on its edge column, on the barrel)."""
    lab, n = label(mask, structure=np.ones((3, 3), bool))
    if not n:
        return np.zeros(mask.shape, bool)
    small = np.bincount(lab.ravel()) <= max_px
    small[0] = False
    return small[lab]


def solid(ind, held):
    """The pixels a masked skin's HD file draws solid: all but index 255,
    and on a `held` (weapon) skin its specks too."""
    see_through = ind == 255
    if held:
        see_through &= ~specks(see_through)
    return ~see_through


def is_companion(f):
    """`fooT.mdl` holds foo.mdl's textures; it's read through foo.mdl."""
    return f.lower().endswith("t.mdl") and os.path.exists(f[:-5] + ".mdl")


def model_files(arg):
    if arg.lower().endswith(".txt"):
        base = os.path.join(C.game_root(), "valve", "models")
        lines = [l.strip() for l in open(arg, encoding="utf-8") if l.strip() and not l.startswith("#")]
        return [l if os.path.isabs(l) else os.path.join(base, l) for l in lines]
    if os.path.isdir(arg):
        return [f for f in glob.glob(os.path.join(arg, "**", "*.mdl"), recursive=True) if not is_companion(f)]
    return [arg]


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out_dir, sources = sys.argv[1], sys.argv[2:]
    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("models_" + style)

    jobs = {}
    held = set()  # keys of skins a v_ or p_ model uses
    for src in sources:
        for f in model_files(src):
            if not os.path.exists(f):
                print(f"  missing {f}, skipped")
                continue
            for name, flags, w, h, idx, pal in mdl_textures(f):
                key = f"{C.file_stem_name(name)}_{C.fnv1a32(idx, pal):08x}"
                if is_held(f):
                    held.add(key)
                if not C.built(os.path.join(out_dir, key + ".tga"), C.pot(w * 4), C.pot(h * 4)):
                    jobs.setdefault(key, (name, flags, w, h, idx, pal))
    print(f"{len(jobs)} model skins to build")

    masks = {}

    def prepare(key, job):
        name, flags, w, h, idx, pal = job
        ind = np.frombuffer(idx, np.uint8).reshape(h, w)
        rgb = np.frombuffer(pal, np.uint8).reshape(256, 3)[ind].copy()
        if flags & STUDIO_NF_MASKED:
            mask = ind == 255
            if mask.all():
                return None
            if mask.any():
                # Every see-through pixel, specks too, takes its nearest
                # solid pixel's colour.
                _, (iy, ix) = distance_transform_edt(mask, return_indices=True)
                rgb = rgb[iy, ix]
            masks[key] = solid(ind, key in held)
        pad = np.pad(rgb, ((h // 2, h // 2), (w // 2, w // 2), (0, 0)), mode="reflect")
        return Image.fromarray(pad)

    def finish(key, job, src):
        name, flags, w, h, idx, pal = job
        tw, th = C.pot(w * 4), C.pot(h * 4)
        centre = (tw // 2, th // 2, tw // 2 + tw, th // 2 + th)
        img = Image.open(src).convert("RGB").resize((tw * 2, th * 2), Image.LANCZOS).crop(centre)
        if key in masks:
            alpha = Image.fromarray(masks.pop(key).astype(np.uint8) * 255, "L").resize((tw, th), Image.BILINEAR)
            img = img.convert("RGBA")
            img.putalpha(alpha.point(lambda v: 255 if v >= 128 else 0))
        C.save_output(img, os.path.join(out_dir, key + ".tga"))

    done = S.upscale_batches(work, style, jobs, prepare, finish)
    print(f"wrote {done} model skin replacement(s) to {out_dir}")


if __name__ == "__main__":
    main()
