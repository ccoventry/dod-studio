r"""Find demos, and moments in them, from Studio's analyzer cache instead of parsing demos.

    demo_find.py --map anzio --pov --weapon garand --per-player 1
        POV demos on a map with "anzio" in its name holding a Garand kill, one per recorder.
    demo_find.py --player m00cat --weapon k98 --moments --headshot yes
        Every K98 headshot m00cat made: demo, viewdemo time, victim.
    demo_find.py --map anzio --weapon garand --by-recorder --moments --per-player 1 --limit 3
        A Garand kill by the recorder himself, from three different recorders: the
        "survey across players, not one recorder" (CLAUDE.md) as one call.
    demo_find.py --multi-map | --damaged | --hltv --min-length 20:00 ...

Filters (all must hold): --map (part of the name), --pov / --hltv, --recorder and --player
(part of a name, a SteamID64 or STEAM_0:X:Y; --player is anyone in the demo), --weapon (a
kill with it; by --player if given, by the recorder with --by-recorder), --victim,
--headshot yes|no|unknown, --min-kills N (by the same killer, else the demo's total),
--multi-map, --damaged / --undamaged, --min-length / --max-length (seconds or m:ss), --path
(part of the demo's path, e.g. "PRE-Anniversary"). --per-player N keeps N demos per recorder
(an HLTV demo counts under its HLTV name). Demos whose file is gone are left out
(--include-missing keeps them); the same file in two installs (name and size) counts once
(--all-copies keeps both).

Clocks. `viewdemo` is the time on the game's demo bar (the last SvcTime, m:ss:cc), what
`ds_demo_seek_to` takes and what the user reads; `file s` is seconds from the start of the
recording (the demo frames' own clock). The hook log's `[demo N]` is a third clock (client
time since launch) that this cache does not know. `length` is the recording's length, m:ss:cc.

The cache: %APPDATA%\dod-studio\analyzer_cache\v<N>\*.json (DS_STUDIO_APPDATA, or --cache
for the folder holding the v<N> folders), the newest v<N> with entries. Only read.
Fields as analysis/src/cache.rs and analysis/src/lib.rs write them.
"""
import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

STEAM64_BASE = 76561197960265728


# ── Pure parts ───────────────────────────────────────────────────────────────

def secs(duration):
    """A serde Duration ({secs, nanos}) or a plain number, as float seconds."""
    if isinstance(duration, dict):
        return duration.get("secs", 0) + duration.get("nanos", 0) / 1e9
    return float(duration or 0)


def bar_time(seconds):
    """Seconds as the viewdemo bar shows them, m:ss:cc (992.38 -> 16:32:38)."""
    total_cs = int(round(seconds * 100))
    minutes, rest = divmod(total_cs, 6000)
    return f"{minutes}:{rest // 100:02d}:{rest % 100:02d}"


def parse_length(text):
    """'1200', '20:00' or '1:02:03' (h:mm:ss) as seconds."""
    parts = [float(p) for p in str(text).split(":")]
    value = 0.0
    for part in parts:
        value = value * 60 + part
    return value


def steam64(text):
    """A SteamID64 string for STEAM_0:X:Y or a 17-digit id; None for a name."""
    text = text.strip()
    m = re.fullmatch(r"STEAM_[0-5]:([01]):(\d+)", text, re.I)
    if m:
        return str(STEAM64_BASE + int(m.group(2)) * 2 + int(m.group(1)))
    return text if re.fullmatch(r"7656\d{13}", text) else None


def player_matches(query, player_id, name):
    sid = steam64(query)
    if sid:
        return player_id == sid
    return query.lower() in (name or "").lower()


def recorder_of(state, demo_type):
    """(id, name) of the POV recorder: the player in the recorded client slot."""
    if demo_type != "POV":
        return None, None
    slot = state.get("pov_player_index")
    for p in state.get("players") or []:
        connection = p.get("connection")
        if isinstance(connection, dict) and (connection.get("Connected") or {}).get("client_id") == slot:
            return p.get("id"), p.get("name")
    return None, None


