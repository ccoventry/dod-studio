"""Downloads what the HD scripts need into a `realesrgan` folder here:
Real-ESRGAN ncnn-vulkan (the upscaler) and the extra style models from the
Upscayl project. Safe to run again: anything already there is skipped.

With --spandrel, also sets up the second backend in a `spandrel` folder here
(HD_SPANDREL overrides): a Python venv with torch (CUDA 13 build, about
3 GB) and spandrel, and the three spandrel styles' model files (about
310 MB). Needs an NVIDIA card for any useful speed.

usage: python setup_tools.py [--spandrel]
"""
import io, os, subprocess, sys, urllib.request, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
DEST = os.path.join(HERE, "realesrgan")
SPANDREL = os.environ.get("HD_SPANDREL") or os.path.join(HERE, "spandrel")
# torch and torchvision come from PyTorch's own index (CUDA 13 builds), and
# only then spandrel from PyPI: PyPI's torch is the CPU build, and pip would
# swap it in if it were newer than the one installed.
TORCH_INDEX = "https://download.pytorch.org/whl/cu130"
# style: (model file name as styles.py's BUILT_IN names it, URL, licence)
SPANDREL_MODELS = {
    "ultrasharpv2": ("4x-UltraSharpV2.safetensors",
                     "https://huggingface.co/Kim2091/UltraSharpV2/resolve/main/4x-UltraSharpV2.safetensors",
                     "CC BY-NC-SA 4.0 (non-commercial)"),
    "pbrify": ("4x-PBRify_UpscalerV4.pth",
               "https://github.com/Kim2091/Kim2091-Models/releases/download/4x-PBRify_UpscalerV4/4x-PBRify_UpscalerV4.pth",
               "CC0 1.0"),
    "webphoto": ("4xNomosWebPhoto_RealPLKSR.pth",
                 "https://github.com/Phhofm/models/releases/download/4xNomosWebPhoto_RealPLKSR/4xNomosWebPhoto_RealPLKSR.pth",
                 "CC BY 4.0"),
}
ZIP = ("https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/"
       "realesrgan-ncnn-vulkan-20220424-windows.zip")
UPSCAYL = "https://raw.githubusercontent.com/upscayl/upscayl/main/resources/models/"
CUSTOM = "https://raw.githubusercontent.com/upscayl/custom-models/main/models/"
MODELS = {  # style: (base URL, model file name)
    "ultrasharp": (UPSCAYL, "ultrasharp-4x"),
    "remacri": (UPSCAYL, "remacri-4x"),
    "siax": (CUSTOM, "4x_NMKD-Siax_200k"),
    "generalv3": (CUSTOM, "RealESRGAN_General_x4_v3"),
}


def fetch(url):
    with urllib.request.urlopen(url) as r:
        return r.read()


def main():
    exe = os.path.join(DEST, "realesrgan-ncnn-vulkan.exe")
    if os.path.exists(exe):
        print("Real-ESRGAN: already there")
    else:
        print("Real-ESRGAN: downloading (45 MB)...", flush=True)
        with zipfile.ZipFile(io.BytesIO(fetch(ZIP))) as z:
            z.extractall(DEST)
        if not os.path.exists(exe):
            sys.exit(f"the zip didn't contain realesrgan-ncnn-vulkan.exe; unzip it into {DEST} by hand")
        print("Real-ESRGAN: done")

    models = os.path.join(DEST, "models")
    os.makedirs(models, exist_ok=True)
    for style, (base, name) in MODELS.items():
        for ext in (".param", ".bin"):
            path = os.path.join(models, name + ext)
            if os.path.exists(path):
                continue
            print(f"{style}: downloading {name}{ext}...", flush=True)
            data = fetch(base + name + ext)
            with open(path, "wb") as f:
                f.write(data)
    print("\nAll set. These models have their own licences (4x-UltraSharp, for one, is\n"
          "non-commercial); check them before sharing anything you make with them.")


def spandrel():
    """The second backend: a venv with torch + spandrel, and the models."""
    python = os.path.join(SPANDREL, "venv", "Scripts", "python.exe")
    if os.path.exists(python):
        print("spandrel venv: already there")
    else:
        print(f"spandrel venv: making {os.path.join(SPANDREL, 'venv')} and installing torch (about 3 GB)...", flush=True)
        subprocess.run([sys.executable, "-m", "venv", os.path.join(SPANDREL, "venv")], check=True)
        subprocess.run([python, "-m", "pip", "install", "--quiet", "--upgrade", "pip"], check=True)
        subprocess.run([python, "-m", "pip", "install", "--quiet", "torch", "torchvision", "--index-url", TORCH_INDEX], check=True)
        subprocess.run([python, "-m", "pip", "install", "--quiet", "spandrel", "pillow", "numpy"], check=True)
        r = subprocess.run([python, "-c", "import torch; print(torch.cuda.is_available())"], capture_output=True, text=True)
        print("spandrel venv: done; CUDA " + ("available" if r.stdout.strip() == "True" else "NOT available (it will run on the CPU, slowly)"))

    models = os.path.join(SPANDREL, "models")
    os.makedirs(models, exist_ok=True)
    for style, (name, url, licence) in SPANDREL_MODELS.items():
        path = os.path.join(models, name)
        if os.path.exists(path):
            continue
        print(f"{style}: downloading {name}...", flush=True)
        data = fetch(url)
        with open(path + ".part", "wb") as f:
            f.write(data)
        os.replace(path + ".part", path)
    print("\nspandrel styles ready: " + ", ".join(f"{s} ({m[2]})" for s, m in SPANDREL_MODELS.items())
          + ".\nEach model's licence is its author's, not this repo's; ultrasharpv2 is non-commercial.")


if __name__ == "__main__":
    main()
    if "--spandrel" in sys.argv[1:]:
        spandrel()
