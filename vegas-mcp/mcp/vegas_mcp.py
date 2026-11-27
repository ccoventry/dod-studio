"""MCP server for VEGAS Pro (#1050): starts its own VEGAS beside the user's
and forwards tool calls to the DoD Studio bridge extension inside it, over
that VEGAS's named pipe \\\\.\\pipe\\dodstudio-vegas-<pid> (one JSON request
per line, one reply per line). vegas_instances.py starts, finds and ends
those VEGAS processes; the bridge never listens in a VEGAS it did not start.

Standard library only. Speaks MCP's stdio transport (newline-delimited
JSON-RPC 2.0), so no `mcp` package is needed. Each tool call runs on its own
thread and every bridge call has a time limit, so the main loop always reads
stdin: `end` works while a call is stuck, and when Claude Code closes the
server's stdin, it ends every VEGAS it started at once, so none is left on
the user's screen. A server killed outright cannot; its VEGAS then ends
itself (the bridge watches the server's process, extension/OwnerWatch.cs).

The shell commands are in USAGE below.
"""

import io
import json
import os
import sys
import threading
import weakref

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # also under python -I

import detail_tools  # noqa: E402
import snapshot_content  # noqa: E402
import vegas_instances  # noqa: E402
from bridge_pipe import Bridge, BridgeBusy, BridgeTimeout, BridgeUnavailable  # noqa: E402
from tool_schema import EVENT, MEDIA, SECONDS, TARGET, TRACK, tool  # noqa: E402

# A call that is still running in VEGAS: its VEGAS is alive, nothing to clean up.
BridgeTimeoutOrBusy = (BridgeTimeout, BridgeBusy)

PROTOCOL = "2025-06-18"
# The versions this server speaks; every one has tools and image content.
# A client asking for another gets PROTOCOL, as the MCP spec says, and
# decides itself whether it can go on.
SUPPORTED_PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")
SERVER_VERSION = "0.5"

# How long a bridge call may take before the server stops waiting for it.
# Most commands take well under 10 s; opening a project or an EDL waits for
# its media, and a render takes as long as the render. `timeout_seconds`
# on any call overrides these.
DEFAULT_TIMEOUT_SECONDS = 120
TIMEOUT_SECONDS = {"render": 3600, "open_project": 600, "import_edl": 600}
# import_edl waits up to its own wait_seconds for the timeline to fill; the
# limit is at least that plus this.
IMPORT_EDL_MARGIN_SECONDS = 60
# The longest limit a call may ask for: longer than any render, and far
# under threading.TIMEOUT_MAX, past which a wait raises OverflowError after
# the request has been sent.
MAX_TIMEOUT_SECONDS = 7 * 24 * 3600

RESAMPLE = {"type": "string", "enum": ["Smart", "Force", "Disable", "OpticalFlow", "ProjectLevel"]}
OFFSET = {"type": "number", "description": "seconds into the media where the event starts (media time, at any speed)"}

PID = {"type": "integer", "description": "a bridge VEGAS's process id (launch, instances)"}

# Handled here, not by the bridge.
LOCAL_TOOLS = [
    tool(
        "launch",
        "Start a VEGAS for the bridge beside any VEGAS the user has open (their VEGAS is never touched; the bridge does "
        "not listen in it), on a hidden desktop of its own, so none of its windows reach the user's screen. Every file "
        "it writes and every project it opens stays inside work_folder: a project from elsewhere is copied in first. "
        "Later tools go to this VEGAS. Returns its pid and the user's VEGAS PIDs; tell the user a second VEGAS was started.",
        {
            "work_folder": {"type": "string", "description": "default: the server's DODSTUDIO_VEGAS_ROOT, else %TEMP%\\dodstudio-vegas"},
            "wait_seconds": {"type": "number", "description": "how long to wait for its bridge; default 120"},
            "minimised": {"type": "boolean", "description": "default true (hidden desktop); false starts it on the user's "
                          "desktop, shown without taking focus, to watch it work: only when the user asks"},
        },
    ),
    tool(
        "end",
        "End a bridge VEGAS by its PID (default: the current one), without saving, and remove its autosave so the next "
        "VEGAS does not ask to recover it. Works while a call to it is stuck (a render that hangs, a dialog nobody "
        "answers). Refuses any VEGAS without a bridge, so the user's VEGAS is never ended.",
        {"pid": PID},
    ),
    tool("instances", "Every running VEGAS: which have a bridge (started for it), which is current, which is busy with a "
         "call that ran past its time limit, and the user's own."),
    tool("use", "Send later tools to another bridge VEGAS.", {"pid": PID}, ["pid"]),
]