def summarize(entry, on_disk=True):
    """The fields queries use, from one cache entry."""
    info = entry["analysis"]["demo_info"]
    state = entry["analysis"]["state"]
    names = {p.get("id"): p.get("name") for p in state.get("players") or []}
    rec_id, rec_name = recorder_of(state, info.get("demo_type"))
    kills = []
    for k in state.get("kill_positions") or []:
        time = k.get("time") or {}
        kills.append({
            "viewdemo": secs(time.get("viewdemo_offset")), "file": secs(time.get("real_offset")),
            "weapon": k.get("weapon") or "Unknown", "killer": k.get("killer"), "victim": k.get("victim"),
            "killer_name": names.get(k.get("killer")) or ("world" if k.get("killer") is None else k.get("killer")),
            "victim_name": names.get(k.get("victim")) or k.get("victim"),
            "headshot": k.get("headshot") or "unknown", "teamkill": bool(k.get("teamkill")),
        })
    signons = state.get("signon_maps") or []
    damage = info.get("damage")
    return {
        "path": entry["file_info"]["path"], "name": entry["file_info"]["name"],
        "size": entry.get("size_bytes"), "on_disk": on_disk,
        "map": info.get("map_name") or state.get("initial_map_name") or "",
        "type": info.get("demo_type"), "length": float(info.get("playback_time") or 0),
        "recorder_id": rec_id, "recorder": rec_name, "hltv_name": state.get("hltv_name"),
        "players": names, "kills": kills,
        "multi_map": len(signons) > 1 or bool(state.get("map_changed")), "maps": signons,
        "damaged": damage is not None, "damage_reason": (damage or {}).get("reason"),
        "no_camera_data": bool(info.get("no_camera_data")),
    }


def weapon_matcher(query, known):
    """A test for a kill's weapon: the exact name when `query` is one (any case: `k98` is
    not `ScopedK98`), else any weapon with `query` in its name."""
    q = query.lower()
    if q in {w.lower() for w in known}:
        return lambda w: w.lower() == q
    return lambda w: q in w.lower()


def matching_kills(demo, args, weapon_ok):
    """The demo's kills that pass the kill filters."""
    out = []
    for k in demo["kills"]:
        if weapon_ok and not weapon_ok(k["weapon"]):
            continue
        if args.player and not player_matches(args.player, k["killer"], k["killer_name"]):
            continue
        if args.by_recorder and (not demo["recorder_id"] or k["killer"] != demo["recorder_id"]):
            continue
        if args.victim and not player_matches(args.victim, k["victim"], k["victim_name"]):
            continue
        if args.headshot and k["headshot"] != args.headshot:
            continue
        if not args.teamkills and k["teamkill"]:
            continue
        out.append(k)
    return out


def demo_passes(demo, args):
    """The demo-level filters."""
    if args.map and args.map.lower() not in demo["map"].lower():
        return False
    if args.pov and demo["type"] != "POV" or args.hltv and demo["type"] != "HLTV":
        return False
    if args.path and args.path.lower() not in demo["path"].lower():
        return False
    if args.recorder and not (demo["recorder_id"] and player_matches(args.recorder, demo["recorder_id"],
                                                                      demo["recorder"])):
        return False
    if args.player and not any(player_matches(args.player, pid, name) for pid, name in demo["players"].items()):
        return False
    if args.multi_map and not demo["multi_map"]:
        return False
    if args.damaged and not demo["damaged"] or args.undamaged and demo["damaged"]:
        return False
    if args.min_length is not None and demo["length"] < parse_length(args.min_length):
        return False
    if args.max_length is not None and demo["length"] > parse_length(args.max_length):
        return False
    return args.include_missing or demo["on_disk"]


def kill_filters_on(args):
    return bool(args.weapon or args.by_recorder or args.victim or args.headshot or args.min_kills
                or (args.player and args.moments))


def query(demos, args):
    """[(demo, kills that matched)] for every demo that passes, in path order."""
    known = {k["weapon"] for d in demos for k in d["kills"]}
    weapon_ok = weapon_matcher(args.weapon, known) if args.weapon else None
    seen, out, per_group = set(), [], {}
    for demo in sorted(demos, key=lambda d: d["path"].lower()):
        if not demo_passes(demo, args):
            continue
        key = (demo["name"].lower(), demo["size"])
        if not args.all_copies and key in seen:
            continue
        kills = matching_kills(demo, args, weapon_ok) if kill_filters_on(args) else []
        if kill_filters_on(args):
            needed = args.min_kills or 1
            if len(kills) < needed:
                continue
        group = demo["recorder_id"] or f"HLTV:{demo['hltv_name'] or demo['name']}"
        if args.per_player and per_group.get(group, 0) >= args.per_player:
            continue
        per_group[group] = per_group.get(group, 0) + 1
        seen.add(key)
        out.append((demo, kills))
        if args.limit and len(out) >= args.limit:
            break
    return out


def demo_row(demo, kills):
    flags = [f for f, on in (("multi-map", demo["multi_map"]), ("damaged", demo["damaged"]),
                             ("no-camera", demo["no_camera_data"]), ("missing", not demo["on_disk"])) if on]
    who = demo["recorder"] or (demo["hltv_name"] and f"HLTV {demo['hltv_name']}") or "-"
    return (f"{demo['name']:<58} {demo['map']:<16} {demo['type']:<4} {bar_time(demo['length']):>10} "
            f"{len(kills) if kills else '':>4}  {who[:30]:<30} {' '.join(flags)}")


