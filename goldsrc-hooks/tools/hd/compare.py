"""A side-by-side sheet of the original and every built style, to pick a
style (or spot a file one of them got wrong).

Each row is one sample, each column the original then one style. World,
model, detail and sky samples show the same 256x256 patch from the middle
of the file at 1:1 pixels, so differences aren't averaged away; sprites are
shown whole, drawn the way the game blends them.

usage: python compare.py <out.png> [<sample> ...] [--map <map>]... [--auto]
                         [--styles <style>,<style>...]
  sample:  world:<map>:<texture>        world:dod_anzio:bido_wall1
           model:<path under dod/models>[:<skin>]   model:v_garand.mdl
           sprite:<path under dod/sprites>[:<frame>]  sprite:muzzleflash1.spr
           detail:<file in gfx/detail>  detail:dt_grass1.tga
           sky:<face file in gfx/env>   sky:kraftstoffft.tga
  With none, a default set; samples whose files aren't there are skipped.
  --map     samples from this map of yours: its most detailed textures that
            have an HD file, and its sky (repeatable)
  --auto    the same from a few of your maps, picked for you (from
            hd_maps.txt's maps when you have one), plus the default models,
            sprites and detail textures
  --styles  only these styles' columns (default: every style)
env:    HD_GAME
"""
import argparse, os, sys
import numpy as np
from PIL import Image, ImageDraw

import hdcommon as C
import styles as S
from goldsrc import SPR_ADDITIVE, SPR_ALPHTEST, SPR_INDEXALPHA, SPR_NORMAL, Sprite, map_textures, mdl_textures, skyname
from struct import error as struct_error

CELL = 256
DEFAULTS = [
    "world:dod_anzio:bido_wall1", "world:dod_lennon2:plaster", "world:dod_armory_b6:hype_mfloor2b",
    "model:v_garand.mdl", "model:player/axis-inf/axis-inf.mdl",
    "sprite:muzzleflash1.spr", "sprite:explosion1.spr:5", "sprite:smoke.spr:2",
    "detail:dt_grass1.tga", "detail:dt_stone2.tga", "sky:kraftstoffft.tga",
]


def indexed(w, h, idx, pal):
    return Image.fromarray(np.frombuffer(pal, np.uint8).reshape(256, 3)[np.frombuffer(idx, np.uint8).reshape(h, w)])


def sample(spec):
    """(kind, label, HD file name, original image, sprite format or None)."""
    game, dod = C.game_root(), C.dod_dir()
    kind, _, rest = spec.partition(":")
    if kind == "world":
        m, _, tex = rest.partition(":")
        for n, w, h, i, p in map_textures(game, m, warn=lambda *_: None):
            if n.lower() == tex.lower():
                return "world", spec, f"{C.file_stem_name(n)}_{C.fnv1a32(i, p):08x}.tga", indexed(w, h, i, p), None
    elif kind == "model":
        path, _, skin = rest.partition(":")
        for n, fl, w, h, i, p in mdl_textures(os.path.join(dod, "models", path)):
            if not skin or n.lower() == skin.lower():
                return "models", spec, f"{C.file_stem_name(n)}_{C.fnv1a32(i, p):08x}.tga", indexed(w, h, i, p), None
    elif kind == "sprite":
        path, _, frame = rest.partition(":")
        spr = Sprite(os.path.join(dod, "sprites", path))
        base = C.file_stem_name(os.path.splitext(os.path.basename(path))[0])
        for n, w, h, i in spr.frames:
            if n == int(frame or 0):
                orig = indexed(w, h, i, spr.palette).convert("RGBA")
                ind = np.frombuffer(i, np.uint8).reshape(h, w)
                if spr.tex_format == SPR_INDEXALPHA:
                    orig = Image.new("RGBA", (w, h), tuple(spr.palette[765:768]) + (255,))
                    orig.putalpha(Image.fromarray(ind))
                elif spr.tex_format == SPR_ALPHTEST:
                    orig.putalpha(Image.fromarray(np.where(ind == 255, 0, 255).astype(np.uint8)))
                return "sprites", spec, f"{base}_{n}_{C.fnv1a32(i, spr.palette):08x}.tga", orig, spr.tex_format
    elif kind in ("detail", "sky"):
        folder = os.path.join(dod, "gfx", "detail" if kind == "detail" else "env")
        return kind, spec, rest, Image.open(os.path.join(folder, rest)).convert("RGB"), None
    return None


def built_in_any(kind, key, styles):
    hd = C.hd_dir()
    return any(os.path.exists(os.path.join(hd, kind, s, key)) for s in styles)


def detail_score(w, h, idx, pal):
    """How much there is to see in a texture: its brightness spread, with
    small ones marked down (a 16x16 patch shows little at 1:1)."""
    rgb = np.frombuffer(pal, np.uint8).reshape(256, 3)[np.frombuffer(idx, np.uint8)]
    return float(rgb.std()) * min(1.0, (w * h) / (128 * 128))


