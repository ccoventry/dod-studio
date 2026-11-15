"""The upscale styles, one per dodstudio_hd/<type>/<style> folder.

Every build script hands its jobs to `upscale_batches`, which saves each
one's padded source PNG and calls `upscale(in_dir, out_dir, style)` (a 4x
PNG of each, under the same name) a batch at a time. The script then does
its own crop/resize/alpha work on each.

  ultrasharp  4x-UltraSharp          -- default: sharp, keeps grain and grime
  remacri     Remacri                -- similar, slightly softer
  siax        4x_NMKD-Siax_200k      -- keeps detail, crunchy on grass/stone
  generalv3   RealESRGAN General v3  -- gentle, close to the original
  x4plus      realesrgan-x4plus      -- smooth, "cartoony"
  plain       Lanczos x4 + mild unsharp -- no AI, nothing invented
  blend       x4plus and plain mixed 50/50 (build_all.py makes it from those
              two; it is never upscaled on its own)

More can be added without touching this file: see my_styles.example.txt.

The AI styles run Real-ESRGAN ncnn-vulkan (see README.md for where to get it
and the extra models): REALESRGAN points at the .exe, default
realesrgan/realesrgan-ncnn-vulkan.exe next to this file.
"""
import os, re, shutil, subprocess, sys, time
from PIL import Image, ImageFilter

import hdcommon as C
from hdcommon import HERE

ESRGAN = os.environ.get("REALESRGAN") or os.path.join(HERE, "realesrgan", "realesrgan-ncnn-vulkan.exe")

# Each style is ("ai", model file name), ("plain", sharpening percent), or
# ("blend", style A, style B, percent of A).
BUILT_IN = {
    "ultrasharp": ("ai", "ultrasharp-4x"),
    "remacri": ("ai", "remacri-4x"),
    "siax": ("ai", "4x_NMKD-Siax_200k"),
    "generalv3": ("ai", "RealESRGAN_General_x4_v3"),
    "x4plus": ("ai", "realesrgan-x4plus"),
    "plain": ("plain", 60),
    "blend": ("blend", "x4plus", "plain", 50),
}
DEFAULT = "ultrasharp"
# What the hook accepts as a style name (texture_hires.rs's `clean_style`).
NAME = re.compile(r"^[a-z0-9_-]{1,32}$")


