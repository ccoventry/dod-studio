"""MCP server for VEGAS Pro (#1050): starts its own VEGAS beside the user's
and forwards tool calls to the DoD Studio bridge extension inside it, over
that VEGAS's named pipe \\\\.\\pipe\\dodstudio-vegas-<pid> (one JSON request
per line, one reply per line). vegas_instances.py starts, finds and ends
those VEGAS processes; the bridge never listens in a VEGAS it did not start.

Standard library only. Speaks MCP's stdio transport (newline-delimited
JSON-RPC 2.0), so no `mcp` package is needed. When Claude Code closes the
server, it ends every VEGAS it started, so none is left on the user's screen.

    python vegas_mcp.py                                  # MCP server on stdin/stdout
    python vegas_mcp.py launch [work-folder]             # start a bridge VEGAS; prints its pid
    python vegas_mcp.py instances                        # every VEGAS, and which have a bridge
    python vegas_mcp.py call [--pid N] <cmd> [json-args] # one bridge command, for scripts and checks
    python vegas_mcp.py end <pid>                        # end a bridge VEGAS and remove its autosave
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # also under python -I

import vegas_instances  # noqa: E402

PROTOCOL = "2025-06-18"

SECONDS = {"type": "number", "description": "seconds"}
TRACK = {"type": "integer", "description": "0-based track index (project_info)"}
EVENT = {"type": "integer", "description": "0-based event index on that track (project_info)"}
MEDIA = {"type": "string", "description": "media file path, or a subclip's key from project_info"}
RESAMPLE = {"type": "string", "enum": ["Smart", "Force", "Disable", "OpticalFlow", "ProjectLevel"]}
TARGET = {
    "type": "string",
    "enum": ["event", "media"],
    "description": "event: the event's own FX chain (track+event); media: the Media FX chain of `media`",
}


def tool(name, description, properties=None, required=()):
    return {
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": properties or {}, "required": list(required)},
    }


PID = {"type": "integer", "description": "a bridge VEGAS's process id (launch, instances)"}

# Handled here, not by the bridge.
LOCAL_TOOLS = [
    tool(
        "launch",
        "Start a VEGAS for the bridge, minimised, beside any VEGAS the user has open (their VEGAS is never touched; the "
        "bridge does not listen in it). Every file it writes and every project it opens stays inside work_folder: a "
        "project from elsewhere is copied in first. Later tools go to this VEGAS. Returns its pid and the user's VEGAS "
        "PIDs; tell the user a second VEGAS was started.",
        {
            "work_folder": {"type": "string", "description": "default: the server's DODSTUDIO_VEGAS_ROOT, else %TEMP%\\dodstudio-vegas"},
            "wait_seconds": {"type": "number", "description": "how long to wait for its bridge; default 120"},
            "minimised": {"type": "boolean", "description": "default true; false shows it (without taking focus) to watch it work"},
        },
    ),
    tool(
        "end",
        "End a bridge VEGAS by its PID (default: the current one), without saving, and remove its autosave so the next "
        "VEGAS does not ask to recover it. Refuses any VEGAS without a bridge, so the user's VEGAS is never ended.",
        {"pid": PID},
    ),
    tool("instances", "Every running VEGAS: which have a bridge (started for it), which is current, and the user's own."),
    tool("use", "Send later tools to another bridge VEGAS.", {"pid": PID}, ["pid"]),
]

TOOLS = LOCAL_TOOLS + [
    tool("ping", "Check the current bridge VEGAS: versions, pid, work folder, open project, dialogs it answered."),
    tool("project_info", "The open project: size, fps, tracks with their events (time, speed, media, FX) and the media pool."),
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
        "Put a media file (imported if needed) or subclip on the timeline as a video event; a new video track unless `track` is given.",
        {
            "media": MEDIA,
            "track": TRACK,
            "start": SECONDS,
            "length": {"type": "number", "description": "timeline seconds; default: the rest of the media at this speed"},
            "offset": {"type": "number", "description": "seconds into the media where the event starts"},
            "playback_rate": {"type": "number", "description": "event speed: 0.2 = 5x slow motion (like ctrl-drag)"},
            "resample": RESAMPLE,
            "name": {"type": "string"},
            "with_audio": {"type": "boolean", "description": "also add the audio on a new audio track, grouped"},
        },
        ["media"],
    ),
    tool(
        "set_event",
        "Change an event: speed (playback_rate), start, length, media offset, resample mode, name.",
        {
            "track": TRACK,
            "event": EVENT,
            "playback_rate": {"type": "number"},
            "start": SECONDS,
            "length": SECONDS,
            "offset": SECONDS,
            "resample": RESAMPLE,
            "name": {"type": "string"},
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
        "Add a video effect to an event (Event FX) or to a media/subclip (Media FX), optionally setting OFX parameters by name or label.",
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
        "Set OFX parameters (or bypass) of the effect at effect_index in an event's or media's FX chain.",
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
        "the first containing it). Never overwrites.",
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
        "run_script",
        "Run a C# VEGAS script (.cs) from the work folder inside VEGAS, for anything the other tools do not cover. It is "
        "compiled first as VEGAS compiles it (C# 5, no System.Core: no LINQ or HashSet); errors come back and nothing runs.",
        {"path": {"type": "string"}, "script_args": {"type": "string"}},
        ["path"],
    ),
]


class BridgeUnavailable(Exception):
    pass


class Bridge:
    """One connection to one bridge VEGAS's pipe."""

    def __init__(self, pid):
        self.pid = pid
        self.path = vegas_instances.pipe_path(pid)
        self.pipe = None
        self.next_id = 0

    def _open(self):
        try:
            self.pipe = open(self.path, "r+b", buffering=0)
        except OSError as e:
            raise BridgeUnavailable("the bridge VEGAS (PID %d) is not reachable at %s (%s)" % (self.pid, self.path, e))

    def close(self):
        if self.pipe is not None:
            try:
                self.pipe.close()
            except OSError:
                pass
            self.pipe = None

    def call(self, cmd, args):
        if self.pipe is None:
            self._open()
        self.next_id += 1
        line = json.dumps({"id": self.next_id, "cmd": cmd, "args": args or {}}) + "\n"
        try:
            self.pipe.write(line.encode("utf-8"))
            reply = self.pipe.readline()
        except OSError:
            reply = b""
        if reply:
            return json.loads(reply.decode("utf-8"))
        # The pipe closed: that VEGAS has gone (crashed, or ended elsewhere).
        # No reconnect to another VEGAS: a command meant for this one never
        # goes anywhere else.
        self.close()
        raise BridgeUnavailable("the bridge VEGAS (PID %d) closed its pipe" % self.pid)