def map_samples(game, m, styles, count):
    """Up to `count` world samples from map `m` (its most detailed textures
    with an HD file in one of `styles`), then its sky if one is built."""
    scored = {}
    for n, w, h, i, p in map_textures(game, m, warn=lambda *_: None):
        # Masked ({) textures are mostly the chroma-key colour; tool textures
        # are never built.
        if n.lower() in C.SKIP or n.startswith("{") or min(w, h) < 16:
            continue
        key = f"{C.file_stem_name(n)}_{C.fnv1a32(i, p):08x}.tga"
        if built_in_any("world", key, styles):
            scored[n.lower()] = max(scored.get(n.lower(), (0, n))[0], detail_score(w, h, i, p)), n
    picks = [f"world:{m}:{n}" for _, n in sorted(scored.values(), reverse=True)[:count]]
    if picks:
        face = skyname(game, m) + "ft.tga"
        if built_in_any("sky", face, styles):
            picks.append(f"sky:{face}")
    return picks


def pick_maps(game, styles, want=3, tries=12):
    """A few maps with HD map textures built, spread across the maps
    hd_maps.txt allows (all of them without it)."""
    names = C.all_maps(game)
    if len(names) > tries:
        step = len(names) / tries
        names = [names[int(k * step)] for k in range(tries)]
    # Try every `want`-th one first, so the maps chosen are spread out
    # rather than the first few alphabetically.
    names = [m for start in range(want) for m in names[start::want]]
    chosen = []
    for m in names:
        try:
            samples = map_samples(game, m, styles, 1)
        except (OSError, ValueError, struct_error):
            continue
        if samples:
            chosen.append(m)
        if len(chosen) == want:
            break
    return chosen


def auto_specs(maps, styles, auto):
    """Samples from `maps` (or, with `auto` and no maps, a few picked), plus
    with `auto` the defaults that aren't world textures or skies."""
    game = C.game_root()
    if auto and not maps:
        maps = pick_maps(game, styles)
        print(f"maps: {', '.join(maps) or '(none with HD map textures built)'}", flush=True)
    specs = []
    per_map = 1 if len(maps) > 1 else 3
    for m in maps:
        try:
            got = map_samples(game, m, styles, per_map)
        except (OSError, ValueError, struct_error):
            got = []
        if not got:
            print(f"skipped {m}: no HD map textures built for it", flush=True)
        specs += [s for s in got if s not in specs]
    if auto:
        specs += [s for s in DEFAULTS if not s.startswith(("world:", "sky:"))]
    return specs


def as_drawn(img, fmt):
    """A sprite as the game blends it: additive on black, the rest over grey."""
    if fmt is None:
        return img.convert("RGB")
    if fmt in (SPR_ADDITIVE, SPR_NORMAL):
        base = np.zeros((img.height, img.width, 3), np.int32)
        if fmt == SPR_NORMAL:
            return img.convert("RGB")
        return Image.fromarray(np.clip(base + np.asarray(img.convert("RGB"), np.int32), 0, 255).astype(np.uint8))
    out = Image.new("RGB", img.size, (90, 90, 90))
    out.paste(img.convert("RGB"), mask=img.getchannel("A"))
    return out


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    ap = argparse.ArgumentParser(usage=__doc__)
    ap.add_argument("out")
    ap.add_argument("samples", nargs="*")
    ap.add_argument("--map", action="append", default=[])
    ap.add_argument("--auto", action="store_true")
    ap.add_argument("--styles")
    args = ap.parse_intermixed_args()
    styles = S.STYLES
    if args.styles:
        wanted = [w.strip().lower() for w in args.styles.split(",") if w.strip()]
        for w in wanted:
            if w not in S.DEFS:
                sys.exit(f"unknown style {w!r}; one of {S.STYLES}")
        styles = wanted
    out, specs = args.out, list(args.samples)
    if args.map or args.auto:
        specs = auto_specs(args.map, styles, args.auto) + specs
    specs = specs or DEFAULTS
    hd = C.hd_dir()
    rows = []
    for spec in specs:
        try:
            s = sample(spec)
        except (OSError, ValueError):
            s = None
        if s:
            rows.append(s)
        else:
            print(f"skipped {spec}: not found")
    cols = ["original"] + styles
    sheet = Image.new("RGB", (CELL * len(cols), 18 + (CELL + 18) * len(rows)), "black")
    d = ImageDraw.Draw(sheet)
    for c, name in enumerate(cols):
        d.text((c * CELL + 4, 3), name, fill="white")
    for r, (kind, label, key, orig, fmt) in enumerate(rows):
        y = 18 + r * (CELL + 18)
        d.text((4, y + 3), label, fill="yellow")
        ref = next((Image.open(p).size for p in (os.path.join(hd, kind, s, key) for s in styles) if os.path.exists(p)),
                   None)
        if ref is None:
            d.text((4, y + 40), "(no style built for this sample yet)", fill="gray")
            continue
        for c, style in enumerate(cols):
            if style == "original":
                im = orig.resize(ref, Image.NEAREST)
            else:
                path = os.path.join(hd, kind, style, key)
                if not os.path.exists(path):
                    d.text((c * CELL + 4, y + 40), "(not built)", fill="gray")
                    continue
                im = Image.open(path).convert("RGBA")
                if im.size != ref:
                    im = im.resize(ref, Image.LANCZOS)
            im = as_drawn(im, fmt)
            if kind == "sprites":
                im.thumbnail((CELL, CELL), Image.LANCZOS)
                sheet.paste(im, (c * CELL, y + 18))
            else:
                x0, y0 = max(0, ref[0] // 2 - CELL // 2), max(0, ref[1] // 2 - CELL // 2)
                sheet.paste(im.crop((x0, y0, x0 + CELL, y0 + CELL)), (c * CELL, y + 18))
    sheet.save(out)
    print(f"wrote {out}: {len(rows)} sample(s)")


if __name__ == "__main__":
    main()
