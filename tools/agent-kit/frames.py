r"""The frame checks agents keep rewriting, on PNG/TGA/BMP/JPG frames or a video.

    frames.py edges <image>... [--threshold 16]
        Black rows at the top and bottom and black columns at the left and right
        (letterboxing, a view drawn into part of the frame).
    frames.py crop <image> --region killfeed|crosshair|chat|scoreboard|objectives [--box X,Y,W,H] [--zoom 2] [--out FILE]
        One named HUD region, scaled to the frame's size. Regions are measured at
        16:9 and approximate: check the crop, and pass --box (pixels, or 0-1
        fractions of the frame) for anything else.
    frames.py sheet <folder|video> [--count 12] [--cols 4] [--width 480] [--out FILE]
        A contact sheet of N frames picked evenly from a take folder or a video,
        each labelled with its frame number.
    frames.py diff <a> <b> [--threshold 8] [--out FILE]
        Mean difference, share of pixels that changed, the box around the change,
        and an amplified difference image.
    frames.py clock <in.mp4> <out_stem> [--fps 120]
        Burns a stopwatch (clip seconds, 2 decimals, counted from frames) on top
        centre and writes <out_stem>.mp4 at real time and <out_stem>_0.25x.mp4. The
        clock reads clip time in both, so "2.45 s" is the same frame in either.
        Draw it last, over the finished (stacked) video.

Needs Pillow; `sheet` on a video and `clock` need FFmpeg (--ffmpeg, DS_FFMPEG,
PATH, then Studio's ffmpeg_path setting). Whether a crosshair is drawn:
goldsrc-hooks/tools/crosshair_frames.py.
"""
import argparse
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

IMAGE_TYPES = {".png", ".tga", ".bmp", ".jpg", ".jpeg"}
# HUD regions as fractions of a 16:9 frame (x, y, width, height). DoD draws the
# kill feed top right, chat above the bottom-left HUD, objective icons top left.
REGIONS = {
    "killfeed": (0.55, 0.0, 0.45, 0.22),
    "chat": (0.0, 0.55, 0.6, 0.3),
    "objectives": (0.0, 0.0, 0.3, 0.1),
    "scoreboard": (0.12, 0.08, 0.76, 0.84),
}
# The crosshair box is a square at the centre, sized by the frame's height.
CROSSHAIR_SIDE = 0.12
CLOCK_FONT = "C\\:/Windows/Fonts/arialbd.ttf"


# ── Pure parts ───────────────────────────────────────────────────────────────

def dark_run(values, threshold):
    """How many leading values are at or below `threshold` (rows or columns that are black)."""
    count = 0
    for value in values:
        if value > threshold:
            break
        count += 1
    return count


def edges_from_maxima(row_max, col_max, threshold):
    """Black rows/columns at each edge from each row's and column's brightest value."""
    return {
        "top": dark_run(row_max, threshold),
        "bottom": dark_run(list(reversed(row_max)), threshold),
        "left": dark_run(col_max, threshold),
        "right": dark_run(list(reversed(col_max)), threshold),
    }


def parse_box(text, width, height):
    """`X,Y,W,H` in pixels, or as 0-1 fractions of the frame when every value is at most 1."""
    values = [float(v) for v in text.split(",")]
    if len(values) != 4:
        raise ValueError(f"--box wants X,Y,W,H, got {text!r}")
    if all(v <= 1 for v in values):
        values = [values[0] * width, values[1] * height, values[2] * width, values[3] * height]
    x, y, w, h = (round(v) for v in values)
    return x, y, x + w, y + h


def region_box(name, width, height):
    """(left, top, right, bottom) in pixels of a named region in a width x height frame."""
    if name == "crosshair":
        side = 2 * round(height * CROSSHAIR_SIDE / 2)  # even, so the box centres exactly
        left, top = (width - side) // 2, (height - side) // 2
        return left, top, left + side, top + side
    fx, fy, fw, fh = REGIONS[name]
    return round(fx * width), round(fy * height), round((fx + fw) * width), round((fy + fh) * height)


def pick_evenly(total, count):
    """`count` indices spread evenly over `total` items, first and last included."""
    if total <= 0:
        return []
    if count >= total:
        return list(range(total))
    if count == 1:
        return [0]
    return sorted({round(i * (total - 1) / (count - 1)) for i in range(count)})