def load_my_styles(path=None):
    """Styles from my_styles.txt (in the install's dodstudio_hd folder, see
    hdcommon.user_file), one per line:

        name = <model file name>             an AI style (the model's .param and
                                             .bin in realesrgan/models)
        name = plain <sharpening 0-500>      no AI, more or less sharpened
        name = blend <style> <style> <0-100> two built styles mixed, the first
                                             one at that percent
    """
    path = path or C.user_file(C.MY_STYLES, "HD_MY_STYLES")
    styles = {}
    if not os.path.exists(path):
        return styles
    for n, raw in enumerate(open(path, encoding="utf-8"), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        where = f"{os.path.basename(path)} line {n}"
        name, eq, value = (p.strip() for p in line.partition("="))
        name = name.lower()
        if not eq or not value:
            sys.exit(f"{where}: expected `name = ...`, got {line!r}")
        if not NAME.match(name):
            sys.exit(f"{where}: {name!r} -- style names are lowercase letters, digits, - and _ only")
        if name in BUILT_IN:
            sys.exit(f"{where}: {name!r} is a built-in style; pick another name")
        words = value.split()
        try:
            if words[0].lower() == "plain":
                pct = int(words[1]) if len(words) > 1 else 60
                if not 0 <= pct <= 500:
                    raise ValueError
                styles[name] = ("plain", pct)
            elif words[0].lower() == "blend":
                a, b, pct = words[1].lower(), words[2].lower(), int(words[3])
                if not 0 <= pct <= 100:
                    raise ValueError
                styles[name] = ("blend", a, b, pct)
            elif len(words) == 1:
                styles[name] = ("ai", words[0])
            else:
                raise ValueError
        except (IndexError, ValueError):
            sys.exit(f"{where}: can't read {value!r}; see my_styles.example.txt")
    for name, d in styles.items():
        if d[0] == "blend":
            for src in d[1:3]:
                if src not in BUILT_IN and src not in styles:
                    sys.exit(f"{os.path.basename(path)}: {name} blends {src!r}, which isn't a style")
    return styles


_defs = None


def defs():
    """Every style, built-in and my_styles.txt's. Read on first use rather
    than at import: my_styles.txt lives in the game folder, which
    build_all.py's --game only sets after importing this."""
    global _defs
    if _defs is None:
        _defs = {**BUILT_IN, **load_my_styles()}
    return _defs


def __getattr__(name):
    """`styles.DEFS` and `styles.STYLES` (the names), through defs()."""
    if name == "DEFS":
        return defs()
    if name == "STYLES":
        return list(defs())
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


def style_from_env():
    s = (os.environ.get("HD_STYLE") or DEFAULT).lower()
    if s not in defs():
        sys.exit(f"unknown HD_STYLE {s!r}; one of {list(defs())}")
    return s


def plain_x4(img, percent=60):
    img = img.convert("RGB")
    big = img.resize((img.width * 4, img.height * 4), Image.LANCZOS)
    return big.filter(ImageFilter.UnsharpMask(radius=2, percent=percent, threshold=2)) if percent else big


def _ai(in_dir, out_dir, model):
    if not os.path.exists(ESRGAN):
        sys.exit(f"Real-ESRGAN not found at {ESRGAN}; see README.md (or set REALESRGAN)")
    models = os.path.join(os.path.dirname(ESRGAN), "models")
    if not os.path.exists(os.path.join(models, model + ".param")):
        sys.exit(f"model {model!r} not in {models}; see README.md for where to download it")
    t = time.time()
    r = subprocess.run([ESRGAN, "-i", in_dir, "-o", out_dir, "-m", models, "-n", model, "-s", "4", "-f", "png"],
                       capture_output=True, text=True)
    print(f"{model}: {time.time() - t:.0f}s, exit {r.returncode}", flush=True)
    if r.returncode != 0:
        print(r.stderr[-2000:], flush=True)


def upscale(in_dir, out_dir, style):
    """4x every PNG in in_dir into out_dir (same file names) in `style`."""
    os.makedirs(out_dir, exist_ok=True)
    names = [n for n in os.listdir(in_dir) if n.lower().endswith(".png")]
    if not names:
        return
    kind, *args = defs()[style]
    if kind == "ai":
        _ai(in_dir, out_dir, args[0])
    elif kind == "plain":
        for n in names:
            plain_x4(Image.open(os.path.join(in_dir, n)), args[0]).save(os.path.join(out_dir, n))
    else:
        sys.exit(f"{style!r} is a blend; build_all.py makes it from {args[0]} and {args[1]}")


# Files per upscaler run: about a minute and a half of ultrasharp. Starting
# the upscaler again costs about a second, so smaller only means more
# progress lines and less lost to a stop. HD_BATCH overrides it (not set
# by DoD Studio: it's for tuning from the command line).
BATCH = 250


def batch_size():
    raw = os.environ.get("HD_BATCH")
    if not raw:
        return BATCH
    if not raw.isdigit() or int(raw) < 1:
        sys.exit(f"HD_BATCH={raw!r}: a whole number of files, 1 or more")
    return int(raw)


def upscale_batches(work, style, jobs, prepare, finish):
    """Upscales `jobs` ({key: job}) in `style`, batch_size() at a time, and
    returns how many `finish` wrote.

    prepare(key, job) returns the padded input image, or None to skip the
    job; finish(key, job, path) makes the output file from the 4x PNG at
    `path`. Each batch's files are written before the next batch starts:
    in one batch, every map's textures are half an hour of upscaling, and a
    build stopped anywhere in it wrote nothing, while the next run's fresh
    work folder threw the finished upscales away (#404). After each batch,
    `@@progress N of M written`, which build_all.py passes on to DoD Studio."""
    keys, size, done = list(jobs), batch_size(), 0
    in_dir, out_dir = os.path.join(work, "in"), os.path.join(work, "out")
    for start in range(0, len(keys), size):
        batch = keys[start:start + size]
        for d in (in_dir, out_dir):
            shutil.rmtree(d, ignore_errors=True)
            os.makedirs(d)
        for key in batch:
            img = prepare(key, jobs[key])
            if img is not None:
                img.save(os.path.join(in_dir, key + ".png"))
        upscale(in_dir, out_dir, style)
        for key in batch:
            path = os.path.join(out_dir, key + ".png")
            if os.path.exists(path):
                finish(key, jobs[key], path)
                done += 1
        print(f"@@progress {done} of {len(jobs)} written", flush=True)
    return done
