"""HD model skins: dodstudio_hd/models/<style>/<texture>_<hash>.tga.

For every skin in the given models, keyed by (texture name, FNV-1a-32 of its
8-bit pixels + 768-byte palette) exactly as texture_hires.rs computes it for
model skins:

  extract -> (masked skins: fill the cut-outs) -> reflect-pad -> 4x in the
  style -> Lanczos to the power-of-two target (capped at 1024/side) ->
  (masked: alpha from the original mask)

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
from scipy.ndimage import distance_transform_edt

import hdcommon as C
import styles as S
from goldsrc import STUDIO_NF_MASKED, mdl_textures


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
    for src in sources:
        for f in model_files(src):
            if not os.path.exists(f):
                print(f"  missing {f}, skipped")
                continue
            for name, flags, w, h, idx, pal in mdl_textures(f):
                key = f"{C.file_stem_name(name)}_{C.fnv1a32(idx, pal):08x}"
                if not os.path.exists(os.path.join(out_dir, key + ".tga")):
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
                _, (iy, ix) = distance_transform_edt(mask, return_indices=True)
                rgb = rgb[iy, ix]
            masks[key] = ~mask
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
