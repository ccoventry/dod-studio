"""MCP server for VEGAS Pro (#1050): forwards tool calls to the DoD Studio
bridge extension running inside VEGAS, over the named pipe
\\\\.\\pipe\\dodstudio-vegas (one JSON request per line, one reply per line).

Standard library only. Speaks MCP's stdio transport (newline-delimited
JSON-RPC 2.0), so no `mcp` package is needed.

    python vegas_mcp.py                         # MCP server on stdin/stdout
    python vegas_mcp.py call <cmd> [json-args]  # one bridge command, for scripts and checks
"""

import json
import sys

PIPE = "\\\\.\\pipe\\dodstudio-vegas"
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


TOOLS = [
    tool("ping", "Check that VEGAS is running with the bridge; returns versions and the open project's path."),
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
        "Open a .veg project. Refuses when the open one has unsaved changes unless discard_changes.",
        {"path": {"type": "string"}, "discard_changes": {"type": "boolean"}},
        ["path"],
    ),
    tool(
        "save_as",
        "Save the project to a NEW .veg path. Never overwrites a file this bridge session did not save itself (overwrite=true for those).",
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
        "Render a range of the timeline to a new file with a template (exact name preferred, else the first containing it). Never overwrites.",
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
        "Run a VEGAS script file (.cs/.js/.vb) inside VEGAS, for anything the other tools do not cover.",
        {"path": {"type": "string"}, "script_args": {"type": "string"}},
        ["path"],
    ),
]


class BridgeUnavailable(Exception):
    pass


class Bridge:
    """One connection to the pipe, reopened when VEGAS restarts."""

    def __init__(self):
        self.pipe = None
        self.next_id = 0

    def _open(self):
        try:
            self.pipe = open(PIPE, "r+b", buffering=0)
        except OSError as e:
            raise BridgeUnavailable(
                "VEGAS bridge not reachable at %s (%s). Is VEGAS running with DodStudioVegasBridge.dll in "
                "%%APPDATA%%\\VEGAS Pro\\21.0\\Application Extensions?" % (PIPE, e)
            )

    def call(self, cmd, args):
        for attempt in (0, 1):
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
            # The pipe closed (VEGAS restarted): reconnect once.
            try:
                self.pipe.close()
            except OSError:
                pass
            self.pipe = None
            if attempt == 1:
                raise BridgeUnavailable("VEGAS closed the bridge connection")
        return None


def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def serve():
    bridge = Bridge()
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
                "serverInfo": {"name": "dodstudio-vegas", "version": "0.1"},
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
            try:
                reply = bridge.call(name, params.get("arguments") or {})
                ok = reply.get("ok")
                text = json.dumps(reply.get("result") if ok else reply.get("error"), indent=1)
                result = {"content": [{"type": "text", "text": text}], "isError": not ok}
            except BridgeUnavailable as e:
                result = {"content": [{"type": "text", "text": str(e)}], "isError": True}
        else:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found: %s" % method}})
            continue
        send({"jsonrpc": "2.0", "id": mid, "result": result})


def main(argv):
    if len(argv) >= 2 and argv[1] == "call":
        args = json.loads(argv[3]) if len(argv) > 3 else {}
        try:
            reply = Bridge().call(argv[2], args)
        except BridgeUnavailable as e:
            print(str(e), file=sys.stderr)
            return 2
        print(json.dumps(reply, indent=1))
        return 0 if reply.get("ok") else 1
    serve()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
