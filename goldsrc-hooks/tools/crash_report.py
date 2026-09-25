"""A readable summary of every game crash on record.

Reads the hook DLL's logs (%APPDATA%\\dod-studio\\logs\\dodstudio_goldsrc_hooks_*.log,
kept 30 days) for the crash recorder's `CRASH:` blocks, and the movie
install's qconsole.log for the engine's own fatal errors. Groups crashes by
where they happened (`module+offset`, which is the same every run), newest
first, with what was happening just before, and marks the ones already known.

usage: python crash_report.py [--game DIR] [--context N] [--all]
  --game     the Half-Life folder DoD Studio launches, for qconsole.log (else
             read from DoD Studio's settings)
  --context  how many log lines before each crash to show (default 8)
  --all      every occurrence, not just the newest of each crash

The crash recorder sees a fault before the game does. A fault the game then
handles itself would also be logged, but the ones that matter end the
session: check that nothing but the crash follows it in that day's log.
"""
import argparse, collections, glob, json, os, re

# Crashes already understood: `module+offset` -> what it is.
# #374: DoD writes into a temp entity the engine could not allocate (NULL).
# One entry per unchecked call site, at the first write after each; every one
# is guarded by src/tempent_fix.rs, so seeing one means the fix did not install
# (the log's "tempent_fix:" lines say why).
TEMPENT = "#374: DoD writes into a NULL temp entity ({}) -- tempent_fix guards this; check its log line"
KNOWN = {
    "client.dll+0x225cc": TEMPENT.format("hit puff dust"),
    "client.dll+0x226a4": TEMPENT.format("hit puff blood"),
    "client.dll+0xb23e": TEMPENT.format("blood stream dust"),
    "client.dll+0xb2e2": TEMPENT.format("blood stream blood"),
    "client.dll+0x31574": TEMPENT.format("shell casing, player model"),
    "client.dll+0x316ad": TEMPENT.format("shell casing, viewmodel"),
    # #375 triage: Server::ParseDeltaPacketEntities (core.dll+0x13320) clears
    # entnum * 340 bytes of a 256-entity array without stopping after its own
    # "entnum>MAX_PACKET_ENTITIES" error, so 257+ entities also zero the
    # m_Instream pointer stored just past the array, and the next ReadByte
    # reads through NULL.
    # The crash record names no demo; only an entity count over 256 reaches
    # this, and every HLTV demo in the movie install that has one is on
    # dod_lennon2 and has it from the first frame (packet_entity_probe).
    "core.dll+0x16d6": "#207: an HLTV demo with more than 256 entities in one packet (so far always dod_lennon2); "
                       "the pre-Anniversary HLTV demo player can't hold them. The Anniversary build's limit is 1024",
    # #384: PM_RecursiveHullCheck (hw.dll+0x6c830) walking a previous map's
    # clip hull after `playdemo` of an HLTV demo on some maps (anzio,
    # harrington): a loop until the stack runs out, or a garbage plane read.
    # Guarded by src/hull_trace_guard.rs, so seeing one means the guard did
    # not install (the log's "hull_trace_guard:" line says why).
    "hw.dll+0x6c839": "#384: the engine's hull trace walked a previous map's collision data and ran out of stack "
                      "-- hull_trace_guard guards this; check its log line",
    "hw.dll+0x6c8d1": "#384: the engine's hull trace read a garbage plane from a previous map's collision data "
                      "-- hull_trace_guard guards this; check its log line",
    # The same plane read in the 25th Anniversary hw.dll's PM_RecursiveHullCheck
    # (+0x1e2540), found offline; its stack-overflow site isn't listed because
    # that build's frame is written at several places before the first push.
    "hw.dll+0x1e2612": "#384 (25th Anniversary build): the engine's hull trace read a garbage plane from a previous "
                       "map's collision data -- hull_trace_guard guards this; check its log line",
}
# Engine fatal errors already understood: a substring of the message -> what it is.
KNOWN_ERRORS = {
    "entnum>MAX_PACKET_ENTITIES": "#207: an HLTV demo with more than 256 entities (so far always dod_lennon2); "
                                  "the demo is fine, the pre-Anniversary engine can't play it",
    "Cannot continue without model": "a demo needs a file that is missing from the game folder; "
                                     "check whether it is there now",
    "Illegible server message - svc_bad": "expected for recovered/spliced demos from the demo-salvage R&D; "
                                          "anywhere else it is a damaged demo",
}
# Log lines that are only noise when reading what led up to a crash.
NOISE = re.compile(r"texture_hires: (world|model|sprite) \"|texture_hires: detail gfx|still cached from|"
                   r"texture_hires: sky gfx|client.dll ClientCmd")

LOGS = os.path.join(os.environ.get("APPDATA", ""), "dod-studio", "logs")
HEADER = re.compile(r"^\[(?P<time>[\d:.]+)\](?: \[demo +(?P<demo>[\d.]+)\])? \[dodstudio_goldsrc_hooks\] (?P<msg>.*)$")
CRASH = re.compile(r"CRASH: (?P<what>.+?) at (?P<where>[\w.]+\+0x[0-9a-f]+)(?: \(0x[0-9a-f]+\))?(?: -- (?P<detail>.*))?$")
FRAME = re.compile(r"CRASH:\s+\[esp\+0x[0-9a-f]+\] (?P<frame>[\w.]+\+0x[0-9a-f]+)")


