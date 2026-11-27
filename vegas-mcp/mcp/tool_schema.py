"""The pieces every tool schema is built from (#1050): the tool() helper and
the argument schemas several tools share."""


def tool(name, description, properties=None, required=(), closed=False):
    """A tool's schema. closed=True refuses arguments it does not list, for a
    tool whose bridge command refuses them too."""
    schema = {"type": "object", "properties": properties or {}, "required": list(required)}
    if closed:
        schema["additionalProperties"] = False
    return {"name": name, "description": description, "inputSchema": schema}


SECONDS = {"type": "number", "description": "seconds"}
TRACK = {"type": "integer", "description": "0-based track index (project_info)"}
EVENT = {"type": "integer", "description": "0-based event index on that track (project_info)"}
MEDIA = {"type": "string", "description": "media file path, or a subclip's key from project_info"}
TARGET = {
    "type": "string",
    "enum": ["event", "media", "track", "output"],
    "description": "which FX chain: event = the event's Event FX (track+event); media = the Media FX of `media`; "
                   "track = a video track's Track FX (track); output = the project's Video Output FX",
}