def grid(count, cols):
    """(columns, rows) for a sheet of `count` tiles at most `cols` wide."""
    cols = max(1, min(cols, count))
    return cols, math.ceil(count / cols)


def changed_share(histogram, threshold):
    """Share of pixels whose difference is above `threshold`, from a 256-bin histogram."""
    total = sum(histogram)
    return sum(histogram[threshold + 1:]) / total if total else 0.0


def clock_filter(fps, font=CLOCK_FONT):
    """ffmpeg drawtext for a stopwatch counted from frame numbers at `fps`."""
    return (f"drawtext=fontfile='{font}':text='%{{eif\\:n/{fps}\\:d}}.%{{eif\\:mod(n\\,{fps})*100/{fps}\\:d\\:2}} s':"
            "x=(w-tw)/2:y=20:fontsize=48:fontcolor=yellow:box=1:boxcolor=black@0.7:boxborderw=10")


def clock_jobs(src, stem, fps):
    """The two ffmpeg command tails (after the binary) for the real-time and the 0.25x copy."""
    jobs = []
    for slow in (False, True):
        out = f"{stem}_0.25x.mp4" if slow else f"{stem}.mp4"
        vf = clock_filter(fps) + (",setpts=4*PTS" if slow else "")
        jobs.append(["-y", "-loglevel", "error", "-i", str(src), "-vf", vf, "-r", "30" if slow else str(fps),
                     "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p",
                     "-movflags", "+faststart", out])
    return jobs


# ── Images ───────────────────────────────────────────────────────────────────

def open_rgb(path):
    from PIL import Image
    return Image.open(path).convert("RGB")


def edges(path, threshold):
    image = open_rgb(path).convert("L")
    w, h = image.size
    row_max = [image.crop((0, y, w, y + 1)).getextrema()[1] for y in range(h)]
    col_max = [image.crop((x, 0, x + 1, h)).getextrema()[1] for x in range(w)]
    return w, h, edges_from_maxima(row_max, col_max, threshold)


def frames_in(folder):
    return sorted(p for p in Path(folder).iterdir() if p.suffix.lower() in IMAGE_TYPES)


