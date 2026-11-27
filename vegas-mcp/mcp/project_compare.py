"""compare_projects (#1050): what differs between two projects, from the
detail the bridge reads out of each (extension/CompareProjects.cs): every
event's times, speed, media offset, fades, FX with parameter values and
keyframes, Pan/Crop keyframes and envelopes; every track's FX, compositing,
Track Motion and envelopes; the Video Output FX, markers, regions and
command markers, the media pool's Media FX.

How things are matched, so a change reads as a change and not as one thing
removed and another added:
- Tracks by type and name in order (difflib), so a track added in the
  middle does not shift every later one; tracks with no name match in order.
- Events within a pair of tracks, in passes: the same file starting at the
  same time from the same media second (within `tolerance` seconds); then
  the same file from the same media second (the event moved on the
  timeline); then the same file at the same start (its media offset
  changed); then the same file (moved, and its offset changed too). What is
  left was added or removed (an event whose take now plays another file is
  both).
- Within a pass, the events that may pair (the same file, and for the
  offset passes the same media second) pair in timeline order: the first
  left in a with the first left in b, and so on, skipping only where that
  pairs more of them or brings them closer in time. So the same clip used
  three times and moved keeps its order, which nearest-first did not.
- Effects in a chain by position; markers, regions and command markers by
  time and label, then by label alone (moved), in time order the same way.

Standard library only.
"""

import difflib
import os

DEFAULT_TOLERANCE = 0.001
# Parameter values and positions are floats VEGAS rounds; closer than this is the same.
VALUE_TOLERANCE = 1e-6
# Fields compared with the time tolerance rather than VALUE_TOLERANCE.
TIME_FIELDS = {"time", "start", "length", "offset", "end", "snap_offset"}
# Not edits: where an event sits in its track's list, and what is selected.
EVENT_IGNORED = {"index", "selected", "group"}
TRACK_IGNORED = {"index", "events"}
PROJECT_FIELDS = ("width", "height", "fps", "pixel_format", "length")
# A diff entry holds the two values; a list longer than this is cut, with its length given.
MAX_LIST_SHOWN = 12

MATCHED_BY = (
    "tracks by type and name in order; events within matched tracks by the same file starting within %g s from the "
    "same media second, then the same file from the same media second (moved), then the same file at the same start "
    "(offset changed), then the same file (moved), each pass pairing in timeline order; effects by position in "
    "their chain; markers by time and label, then by label (moved)"
)


def compare(a, b, tolerance=DEFAULT_TOLERANCE):
    """What differs between project dumps a and b."""
    tolerance = float(tolerance)
    out = {"matched_by": MATCHED_BY % tolerance, "tolerance": tolerance}
    out["project"] = diff({k: a.get(k) for k in PROJECT_FIELDS}, {k: b.get(k) for k in PROJECT_FIELDS}, "", tolerance)
    out["output_effects"] = diff(effects(a.get("output_effects")), effects(b.get("output_effects")), "output_effects", tolerance)
    tracks, events = compare_tracks(a.get("tracks") or [], b.get("tracks") or [], tolerance)
    out["tracks"] = tracks
    out["events"] = events
    out["markers"] = {kind: compare_markers(a.get(kind) or [], b.get(kind) or [], tolerance)
                      for kind in ("markers", "regions", "command_markers")}
    out["media"] = compare_media(a.get("media") or [], b.get("media") or [], tolerance)
    groups = (len(a.get("groups") or []), len(b.get("groups") or []))
    if groups[0] != groups[1]:
        out["groups"] = {"a": groups[0], "b": groups[1]}
    out["summary"] = summary(out)
    out["same"] = not any(out["summary"].values())
    return out


def argument_problem(arguments):
    """What is wrong with compare_projects' own arguments, or None. Checked
    before the bridge reads both projects, which can take minutes, so a bad
    tolerance fails at once rather than after that."""
    tolerance = arguments.get("tolerance")
    if tolerance is None:
        return None
    if not is_number(tolerance) or not 0 <= tolerance < float("inf"):
        return "tolerance must be a number of seconds, 0 or more; got %r" % (tolerance,)
    return None