TOOLS = LOCAL_TOOLS + [
    tool("ping", "Check the current bridge VEGAS: versions, pid, work folder, open project, dialogs it answered."),
    tool(
        "project_info",
        "The open project: size, fps, tracks with their events (time, speed, media offset, FX; mute, locked, group, "
        "and loop false, only when not VEGAS's default) and the media pool. detail=true adds each event's fades and snap offset, each track's FX, "
        "compositing and envelope/keyframe counts, the Video Output FX, markers, regions and event groups; the "
        "keyframes and points themselves come from the get_* tools.",
        {"detail": {"type": "boolean", "description": "default false"}},
    ),
    tool(
        "new_project",
        "Close the open project WITHOUT saving and start an empty one. Refuses when it has unsaved changes unless discard_changes.",
        {
            "width": {"type": "integer"},
            "height": {"type": "integer"},
            "fps": {"type": "number"},
            "pixel_format": {"type": "string", "enum": ["Int8Bit", "Int8BitFullRange", "Float32Bit", "Float32BitFullRange"]},
            "discard_changes": {"type": "boolean"},
        },
    ),
    tool(
        "open_project",
        "Open a .veg project. One outside the work folder is copied into <work folder>\\copies and the copy opened "
        "(the original is never opened or saved). Missing media is left offline. Refuses when the open one has unsaved "
        "changes unless discard_changes.",
        {"path": {"type": "string"}, "discard_changes": {"type": "boolean"}},
        ["path"],
    ),
    tool(
        "import_edl",
        "Open an EDL Text File (DoD Studio's Export VEGAS EDL / Export Project to VEGAS) as VEGAS does: as a NEW project "
        "that replaces the open one (refused when it has unsaved changes unless discard_changes). Then optional size/fps "
        "and Select None. Returns track/event counts, how many events are offline, and project_info.",
        {
            "path": {"type": "string"},
            "wait_seconds": {"type": "number", "description": "how long to wait for the events to appear; default 60"},
            "width": {"type": "integer"},
            "height": {"type": "integer"},
            "fps": {"type": "number"},
            "select_none": {"type": "boolean", "description": "default true"},
            "discard_changes": {"type": "boolean"},
        },
        ["path"],
    ),
    tool(
        "save_as",
        "Save the project to a NEW .veg path inside the work folder. Never overwrites a file this bridge session did not "
        "save itself (overwrite=true for those).",
        {"path": {"type": "string"}, "overwrite": {"type": "boolean"}},
        ["path"],
    ),
    tool(
        "add_media",
        "Put a media file (imported if needed) or subclip on the timeline as a video event; on a new video track unless "
        "`track` is given. A new track goes after every existing one, so no track's index changes (VEGAS draws track 0 "
        "in front, so it is behind them); the reply gives its index.",
        {
            "media": MEDIA,
            "track": TRACK,
            "start": SECONDS,
            "length": {"type": "number", "description": "timeline seconds; default: the rest of the media at this speed"},
            "offset": OFFSET,
            "playback_rate": {"type": "number", "description": "event speed: 0.2 = 5x slow motion (like ctrl-drag)"},
            "resample": RESAMPLE,
            "name": {"type": "string"},
            "with_audio": {"type": "boolean", "description": "also add the audio on a new audio track, grouped"},
        },
        ["media"],
    ),
    tool(
        "set_event",
        "Change an event: speed (playback_rate; the media second it starts at stays unless offset), start, length, "
        "media offset, resample mode, name, mute, locked, loop, snap offset.",
        {
            "track": TRACK,
            "event": EVENT,
            "playback_rate": {"type": "number"},
            "start": SECONDS,
            "length": SECONDS,
            "offset": OFFSET,
            "resample": RESAMPLE,
            "name": {"type": "string"},
            "mute": {"type": "boolean"},
            "locked": {"type": "boolean"},
            "loop": {"type": "boolean"},
            "snap_offset": {"type": "number", "description": "where the event snaps, seconds from its start"},
        },
        ["track", "event"],
    ),
    tool(
        "create_subclip",
        "Make a subclip of a media file (its own Media FX chain); returns its key for add_media / add_effect.",
        {"media": MEDIA, "start": SECONDS, "length": SECONDS, "name": {"type": "string"}, "reverse": {"type": "boolean"}},
        ["media", "start", "length"],
    ),
    tool("list_effects", "Installed video effects (name, unique id, OFX or not), filtered by part of a name or id.", {"filter": {"type": "string"}}),
    tool(
        "add_effect",
        "Add a video effect to an event (Event FX), a media/subclip (Media FX), a video track (Track FX) or the Video "
        "Output FX, optionally setting OFX parameters by name or label (points as [x, y], colours as [r, g, b, a] 0-1).",
        {
            "target": TARGET,
            "track": TRACK,
            "event": EVENT,
            "media": MEDIA,
            "effect": {"type": "string", "description": "effect name or unique id (list_effects)"},
            "params": {"type": "object", "description": "parameter name or label -> value"},
        },
        ["effect"],
    ),
    tool(
        "set_effect_params",
        "Set OFX parameters (static values) or bypass of the effect at effect_index in an FX chain (event, media, "
        "track or output); set_keyframes animates them. Legacy (non-OFX) effects' parameters are refused.",
        {
            "target": TARGET,
            "track": TRACK,
            "event": EVENT,
            "media": MEDIA,
            "effect_index": {"type": "integer"},
            "params": {"type": "object"},
            "bypass": {"type": "boolean"},
        },
        ["effect_index"],
    ),
    tool(
        "blend_by_speed",
        "Frame Blend (#1051) that follows each event's speed: gives every video event on the track (or just `event`) its own "
        "subclip with Frame Blend as Media FX, frames = capture_fps / project fps x speed (5 at normal speed on 60 fps, 2 at "
        "2.5x slow, none at 5x slow). Adds a take; the old take stays.",
        {"track": TRACK, "event": EVENT, "capture_fps": {"type": "number"}, "effect": {"type": "string"}},
        ["track"],
    ),
    tool(
        "list_render_templates",
        "Render templates, filtered by renderer (file type) and template name parts, fps, width, height.",
        {
            "renderer": {"type": "string"},
            "template": {"type": "string"},
            "fps": {"type": "number"},
            "width": {"type": "integer"},
            "height": {"type": "integer"},
            "limit": {"type": "integer"},
        },
    ),
    tool(
        "render",
        "Render a range of the timeline to a new file inside the work folder with a template (exact name preferred, else "
        "the first containing it). Never overwrites. `files` lists what was written (an Image Sequence writes numbered "
        "files, name_000000.png and on). The time limit is an hour unless timeout_seconds.",
        {
            "path": {"type": "string"},
            "template": {"type": "string"},
            "renderer": {"type": "string"},
            "fps": {"type": "number"},
            "width": {"type": "integer"},
            "height": {"type": "integer"},
            "start": SECONDS,
            "length": SECONDS,
        },
        ["path", "template"],
    ),
    tool(
        "snapshot",
        "See the project at a timeline time: the composited frame (every track and FX, at Best quality and full size), "
        "returned inline as an image and saved as a PNG under <work folder>\\snapshots (never overwriting). Use it to check "
        "an edit: a clip in place, an effect's look, a black gap. Leaves the cursor, preview settings and project as they "
        "were. A time at or past the project's end is an error.",
        {
            "time": SECONDS,
            "times": {"type": "array", "items": {"type": "number"}, "description": "several times (up to 10), one PNG each, instead of time"},
            "width": {"type": "integer", "description": "scale down to this width, aspect kept; default 1280, 0 = the project's full size; never scaled up"},
            "folder": {"type": "string", "description": "the work folder or a folder inside it; default <work folder>\\snapshots"},
            "inline": {"type": "boolean", "description": "default true: also return each frame as an image (the PNG under 1 MB, else a JPEG of it; at most 2000 px wide; 4 MB a call)"},
        },
    ),
    tool(
        "run_script",
        "Run a C# VEGAS script (.cs) from the work folder inside VEGAS, for anything the other tools do not cover. It is "
        "compiled first as VEGAS compiles it (C# 5, no System.Core: no LINQ or HashSet); errors come back and nothing runs.",
        {"path": {"type": "string"}, "script_args": {"type": "string"}},
        ["path"],
    ),
] + detail_tools.TOOLS