def moment_row(demo, kill):
    return (f"{demo['name']:<58} {bar_time(kill['viewdemo']):>10} {kill['file']:>9.2f} {kill['weapon']:<12} "
            f"{kill['headshot']:<7} {kill['killer_name'][:24]:<24} -> {(kill['victim_name'] or '?')[:24]}")


DEMO_HEAD = f"{'demo':<58} {'map':<16} {'type':<4} {'length':>10} {'hits':>4}  {'recorder':<30} flags"
MOMENT_HEAD = (f"{'demo':<58} {'viewdemo':>10} {'file s':>9} {'weapon':<12} {'hs':<7} {'killer':<24} -> victim")


# ── The cache ────────────────────────────────────────────────────────────────

def schema_dir(root):
    """The newest v<N> folder under `root` with entries in it."""
    found = []
    for folder in Path(root).glob("v*"):
        if re.fullmatch(r"v\d+", folder.name) and any(folder.glob("*.json")):
            found.append((int(folder.name[1:]), folder))
    if not found:
        raise SystemExit(f"no analyzer cache entries under {Path(root).resolve()}")
    return max(found)[1]


def load(folder):
    demos, bad = [], 0
    for path in sorted(folder.glob("*.json")):
        if path.name.endswith(".players.json"):
            continue
        try:
            entry = json.loads(path.read_text(encoding="utf-8"))
            demos.append(summarize(entry, Path(entry["file_info"]["path"]).exists()))
        except (OSError, ValueError, KeyError, TypeError):
            bad += 1
    return demos, bad


# ── Command ──────────────────────────────────────────────────────────────────

def parser():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--cache", type=Path, help="the folder holding the v<N> folders")
    p.add_argument("--map")
    kind = p.add_mutually_exclusive_group()
    kind.add_argument("--pov", action="store_true")
    kind.add_argument("--hltv", action="store_true")
    p.add_argument("--recorder", help="name part, SteamID64 or STEAM_0:X:Y of the POV recorder")
    p.add_argument("--player", help="anyone in the demo; with kill filters, the killer")
    p.add_argument("--by-recorder", action="store_true", help="kills made by the demo's recorder")
    p.add_argument("--weapon", help="e.g. garand, k98, scoped, grenade")
    p.add_argument("--victim")
    p.add_argument("--headshot", choices=["yes", "no", "unknown"])
    p.add_argument("--teamkills", action="store_true", help="count teamkills too")
    p.add_argument("--min-kills", type=int)
    p.add_argument("--multi-map", action="store_true")
    dmg = p.add_mutually_exclusive_group()
    dmg.add_argument("--damaged", action="store_true")
    dmg.add_argument("--undamaged", action="store_true")
    p.add_argument("--min-length")
    p.add_argument("--max-length")
    p.add_argument("--path", help="part of the demo's path, e.g. PRE-Anniversary")
    p.add_argument("--per-player", type=int, help="at most N demos per recorder")
    p.add_argument("--limit", type=int)
    p.add_argument("--moments", action="store_true", help="one row per matching kill")
    p.add_argument("--max-moments", type=int, default=5, help="kills shown per demo with --moments")
    p.add_argument("--include-missing", action="store_true")
    p.add_argument("--all-copies", action="store_true")
    p.add_argument("--json", action="store_true")
    return p


def main(argv=None):
    args = parser().parse_args(argv)
    root = args.cache or kit_paths.studio_appdata() / "analyzer_cache"
    folder = schema_dir(root)
    demos, bad = load(folder)
    results = query(demos, args)
    if args.json:
        out = []
        for demo, kills in results:
            row = {k: v for k, v in demo.items() if k not in ("kills", "players")}
            row["viewdemo_length"] = bar_time(demo["length"])
            if kills:
                row["matching_kills"] = [dict(k, viewdemo_bar=bar_time(k["viewdemo"])) for k in kills]
            out.append(row)
        print(json.dumps(out, indent=1))
        return 0
    print(f"{folder.resolve()}: {len(demos)} entries{f', {bad} unreadable' if bad else ''}; "
          f"{len(results)} demo(s) match")
    if args.moments:
        print(MOMENT_HEAD)
        for demo, kills in results:
            for kill in kills[:args.max_moments]:
                print(moment_row(demo, kill))
        print("viewdemo = the demo bar (m:ss:cc, what ds_demo_seek_to takes); file s = seconds from the recording's start")
    else:
        print(DEMO_HEAD)
        for demo, kills in results:
            print(demo_row(demo, kills))
    return 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main())
