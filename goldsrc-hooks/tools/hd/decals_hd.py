"""HD decals: dodstudio_hd/decals/<style>/<name>_<hash>.tga (#729).

For every decal in decals.wad -- bullet holes, blood, scorch marks, the map
logos -- keyed by (name, FNV-1a-32 of mip 0 + the 768-byte palette) exactly
as texture_hires.rs computes it when Draw_MiptexTexture uploads the decal
through GL_LoadTexture2. The name is the one the engine uploads, not the
lump's: it overwrites the first character from the palette (goldsrc.decal_name):

  `{`  palette entry 255 is pure blue: a cut-out, like a masked wall texture.
       Cut-outs filled with their nearest colour first; alpha from the
       original mask (bilinear + threshold).
  `}`  anything else: index-alpha, like a smoke sprite. The index *is* the
       alpha, upscaled as a greyscale image; RGB is palette[255], all the
       engine ever draws.

Each is reflect-padded (decals don't tile), 4x'd in the style and Lanczos'd to
the power-of-two target (capped at HD_CAP a side, 1024 by default).

The engine merges every decals.wad on its search path (Decal_MergeInDecals),
so with no wad named both dod/decals.wad and valve/decals.wad are built in
full: whichever copy of a name the engine picks has a file, and the hash in
the name keeps the copies apart (identical ones are built once).

usage: python decals_hd.py <out_dir> [<decals.wad> ...]
env:   HD_STYLE (default ultrasharp), HD_GAME, HD_WORK, HD_BATCH
"""
import os, sys
import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt

import hdcommon as C
import styles as S
from goldsrc import decal_name, is_cutout_decal, wad_miptexes


def game_wads(game):
    return [p for p in (os.path.join(game, g, "decals.wad") for g in ("dod", "valve")) if os.path.exists(p)]


def pad_amount(n):
    return n // 2


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out_dir, wads = sys.argv[1], sys.argv[2:] or game_wads(C.game_root())
    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("decals_" + style)

    jobs, blank = {}, 0
    for wad in wads:
        for lump, w, h, idx, pal in wad_miptexes(wad):
            if len(idx) != w * h or len(pal) != 768:
                continue
            cutout = is_cutout_decal(pal)
            # A cut-out that is all cut out: nothing to upscale, and the hook
            # labels it blank rather than looking for a file.
            if cutout and idx.count(255) == len(idx):
                blank += 1
                continue
            key = f"{C.file_stem_name(decal_name(lump, pal))}_{C.fnv1a32(idx, pal):08x}"
            if not C.built(os.path.join(out_dir, key + ".tga"), C.pot(w * 4), C.pot(h * 4)):
                jobs.setdefault(key, (cutout, w, h, idx, pal))
    print(f"{len(jobs)} decal(s) to build from {len(wads)} wad(s)" + (f" ({blank} blank skipped)" if blank else ""))

    masks = {}

    def prepare(key, job):
        cutout, w, h, idx, pal = job
        ind = np.frombuffer(idx, np.uint8).reshape(h, w)
        if cutout:
            rgb = np.frombuffer(pal, np.uint8).reshape(256, 3)[ind].copy()
            mask = ind == 255
            if mask.any():
                _, (iy, ix) = distance_transform_edt(mask, return_indices=True)
                rgb = rgb[iy, ix]
            masks[key] = ~mask
        else:
            rgb = np.repeat(ind[:, :, None], 3, axis=2)
        py, px = pad_amount(h), pad_amount(w)
        mode = "reflect" if min(h, w) > 1 else "edge"
        return Image.fromarray(np.pad(rgb, ((py, py), (px, px), (0, 0)), mode=mode))

    def finish(key, job, src):
        cutout, w, h, idx, pal = job
        tw, th = C.pot(w * 4), C.pot(h * 4)
        py, px = pad_amount(h), pad_amount(w)
        # The padded decal scaled so the original part is exactly tw x th.
        sw, sh = round(tw * (w + 2 * px) / w), round(th * (h + 2 * py) / h)
        ox, oy = round(tw * px / w), round(th * py / h)
        img = Image.open(src).convert("RGB").resize((sw, sh), Image.LANCZOS).crop((ox, oy, ox + tw, oy + th))
        if cutout:
            alpha = Image.fromarray(masks.pop(key).astype(np.uint8) * 255, "L").resize((tw, th), Image.BILINEAR)
            img = img.convert("RGBA")
            img.putalpha(alpha.point(lambda v: 255 if v >= 128 else 0))
        else:
            alpha = img.convert("L")
            img = Image.new("RGBA", (tw, th), tuple(pal[765:768]) + (255,))
            img.putalpha(alpha)
        C.save_output(img, os.path.join(out_dir, key + ".tga"))

    done = S.upscale_batches(work, style, jobs, prepare, finish)
    print(f"wrote {done} decal replacement(s) to {out_dir}")


if __name__ == "__main__":
    main()