TIMEOUT_ARGUMENT = "timeout_seconds"
for _t in TOOLS[len(LOCAL_TOOLS):]:
    _t["inputSchema"]["properties"][TIMEOUT_ARGUMENT] = {
        "type": "number",
        "description": "how long to wait for VEGAS; default %d s (%s). Past it the call is an error and that VEGAS "
                       "refuses calls until its reply comes, or until end" % (
                           TIMEOUT_SECONDS.get(_t["name"], DEFAULT_TIMEOUT_SECONDS),
                           "at least wait_seconds + %d" % IMPORT_EDL_MARGIN_SECONDS if _t["name"] == "import_edl" else "the default"),
    }


def time_limit(cmd, args):
    """How many seconds a bridge call may take: timeout_seconds (at most
    MAX_TIMEOUT_SECONDS), else the command's default. Raises on a value that
    is not a finite number above 0."""
    if args.get(TIMEOUT_ARGUMENT) is not None:
        try:
            limit = float(args[TIMEOUT_ARGUMENT])
        except (TypeError, ValueError, OverflowError):
            limit = 0
        if not 0 < limit < float("inf"):
            raise BridgeUnavailable("%s must be a number of seconds above 0, got %r" % (TIMEOUT_ARGUMENT, args[TIMEOUT_ARGUMENT]))
        return min(limit, MAX_TIMEOUT_SECONDS)
    limit = float(TIMEOUT_SECONDS.get(cmd, DEFAULT_TIMEOUT_SECONDS))
    if cmd == "import_edl":
        try:
            limit = max(limit, float(args.get("wait_seconds") or 0) + IMPORT_EDL_MARGIN_SECONDS)
        except (TypeError, ValueError):
            pass  # the bridge reports a bad wait_seconds itself
    return min(limit, MAX_TIMEOUT_SECONDS)


