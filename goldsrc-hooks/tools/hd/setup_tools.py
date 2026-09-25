"""Downloads what the HD scripts need into a `realesrgan` folder here:
Real-ESRGAN ncnn-vulkan (the upscaler) and the extra style models from the
Upscayl project. Safe to run again: anything already there is skipped.

usage: python setup_tools.py
"""
import io, os, sys, urllib.request, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
DEST = os.path.join(HERE, "realesrgan")
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


if __name__ == "__main__":
    main()
