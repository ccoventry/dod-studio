"""Tool schemas for reading and setting a project's finer detail (#1050):
effect keyframes, Pan/Crop, Track Motion, fades and transitions, markers,
envelopes and event groups. The bridge commands of the same names are in
extension/Keyframes.cs, PanCrop.cs, TrackMotion.cs, Transitions.cs,
Markers.cs, Envelopes.cs and EventGroups.cs.

Every set_* tool is one undo step in VEGAS and is rolled back whole when it
fails part-way. Keyframe lists share one rule: mode "replace" (the default)
makes the list exactly what is given, "add" adds to it, replacing a keyframe
at the same time; VEGAS always keeps a keyframe at time 0, which takes the
first given keyframe's values when none is given there.
"""

from tool_schema import EVENT, MEDIA, SECONDS, TARGET, tool

MODE = {
    "type": "string",
    "enum": ["replace", "add"],
    "description": "replace (default): exactly these keyframes; add: add them, replacing any at the same time",
}
VIDEO_INTERPOLATION = {
    "type": "string",
    "enum": ["Linear", "Hold", "Slow", "Fast", "Smooth", "Sharp"],
    "description": "how this keyframe moves to the next (default Linear)",
}
CURVE = {
    "type": "string",
    "enum": ["Linear", "Fast", "Slow", "Smooth", "Sharp", "Hold"],
    "description": "Hold = no change until the next point",
}
POINT = {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[x, y]"}
TRACK_REQUIRED = {"type": "integer", "description": "0-based track index (project_info)"}
EFFECT_ARGS = {
    "target": TARGET,
    "track": {"type": "integer", "description": "0-based track index (target event or track)"},
    "event": {"type": "integer", "description": "0-based event index on that track (target event)"},
    "media": MEDIA,
    "effect_index": {"type": "integer", "description": "the effect's place in that chain (project_info)"},
}

TOOLS = [
    tool(
        "get_keyframes",
        "Read an OFX effect's parameter keyframes (time, value, interpolation) on any FX chain. Times are in the "
        "chain's own time: Event FX from the event's start (whatever its speed or offset), Media FX from the media's "
        "start, Track FX and Video Output FX from the project's start. Lists the animated parameters, or `param`, or "
        "every parameter that can animate with all=true.",
        dict(EFFECT_ARGS, **{
            "param": {"type": "string", "description": "one parameter, by name or label"},
            "all": {"type": "boolean", "description": "every parameter that can animate, keyframed or not"},
        }),
        ["effect_index"],
    ),
    tool(
        "set_keyframes",
        "Keyframe one OFX parameter of an effect on any FX chain (times as get_keyframes: Event FX from the event's "
        "start, and no later than its end). mode replace (default) makes its keyframes exactly these, add adds them "
        "(a parameter not animated yet keeps its static value at 0), clear removes them all (leaving `value`, if "
        "given, as its static value). Legacy (non-OFX) effects are refused.",
        dict(EFFECT_ARGS, **{
            "param": {"type": "string", "description": "parameter name or label"},
            "keyframes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "time": SECONDS,
                        "value": {"description": "number, bool, string, choice name, [x, y] / [x, y, z] for points, "
                                                 "[r, g, b(, a)] 0-1 for colours"},
                        "interpolation": {"type": "string", "enum": ["Linear", "Fast", "Slow", "Smooth", "Sharp", "Hold", "Manual"],
                                          "description": "how this keyframe moves to the next (VEGAS's default Linear)"},
                    },
                    "required": ["time", "value"],
                },
            },
            "mode": {"type": "string", "enum": ["replace", "add", "clear"]},
            "value": {"description": "with mode clear: the static value it keeps"},
        }),
        ["effect_index", "param"],
    ),
    tool(
        "get_pan_crop",
        "Read a video event's Pan/Crop keyframes: time (s from the event's start), bounds (the four corners of the part "
        "of the source shown: top-left, top-right, bottom-right, bottom-left, in the source media's own pixels, y down), "
        "center, width, height, zoom (vs the default frame: the whole media, the box VEGAS gives a new event), rotation "
        "(degrees; positive turns the picture anticlockwise), smoothness, interpolation.",
        {"track": TRACK_REQUIRED, "event": EVENT},
        ["track", "event"],
    ),
    tool(
        "set_pan_crop",
        "Set a video event's Pan/Crop (times in s from the event's start, up to its end; positions in the source media's "
        "pixels, y down). Each keyframe gives one of bounds, rect, zoom (with an optional center), or center alone "
        "(moves the box, keeping its size), plus rotation, smoothness, interpolation. Shortcuts for a whole move in one call: zoom (into a point, optionally animated) and pan "
        "(between two points). reset puts back the default frame. Returns get_pan_crop.",
        {
            "track": TRACK_REQUIRED,
            "event": EVENT,
            "keyframes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "time": SECONDS,
                        "zoom": {"type": "number", "description": "2 = twice as large (a box half the default frame's size)"},
                        "center": dict(POINT, description="[x, y] the middle of the shown box, source pixels; with zoom "
                                                          "its default is the media's middle, alone it moves the box as it is"),
                        "rect": {"type": "object", "description": "{x, y, width, height}: the shown box's top-left and size",
                                 "properties": {"x": {"type": "number"}, "y": {"type": "number"},
                                                "width": {"type": "number"}, "height": {"type": "number"}}},
                        "bounds": {"type": "array", "items": POINT, "minItems": 4, "maxItems": 4,
                                   "description": "four corners: top-left, top-right, bottom-right, bottom-left"},
                        "rotation": {"type": "number", "description": "degrees; positive turns the picture anticlockwise"},
                        "smoothness": {"type": "number"},
                        "interpolation": VIDEO_INTERPOLATION,
                    },
                    "required": ["time"],
                },
            },
            "zoom": {
                "type": "object",
                "description": "zoom to `scale` at `center`: held from `start` (default 0; before it the default frame, "
                               "zoom 1, holds), or with `end` moving there from from_scale (default 1) at from_center "
                               "(default the middle)",
                "properties": {"scale": {"type": "number"}, "center": POINT, "start": SECONDS, "end": SECONDS,
                               "from_scale": {"type": "number"}, "from_center": POINT, "interpolation": VIDEO_INTERPOLATION},
                "required": ["scale"],
            },
            "pan": {
                "type": "object",
                "description": "move the centre from `from` at `start` (default 0) to `to` at `end` (default the event's end), at zoom `scale` (default 1)",
                "properties": {"from": POINT, "to": POINT, "start": SECONDS, "end": SECONDS, "scale": {"type": "number"},
                               "interpolation": VIDEO_INTERPOLATION},
                "required": ["from", "to"],
            },
            "mode": MODE,
            "reset": {"type": "boolean", "description": "back to the default frame (one keyframe) before anything else"},
        },
        ["track", "event"],
    ),
    tool(
        "get_track_motion",
        "Read a video track's Track Motion: keyframes (time in project seconds, x and y in project pixels from the frame's "
        "centre with y up, width and height in project pixels, rotation in degrees, smoothness, interpolation), and its "
        "2D shadow and glow (enabled, keyframes with blur, intensity, color).",
        {"track": TRACK_REQUIRED},
        ["track"],
    ),
    tool(
        "set_track_motion",
        "Set a video track's Track Motion keyframes (time in project seconds; x, y from the frame's centre, y up, project "
        "pixels; width/height or scale 1 = the whole frame; rotation degrees). A field a keyframe leaves out keeps the "
        "track's value there. shadow and glow take {enabled, keyframes, mode}. Returns get_track_motion.",
        {
            "track": TRACK_REQUIRED,
            "keyframes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "time": SECONDS,
                        "x": {"type": "number"},
                        "y": {"type": "number"},
                        "width": {"type": "number"},
                        "height": {"type": "number"},
                        "scale": {"type": "number", "description": "width and height as a share of the frame's (0.5 = half size)"},
                        "rotation": {"type": "number", "description": "degrees; positive turns the picture clockwise"},
                        "smoothness": {"type": "number"},
                        "interpolation": VIDEO_INTERPOLATION,
                    },
                    "required": ["time"],
                },
            },
            "mode": MODE,
            "reset": {"type": "boolean", "description": "back to one keyframe at the whole frame before anything else"},
            "shadow": {"type": "object", "description": "{enabled, keyframes (as above plus blur, intensity, color [r, g, b, a] "
                                                        "0-1), mode, reset (back to the one keyframe VEGAS keeps)}"},
            "glow": {"type": "object", "description": "{enabled, keyframes (as above plus blur, intensity, color [r, g, b, a] "
                                                      "0-1), mode, reset (back to the one keyframe VEGAS keeps)}"},
        },
        ["track"],
    ),
    tool("list_transitions", "Installed video transitions (name, unique id, OFX or not), filtered by part of a name or id.",
         {"filter": {"type": "string"}}),
    tool(
        "set_fade",
        "Set an event's fade in or out: length (timeline s), curve, reciprocal_curve (the curve of the other event in a "
        "crossfade), and on a video event the transition it plays (name or id from list_transitions, \"none\" removes "
        "it), with a preset and OFX params. Returns both fades.",
        {
            "track": TRACK_REQUIRED,
            "event": EVENT,
            "edge": {"type": "string", "enum": ["in", "out"]},
            "length": SECONDS,
            "curve": CURVE,
            "reciprocal_curve": CURVE,
            "transition": {"type": "string"},
            "preset": {"type": "string", "description": "a preset of that transition, by name"},
            "params": {"type": "object", "description": "OFX parameter name or label -> value"},
        },
        ["track", "event", "edge"],
    ),
    tool(
        "crossfade",
        "Crossfade an event with the one before it on its track over `length` s: the later event's start moves so they "
        "overlap by that much, its picture staying where it was on the timeline (it needs that much media before its "
        "first frame), the earlier fades out (curve_out) as the later fades in (curve_in) with an optional transition. "
        "For a clip's audio, crossfade its audio track's events too.",
        {
            "track": TRACK_REQUIRED,
            "event": {"type": "integer", "description": "the later event (0-based on that track)"},
            "length": SECONDS,
            "curve_in": CURVE,
            "curve_out": CURVE,
            "transition": {"type": "string", "description": "name or id from list_transitions; default a plain crossfade"},
            "preset": {"type": "string"},
            "params": {"type": "object"},
        },
        ["track", "event", "length"],
    ),
    tool("get_markers", "The project's markers (time, label), regions (time, length, end, label) and command markers "
         "(time, type, parameter, comment), each with its index, in time order. Times are timeline seconds."),
    tool(
        "set_markers",
        "Add, move, rename or delete markers, regions and command markers in one call (one undo step). ops run in "
        "order, and every index is from the lists as they were before the call (get_markers), since VEGAS keeps them "
        "in time order. Returns get_markers.",
        {
            "ops": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "op": {"type": "string", "enum": ["add", "move", "rename", "delete"]},
                        "kind": {"type": "string", "enum": ["marker", "region", "command"], "description": "default marker"},
                        "index": {"type": "integer", "description": "move, rename, delete: the index get_markers gave before this call"},
                        "time": SECONDS,
                        "length": {"type": "number", "description": "a region's length, seconds"},
                        "label": {"type": "string"},
                        "type": {"type": "string", "description": "command marker type, e.g. TEXT, URL, TITLE (default TEXT)"},
                        "parameter": {"type": "string", "description": "command marker parameter"},
                        "comment": {"type": "string", "description": "command marker comment"},
                    },
                    "required": ["op"],
                },
            },
        },
        ["ops"],
    ),
    tool(
        "get_envelopes",
        "The envelopes on a track (audio: Volume, Pan, Mute; video: Composite level, FadeToColor, Mute) or, with "
        "`event`, on a video event (Velocity). Each has min, max, neutral and points (time, value, curve); times are "
        "project seconds on a track, seconds from the event's start on an event. can_add lists the types it can take. "
        "Audio events have no envelopes in VEGAS's scripting API.",
        {"track": TRACK_REQUIRED, "event": EVENT},
        ["track"],
    ),
    tool(
        "set_envelope",
        "Set an envelope's points on a track or a video event (added if missing): Volume (0-2, 1 = 0 dB), Pan (-1 left "
        "to 1 right), Composite (video track level, 0-1), Velocity (an event's speed), and the other types get_envelopes "
        "lists. mode replace (default) makes the points exactly these, add adds them, remove deletes the envelope.",
        {
            "track": TRACK_REQUIRED,
            "event": {"type": "integer", "description": "a video event on that track, for its Velocity envelope"},
            "type": {"type": "string", "description": "Volume, Pan, Composite, FadeToColor, Mute, Velocity, ..."},
            "points": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {"time": SECONDS, "value": {"type": "number"},
                                   "curve": dict(CURVE, description="from this point to the next; default Linear")},
                    "required": ["time", "value"],
                },
            },
            "mode": {"type": "string", "enum": ["replace", "add", "remove"]},
        },
        ["track", "type"],
    ),
    tool(
        "group_events",
        "Group events so they move together (each leaves any group it was in), or ungroup them. project_info shows each "
        "grouped event's `group`; with detail=true its `groups` lists the members.",
        {
            "action": {"type": "string", "enum": ["group", "ungroup"]},
            "events": {
                "type": "array",
                "items": {"type": "object", "properties": {"track": {"type": "integer"}, "event": {"type": "integer"}},
                          "required": ["track", "event"]},
            },
        },
        ["action", "events"],
    ),
]
