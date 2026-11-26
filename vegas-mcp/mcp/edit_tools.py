"""Tool schemas for working on a whole edit (#1050): effect presets, one
change to many events, and comparing two projects. The bridge commands of
the same names are in extension/Presets.cs, Batch.cs and CompareProjects.cs;
compare_projects' comparison itself is project_compare.py, run on the
bridge's reply.
"""

from tool_schema import tool

KIND = {"type": "string", "enum": ["effect", "transition"], "description": "a video effect (default) or a transition"}
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
        "arguments mean the same; effect and Pan/Crop changes skip audio events. Returns what changed on which event, and "
        "what was skipped and why.",
        {
            "filter": {
                "type": "object",
                "description": "all given must hold; {} picks every event",
                "properties": {
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
            "set_event": {"type": "object", "description": "set_event's fields (playback_rate, length, offset, name, mute, "
                          "locked, loop, resample, snap_offset, start) or shift"},
            "add_effect": {"type": "object", "description": "{effect, preset, params, target}",
                           "properties": {"effect": PLUGIN, "preset": {"type": "string"}, "params": {"type": "object"},
                                          "target": EFFECT_TARGET},
                           "required": ["effect"]},
            "set_effect": {"type": "object", "description": "{effect, preset, params, bypass, target}",
                           "properties": {"effect": PLUGIN, "preset": {"type": "string"}, "params": {"type": "object"},
                                          "bypass": {"type": "boolean"}, "target": EFFECT_TARGET},
                           "required": ["effect"]},
            "remove_effect": {"type": "object", "description": "{effect, target}: every effect of that plug-in in the chain",
                              "properties": {"effect": PLUGIN, "target": EFFECT_TARGET}, "required": ["effect"]},
            "pan_crop": {"type": "object", "description": "set_pan_crop's arguments (keyframes, zoom, pan, mode, reset)"},
            "fade_in": {"type": "object", "description": "set_fade's arguments (length, curve, transition, preset, params)"},
            "fade_out": {"type": "object", "description": "set_fade's arguments"},
        },
        ["filter"],
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