class CurrentGone(BridgeUnavailable):
    """The current VEGAS has gone. `clean_up` (slow: the pipe, its autosave, a
    crash box) is left to the caller, to run without holding Instances.lock."""

    def __init__(self, pid, clean_up):
        BridgeUnavailable.__init__(self, "the bridge VEGAS (PID %d) is gone; call launch for a new one" % pid)
        self.pid = pid
        self.clean_up = clean_up


class Instances:
    """The bridge VEGAS processes: the ones this server started, and which one tools go to.

    Tool calls run on threads of their own, so `lock` guards this state; it is
    never held while a bridge call waits on VEGAS, so `end` and the shutdown
    go ahead while one is stuck."""

    def __init__(self):
        self.lock = threading.RLock()
        # pid -> vegas_instances.Launched: the VEGAS this server started. Their
        # open process handles keep those PIDs from going to another process.
        self.launched = {}
        self.current = None
        self.bridges = {}
        # Set when the current VEGAS went away: the next bridge call is refused
        # until launch or use picks one, so a retried command never lands in
        # another VEGAS's project.
        self.lost = None
        # What end is ending right now (PIDs), and the connections it has
        # ended: a call that fails because end ended its VEGAS says so, and
        # neither cleans up after it again nor marks it lost.
        self.ending = set()
        self.ended_bridges = weakref.WeakSet()

    def bridge(self, pid):
        with self.lock:
            if pid not in self.bridges:
                self.bridges[pid] = Bridge(pid)
            return self.bridges[pid]

    def target(self):
        """The PID tools go to now. Raises when there is none to pick safely;
        CurrentGone when the current one has gone, leaving its clean-up to the
        caller. Call holding lock."""
        if self.lost is not None:
            raise BridgeUnavailable("the bridge VEGAS (PID %d) has gone: call launch, or use to pick another" % self.lost)
        live = vegas_instances.bridge_pids()
        if self.current in live:
            return self.current
        if self.current is not None:
            gone = self.current
            raise CurrentGone(gone, self._forget_later(gone))
        # Only a VEGAS this server started is picked without asking: another
        # bridge VEGAS belongs to another session or script, and its project
        # is theirs, so reaching it takes an explicit use.
        ours = [p for p in live if p in self.launched and p not in self.ending]
        if len(ours) == 1:
            self.current = ours[0]
            return self.current
        if ours:
            raise BridgeUnavailable("this server started several bridge VEGAS (PIDs %s): pick one with use" % ours)
        if live:
            raise BridgeUnavailable(
                "no bridge VEGAS started by this server: call launch for one (bridge VEGAS started elsewhere, "
                "PIDs %s, are reached only with use)" % live
            )
        raise BridgeUnavailable("no bridge VEGAS is running: call launch first")

    def forget(self, pid):
        """Drop a VEGAS that has gone and mark it lost; when this server started
        it, clean up after it (autosave, crash box), going by its own handle.
        Returns what it did."""
        return self._forget_later(pid)()

    def _forget_later(self, pid):
        """forget's bookkeeping, done now under the lock, and its slow part
        (closing the pipe, the autosave, PowerShell for a crash box) returned
        as a function to run outside it, so other calls and end do not wait
        on that. A VEGAS that end is ending is end's to clean up."""
        with self.lock:
            if pid in self.ending:
                return lambda: "end is ending it"
            b = self.bridges.pop(pid, None)
            if self.current == pid:
                self.current = None
                self.lost = pid
            launched = self.launched.get(pid)
            ours_gone = launched is not None and launched.exited()
            if ours_gone:
                del self.launched[pid]

        def clean_up():
            if b:
                b.close()
            if not ours_gone:
                return "nothing to clean up"
            done = vegas_instances.cleanup(pid)
            launched.close_desktop()
            return done

        return clean_up

    def call(self, cmd, args):
        args = dict(args or {})
        limit = time_limit(cmd, args)
        args.pop(TIMEOUT_ARGUMENT, None)
        try:
            with self.lock:
                pid = self.target()
                bridge = self.bridge(pid)
        except CurrentGone as gone:
            raise BridgeUnavailable("the bridge VEGAS (PID %d) is gone (%s); call launch for a new one" % (gone.pid, gone.clean_up()))
        try:
            reply = bridge.call(cmd, args, limit)
        except BridgeUnavailable as e:
            if isinstance(e, BridgeTimeoutOrBusy):
                raise  # still running: nothing to clean up
            with self.lock:
                ended = bridge in self.ended_bridges or pid in self.ending
                launched = self.launched.get(pid)
            if ended:
                raise BridgeUnavailable("%s: end ended that VEGAS while %s ran" % (e, cmd))
            exited = launched.exited() if launched is not None else not vegas_instances.is_running(pid)
            if exited:
                raise BridgeUnavailable("%s; it has exited (%s)" % (e, self.forget(pid)))
            raise
        late = bridge.take_late()
        if late:
            reply["earlier_call"] = late
        return reply

    def launch(self, args):
        # A VEGAS of ours that died since is cleaned up first, so its own
        # autosave does not read as a crashed session's and block this launch.
        with self.lock:
            gone = [p for p, one in self.launched.items() if one.exited()]
        for pid in gone:
            self.forget(pid)
        # Owned by this server: if it is killed outright, the VEGAS ends itself.
        info, launched = vegas_instances.launch(
            args.get("work_folder"), args.get("wait_seconds", vegas_instances.DEFAULT_WAIT_SECONDS),
            bool(args.get("minimised", True)), owned=True,
        )
        with self.lock:
            self.launched[launched.pid] = launched
            self.current = launched.pid
            self.lost = None
        beside = []
        if info["users_vegas"]:
            beside.append("the user's VEGAS (PID %s)" % ", ".join(str(p) for p in info["users_vegas"]))
        if info["other_bridge_vegas"]:
            beside.append("other bridge VEGAS (PID %s)" % ", ".join(str(p) for p in info["other_bridge_vegas"]))
        info["note"] = (
            "started a VEGAS for the bridge (PID %d), %s" % (
                info["pid"], "on a hidden desktop" if launched.desktop is not None else "shown on the user's desktop")
            + (" beside " + " and ".join(beside) if beside else "")
            + "; tools now go to this one"
        )
        return info

    def end(self, args):
        with self.lock:
            pid = pid_argument(args) if "pid" in args else self.current
            if pid is None:
                raise BridgeUnavailable("no current bridge VEGAS: pass pid (instances lists them)")
            launched = self.launched.get(pid)
            b = self.bridges.get(pid)
            # end wins over a call it cuts short: that call neither cleans up
            # again nor marks the VEGAS lost, and no call picks it meanwhile,
            # so the next one goes to another VEGAS this server started.
            self.ending.add(pid)
            if b is not None:
                self.ended_bridges.add(b)
            was_current = self.current == pid
            if was_current:
                self.current = None
        # Not under the lock, and never waiting on the pipe: a call stuck in
        # that VEGAS ends with it (its pipe closes and the read returns).
        try:
            if launched is not None:
                result = launched.end()  # raises while it still runs: the record stays, so end_all retries it
            else:
                # Checked while this server's connection still holds the pipe, so
                # the listing cannot miss it between clients.
                result = vegas_instances.end_bridge_pid(pid)
        except BaseException:
            with self.lock:
                self.ending.discard(pid)
                if b is not None:
                    self.ended_bridges.discard(b)
                if was_current and self.current is None:
                    self.current = pid
            raise
        with self.lock:
            self.launched.pop(pid, None)
            if self.bridges.get(pid) is b:
                self.bridges.pop(pid, None)
            self.ending.discard(pid)
        if b:
            b.close()
        return result

    def describe(self):
        bridged = vegas_instances.bridge_pids()
        others = [p for p in vegas_instances.vegas_pids() if p not in bridged]
        with self.lock:
            busy = {pid: b.busy() for pid, b in self.bridges.items()}
            return {
                "bridge_vegas": bridged,
                "current": self.current,
                "started_by_this_server": sorted(self.launched),
                "busy": {str(pid): what for pid, what in busy.items() if what},
                "other_vegas": others,
            }

    def use(self, args):
        pid = pid_argument(args)
        if pid not in vegas_instances.bridge_pids():
            raise BridgeUnavailable("PID %d has no bridge pipe (instances lists the bridge VEGAS)" % pid)
        with self.lock:
            self.current = pid
            self.lost = None
        return {"current": pid}

    def end_all(self):
        """On shutdown: end every VEGAS this server started, so none stays on
        the user's screen. All are terminated first and cleaned up after, so a
        shutdown cut short leaves as few running as possible. Never waits on a
        pipe, so a call stuck in one of them does not hold it up."""
        with self.lock:
            launched = list(self.launched.values())
            self.launched.clear()
        for one in launched:
            try:
                if not one.exited():
                    one.proc.kill()
            except OSError as e:
                # Already gone, or going: one.end() below tries again and reports.
                log("end_all: terminating VEGAS PID %d: %s" % (one.pid, e))
        for one in launched:
            try:
                log("end_all: %s" % one.end())
            except Exception as e:  # noqa: BLE001 - best effort on the way out; the next one still gets ended
                log("end_all: ending VEGAS PID %d failed: %s: %s" % (one.pid, type(e).__name__, e))

    def run_local(self, name, args):
        if name == "launch":
            return self.launch(args)
        if name == "end":
            return self.end(args)
        if name == "instances":
            return self.describe()
        if name == "use":
            return self.use(args)
        raise KeyError(name)


