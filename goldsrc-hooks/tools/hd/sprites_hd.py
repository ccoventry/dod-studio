"""HD world sprites: dodstudio_hd/sprites/<style>/<sprite>_<frame>_<hash>.tga.

For every frame of every world sprite (muzzle flashes, explosions, smoke...),
keyed by (<sprite>_<frame>, FNV-1a-32 of the frame's pixels + the sprite's
palette) exactly as texture_hires.rs computes it:

  normal / additive  palette -> RGB, reflect-pad, 4x in the style
  alphtest           cut-outs filled first; alpha from the original mask
  indexalpha         the palette index *is* the alpha: upscaled as a greyscale
                     image; RGB is palette[255], all the engine ever draws

then Lanczos to the power-of-two target (4x, capped at 1024/side). Fully
transparent frames (blanked-out sprites) are skipped: nothing to upscale.

HUD sprites (crosshairs, weapon icons, scopes...) are skipped too: the game
draws them pixel for pixel, so texture_hires never replaces them.

usage: python sprites_hd.py <out_dir> [<source> ...]
  source: a .spr file, a folder of them, or a .txt list of .spr paths.
  With none, every world sprite of the game (dod/ shadowing valve/), plus
  those of any HD_ALSO install.
env:    HD_STYLE (default ultrasharp), HD_GAME, HD_ALSO, HD_WORK, HD_BATCH
"""
import glob, os, re, sys
import numpy as np
from PIL import Image
from scipy.ndimage import distance_transform_edt

import hdcommon as C
import styles as S
from goldsrc import SPR_ALPHTEST, SPR_INDEXALPHA, Sprite

# Drawn by the HUD, never as a world sprite. Names from every sprites/*.txt
# HUD layout are added to these at run time.
HUD = re.compile(r"^(320|640|1clip_|1hud_|1rockets|clip_|crosshair|customogxhair|customxhair|dodcross"
                 r"|hud_|scope_|weapons\d|hint_|hltv_icons|number_)")


def hud_layout_names(game):
    names = set()
    for g in ("dod", "valve"):
        for txt in glob.glob(os.path.join(game, g, "sprites", "*.txt")):
            for line in open(txt, encoding="latin1"):
                cols = line.split()
                if len(cols) >= 3 and not line.lstrip().startswith("//"):
                    names.add(cols[2].lower())
    return names


def is_hud(path, hud):
    base = os.path.splitext(os.path.basename(path))[0].lower()
    return base in hud or bool(HUD.match(base))


def world_sprites(installs):
    """Every non-HUD sprite of each install, dod/ shadowing valve/ at the
    same relative path, as the engine's search path does."""
    hud = hud_layout_names(installs[0])
    found = {}
    for hl in installs:
        for g in ("dod", "valve"):
            root = os.path.join(hl, g, "sprites")
            for f in glob.glob(os.path.join(root, "**", "*.spr"), recursive=True):
                rel = os.path.relpath(f, root).lower().replace("\\", "/")
                if not is_hud(f, hud):
                    found.setdefault((hl, rel), f)
    return sorted(found.values())


def sprite_files(arg):
    if arg.lower().endswith(".txt"):
        return [l.strip() for l in open(arg, encoding="utf-8") if l.strip() and not l.startswith("#")]
    if os.path.isdir(arg):
        return glob.glob(os.path.join(arg, "**", "*.spr"), recursive=True)
    return [arg]


def pad_amount(n):
    return n // 2


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    out_dir, sources = sys.argv[1], sys.argv[2:]
    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("sprites_" + style)

    files = [f for s in sources for f in sprite_files(s)] or world_sprites([C.game_root()] + C.extra_installs())
    jobs = {}
    for f in files:
        try:
            spr = Sprite(f)
        except (OSError, ValueError) as e:
            print(f"skipped {f}: {e}")
            continue
        if spr.numcolors != 256 or spr.tex_format not in (0, 1, 2, 3):
            print(f"skipped {f}: {spr.numcolors} colours, format {spr.tex_format}")
            continue
        base = C.file_stem_name(os.path.splitext(os.path.basename(f))[0])
        for n, w, h, idx in spr.frames:
            if spr.tex_format == SPR_ALPHTEST and idx.count(255) == len(idx):
                continue  # a blanked-out frame: nothing to upscale
            key = f"{base}_{n}_{C.fnv1a32(idx, spr.palette):08x}"
            if not os.path.exists(os.path.join(out_dir, key + ".tga")):
                jobs.setdefault(key, (spr.tex_format, w, h, idx, spr.palette))
    print(f"{len(jobs)} sprite frames to build")

    masks = {}

    def prepare(key, job):
        fmt, w, h, idx, pal = job
        ind = np.frombuffer(idx, np.uint8).reshape(h, w)
        if fmt == SPR_INDEXALPHA:
            rgb = np.repeat(ind[:, :, None], 3, axis=2)
        else:
            rgb = np.frombuffer(pal, np.uint8).reshape(256, 3)[ind].copy()
        if fmt == SPR_ALPHTEST:
            mask = ind == 255
            if mask.all():
                return None
            if mask.any():
                _, (iy, ix) = distance_transform_edt(mask, return_indices=True)
                rgb = rgb[iy, ix]
            masks[key] = ~mask
        py, px = pad_amount(h), pad_amount(w)
        mode = "reflect" if min(h, w) > 1 else "edge"
        pad = np.pad(rgb, ((py, py), (px, px), (0, 0)), mode=mode)
        return Image.fromarray(pad)

    def finish(key, job, src):
        fmt, w, h, idx, pal = job
        tw, th = C.pot(w * 4), C.pot(h * 4)
        py, px = pad_amount(h), pad_amount(w)
        # The padded frame scaled so the original part is exactly tw x th.
        sw, sh = round(tw * (w + 2 * px) / w), round(th * (h + 2 * py) / h)
        ox, oy = round(tw * px / w), round(th * py / h)
        img = Image.open(src).convert("RGB").resize((sw, sh), Image.LANCZOS).crop((ox, oy, ox + tw, oy + th))
        if fmt == SPR_INDEXALPHA:
            alpha = img.convert("L")
            img = Image.new("RGBA", (tw, th), tuple(pal[765:768]) + (255,))
            img.putalpha(alpha)
        elif key in masks:
            alpha = Image.fromarray(masks.pop(key).astype(np.uint8) * 255, "L").resize((tw, th), Image.BILINEAR)
            img = img.convert("RGBA")
            img.putalpha(alpha.point(lambda v: 255 if v >= 128 else 0))
        C.save_output(img, os.path.join(out_dir, key + ".tga"))

    done = S.upscale_batches(work, style, jobs, prepare, finish)
    print(f"wrote {done} sprite frame replacement(s) to {out_dir}")


if __name__ == "__main__":
    main()
