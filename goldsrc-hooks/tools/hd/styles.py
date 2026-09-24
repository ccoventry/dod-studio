"""The upscale styles, one per dodstudio_hd/<type>/<style> folder.

Every build script prepares padded source PNGs in an input folder and calls
`upscale(in_dir, out_dir, style)`, which writes a 4x PNG of each to out_dir
under the same name. The script then does its own crop/resize/alpha work.

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
import os, re, subprocess, sys, time
from PIL import Image, ImageFilter

from hdcommon import HERE

ESRGAN = os.environ.get("REALESRGAN") or os.path.join(HERE, "realesrgan", "realesrgan-ncnn-vulkan.exe")
MY_STYLES = os.environ.get("HD_MY_STYLES") or os.path.join(HERE, "my_styles.txt")

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


def load_my_styles(path=MY_STYLES):
    """Styles from my_styles.txt, one per line:

        name = <model file name>             an AI style (the model's .param and
                                             .bin in realesrgan/models)
        name = plain <sharpening 0-500>      no AI, more or less sharpened
        name = blend <style> <style> <0-100> two built styles mixed, the first
                                             one at that percent
    """
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


DEFS = {**BUILT_IN, **load_my_styles()}
STYLES = list(DEFS)


def style_from_env():
    s = (os.environ.get("HD_STYLE") or DEFAULT).lower()
    if s not in DEFS:
        sys.exit(f"unknown HD_STYLE {s!r}; one of {STYLES}")
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
    kind, *args = DEFS[style]
    if kind == "ai":
        _ai(in_dir, out_dir, args[0])
    elif kind == "plain":
        for n in names:
            plain_x4(Image.open(os.path.join(in_dir, n)), args[0]).save(os.path.join(out_dir, n))
    else:
        sys.exit(f"{style!r} is a blend; build_all.py makes it from {args[0]} and {args[1]}")
