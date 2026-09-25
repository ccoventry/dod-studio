"""HD map (world) textures: dodstudio_hd/world/<style>/<name>_<hash>.tga.

For every texture the given maps use (embedded in the BSP or pulled from a
wad), keyed by (name, FNV-1a-32 of mip-0 indices + 768-byte palette) exactly
as texture_hires.rs computes it at runtime:

  extract -> (masked `{` textures: fill the cut-outs with their nearest
  colour) -> wrap-pad half a tile (at most PAD px) each side -> 4x in the style -> Lanczos the
  tile's own part to the power-of-two target (capped at 1024/side), the
  padding feeding the filter at the edges -> (masked: alpha from the
  original mask, bilinear + threshold)

The wrap padding matters: without it the upscaler treats each edge as a
border, and every place the texture repeats on a wall shows a seam.
Identical textures shared between maps are built once.

usage: python world_hd.py <out_dir> <map> [<map> ...]     e.g. dod_anzio
       python world_hd.py <out_dir> --all                  every map in dod/maps, or
                                                           the ones hd_maps.txt lists
env:   HD_STYLE (default ultrasharp), HD_GAME, HD_WORK
"""
import os, sys
import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt

import hdcommon as C
import styles as S
from goldsrc import map_textures

# Wrap padding each side, in source pixels: half a tile, up to PAD. Half a
# tile makes the upscaler work on 4x each texture's area, so capping it pays
# off on the big textures (#383). Measured on dod_anzio + dod_harrington's
# 186 textures: textures up to 128 px are unchanged (half a tile is 64 px or
# less), and the build's upscaler step is 1.5x faster with the cap at 64.
# A seam -- the step across a tile's own edge against the steps inside it --
# stays within 4% of half-tile padding's for every texture with ultrasharp,
# and x4plus has no more tiles with a visible seam than before. A 32 px cap
# was 2.1x faster but gave x4plus a clear seam on a 128 px glass texture.
PAD = 64


def margins(w, h):
    """(rows, columns) of wrap padding for a w x h texture."""
    return min(PAD, h // 2), min(PAD, w // 2)


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out_dir, maps = sys.argv[1], sys.argv[2:]
    game = C.game_root()
    if maps == ["--all"]:
        maps = C.all_maps(game)
    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("world_" + style)

    jobs, blank = {}, set()
    for m in maps:
        for name, w, h, idx, pal in map_textures(game, m):
            base = name.lower()
            if base in C.SKIP or base.startswith("sky") or len(idx) != w * h or len(pal) != 768:
                continue
            key = f"{C.file_stem_name(name)}_{C.fnv1a32(idx, pal):08x}"
            # A masked texture that is all cut-out: nothing to upscale, and
            # the hook labels it "blank" rather than looking for a file.
            if name.startswith("{") and idx.count(255) == len(idx):
                blank.add(key)
                continue
            jobs.setdefault(key, (name, w, h, idx, pal))
    todo = {k: v for k, v in jobs.items() if not os.path.exists(os.path.join(out_dir, k + ".tga"))}
    print(f"{len(jobs)} unique textures across {len(maps)} map(s), {len(todo)} still to build"
          + (f" ({len(blank)} blank placeholder(s) skipped)" if blank else ""))

    masks = {}
    for key, (name, w, h, idx, pal) in todo.items():
        ind = np.frombuffer(idx, np.uint8).reshape(h, w)
        rgb = np.frombuffer(pal, np.uint8).reshape(256, 3)[ind].copy()
        if name.startswith("{"):
            mask = ind == 255
            if mask.any():
                _, (iy, ix) = distance_transform_edt(mask, return_indices=True)
                rgb = rgb[iy, ix]
            masks[key] = ~mask
        py, px = margins(w, h)
        rgb = np.pad(rgb, ((py, py), (px, px), (0, 0)), mode="wrap")
        Image.fromarray(rgb, "RGB").save(os.path.join(work, "in", key + ".png"))

    S.upscale(os.path.join(work, "in"), os.path.join(work, "out"), style)

    done = 0
    for key, (name, w, h, idx, pal) in todo.items():
        src = os.path.join(work, "out", key + ".png")
        if not os.path.exists(src):
            continue
        tw, th = C.pot(w * 4), C.pot(h * 4)
        # The upscaled image is the tile plus 4x the padding each side: resize
        # just the tile's part to the target. Pillow reads past a resize box
        # for the filter's support, so the edges are filtered from the
        # wrapped-around padding, as they would be across a wall.
        py, px = margins(w, h)
        box = (px * 4, py * 4, (px + w) * 4, (py + h) * 4)
        img = Image.open(src).convert("RGB").resize((tw, th), Image.LANCZOS, box=box)
        if key in masks:
            m = np.pad(masks[key], ((py, py), (px, px)), mode="wrap")
            alpha = Image.fromarray(m.astype(np.uint8) * 255, "L").resize(
                (tw, th), Image.BILINEAR, box=(px, py, px + w, py + h))
            img = img.convert("RGBA")
            img.putalpha(alpha.point(lambda v: 255 if v >= 128 else 0))
        C.save_output(img, os.path.join(out_dir, key + ".tga"))
        done += 1
    print(f"wrote {done} replacement(s) to {out_dir}")


if __name__ == "__main__":
    main()