def reply(body, arguments):
    """The tool's answer from the bridge's reply: the comparison, and which files were read."""
    result = compare(body["a"], body["b"], arguments.get("tolerance", DEFAULT_TOLERANCE))
    result["read"] = {"a": body.get("read_a"), "b": body.get("read_b")}
    if body.get("restored"):
        result["restored"] = body["restored"]
    return result


# ---- matching ----


def pair_up(xs, ys, passes, when):
    """Pairs of xs and ys, pass by pass. Each pass is (name, groups, fits,
    cost): groups(items) gives each item's group key (only items of one group
    may pair), fits(x, y) whether two may pair, cost(x, y) how far apart they
    are. Within a group, the items left pair in order of when(item)
    (pair_in_order). Returns (pairs as (x, y, pass name), xs left, ys left),
    the lefts in their given order."""
    left_x = list(xs)
    left_y = list(ys)
    pairs = []
    for name, groups, fits, cost in passes:
        keys = groups(left_x + left_y)
        by_x, by_y = {}, {}
        for i, x in enumerate(left_x):
            by_x.setdefault(keys[i], []).append(x)
        for j, y in enumerate(left_y):
            by_y.setdefault(keys[len(left_x) + j], []).append(y)
        taken = set()
        for key in by_x:
            if key not in by_y:
                continue
            for x, y in pair_in_order(sorted(by_x[key], key=when), sorted(by_y[key], key=when), fits, cost):
                pairs.append((x, y, name))
                taken.add(id(x))
                taken.add(id(y))
        left_x = [x for x in left_x if id(x) not in taken]
        left_y = [y for y in left_y if id(y) not in taken]
    return pairs, left_x, left_y


