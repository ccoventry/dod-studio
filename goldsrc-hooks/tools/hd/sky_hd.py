"""HD skybox faces: dodstudio_hd/sky/<style>/<sky><face>.tga.

For each sky, reads the six gfx/env/<sky><face>.tga faces (dod/, else
valve/; never writes there) and:

  reflect-pad -> 4x in the style -> Lanczos to 4x the original (capped at
  1024/side)

Faces are upscaled one by one, so where two meet at a cube edge they can
differ slightly; the reflect padding keeps each edge close to its original,
which is what the neighbouring face was drawn to match.

usage: python sky_hd.py <out_dir> <map> [<map> ...]   the skies those maps use
       python sky_hd.py <out_dir> --all               every complete sky in
                                                       dod/gfx/env, plus the
                                                       Half-Life skies any map
                                                       in dod/maps names
env:   HD_STYLE (default ultrasharp), HD_GAME, HD_WORK
"""
import os, sys
import numpy as np
from PIL import Image

import hdcommon as C
import styles as S
from goldsrc import skyname

FACES = ("rt", "bk", "lf", "ft", "up", "dn")  # R_LoadSkys' order


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out_dir, maps = sys.argv[1], sys.argv[2:]
    game = C.game_root()
    env_dirs = [os.path.join(game, "dod", "gfx", "env"), os.path.join(game, "valve", "gfx", "env")]

    def face_path(name):
        return next((p for p in (os.path.join(d, name) for d in env_dirs) if os.path.exists(p)), None)

    if maps == ["--all"]:
        names = {f[:-6].lower() for f in os.listdir(env_dirs[0]) if f.lower().endswith(".tga")}
        names |= {skyname(game, b[:-4]) for b in os.listdir(os.path.join(game, "dod", "maps"))
                  if b.lower().endswith(".bsp")}
    else:
        names = {skyname(game, m) for m in maps}
    skies = sorted(n for n in names if all(face_path(n + f + ".tga") for f in FACES))

    style = S.style_from_env()
    os.makedirs(out_dir, exist_ok=True)
    work = C.work_dir("sky_" + style)
    jobs = {}
    for sky in skies:
        for face in FACES:
            name = sky + face + ".tga"
            if os.path.exists(os.path.join(out_dir, name)):
                continue
            a = np.asarray(Image.open(face_path(name)).convert("RGB"))
            h, w = a.shape[:2]
            jobs[name] = (w, h)
            pad = np.pad(a, ((h // 4, h // 4), (w // 4, w // 4), (0, 0)), mode="reflect")
            Image.fromarray(pad).save(os.path.join(work, "in", name[:-4] + ".png"))
    print(f"{len(jobs)} sky faces to build")

    S.upscale(os.path.join(work, "in"), os.path.join(work, "out"), style)

    done = 0
    for name, (w, h) in jobs.items():
        src = os.path.join(work, "out", name[:-4] + ".png")
        if not os.path.exists(src):
            continue
        tw, th = min(w * 4, C.CAP), min(h * 4, C.CAP)
        # The upscaled image is 1.5 faces wide (a quarter face of padding each
        # side): scale it to 1.5x the target, keep the middle.
        big = Image.open(src).convert("RGB").resize((tw * 3 // 2, th * 3 // 2), Image.LANCZOS)
        big.crop((tw // 4, th // 4, tw // 4 + tw, th // 4 + th)).save(os.path.join(out_dir, name))
        done += 1
    print(f"wrote {done} HD sky face(s) to {out_dir}")


if __name__ == "__main__":
    main()