def pid_argument(args):
    """The pid argument as an int, or an error naming it."""
    try:
        return int(args["pid"])
    except (KeyError, TypeError, ValueError):
        raise BridgeUnavailable("pid must be a process id (instances lists them), got %r" % args.get("pid"))


_send_lock = threading.Lock()


def send(msg):
    # ASCII-only JSON (json.dumps escapes the rest), so stdout's code page never matters.
    # One line at a time: replies come from the tool calls' own threads.
    with _send_lock:
        sys.stdout.write(json.dumps(msg) + "\n")
        sys.stdout.flush()


def log(line):
    """A line for whoever reads the server's stderr (Claude Code's MCP log);
    stdout carries only JSON-RPC."""
    try:
        sys.stderr.write("dodstudio-vegas: %s\n" % line)
        sys.stderr.flush()
    except (OSError, ValueError, AttributeError):
        pass  # no stderr (closed, or pythonw): nothing to tell


def serve():
    instances = Instances()
    try:
        serve_lines(instances)
    finally:
        instances.end_all()


def tool_result(value, ok=True):
    text = value if isinstance(value, str) else json.dumps(value, indent=1)
    return {"content": [{"type": "text", "text": text}], "isError": not ok}


def call_tool(instances, name, arguments):
    try:
        if name in LOCAL_NAMES:
            return tool_result(instances.run_local(name, arguments))
        reply = instances.call(name, arguments)
        ok = reply.get("ok")
        body = reply.get("result") if ok else reply.get("error")
        images = []
        if ok and name == "snapshot" and arguments.get("inline", True) is not False:
            images = snapshot_images(body)
        if reply.get("dialogs") or reply.get("earlier_call"):
            # Dialogs VEGAS raised during the call and how the bridge answered
            # them, and how an earlier call that ran past its limit ended.
            body = {"result" if ok else "error": body}
            if reply.get("dialogs"):
                body["dialogs_answered"] = reply["dialogs"]
            if reply.get("earlier_call"):
                body["earlier_call"] = reply["earlier_call"]
        result = tool_result(body, ok)
        result["content"].extend(images)
        return result
    except (BridgeUnavailable, vegas_instances.VegasError) as e:
        return tool_result(str(e), False)
    except Exception as e:  # noqa: BLE001 - one bad call must not end the server, which ends every VEGAS it started
        return tool_result("%s failed: %s: %s" % (name, type(e).__name__, e), False)