class Instances:
    """The bridge VEGAS processes: the ones this server started, and which one tools go to."""

    def __init__(self):
        self.launched = set()
        self.current = None
        self.bridges = {}

    def bridge(self, pid):
        if pid not in self.bridges:
            self.bridges[pid] = Bridge(pid)
        return self.bridges[pid]

    def target(self):
        live = vegas_instances.bridge_pids()
        if self.current in live:
            return self.current
        if self.current is not None:
            raise BridgeUnavailable(
                "the bridge VEGAS (PID %d) is gone (%s); call launch for a new one"
                % (self.current, self.forget(self.current))
            )
        # Only a VEGAS this server started is picked without asking: another
        # bridge VEGAS belongs to another session or script, and its project
        # is theirs, so reaching it takes an explicit use.
        ours = [p for p in live if p in self.launched]
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
        """Drop a VEGAS that has gone; clean up after it (autosave, crash box) when it was ours."""
        b = self.bridges.pop(pid, None)
        if b:
            b.close()
        if self.current == pid:
            self.current = None
        if pid in self.launched and not vegas_instances.is_running(pid):
            self.launched.discard(pid)
            return vegas_instances.cleanup(pid)
        return "nothing to clean up"

    def call(self, cmd, args):
        pid = self.target()
        try:
            return self.bridge(pid).call(cmd, args)
        except BridgeUnavailable as e:
            if not vegas_instances.is_running(pid):
                raise BridgeUnavailable("%s; it has exited (%s)" % (e, self.forget(pid)))
            raise

    def launch(self, args):
        info = vegas_instances.launch(args.get("work_folder"), args.get("wait_seconds", 120), args.get("minimised", True))
        self.launched.add(info["pid"])
        self.current = info["pid"]
        beside = []
        if info["users_vegas"]:
            beside.append("the user's VEGAS (PID %s)" % ", ".join(str(p) for p in info["users_vegas"]))
        if info["other_bridge_vegas"]:
            beside.append("other bridge VEGAS (PID %s)" % ", ".join(str(p) for p in info["other_bridge_vegas"]))
        info["note"] = (
            "started a VEGAS for the bridge (PID %d), minimised" % info["pid"]
            + (" beside " + " and ".join(beside) if beside else "")
            + "; tools now go to this one"
        )
        return info

    def end(self, args):
        pid = args.get("pid") or self.current
        if pid is None:
            raise BridgeUnavailable("no current bridge VEGAS: pass pid (instances lists them)")
        b = self.bridges.pop(pid, None)
        if b:
            b.close()
        result = vegas_instances.end(pid, launched=pid in self.launched)
        self.launched.discard(pid)
        if self.current == pid:
            self.current = None
        return result

    def list(self):
        bridged = vegas_instances.bridge_pids()
        return {
            "bridge_vegas": bridged,
            "current": self.current,
            "started_by_this_server": sorted(self.launched),
            "other_vegas": [p for p in vegas_instances.vegas_pids() if p not in bridged],
        }

    def use(self, args):
        pid = int(args["pid"])
        if pid not in vegas_instances.bridge_pids():
            raise BridgeUnavailable("PID %d has no bridge pipe (instances lists the bridge VEGAS)" % pid)
        self.current = pid
        return {"current": pid}

    def end_all(self):
        """On shutdown: end every VEGAS this server started, so none stays on the user's screen."""
        for pid in sorted(self.launched):
            try:
                vegas_instances.end(pid, launched=True)
            except Exception:  # noqa: BLE001 - best effort on the way out
                pass
        self.launched.clear()

    def run_local(self, name, args):
        if name == "launch":
            return self.launch(args)
        if name == "end":
            return self.end(args)
        if name == "instances":
            return self.list()
        if name == "use":
            return self.use(args)
        raise KeyError(name)


