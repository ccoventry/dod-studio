#!/usr/bin/env python3
r"""Is the crosshair there? Reads frames recorded from the game and says, frame
by frame, whether a crosshair is drawn at the centre of the view (#310).

Three uses, all on a folder of PNG frames recorded at a fixed rate (HLAE's
`mirv_recordmovie`, or `game_probe.py`'s `clip` step):

    crosshair_frames.py runs <frames dir>
        The runs of frames with and without a crosshair.

    crosshair_frames.py hook <hook log> <frames dir>
        A spectated (HLTV) recording against the hook's own account. With
        `dodstudio_debug_log_weapon_model 1` the hook logs a line each time
        it decides the player's own view would hide or draw the crosshair;
        this counts the frames that agree. The log must hold one run only:
        cut it from the last "remote: listening" line.

    crosshair_frames.py pov <state.tsv> <player.mdl> <frames dir> <record start>
        A POV recording against the demo's own state, which is the reference
        for what "the player's own view" does. <state.tsv> comes from
        `cargo run -p analysis --example crosshair_pov_probe`, <player.mdl>
        is the player model that names the sequences (models/player/us-inf/
        us-inf.mdl), and <record start> is the demo clock on the hook log's
        "mirv_recordmovie_start" line. Prints each run with every change of
        state inside it.

Options: --fps N (default 30), --centre X,Y when the view is not centred in
the frame (a 640x360 game is recorded into the bottom of a 640x480 frame:
--centre 320,300), --offset S for `pov` to skip the fit.

## How it decides

Two crosshairs are known, and a frame has one if either test passes:

- the translucent yellow one (four arms two pixels wide): how much yellower
  the arm pixels are than the pixels just beside them. Near 0 without, near
  80 with; the threshold is 35.
- the white one with a dark outline: how much brighter the arm pixels are
  than their outline. The threshold is 100.

Both compare medians, so a wall edge through one arm does not decide it. A
crosshair of another shape or colour needs another test added here.

## The two clocks

The hook log's demo clock and a demo file's own frame times differ by a few
seconds (the time the demo spent loading), and by a different amount each
run. `pov` fits the offset on the one thing certain to hide the crosshair:
the sprint key held with a move key.
"""
import argparse
import bisect
import collections
import glob
import os
import re
import struct
import sys

from PIL import Image

ARMS = [(x, y) for x in (15, 16) for y in (9, 10, 11, 12, 18, 19, 20, 21)] + \
       [(x, y) for y in (15, 16) for x in (9, 10, 11, 12, 19, 20, 21, 22)]
BESIDE = [(x, y) for x in (13, 18) for y in (9, 10, 11, 12, 18, 19, 20, 21)] + \
         [(x, y) for y in (13, 18) for x in (9, 10, 11, 12, 19, 20, 21, 22)]
YELLOW_THRESHOLD = 35
# The white style: arms 2x4 further out, each inside a dark box.
WHITE_ARMS = [(x, y) for x in (15, 16) for y in (6, 7, 8, 9, 22, 23, 24, 25)] +              [(x, y) for y in (15, 16) for x in (6, 7, 8, 9, 22, 23, 24, 25)]
WHITE_OUTLINE = [(x, y) for x in (13, 18) for y in (6, 7, 8, 9, 22, 23, 24, 25)] +                 [(x, y) for y in (13, 18) for x in (6, 7, 8, 9, 22, 23, 24, 25)]
WHITE_THRESHOLD = 100

IN_MOVE = 8 | 16 | 512 | 1024  # forward, back, moveleft, moveright
IN_RUN = 4096
BUTTONS = {1: "attack", 2: "jump", 4: "duck", 8: "fwd", 16: "back", 32: "use", 128: "left",
           256: "right", 512: "mvleft", 1024: "mvright", 2048: "attack2", 4096: "RUN", 8192: "reload"}


def yellowness(pixel):
    return (pixel[0] + pixel[1]) / 2 - pixel[2]