def snapshot_images(body):
    """The image blocks for a snapshot reply, taken out of its text; a frame
    left out says why in its `not_inline`."""
    if not isinstance(body, dict):
        return []
    return snapshot_content.image_blocks(body.get("snapshots") or [])


LOCAL_NAMES = {t["name"] for t in LOCAL_TOOLS}


def negotiated_protocol(requested):
    """The protocol version to answer initialize with: the client's when this
    server speaks it, else this server's own (the client then decides)."""
    return requested if requested in SUPPORTED_PROTOCOLS else PROTOCOL


def answer_call(instances, mid, name, arguments):
    send({"jsonrpc": "2.0", "id": mid, "result": call_tool(instances, name, arguments)})


def serve_lines(instances, lines=None):
    """Answer JSON-RPC lines from stdin (or `lines`, for tests). A tool call
    runs on a thread of its own, so this loop always reads on: `end` reaches
    a VEGAS while a call to it is stuck, and the end of stdin is seen at once.
    With `lines`, waits for every call before returning."""
    names = {t["name"] for t in TOOLS}
    calls = []
    # MCP clients send UTF-8; Python reads a piped stdin in the ANSI code page
    # (before 3.15), which mangles any non-ASCII path.
    # errors="replace": a stray invalid byte spoils one line, never the server.
    for raw in lines if lines is not None else io.TextIOWrapper(sys.stdin.buffer, encoding="utf-8", errors="replace"):
        raw = raw.strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except ValueError:
            msg = None
        if not isinstance(msg, dict):
            send({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error: not a JSON-RPC object"}})
            continue
        method = msg.get("method")
        mid = msg.get("id")
        if mid is None:
            continue  # notifications (initialized, cancelled) need no reply
        if method == "initialize":
            result = {
                "protocolVersion": negotiated_protocol((msg.get("params") or {}).get("protocolVersion")),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "dodstudio-vegas", "version": SERVER_VERSION},
            }
        elif method == "ping":
            result = {}
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            params = msg.get("params") or {}
            name = params.get("name")
            if name not in names:
                send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": "unknown tool %s" % name}})
                continue
            worker = threading.Thread(target=answer_call, args=(instances, mid, name, params.get("arguments") or {}),
                                      daemon=True, name="tool %s" % name)
            worker.start()
            if lines is not None:
                calls.append(worker)
            continue
        else:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found: %s" % method}})
            continue
        send({"jsonrpc": "2.0", "id": mid, "result": result})
    if lines is not None:
        for worker in calls:
            worker.join()


USAGE = """usage:
    vegas_mcp.py                               MCP server on stdin/stdout
    vegas_mcp.py launch [work-folder]          start a bridge VEGAS (hidden; runs until end); prints its pid
    vegas_mcp.py instances                     every VEGAS, and which have a bridge
    vegas_mcp.py call --pid N <cmd> [json]     one bridge command
    vegas_mcp.py end <pid>                     end a bridge VEGAS and remove its autosave"""


def main(argv):
    """The shell commands. A VEGAS started with `launch` here keeps running until `end`."""
    if len(argv) < 2:
        serve()
        return 0
    verb, rest = argv[1], argv[2:]
    try:
        if verb == "launch" and len(rest) <= 1:
            info, _ = vegas_instances.launch(rest[0] if rest else None)
            print(json.dumps(info, indent=1))
            return 0
        if verb == "end" and len(rest) == 1:
            print(vegas_instances.end_bridge_pid(pid_argument({"pid": rest[0]})))
            return 0
        if verb == "instances" and not rest:
            print(json.dumps(Instances().describe(), indent=1))
            return 0
        if verb == "call" and len(rest) in (3, 4) and rest[0] == "--pid":
            # A shell call names its VEGAS: no guessing which bridge VEGAS is meant.
            instances = Instances()
            instances.current = pid_argument({"pid": rest[1]})
            try:
                args = json.loads(rest[3]) if len(rest) == 4 else {}
            except ValueError as e:
                raise BridgeUnavailable("the arguments are not JSON (%s): %s" % (e, rest[3]))
            reply = instances.call(rest[2], args)
            if rest[2] == "snapshot" and reply.get("ok"):
                snapshot_images(reply["result"])  # a shell shows the paths, not pages of base64
            print(json.dumps(reply, indent=1))
            return 0 if reply.get("ok") else 1
    except (BridgeUnavailable, vegas_instances.VegasError) as e:
        print(str(e), file=sys.stderr)
        return 2
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
