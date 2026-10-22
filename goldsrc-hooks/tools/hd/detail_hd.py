"""HD detail textures: dodstudio_hd/detail/<style>/<same name>.tga.

Reads the game's gfx/detail/*.tga (never writes there), and for each:

  wrap-pad half a tile (at most PAD px) each side -> 4x in the style ->
  Lanczos the tile's own part to the power-of-two target (capped at HD_CAP
  a side, 1024 by default), the padding feeding the filter at its edges ->
  match each channel's mean and spread to the original

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
env:   HD_STYLE (default ultrasharp), HD_GAME, HD_WORK, HD_BATCH
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


# Wrap padding each side, in source pixels: half a tile, up to PAD, as in
# world_hd.py (#383). Half a tile of a 512 px detail texture made the
# upscaler work on a 1024 px image. Measured on 32 of the movie install's
# detail textures (16 at 512 px, 8 at 256, 8 at 128): 2.2x faster with
# ultrasharp and with x4plus; the 128 px ones come out byte-identical; of the
# 20 whose source tiles cleanly, none has a worse seam than before with
# ultrasharp (within 1%) and none is worse by 10% with x4plus.
PAD = 64


def margins(w, h):
    """(rows, columns) of wrap padding for a w x h texture."""
    return min(PAD, h // 2), min(PAD, w // 2)


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
        jobs[name[:-4]] = (name, a)
    print(f"{len(jobs)} detail textures to build")

    def prepare(key, job):
        name, a = job
        h, w = a.shape[:2]
        py, px = margins(w, h)
        return Image.fromarray(np.pad(a, ((py, py), (px, px), (0, 0)), mode="wrap"))

    def finish(key, job, src):
        name, a = job
        h, w = a.shape[:2]
        tw, th = target(w), target(h)
        # The upscaled image is the tile plus 4x the padding each side: resize
        # just the tile's part, the padding feeding the filter at its edges.
        py, px = margins(w, h)
        box = (px * 4, py * 4, (px + w) * 4, (py + h) * 4)
        up = np.asarray(Image.open(src).convert("RGB").resize((tw, th), Image.LANCZOS, box=box)).astype(np.float32)
        o = a.astype(np.float32)
        for c in range(3):
            um, us = up[..., c].mean(), up[..., c].std()
            om, os_ = o[..., c].mean(), o[..., c].std()
            up[..., c] = (up[..., c] - um) * (os_ / us if us > 1e-3 else 1.0) + om
        C.save_output(Image.fromarray(np.clip(up + 0.5, 0, 255).astype(np.uint8), "RGB"), os.path.join(out_dir, name))

    done = S.upscale_batches(work, style, jobs, prepare, finish)
    print(f"wrote {done} HD detail texture(s) to {out_dir}")


if __name__ == "__main__":
    main()