def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


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
        if reply.get("dialogs"):
            # Dialogs VEGAS raised during the call and how the bridge answered them.
            body = {"result" if ok else "error": body, "dialogs_answered": reply["dialogs"]}
        return tool_result(body, ok)
    except (BridgeUnavailable, vegas_instances.VegasError) as e:
        return tool_result(str(e), False)


LOCAL_NAMES = {t["name"] for t in LOCAL_TOOLS}


def serve_lines(instances):
    names = {t["name"] for t in TOOLS}
    for raw in sys.stdin:
        raw = raw.strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except ValueError:
            send({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}})
            continue
        method = msg.get("method")
        mid = msg.get("id")
        if mid is None:
            continue  # notifications (initialized, cancelled) need no reply
        if method == "initialize":
            requested = (msg.get("params") or {}).get("protocolVersion") or PROTOCOL
            result = {
                "protocolVersion": requested,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "dodstudio-vegas", "version": "0.2"},
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
            result = call_tool(instances, name, params.get("arguments") or {})
        else:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found: %s" % method}})
            continue
        send({"jsonrpc": "2.0", "id": mid, "result": result})


def main(argv):
    """The shell commands. A VEGAS started with `launch` here keeps running until `end`."""
    verb = argv[1] if len(argv) >= 2 else None
    try:
        if verb == "launch":
            print(json.dumps(vegas_instances.launch(argv[2] if len(argv) > 2 else None), indent=1))
            return 0
        if verb == "end":
            print(vegas_instances.end(int(argv[2])))
            return 0
        if verb == "instances":
            print(json.dumps(Instances().list(), indent=1))
            return 0
        if verb == "call":
            rest = argv[2:]
            instances = Instances()
            if len(rest) >= 2 and rest[0] == "--pid":
                instances.current = int(rest[1])
                rest = rest[2:]
            else:
                # A one-off shell call starts nothing, so it takes the only
                # bridge VEGAS when there is exactly one.
                live = vegas_instances.bridge_pids()
                if len(live) != 1:
                    raise BridgeUnavailable("%d bridge VEGAS running (PIDs %s): pass --pid" % (len(live), live))
                instances.current = live[0]
            args = json.loads(rest[1]) if len(rest) > 1 else {}
            reply = instances.call(rest[0], args)
            print(json.dumps(reply, indent=1))
            return 0 if reply.get("ok") else 1
    except (BridgeUnavailable, vegas_instances.VegasError) as e:
        print(str(e), file=sys.stderr)
        return 2
    serve()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
