"""Holds the hook's stand-in rounds (its log) against the probe's offline list.

    cargo run --release -p analysis --example hltv_shot_evidence_probe -- <demo> --list > rounds.txt
    python goldsrc-hooks/tools/compare_missing_shots.py rounds.txt [hook log] [--from <game seconds>]

--from leaves out what the hook played before that moment on the game's
clock, for a session that fast-forwarded to the stretch being checked.

The session to check is the last one in the hook log (today's by default),
played with dodstudio_spec_match_pov 2. See
docs/goldsrc_hltv_missing_gunshots.md.
"""
import re
import sys
from datetime import datetime
from pathlib import Path
import os

args = sys.argv[1:]
start_at = 0.0
if "--from" in args:
    at = args.index("--from")
    start_at = float(args[at + 1])
    del args[at:at + 2]
silent = Path(args[0])
log = Path(args[1]) if len(args) > 1 else Path(os.environ["APPDATA"]) / "dod-studio" / "logs" / f"dodstudio_goldsrc_hooks_{datetime.now():%Y%m%d}.log"

offline = []
for line in silent.read_text(errors="replace").splitlines():
    m = re.match(r"\s*([\d.]+)s\s+entity\s+(\d+)", line)
    if m:
        offline.append((float(m.group(1)), int(m.group(2))))

text = log.read_text(encoding="utf-8", errors="replace")
session = text[text.rfind("new session"):]
game = [(float(t), int(p), w) for t, p, w in re.findall(
    r"\[demo\s+([\d.]+)\].*missing_shots: player (\d+) fired a round with no fire event -- playing (\w+)", session)]
status = re.findall(r"missing gunshots: [^\n]*", session)
game = [g for g in game if g[0] >= start_at]
if not game:
    sys.exit("no stand-in rounds in the last session")

def pair_off(offset):
    """The offline rounds inside the played stretch, those of them the hook
    did not play, and the rounds it played that are not among them."""
    first, last = game[0][0] - offset, game[-1][0] - offset
    window = [(t, e) for t, e in offline if first - 0.05 <= t <= last + 0.05]
    left = list(window)
    extra = []
    for t, p, w in game:
        hit = next((o for o in left if o[1] == p and abs(o[0] + offset - t) <= 0.08), None)
        if hit:
            left.remove(hit)
        else:
            extra.append((t, p, w))
    return window, left, extra


# The game's clock is the file's plus a per-session constant. Every offline
# round by the first player near the first played one is a candidate for it;
# the right one is the constant that pairs the most rounds.
first_time, first_player = game[0][0], game[0][1]
candidates = [first_time - t for t, e in offline if e == first_player and abs(first_time - t) <= 15]
if not candidates:
    sys.exit(f"the offline list has nothing from player {first_player} near {first_time}s")
offset = min(candidates, key=lambda c: len(pair_off(c)[2]))
window, unmatched_offline, extra = pair_off(offset)

print(f"clock offset {offset:.3f}s; file window {window[0][0]:.1f}s to {window[-1][0]:.1f}s")
print(f"offline rounds with no event in that window: {len(window)}")
print(f"played by the hook: {len(game)}; matched one-to-one: {len(game) - len(extra)}")
print(f"offline but not played: {len(unmatched_offline)}", unmatched_offline[:10])
print(f"played but not offline: {len(extra)}", extra[:10])
for line in status[-1:]:
    print(line)