def median(values):
    values = sorted(values)
    return values[len(values) // 2]


def has_crosshair(path, centre):
    image = Image.open(path).convert("RGB")
    cx, cy = centre or (image.size[0] // 2, image.size[1] // 2)
    box = image.crop((cx - 16, cy - 16, cx + 16, cy + 16)).load()
    yellow = median(yellowness(box[x, y]) for x, y in ARMS) -         median(yellowness(box[x, y]) for x, y in BESIDE)
    white = median(min(box[x, y]) for x, y in WHITE_ARMS) -         median(max(box[x, y]) for x, y in WHITE_OUTLINE)
    return yellow >= YELLOW_THRESHOLD or white >= WHITE_THRESHOLD


def runs(frames_dir, centre):
    """[(first frame, last frame, crosshair drawn)], frames counted from 0."""
    out = []
    for index, path in enumerate(sorted(glob.glob(os.path.join(frames_dir, "*.png")))):
        drawn = has_crosshair(path, centre)
        if out and out[-1][2] == drawn:
            out[-1][1] = index
        else:
            out.append([index, index, drawn])
    return out


def cmd_runs(args):
    for first, last, drawn in runs(args.frames, args.centre):
        print(f"{first:6d}-{last:6d}  {'crosshair' if drawn else 'none     '}  ({last - first + 1} frames)")


def cmd_hook(args):
    start = None
    trail = []
    for line in open(args.log, encoding="utf-8", errors="replace"):
        clock = re.search(r"\[demo\s+([\d.]+)\]", line)
        if not clock:
            continue
        at = float(clock.group(1))
        if "mirv_recordmovie_start" in line:
            start = at
        elif "POV would" in line:
            hide = "draw the crosshair" not in line
            trail.append((at, "hide: " + line.rsplit("-- ", 1)[1].strip() if hide else "draw"))
    if start is None:
        sys.exit("no mirv_recordmovie_start line in the log")
    times = [at for at, _ in trail]

    def said(at):
        i = bisect.bisect_right(times, at) - 1
        return trail[i][1] if i >= 0 else None

    agree = total = 0
    for first, last, drawn in runs(args.frames, args.centre):
        for frame in range(first, last + 1):
            hook = said(start + frame / args.fps)
            if hook is not None:
                total += 1
                agree += (hook == "draw") == drawn
        print(f"frames {first:5d}-{last:5d}  demo {start + first / args.fps:8.2f}-"
              f"{start + (last + 1) / args.fps:8.2f}  {'crosshair' if drawn else 'none     '}"
              f"  hook: {said(start + (first + 1) / args.fps)}")
    print(f"{agree}/{total} frames agree")


def cmd_pov(args):
    data = open(args.mdl, "rb").read()
    count, table = struct.unpack_from("<ii", data, 164)
    labels = [data[table + i * 176: table + i * 176 + 32].split(b"\0")[0].decode() for i in range(count)]
    rows = []
    for line in open(args.state):
        at, field, value = line.rstrip("\n").split("\t")
        rows.append((float(at), field, value))
    rows.sort(key=lambda row: row[0])
    by_field = collections.defaultdict(list)
    for at, field, value in rows:
        by_field[field].append((at, value))

    def value_at(field, at, default):
        changes = by_field[field]
        i = bisect.bisect_right(changes, (at, "￿")) - 1
        return changes[i][1] if i >= 0 else default

    def sprint_keys(at):
        buttons = int(float(value_at("buttons", at, "0")))
        return bool(buttons & IN_RUN and buttons & IN_MOVE)

    def show(field, value):
        if field in ("sequence", "gaitsequence"):
            index = int(float(value))
            return labels[index] if index < count else value
        if field == "buttons":
            held = int(float(value))
            return "+".join(name for bit, name in BUTTONS.items() if held & bit) or "none"
        return value

    found = runs(args.frames, args.centre)
    offset = args.offset
    if offset is None:
        def fit(candidate):
            hidden = total = 0
            for first, last, drawn in found:
                for frame in range(first, last + 1):
                    if sprint_keys(args.start + frame / args.fps - candidate):
                        total += 1
                        hidden += not drawn
            return hidden / max(total, 1), total
        (share, frames), offset = max((fit(o / 60), o / 60) for o in range(0, 900))
        print(f"fitted clock offset {offset:.3f}s: the crosshair is hidden in {share:.0%} of the "
              f"{frames} frames with the sprint keys held")
    for first, last, drawn in found:
        t0 = args.start + first / args.fps - offset
        t1 = args.start + (last + 1) / args.fps - offset
        print(f"{'shown ' if drawn else 'HIDDEN'} {t0:8.2f}-{t1:8.2f} ({t1 - t0:5.2f}s)  at start:"
              f" body={show('sequence', value_at('sequence', t0, '0'))}"
              f" gait={show('gaitsequence', value_at('gaitsequence', t0, '0'))}"
              f" keys={show('buttons', value_at('buttons', t0, '0'))}"
              f" prone={value_at('iuser3', t0, '0')} held={value_at('weaponmodel', t0, '?')}")
        for at, field, value in rows:
            if t0 <= at < t1 and field not in ("weaponanim", "flags", "health", "onground"):
                print(f"           {at:8.2f} {field} -> {show(field, value)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fps", type=float, default=30.0)
    parser.add_argument("--centre", type=lambda text: tuple(int(v) for v in text.split(",")), default=None)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("runs")
    p.add_argument("frames")
    p.set_defaults(run=cmd_runs)
    p = sub.add_parser("hook")
    p.add_argument("log")
    p.add_argument("frames")
    p.set_defaults(run=cmd_hook)
    p = sub.add_parser("pov")
    p.add_argument("state")
    p.add_argument("mdl")
    p.add_argument("frames")
    p.add_argument("start", type=float)
    p.add_argument("--offset", type=float, default=None)
    p.set_defaults(run=cmd_pov)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
