"""Build every HD asset type in the chosen styles into
<game>/dod/dodstudio_hd/<type>/<style>.

Resumable: each step skips files that already exist, so re-running after an
interruption (or after adding maps and models) only builds what's missing.
Progress goes to dodstudio_hd/build_all.log and the console.

  world    every map in dod/maps (embedded and wad textures)
  models   every .mdl under dod/models, the Half-Life models DoD falls back
           to (valve_models.txt), and the models folders of --also installs
           and --extra-models folders
  sprites  every world sprite of the game and of --also installs
  detail   every gfx/detail/*.tga
  sky      every sky in dod/gfx/env, plus the Half-Life ones maps name

Blend styles (`blend`, and any in my_styles.txt) aren't upscaled at all:
they're two built styles mixed file by file, so they're built after every
other style in the run, and need their two source styles built already.

usage: python build_all.py [--game DIR] [--also DIR]... [--extra-models DIR]...
                           [--types world,models,...] [style ...]
  --game          the Half-Life folder DoD Studio launches (else HD_GAME, else
                  the only Steam install with a dod/ folder)
  --also          another Half-Life install whose models and sprites differ
                  (e.g. a stock one next to a movie one): its versions get
                  HD copies too, ready if you ever copy them over
  --extra-models  any other folder of .mdl files to include
  --types         which asset types (default: all, quickest first)
  style ...       which styles (default: the 7 built-in ones and any in
                  my_styles.txt)
"""
import argparse, datetime, os, subprocess, sys, time
from PIL import Image

import hdcommon as C
import styles as S

STYLE_ORDER = ["ultrasharp", "plain", "x4plus", "blend", "generalv3", "remacri", "siax"]
TYPES = ["sky", "sprites", "models", "detail", "world"]  # quickest first


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--game")
    ap.add_argument("--also", action="append", default=[])
    ap.add_argument("--extra-models", action="append", default=[])
    ap.add_argument("--types", default=",".join(TYPES))
    ap.add_argument("styles", nargs="*")
    args = ap.parse_args()

    if args.game:
        os.environ["HD_GAME"] = args.game
    if args.also:
        os.environ["HD_ALSO"] = os.pathsep.join(args.also)
    game = C.game_root()
    hd = C.hd_dir()
    os.makedirs(hd, exist_ok=True)
    log_path = os.path.join(hd, "build_all.log")

    def log(msg):
        line = f"[{datetime.datetime.now():%H:%M:%S}] {msg}"
        print(line, flush=True)
        with open(log_path, "a", encoding="utf-8") as f:
            f.write(line + "\n")

    styles = args.styles or STYLE_ORDER + [s for s in S.STYLES if s not in STYLE_ORDER]
    for s in styles:
        if s not in S.DEFS:
            sys.exit(f"unknown style {s!r}; one of {S.STYLES}")
    # Blends last: they mix files the other styles make.
    styles = [s for s in styles if S.DEFS[s][0] != "blend"] + [s for s in styles if S.DEFS[s][0] == "blend"]
    types = [t for t in args.types.split(",") if t]
    for t in types:
        if t not in TYPES:
            sys.exit(f"unknown type {t!r}; one of {TYPES}")

    def step(style, kind):
        out = os.path.join(hd, kind, style)
        os.makedirs(out, exist_ok=True)
        env = dict(os.environ, HD_STYLE=style)
        if kind == "world":
            cmd = ["world_hd.py", out, "--all"]
        elif kind == "models":
            cmd = ["models_hd.py", out, os.path.join(game, "dod", "models"),
                   os.path.join(C.HERE, "valve_models.txt")]
            cmd += [os.path.join(a, "dod", "models") for a in args.also] + args.extra_models
        elif kind == "sprites":
            cmd = ["sprites_hd.py", out]
        elif kind == "detail":
            cmd = ["detail_hd.py", out]
        else:
            cmd = ["sky_hd.py", out, "--all"]
        before = len(os.listdir(out))
        t = time.time()
        r = subprocess.run([sys.executable, "-u"] + cmd, cwd=C.HERE, env=env, capture_output=True, text=True)
        tail = [l for l in r.stdout.splitlines() if "not found in any wad" not in l][-3:]
        log(f"{style:10s} {kind:7s} exit {r.returncode}, {len(os.listdir(out)) - before} new, "
            f"{len(os.listdir(out))} total, {time.time() - t:.0f}s :: {' | '.join(tail)}")
        if r.returncode != 0:
            log(f"  {(r.stderr or r.stdout)[-1500:]}")
            sys.exit(r.returncode)

    def blend(style, kind):
        """<style>/<file> = <a>/<file> and <b>/<file> mixed, pct% of a."""
        _, a, b, pct = S.DEFS[style]
        a_dir, b_dir = os.path.join(hd, kind, a), os.path.join(hd, kind, b)
        if not (os.path.isdir(a_dir) and os.path.isdir(b_dir)):
            log(f"{style:10s} {kind:7s} skipped: build {a} and {b} first")
            return
        out = os.path.join(hd, kind, style)
        os.makedirs(out, exist_ok=True)
        t, made = time.time(), 0
        for name in sorted(os.listdir(a_dir)):
            dst, src_b = os.path.join(out, name), os.path.join(b_dir, name)
            if os.path.exists(dst) or not os.path.exists(src_b):
                continue
            ia, ib = Image.open(os.path.join(a_dir, name)), Image.open(src_b)
            if ia.size != ib.size:
                ib = ib.resize(ia.size, Image.LANCZOS)
            weight = 1 - pct / 100  # Image.blend's weight is the second image's
            mixed = Image.blend(ia.convert("RGB"), ib.convert("RGB"), weight)
            if ia.mode == "RGBA":
                # Masked textures have the same alpha in both; index-alpha
                # sprites' alpha is the upscaled image itself, so it's mixed too.
                alpha = ia.getchannel("A")
                if ib.mode == "RGBA":
                    alpha = Image.blend(alpha, ib.getchannel("A"), weight)
                mixed = mixed.convert("RGBA")
                mixed.putalpha(alpha)
            mixed.save(dst)
            made += 1
        log(f"{style:10s} {kind:7s} {made} new, {len(os.listdir(out))} total, {time.time() - t:.0f}s")

    keep_awake(log)
    log(f"=== build_all: {game}; styles {styles}; types {types}")
    for style in styles:
        for kind in types:
            if S.DEFS[style][0] == "blend":
                blend(style, kind)
            else:
                step(style, kind)
    log("=== build_all done")


def keep_awake(log):
    """Ask Windows not to sleep while this runs (ends with the process;
    power settings are untouched)."""
    try:
        import ctypes
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED = 0x80000000, 0x00000001
        ctypes.windll.kernel32.SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)
    except Exception as e:  # not fatal: at worst the machine sleeps and resumes later
        log(f"keep-awake request failed: {e}")


if __name__ == "__main__":
    main()
