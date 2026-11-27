"""Tool schemas for working on a whole edit (#1050): effect presets, one
change to many events, and comparing two projects. The bridge commands of
the same names are in extension/Presets.cs, Batch.cs and CompareProjects.cs;
compare_projects' comparison itself is project_compare.py, run on the
bridge's reply.
"""

from tool_schema import SECONDS, tool


def closed(description, properties, required=()):
    """An object argument that takes only these keys, as the bridge's
    apply_to_events does (Batch.cs refuses any other)."""
    return {"type": "object", "description": description, "properties": properties, "required": list(required),
            "additionalProperties": False}


FADE = {
    "length": SECONDS, "curve": {"type": "string"}, "reciprocal_curve": {"type": "string"},
    "transition": {"type": "string"}, "preset": {"type": "string"}, "params": {"type": "object"},
}
KIND ={"type": "string", "enum": ["effect", "transition"], "description": "a video effect (default) or a transition"}
PLUGIN = {"type": "string", "description": "name or unique id (list_effects, list_transitions)"}
EFFECT_TARGET = {"type": "string", "enum": ["event", "media"],
                 "description": "event (default): each event's Event FX; media: the Media FX of the media each event plays"}

TOOLS = [
    tool(
        "list_presets",
        "The presets of a video effect or transition by name: VEGAS's own and the ones the user saved, in one list "
        "(VEGAS does not tell them apart). Apply one with add_effect / set_effect_params / set_fade `preset`.",
        {"effect": PLUGIN, "kind": KIND},
        ["effect"],
    ),
    tool(
        "read_preset",
        "What an OFX effect's or transition's preset sets, without changing the project: each parameter's value with the "
        "preset applied, its default, and changed=true where they differ. Read from a throwaway copy that is removed again "
        "(the project stays unmodified). Legacy (non-OFX) plug-ins take presets by name, but their values cannot be read.",
        {"effect": PLUGIN, "preset": {"type": "string", "description": "preset name (list_presets), any case"}, "kind": KIND},
        ["effect", "preset"],
    ),
    tool(
        "apply_to_events",
        "Make the same change to every event a filter picks, in one call and one undo step (a failure rolls it all back): "
        "set_event fields (or shift, seconds to move each), add_effect (with preset and params), set_effect (preset, "
        "params or bypass on every effect of that plug-in already in the chain), remove_effect, pan_crop (set_pan_crop's "
        "arguments, e.g. a zoom), fade_in / fade_out (set_fade's arguments). Each runs that tool for each event, so its "
        "arguments mean the same; effect and Pan/Crop changes skip audio events, and a Media FX change is made once per "
        "media. An unknown key anywhere is refused. Returns what changed on which event, what was skipped and why, and "
        "warnings where a moved or longer event now overlaps one the filter did not pick.",
        {
            "filter": {
                "type": "object",
                "description": "all given must hold; {\"all\": true} (alone) picks every event, and an empty filter is refused",
                "additionalProperties": False,
                "properties": {
                    "all": {"type": "boolean", "enum": [True], "description": "every event"},
                    "track": {"type": "integer"},
                    "tracks": {"type": "array", "items": {"type": "integer"}},
                    "start": {"type": "number", "description": "events overlapping the timeline from here (seconds)"},
                    "end": {"type": "number", "description": "... to here"},
                    "media": {"type": "string", "description": "part of the media file path or subclip key, any case"},
                    "name": {"type": "string", "description": "part of the event's name, any case"},
                    "selected": {"type": "boolean"},
                    "type": {"type": "string", "enum": ["video", "audio"]},
                },
            },
            "set_event": closed("set_event's fields, with shift (seconds to move each event) in place of start", {
                "playback_rate": {"type": "number"}, "start": SECONDS, "shift": SECONDS, "length": SECONDS,
                "offset": {"type": "number", "description": "media seconds"}, "resample": {"type": "string"},
                "name": {"type": "string"}, "mute": {"type": "boolean"}, "locked": {"type": "boolean"},
                "loop": {"type": "boolean"}, "snap_offset": SECONDS}),
            "add_effect": closed("{effect, preset, params, target}", {
                "effect": PLUGIN, "preset": {"type": "string"}, "params": {"type": "object"}, "target": EFFECT_TARGET},
                ["effect"]),
            "set_effect": closed("{effect, preset, params, bypass, target}", {
                "effect": PLUGIN, "preset": {"type": "string"}, "params": {"type": "object"}, "bypass": {"type": "boolean"},
                "target": EFFECT_TARGET}, ["effect"]),
            "remove_effect": closed("{effect, target}: every effect of that plug-in in the chain",
                                    {"effect": PLUGIN, "target": EFFECT_TARGET}, ["effect"]),
            "pan_crop": closed("set_pan_crop's arguments", {
                "keyframes": {"type": "array", "items": {"type": "object"}}, "zoom": {"type": "object"},
                "pan": {"type": "object"}, "mode": {"type": "string", "enum": ["replace", "add"]}, "reset": {"type": "boolean"}}),
            "fade_in": closed("set_fade's arguments", FADE),
            "fade_out": closed("set_fade's arguments", FADE),
        },
        ["filter"],
        closed=True,
    ),
    tool(
        "compare_projects",
        "What differs between two projects: `a` and `b` are .veg paths or \"open\" (the open project). Reports the "
        "project's settings, tracks added/removed/changed (FX, compositing, Track Motion, envelopes), events added, "
        "removed, moved and changed (speed, length, offset, fades and transitions, FX parameters and keyframes, "
        "Pan/Crop, velocity), Video Output FX, markers/regions/command markers, and Media FX; `matched_by` says how "
        "events were paired. Each file is read from a copy in <work folder>\\copies; the open project is reopened "
        "afterwards, so it must be saved (refused with unsaved changes).",
        {
            "a": {"type": "string", "description": "a .veg path, or \"open\""},
            "b": {"type": "string", "description": "a .veg path, or \"open\""},
            "tolerance": {"type": "number", "description": "seconds within which two times are the same; default 0.001"},
        },
        ["a", "b"],
    ),
]
