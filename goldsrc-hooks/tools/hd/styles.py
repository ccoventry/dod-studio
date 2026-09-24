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

The AI styles run Real-ESRGAN ncnn-vulkan (see README.md for where to get it
and the extra models): REALESRGAN points at the .exe, default
realesrgan/realesrgan-ncnn-vulkan.exe next to this file.
"""
import os, subprocess, sys, time
from PIL import Image, ImageFilter

from hdcommon import HERE

ESRGAN = os.environ.get("REALESRGAN") or os.path.join(HERE, "realesrgan", "realesrgan-ncnn-vulkan.exe")

AI_MODELS = {
    "ultrasharp": "ultrasharp-4x",
    "remacri": "remacri-4x",
    "siax": "4x_NMKD-Siax_200k",
    "generalv3": "RealESRGAN_General_x4_v3",
    "x4plus": "realesrgan-x4plus",
}
STYLES = ["ultrasharp", "remacri", "siax", "generalv3", "x4plus", "plain", "blend"]
DEFAULT = "ultrasharp"


def style_from_env():
    s = (os.environ.get("HD_STYLE") or DEFAULT).lower()
    if s not in STYLES:
        sys.exit(f"unknown HD_STYLE {s!r}; one of {STYLES}")
    return s


def plain_x4(img):
    img = img.convert("RGB")
    return img.resize((img.width * 4, img.height * 4), Image.LANCZOS).filter(
        ImageFilter.UnsharpMask(radius=2, percent=60, threshold=2))


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
    if style in AI_MODELS:
        _ai(in_dir, out_dir, AI_MODELS[style])
    elif style == "plain":
        for n in names:
            plain_x4(Image.open(os.path.join(in_dir, n))).save(os.path.join(out_dir, n))
    else:
        sys.exit(f"{style!r} isn't built directly; build_all.py makes it from x4plus and plain")