def pair_in_order(xs, ys, fits, cost):
    """Pairs of xs and ys that keep both lists' order (no two pairs cross):
    as many pairs as fit, and of those the smallest total cost. xs and ys
    are already in order."""
    n, m = len(xs), len(ys)
    # best[i][j]: (pairs, -cost) for xs[:i] and ys[:j]; took[i][j]: how it was reached.
    best = [[(0, 0.0)] * (m + 1) for _ in range(n + 1)]
    took = [[None] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            options = [(best[i - 1][j], "skip x"), (best[i][j - 1], "skip y")]
            if fits(xs[i - 1], ys[j - 1]):
                count, score = best[i - 1][j - 1]
                options.append(((count + 1, score - cost(xs[i - 1], ys[j - 1])), "pair"))
            best[i][j], took[i][j] = max(options, key=lambda o: o[0])
    pairs = []
    i, j = n, m
    while i > 0 and j > 0:
        step = took[i][j]
        if step == "pair":
            pairs.append((xs[i - 1], ys[j - 1]))
            i, j = i - 1, j - 1
        elif step == "skip x":
            i -= 1
        else:
            j -= 1
    pairs.reverse()
    return pairs


def media_of(event):
    path = event.get("media_path") or event.get("media")
    return os.path.normcase(path) if path else None


def file_key(event):
    """What counts as the same file: its path, or for an event with no media its name."""
    media = media_of(event)
    return ("media", media) if media is not None else ("name", event.get("name"))


def event_start(e):
    return e.get("start", 0)


def clusters(values, tolerance):
    """Each value's cluster: the values in order, split where two neighbours
    are more than `tolerance` apart, so values within it of each other share one."""
    out = {}
    n = 0
    previous = None
    for v in sorted(set(values)):
        if previous is not None and v - previous > tolerance:
            n += 1
        out[v] = n
        previous = v
    return out


def event_passes(tolerance):
    def offset(e):
        return e.get("offset") or 0

    def gap(x, y):
        return abs(event_start(x) - event_start(y))

    def by_file(items):
        return [file_key(e) for e in items]

    def by_file_and_offset(items):
        cluster = clusters([offset(e) for e in items], tolerance)
        return [(file_key(e), cluster[offset(e)]) for e in items]

    def same_start(x, y):
        return gap(x, y) <= tolerance

    def anywhere(x, y):
        return True

    return [
        ("same file, start and media offset", by_file_and_offset, same_start, gap),
        ("same file and media offset (moved)", by_file_and_offset, anywhere, gap),
        ("same file and start (media offset changed)", by_file, same_start, gap),
        ("same file, in timeline order (moved)", by_file, anywhere, gap),
    ]


def compare_tracks(ta, tb, tolerance):
    """(track changes, event changes) between two track lists."""
    tracks = {"added": [], "removed": [], "changed": []}
    events = {"added": [], "removed": [], "moved": [], "changed": []}
    signature = lambda t: "%s|%s" % (t.get("type"), t.get("name") or "")  # noqa: E731
    matcher = difflib.SequenceMatcher(None, [signature(t) for t in ta], [signature(t) for t in tb], autojunk=False)
    for op, i1, i2, j1, j2 in matcher.get_opcodes():
        pairs = list(zip(range(i1, i2), range(j1, j2))) if op in ("equal", "replace") else []
        for i, j in pairs:
            compare_track_pair(ta[i], tb[j], tolerance, tracks, events)
        for i in range(i1 + len(pairs), i2):
            tracks["removed"].append(track_ref(ta[i]))
        for j in range(j1 + len(pairs), j2):
            tracks["added"].append(track_ref(tb[j]))
    return tracks, events


def compare_track_pair(x, y, tolerance, tracks, events):
    changes = diff(track_fields(x), track_fields(y), "", tolerance)
    if changes:
        tracks["changed"].append(dict(track_pair_ref(x, y), changes=changes))
    pairs, removed, added = pair_up(x.get("events") or [], y.get("events") or [], event_passes(tolerance), event_start)
    for ea, eb, how in pairs:
        ref = dict(track_pair_ref(x, y), event_a=ea.get("index"), event_b=eb.get("index"), media=short_media(eb))
        if abs(ea.get("start", 0) - eb.get("start", 0)) > tolerance:
            events["moved"].append(dict(ref, start_a=ea.get("start"), start_b=eb.get("start"), matched_by=how))
        changes = diff(event_fields(ea), event_fields(eb), "", tolerance)
        if changes:
            events["changed"].append(dict(ref, start=eb.get("start"), changes=changes))
    for e in removed:
        events["removed"].append(dict(track=x.get("index"), event=e.get("index"), start=e.get("start"),
                                      length=e.get("length"), media=short_media(e)))
    for e in added:
        events["added"].append(dict(track=y.get("index"), event=e.get("index"), start=e.get("start"),
                                    length=e.get("length"), media=short_media(e)))


def track_ref(t):
    return {"track": t.get("index"), "type": t.get("type"), "name": t.get("name"), "events": len(t.get("events") or [])}


def track_pair_ref(x, y):
    ref = {"track": y.get("index")}
    if x.get("index") != y.get("index"):
        ref["track_a"] = x.get("index")
    return ref


def short_media(event):
    path = event.get("media_path") or event.get("media")
    return os.path.basename(path) if path else None


def compare_markers(ma, mb, tolerance):
    key = lambda m: m.get("label") or m.get("comment") or ""  # noqa: E731
    gap = lambda x, y: abs(x.get("time", 0) - y.get("time", 0))  # noqa: E731
    by_label = lambda items: [key(m) for m in items]  # noqa: E731
    passes = [("same time and label", by_label, lambda x, y: gap(x, y) <= tolerance, gap),
              ("same label (moved)", by_label, lambda x, y: True, gap)]
    pairs, removed, added = pair_up(ma, mb, passes, lambda m: m.get("time", 0))
    out = {"added": [strip_index(m) for m in added], "removed": [strip_index(m) for m in removed], "changed": []}
    for x, y, _ in pairs:
        changes = diff(strip_index(x), strip_index(y), "", tolerance)
        if changes:
            out["changed"].append({"index_a": x.get("index"), "index_b": y.get("index"), "label": key(y), "changes": changes})
    return out


def compare_media(ma, mb, tolerance):
    by_a = {m.get("key"): m for m in ma}
    by_b = {m.get("key"): m for m in mb}
    out = {"added": sorted(k for k in by_b if k not in by_a), "removed": sorted(k for k in by_a if k not in by_b), "changed": []}
    for k in sorted(set(by_a) & set(by_b)):
        changes = diff(effects(by_a[k].get("effects")), effects(by_b[k].get("effects")), "effects", tolerance)
        if changes:
            out["changed"].append({"media": k, "changes": changes})
    return out


# ---- what is compared ----


def strip_index(d):
    return {k: v for k, v in d.items() if k != "index"}


def effects(chain):
    """An FX chain as a list of {id, name, bypass, params: {name: value}, keyframes}."""
    out = []
    for e in chain or []:
        one = {"id": e.get("id"), "name": e.get("name"), "bypass": e.get("bypass")}
        if e.get("params") is not None:
            one["params"] = {p["name"]: p.get("value") for p in e["params"]}
        if e.get("keyframes"):
            one["keyframes"] = e["keyframes"]
        out.append(one)
    return out


def envelopes(envs):
    """Envelopes by type, so one added beside the others does not shift them."""
    if isinstance(envs, dict):  # project_info's counts, not points
        return envs
    return {e.get("type"): {k: v for k, v in e.items() if k != "type"} for e in envs or []}


def fade(f):
    if not isinstance(f, dict):
        return f
    f = dict(f)
    if isinstance(f.get("transition"), dict):
        t = f["transition"]
        f["transition"] = effects([t])[0]
    return f


def event_fields(e):
    d = {k: v for k, v in e.items() if k not in EVENT_IGNORED and k != "start"}
    d["effects"] = effects(e.get("effects"))
    d["envelopes"] = envelopes(e.get("envelopes"))
    for edge in ("fade_in", "fade_out"):
        if edge in d:
            d[edge] = fade(d[edge])
    return d


def track_fields(t):
    d = {k: v for k, v in t.items() if k not in TRACK_IGNORED}
    if "effects" in d:
        d["effects"] = effects(d["effects"])
    d["envelopes"] = envelopes(t.get("envelopes"))
    return d


# ---- differences ----


def same_number(x, y, tolerance):
    return abs(x - y) <= tolerance * max(1.0, abs(x), abs(y)) if tolerance == VALUE_TOLERANCE else abs(x - y) <= tolerance


def is_number(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def diff(x, y, path, tolerance, field=None):
    """Where x and y differ, as [{field, a, b}]: dicts by key, lists of the
    same length item by item (a point or colour, a list of numbers, as one
    value), anything else as a whole."""
    if isinstance(x, dict) and isinstance(y, dict):
        out = []
        for k in sorted(set(x) | set(y), key=str):
            sub = "%s.%s" % (path, k) if path else str(k)
            if k not in x:
                out.append({"field": sub, "a": None, "b": shown(y[k]), "note": "only in b"})
            elif k not in y:
                out.append({"field": sub, "a": shown(x[k]), "b": None, "note": "only in a"})
            else:
                out += diff(x[k], y[k], sub, tolerance, k)
        return out
    if isinstance(x, list) and isinstance(y, list) and len(x) == len(y) and not all(is_number(v) for v in x + y):
        out = []
        for i, (u, v) in enumerate(zip(x, y)):
            out += diff(u, v, "%s[%d]" % (path, i), tolerance, field)
        return out
    return [] if same(x, y, tolerance if field in TIME_FIELDS else VALUE_TOLERANCE) else [{"field": path, "a": shown(x), "b": shown(y)}]


def same(x, y, tolerance):
    if is_number(x) and is_number(y):
        return same_number(float(x), float(y), tolerance)
    if isinstance(x, list) and isinstance(y, list):
        return len(x) == len(y) and all(same(u, v, tolerance) for u, v in zip(x, y))
    return x == y


def shown(v):
    if isinstance(v, list) and len(v) > MAX_LIST_SHOWN:
        return {"first": v[:MAX_LIST_SHOWN], "count": len(v)}
    return v


def summary(out):
    s = {
        "project": len(out["project"]),
        "output_effects": len(out["output_effects"]),
        "tracks_added": len(out["tracks"]["added"]),
        "tracks_removed": len(out["tracks"]["removed"]),
        "tracks_changed": len(out["tracks"]["changed"]),
        "events_added": len(out["events"]["added"]),
        "events_removed": len(out["events"]["removed"]),
        "events_moved": len(out["events"]["moved"]),
        "events_changed": len(out["events"]["changed"]),
        "media_added": len(out["media"]["added"]),
        "media_removed": len(out["media"]["removed"]),
        "media_changed": len(out["media"]["changed"]),
        "groups": 1 if "groups" in out else 0,
    }
    for kind, m in out["markers"].items():
        s[kind] = len(m["added"]) + len(m["removed"]) + len(m["changed"])
    return s
