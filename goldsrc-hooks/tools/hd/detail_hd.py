"""HD detail textures: dodstudio_hd/detail/<style>/<same name>.tga.

Reads the game's gfx/detail/*.tga (never writes there), and for each:

  wrap-pad half a tile each side -> 4x in the style -> Lanczos to 2x the
  power-of-two target (capped at HD_CAP a side, 1024 by default) -> crop the
  centre tile -> match
  each channel's mean and spread to the original

Wrap padding matters even more here than for walls: a detail texture is
tiled many times across every surface it's on. Matching mean and spread
matters because the engine blends detail multiplicatively over the wall
(mid-grey = no change): an upscaler that brightened or flattened it would
brighten or flatten every wall that uses it.

Files already built at the cap (or whose 4x wouldn't be any bigger) are
skipped. Most detail textures are 512 a side, so a cap of 2048 is where
they gain: at 1024 they are only 2x. Detail textures stop at 2048 whatever
the cap (DETAIL_CAP): the game reads each one into a buffer it allocates
for every detail texture a map loads, and the hook sizes that buffer for
2048 (texture_hires.rs, DETAIL_MAX_SIDE). A bigger file would be skipped.

usage: python detail_hd.py <out_dir> [<folder of .tga>]    (default gfx/detail)
env:   HD_STYLE (default ultrasharp), HD_GAME, HD_WORK
"""
import glob, os, sys
import numpy as np
from PIL import Image

import hdcommon as C
import styles as S

# The hook's DETAIL_MAX_SIDE: the largest detail texture the game will load.
DETAIL_CAP = min(C.CAP, 2048)


def target(n):
    """4x `n`, rounded up to a power of two, at most DETAIL_CAP."""
    return min(C.pot(n * 4), DETAIL_CAP)


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out_dir = sys.argv[1]
    src_dir = sys.argv[2] if len(sys.argv) > 2 else os.path.join(C.dod_dir(), "gfx", "detail")
    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("detail_" + style)

    jobs = {}
    for f in sorted(glob.glob(os.path.join(src_dir, "*.tga"))):
        name = os.path.basename(f)
        with Image.open(f) as im:
            w, h = im.size
        if target(w) <= w and target(h) <= h:
            continue  # already at the cap: nothing to gain
        if C.built(os.path.join(out_dir, name), target(w), target(h)):
            continue
        a = np.asarray(Image.open(f).convert("RGB"))
        jobs[name] = a
        pad = np.pad(a, ((h // 2, h // 2), (w // 2, w // 2), (0, 0)), mode="wrap")
        Image.fromarray(pad).save(os.path.join(work, "in", name[:-4] + ".png"))
    print(f"{len(jobs)} detail textures to build")

    S.upscale(os.path.join(work, "in"), os.path.join(work, "out"), style)

    done = 0
    for name, a in jobs.items():
        src = os.path.join(work, "out", name[:-4] + ".png")
        if not os.path.exists(src):
            continue
        h, w = a.shape[:2]
        tw, th = target(w), target(h)
        centre = (tw // 2, th // 2, tw // 2 + tw, th // 2 + th)
        up = np.asarray(Image.open(src).convert("RGB").resize((tw * 2, th * 2), Image.LANCZOS).crop(centre)).astype(np.float32)
        o = a.astype(np.float32)
        for c in range(3):
            um, us = up[..., c].mean(), up[..., c].std()
            om, os_ = o[..., c].mean(), o[..., c].std()
            up[..., c] = (up[..., c] - um) * (os_ / us if us > 1e-3 else 1.0) + om
        C.save_output(Image.fromarray(np.clip(up + 0.5, 0, 255).astype(np.uint8), "RGB"), os.path.join(out_dir, name))
        done += 1
    print(f"wrote {done} HD detail texture(s) to {out_dir}")


if __name__ == "__main__":
    main()