def ffmpeg_path(arg=None):
    """FFmpeg from --ffmpeg, DS_FFMPEG, PATH, then Studio's setting."""
    if arg or os.environ.get("DS_FFMPEG"):
        return arg or os.environ["DS_FFMPEG"]
    found = shutil.which("ffmpeg")
    if found:
        return found
    try:
        settings = json.loads((kit_paths.studio_appdata() / "settings.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        settings = {}
    if settings.get("ffmpeg_path"):
        return settings["ffmpeg_path"]
    raise SystemExit("FFmpeg not found: pass --ffmpeg or set DS_FFMPEG")


def video_frames(video, count, ffmpeg, workdir):
    """Extracts `count` frames spread over a video into `workdir`; returns their paths."""
    probe = subprocess.run([ffmpeg, "-i", str(video), "-map", "0:v:0", "-c", "copy", "-f", "null", "-"],
                           capture_output=True, text=True).stderr
    frames = [int(n) for n in re.findall(r"frame=\s*(\d+)", probe)]
    total = frames[-1] if frames else count
    picks = pick_evenly(total, count)
    select = "+".join(f"eq(n\\,{n})" for n in picks)
    subprocess.run([ffmpeg, "-loglevel", "error", "-i", str(video), "-vf", f"select='{select}'", "-vsync", "0",
                    str(Path(workdir) / "f%05d.png")], check=True)
    return list(zip(picks, frames_in(workdir)))


def make_sheet(tiles, cols, width, out):
    """Saves a contact sheet of (label, image path) tiles."""
    from PIL import Image, ImageDraw
    first = open_rgb(tiles[0][1])
    tile_h = round(first.height * width / first.width)
    ncols, nrows = grid(len(tiles), cols)
    sheet = Image.new("RGB", (ncols * width, nrows * tile_h), "black")
    draw = ImageDraw.Draw(sheet)
    for i, (label, path) in enumerate(tiles):
        x, y = (i % ncols) * width, (i // ncols) * tile_h
        sheet.paste(open_rgb(path).resize((width, tile_h)), (x, y))
        draw.rectangle((x, y, x + 8 * len(str(label)) + 8, y + 16), fill="black")
        draw.text((x + 4, y + 2), str(label), fill="yellow")
    sheet.save(out)


# ── Commands ─────────────────────────────────────────────────────────────────

def cmd_edges(args):
    for path in args.images:
        w, h, e = edges(path, args.threshold)
        print(f"{path}: {w}x{h} black top {e['top']} bottom {e['bottom']} left {e['left']} right {e['right']}")


def cmd_crop(args):
    image = open_rgb(args.image)
    box = parse_box(args.box, *image.size) if args.box else region_box(args.region, *image.size)
    crop = image.crop(box)
    if args.zoom != 1:
        from PIL import Image
        crop = crop.resize((crop.width * args.zoom, crop.height * args.zoom), Image.NEAREST)
    out = args.out or str(Path(args.image).with_name(f"{Path(args.image).stem}_{args.region or 'box'}.png"))
    crop.save(out)
    print(f"{out}: box {box}")


def cmd_sheet(args):
    source = Path(args.source)
    out = args.out or str(source.with_name(f"{source.stem}_sheet.png"))
    if source.is_dir():
        files = frames_in(source)
        tiles = [(files[i].stem, files[i]) for i in pick_evenly(len(files), args.count)]
        if not tiles:
            raise SystemExit(f"no frames in {source}")
        make_sheet(tiles, args.cols, args.width, out)
    else:
        with tempfile.TemporaryDirectory() as work:
            tiles = [(f"frame {n}", p) for n, p in video_frames(source, args.count, ffmpeg_path(args.ffmpeg), work)]
            make_sheet(tiles, args.cols, args.width, out)
    print(f"{out}: {len(tiles)} frames")


def cmd_diff(args):
    from PIL import ImageChops
    a, b = open_rgb(args.a), open_rgb(args.b)
    if a.size != b.size:
        raise SystemExit(f"sizes differ: {a.size} vs {b.size}")
    diff = ImageChops.difference(a, b).convert("L")
    hist = diff.histogram()
    mean = sum(i * n for i, n in enumerate(hist)) / sum(hist)
    mask = diff.point(lambda v: 255 if v > args.threshold else 0)
    print(f"mean difference {mean:.2f}/255, changed {changed_share(hist, args.threshold) * 100:.2f}% "
          f"of pixels (> {args.threshold}), box {mask.getbbox()}")
    if args.out:
        diff.point(lambda v: min(255, v * 8)).save(args.out)
        print(args.out)


def cmd_clock(args):
    ffmpeg = ffmpeg_path(args.ffmpeg)
    for job in clock_jobs(args.src, args.stem, args.fps):
        subprocess.run([ffmpeg, *job], check=True)
        print(job[-1])


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--ffmpeg")
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("edges")
    s.add_argument("images", nargs="+")
    s.add_argument("--threshold", type=int, default=16)
    s = sub.add_parser("crop")
    s.add_argument("image")
    s.add_argument("--region", choices=sorted([*REGIONS, "crosshair"]))
    s.add_argument("--box")
    s.add_argument("--zoom", type=int, default=1)
    s.add_argument("--out")
    s = sub.add_parser("sheet")
    s.add_argument("source")
    s.add_argument("--count", type=int, default=12)
    s.add_argument("--cols", type=int, default=4)
    s.add_argument("--width", type=int, default=480)
    s.add_argument("--out")
    s = sub.add_parser("diff")
    s.add_argument("a")
    s.add_argument("b")
    s.add_argument("--threshold", type=int, default=8)
    s.add_argument("--out")
    s = sub.add_parser("clock")
    s.add_argument("src")
    s.add_argument("stem")
    s.add_argument("--fps", type=int, default=120)
    args = p.parse_args(argv)
    if args.cmd == "crop" and not (args.region or args.box):
        p.error("crop needs --region or --box")
    {"edges": cmd_edges, "crop": cmd_crop, "sheet": cmd_sheet, "diff": cmd_diff, "clock": cmd_clock}[args.cmd](args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