def game_dir(arg):
    if arg:
        return arg
    try:
        cfg = json.load(open(os.path.join(LOGS, "..", "settings.json"), encoding="utf-8"))
        return os.path.dirname(cfg.get("hl_path") or "") or None
    except (OSError, ValueError):
        return None


def hook_crashes(context):
    """[(date, time, demo time, where, what, detail, frames, lines before, level, session start)]"""
    out = []
    for path in sorted(glob.glob(os.path.join(LOGS, "dodstudio_goldsrc_hooks_*.log"))):
        date = re.search(r"_(\d{8})\.log$", path).group(1)
        date = f"{date[:4]}-{date[4:6]}-{date[6:]}"
        lines = open(path, encoding="utf-8", errors="replace").read().splitlines()
        level, session = None, None
        before = collections.deque(maxlen=context)
        i = 0
        while i < len(lines):
            m = HEADER.match(lines[i])
            msg = m.group("msg") if m else lines[i]
            if m and msg.startswith("goldsrc-hooks worker thread started"):
                level, session = None, m.group("time")
                before.clear()
            if m and msg.startswith("level: "):
                level = msg[len("level: "):]
            c = CRASH.match(msg) if m else None
            if c and not msg.startswith("CRASH:   "):
                frames = []
                j = i + 1
                while j < len(lines) and "CRASH:   " in lines[j]:
                    f = FRAME.search(lines[j])
                    if f:
                        frames.append(f.group("frame"))
                    j += 1
                out.append((date, m.group("time"), m.group("demo"), c.group("where"), c.group("what"),
                            c.group("detail"), frames, list(before), level, session))
                i = j
                continue
            if lines[i].strip() and not NOISE.search(lines[i]) and "CRASH:" not in lines[i]:
                before.append(lines[i])
            i += 1
    return out


def engine_errors(game):
    """{message: [demo, ...]} for qconsole.log's fatal errors."""
    found = collections.defaultdict(list)
    if not game:
        return found
    path = os.path.join(game, "qconsole.log")
    if not os.path.exists(path):
        return found
    demo = None
    lines = open(path, encoding="latin1").read().replace("\r", "").splitlines()
    for n, line in enumerate(lines):
        if line.startswith("Playing demo from "):
            demo = line[len("Playing demo from "):].rstrip(".")
        elif line.startswith("Host_Error:") or line.startswith("Sys_Error"):
            found[line.strip()].append(demo)
        elif "***** FATAL ERROR *****" in line and n + 1 < len(lines):
            found["FATAL ERROR: " + lines[n + 1].strip()].append(demo)
        elif "Cannot continue without model" in line:
            found[line.strip()].append(demo)
    return found


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--game")
    ap.add_argument("--context", type=int, default=8)
    ap.add_argument("--all", action="store_true")
    args = ap.parse_args()

    crashes = hook_crashes(args.context)
    by_where = collections.defaultdict(list)
    for c in crashes:
        by_where[c[3]].append(c)
    groups = sorted(by_where.items(), key=lambda kv: max((c[0], c[1]) for c in kv[1]), reverse=True)

    print(f"== Crashes recorded by the hook DLL ({LOGS}) ==")
    if not groups:
        print("none")
    for where, occ in groups:
        occ.sort(key=lambda c: (c[0], c[1]), reverse=True)
        first = occ[0]
        print(f"\n{where}  x{len(occ)}  ({first[4]}{', ' + first[5] if first[5] else ''})")
        print(f"  {KNOWN.get(where, 'NOT KNOWN YET: worth an issue')}")
        modules = list(dict.fromkeys(f.split("+")[0] for f in first[6]))
        print(f"  call stack runs through: {' <- '.join(modules) or '(none recorded)'}")
        for date, time, demo, _, _, _, frames, before, level, session in (occ if args.all else occ[:1]):
            when = f"{date} {time}" + (f", {float(demo):.0f} s of playback into the session" if demo else "")
            print(f"  -- {when}" + (f", last level loaded: {level}" if level else "") + (f" (session started {session})" if session else ""))
            for line in before:
                m = HEADER.match(line)
                print(f"       {m.group('msg') if m else line}"[:200])
        if not args.all and len(occ) > 1:
            print(f"  (also {', '.join(f'{c[0]} {c[1]}' for c in occ[1:])}; --all shows each)")

    game = game_dir(args.game)
    errors = engine_errors(game)
    print(f"\n== Engine fatal errors ({os.path.join(game, 'qconsole.log') if game else 'no game folder found; use --game'}) ==")
    if not errors:
        print("none")
    for msg, demos in sorted(errors.items(), key=lambda kv: -len(kv[1])):
        named = collections.Counter(d or "(demo unknown)" for d in demos)
        print(f"\n{msg}  x{len(demos)}")
        known = [note for key, note in KNOWN_ERRORS.items() if key in msg]
        if known:
            print(f"  ({known[0]})")
        for d, n in named.most_common(5):
            print(f"  {d}" + (f" x{n}" if n > 1 else ""))


if __name__ == "__main__":
    main()
