"""HD map (world) textures: dodstudio_hd/world/<style>/<name>_<hash>.tga.

For every texture the given maps use (embedded in the BSP or pulled from a
wad), keyed by (name, FNV-1a-32 of mip-0 indices + 768-byte palette) exactly
as texture_hires.rs computes it at runtime:

  extract -> (masked `{` textures: fill the cut-outs with their nearest
  colour) -> wrap-pad half a tile each side -> 4x in the style -> Lanczos to
  2x the power-of-two target (capped at 1024/side) -> crop the centre tile ->
  (masked: alpha from the original mask, bilinear + threshold)

The wrap padding matters: without it the upscaler treats each edge as a
border, and every place the texture repeats on a wall shows a seam.
Identical textures shared between maps are built once.

usage: python world_hd.py <out_dir> <map> [<map> ...]     e.g. dod_anzio
       python world_hd.py <out_dir> --all                  every map in dod/maps
env:   HD_STYLE (default ultrasharp), HD_GAME, HD_WORK
"""
import glob, os, sys
import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt

import hdcommon as C
import styles as S
from goldsrc import map_textures


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out_dir, maps = sys.argv[1], sys.argv[2:]
    game = C.game_root()
    if maps == ["--all"]:
        maps = sorted(os.path.basename(f)[:-4] for f in glob.glob(os.path.join(game, "dod", "maps", "*.bsp")))
    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("world_" + style)

    jobs = {}
    for m in maps:
        for name, w, h, idx, pal in map_textures(game, m):
            base = name.lower()
            if base in C.SKIP or base.startswith("sky") or len(idx) != w * h or len(pal) != 768:
                continue
            key = f"{C.file_stem_name(name)}_{C.fnv1a32(idx, pal):08x}"
            jobs.setdefault(key, (name, w, h, idx, pal))
    todo = {k: v for k, v in jobs.items() if not os.path.exists(os.path.join(out_dir, k + ".tga"))}
    print(f"{len(jobs)} unique textures across {len(maps)} map(s), {len(todo)} still to build")

    masks = {}
    for key, (name, w, h, idx, pal) in todo.items():
        ind = np.frombuffer(idx, np.uint8).reshape(h, w)
        rgb = np.frombuffer(pal, np.uint8).reshape(256, 3)[ind].copy()
        if name.startswith("{"):
            mask = ind == 255
            if mask.all():
                continue  # a blank placeholder: nothing to upscale
            if mask.any():
                _, (iy, ix) = distance_transform_edt(mask, return_indices=True)
                rgb = rgb[iy, ix]
            masks[key] = ~mask
        rgb = np.pad(rgb, ((h // 2, h // 2), (w // 2, w // 2), (0, 0)), mode="wrap")
        Image.fromarray(rgb, "RGB").save(os.path.join(work, "in", key + ".png"))

    S.upscale(os.path.join(work, "in"), os.path.join(work, "out"), style)

    done = 0
    for key, (name, w, h, idx, pal) in todo.items():
        src = os.path.join(work, "out", key + ".png")
        if not os.path.exists(src):
            continue
        tw, th = C.pot(w * 4), C.pot(h * 4)
        # The upscaled image is 2x2 tiles (half a tile of padding each side):
        # resize all of it to twice the target, then keep the centre tile.
        centre = (tw // 2, th // 2, tw // 2 + tw, th // 2 + th)
        img = Image.open(src).convert("RGB").resize((tw * 2, th * 2), Image.LANCZOS).crop(centre)
        if key in masks:
            m = np.pad(masks[key], ((h // 2, h // 2), (w // 2, w // 2)), mode="wrap")
            alpha = Image.fromarray(m.astype(np.uint8) * 255, "L").resize((tw * 2, th * 2), Image.BILINEAR).crop(centre)
            img = img.convert("RGBA")
            img.putalpha(alpha.point(lambda v: 255 if v >= 128 else 0))
        img.save(os.path.join(out_dir, key + ".tga"))
        done += 1
    print(f"wrote {done} replacement(s) to {out_dir}")


if __name__ == "__main__":
    main()
