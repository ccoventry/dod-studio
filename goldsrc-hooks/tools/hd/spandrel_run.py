"""4x every PNG in a folder with a PyTorch upscaling model, through spandrel.

The second upscaler backend (styles.py's `spandrel` kind). Real-ESRGAN
ncnn-vulkan runs only ESRGAN-shaped networks; spandrel (the loader chaiNNer
uses) runs the newer ones too -- DAT, SPAN, RealPLKSR, HAT, ATD and the rest
-- straight from the .pth/.safetensors files OpenModelDB links to.

Runs under the venv setup_tools.py --spandrel makes (torch with CUDA, and
spandrel), which is why this is its own script: the build scripts never
import torch themselves, so ncnn styles work without it.

usage: python spandrel_run.py <model.pth|.safetensors> <in_dir> <out_dir> [--tile N]

Tiles the image when it is larger than --tile a side (default 512; halved
again on an out-of-memory error), overlapping tiles by 32 pixels and keeping
each tile's middle, so tile edges never show. Prints one line per file with
its time, and `total <files> <seconds>` at the end.
"""
import argparse, os, sys, time

import numpy as np
import torch
from PIL import Image

OVERLAP = 32


def load(path):
    from spandrel import ImageModelDescriptor, ModelLoader
    model = ModelLoader().load_from_file(path)
    if not isinstance(model, ImageModelDescriptor):
        sys.exit(f"{path}: not an image-to-image model")
    if model.scale != 4:
        sys.exit(f"{path}: a {model.scale}x model; the build expects 4x")
    device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    model = model.to(device).eval()
    half = device.type == "cuda" and model.supports_half
    if half:
        model = model.half()
    return model, device, half


def run_whole(model, device, half, rgb):
    """rgb: HxWx3 uint8 -> 4H x 4W x 3 uint8, padded to what the model needs."""
    req = model.size_requirements
    h, w = rgb.shape[:2]
    m = max(req.multiple_of, 1)
    ph = (-max(h, req.minimum)) % m + max(req.minimum - h, 0)
    pw = (-max(w, req.minimum)) % m + max(req.minimum - w, 0)
    if ph or pw:
        rgb = np.pad(rgb, ((0, ph), (0, pw), (0, 0)), mode="reflect")
    x = torch.from_numpy(np.ascontiguousarray(rgb).copy()).permute(2, 0, 1).unsqueeze(0).to(device)
    x = (x.half() if half else x.float()) / 255.0
    with torch.inference_mode():
        y = model(x)
    y = y.squeeze(0).permute(1, 2, 0).float().clamp(0, 1).mul(255).round().to(torch.uint8).cpu().numpy()
    return y[: h * model.scale, : w * model.scale]


def run_tiled(model, device, half, rgb, tile):
    h, w = rgb.shape[:2]
    if h <= tile and w <= tile:
        return run_whole(model, device, half, rgb)
    s = model.scale
    out = np.zeros((h * s, w * s, 3), np.uint8)
    for y0 in range(0, h, tile):
        for x0 in range(0, w, tile):
            y1, x1 = min(y0 + tile, h), min(x0 + tile, w)
            # The tile with its overlap, then only the tile's own area kept.
            ya, xa = max(y0 - OVERLAP, 0), max(x0 - OVERLAP, 0)
            yb, xb = min(y1 + OVERLAP, h), min(x1 + OVERLAP, w)
            up = run_whole(model, device, half, rgb[ya:yb, xa:xb])
            out[y0 * s:y1 * s, x0 * s:x1 * s] = up[(y0 - ya) * s:(y1 - ya) * s, (x0 - xa) * s:(x1 - xa) * s]
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("model")
    ap.add_argument("in_dir")
    ap.add_argument("out_dir")
    ap.add_argument("--tile", type=int, default=512)
    args = ap.parse_args()

    model, device, half = load(args.model)
    os.makedirs(args.out_dir, exist_ok=True)
    names = sorted(n for n in os.listdir(args.in_dir) if n.lower().endswith(".png"))
    print(f"{os.path.basename(args.model)}: {model.architecture.name} on {device.type}"
          f"{' fp16' if half else ''}, {len(names)} file(s)", flush=True)
    tile, start = args.tile, time.time()
    for n in names:
        rgb = np.asarray(Image.open(os.path.join(args.in_dir, n)).convert("RGB"))
        t = time.time()
        while True:
            try:
                out = run_tiled(model, device, half, rgb, tile)
                break
            except torch.cuda.OutOfMemoryError:
                torch.cuda.empty_cache()
                if tile <= 64:
                    raise
                tile //= 2
                print(f"  out of memory, tiling at {tile}", flush=True)
        Image.fromarray(out).save(os.path.join(args.out_dir, n))
        print(f"  {n} {rgb.shape[1]}x{rgb.shape[0]} {time.time() - t:.2f}s", flush=True)
    print(f"total {len(names)} {time.time() - start:.1f}s", flush=True)


if __name__ == "__main__":
    main()
