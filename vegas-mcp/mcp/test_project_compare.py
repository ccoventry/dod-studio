"""Tests for compare_projects' comparison (project_compare.py), no VEGAS needed (#1050).

    python -I -m unittest discover -s vegas-mcp/mcp -p "test_*.py"
"""

import copy
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import project_compare  # noqa: E402
import vegas_mcp  # noqa: E402

CLIP = "D:\\media\\clip300.mp4"
OTHER = "D:\\media\\other.mov"


def effect(name, params, keyframes=None):
    e = {"index": 0, "name": name, "id": "{Svfx:test:%s}" % name.lower(), "bypass": False,
         "params": [{"name": k, "label": k, "type": "Double", "value": v} for k, v in params.items()]}
    if keyframes:
        e["keyframes"] = keyframes
    return e


def event(index, start, length=2.0, media=CLIP, rate=1.0, offset=0.0, **more):
    e = {"index": index, "name": "", "start": start, "length": length, "playback_rate": rate, "selected": False,
         "media": media, "media_path": media, "offline": False, "offset": offset, "resample": "Disable",
         "effects": [], "snap_offset": 0.0, "fade_in": {"length": 0.0, "curve": "Linear", "reciprocal_curve": "Linear"},
         "fade_out": {"length": 0.0, "curve": "Linear", "reciprocal_curve": "Linear"}, "envelopes": [],
         "pan_crop": [{"time": 0.0, "center": [640.0, 360.0], "zoom": 1.0, "rotation": 0.0, "interpolation": "Linear"}]}
    e.update(more)
    return e


def track(index, events, name=None, kind="video"):
    t = {"index": index, "type": kind, "name": name, "events": events, "envelopes": [], "effects": [],
         "composite_level": 1.0, "composite_mode": "SrcAlpha",
         "track_motion": {"keyframes": [{"time": 0.0, "x": 0.0, "y": 0.0, "width": 1280.0, "height": 720.0}],
                          "shadow": {"enabled": False, "keyframes": []}, "glow": {"enabled": False, "keyframes": []}}}
    return t


def project(tracks, markers=None):
    return {"path": "D:\\work\\a.veg", "modified": False, "width": 1280, "height": 720, "fps": 60.0,
            "pixel_format": "Int8Bit", "length": 10.0, "tracks": tracks, "output_effects": [],
            "markers": markers or [], "regions": [], "command_markers": [], "groups": [],
            "media": [{"key": CLIP, "path": CLIP, "effects": []}]}


def base():
    return project([track(0, [event(0, 0.0), event(1, 2.0, offset=1.0), event(2, 4.0)]),
                    track(1, [event(0, 1.0, media=OTHER)])],
                   markers=[{"index": 0, "time": 1.0, "label": "drop"}])


def reindex(events):
    for i, e in enumerate(sorted(events, key=lambda e: e["start"])):
        e["index"] = i
    events.sort(key=lambda e: e["index"])


class Compare(unittest.TestCase):
    def test_a_project_matches_itself(self):
        r = project_compare.compare(base(), base())
        self.assertTrue(r["same"], r["summary"])

    def test_float_noise_is_no_change(self):
        b = base()
        b["tracks"][0]["events"][1]["start"] += 1e-7
        b["tracks"][0]["events"][1]["pan_crop"][0]["center"] = [640.0000001, 360.0]
        self.assertTrue(project_compare.compare(base(), b)["same"])

    def test_each_known_change_is_found_and_nothing_else(self):
        a, b = base(), base()
        evs = b["tracks"][0]["events"]
        evs[0]["playback_rate"] = 0.5                                     # speed
        evs[1]["start"] = 6.0                                              # moved, same media second
        evs[2]["effects"] = [effect("Invert", {"Amount": 0.75})]           # FX added
        evs[2]["pan_crop"][0]["zoom"] = 2.0                                # Pan/Crop
        reindex(evs)
        b["tracks"][1]["track_motion"]["keyframes"][0]["x"] = 100.0       # Track Motion
        b["markers"][0]["time"] = 1.5                                      # marker moved
        b["tracks"][1]["events"].append(event(1, 5.0, media=OTHER))       # event added
        r = project_compare.compare(a, b)
        s = r["summary"]
        self.assertEqual((s["events_moved"], s["events_added"], s["events_removed"], s["tracks_changed"], s["markers"]),
                         (1, 1, 0, 1, 1), s)
        moved = r["events"]["moved"][0]
        self.assertEqual((moved["start_a"], moved["start_b"]), (2.0, 6.0))
        self.assertIn("media offset", moved["matched_by"])
        fields = {(c["start"], d["field"]) for c in r["events"]["changed"] for d in c["changes"]}
        self.assertEqual(fields, {(0.0, "playback_rate"), (4.0, "effects"), (4.0, "pan_crop[0].zoom")}, r["events"]["changed"])
        self.assertEqual(r["tracks"]["changed"][0]["changes"][0]["field"], "track_motion.keyframes[0].x")
        self.assertEqual(r["markers"]["markers"]["changed"][0]["changes"], [{"field": "time", "a": 1.0, "b": 1.5}])

    def test_parameter_and_keyframe_changes_name_the_parameter(self):
        a, b = base(), base()
        a["tracks"][0]["events"][0]["effects"] = [effect("Invert", {"Amount": 0.5}, {"Amount": [{"time": 0.0, "value": 0.0}]})]
        b["tracks"][0]["events"][0]["effects"] = [effect("Invert", {"Amount": 0.6}, {"Amount": [{"time": 0.0, "value": 1.0}]})]
        changes = project_compare.compare(a, b)["events"]["changed"][0]["changes"]
        self.assertEqual([c["field"] for c in changes], ["effects[0].keyframes.Amount[0].value", "effects[0].params.Amount"])

    def test_an_event_moved_and_retrimmed_is_still_one_event(self):
        a, b = base(), base()
        e = b["tracks"][0]["events"][2]
        e["start"], e["offset"] = 5.0, 0.5
        r = project_compare.compare(a, b)
        self.assertEqual((r["summary"]["events_moved"], r["summary"]["events_added"], r["summary"]["events_removed"]), (1, 0, 0))
        self.assertIn("timeline order", r["events"]["moved"][0]["matched_by"])

    def test_a_track_added_in_the_middle_shifts_nothing(self):
        a = base()
        a["tracks"][0]["name"], a["tracks"][1]["name"] = "clips", "overlay"
        b = copy.deepcopy(a)
        b["tracks"].insert(1, track(1, [event(0, 0.0, media=OTHER)], name="new"))
        b["tracks"][2]["index"] = 2
        r = project_compare.compare(a, b)
        self.assertEqual([t["name"] for t in r["tracks"]["added"]], ["new"])
        self.assertEqual(r["summary"]["events_added"] + r["summary"]["events_removed"] + r["summary"]["tracks_changed"], 0, r["summary"])

    def test_a_removed_event_and_output_fx(self):
        a, b = base(), base()
        del b["tracks"][0]["events"][1]
        reindex(b["tracks"][0]["events"])
        b["output_effects"] = [effect("Sharpen", {"Amount": 1.0})]
        r = project_compare.compare(a, b)
        self.assertEqual([e["start"] for e in r["events"]["removed"]], [2.0])
        self.assertEqual(r["output_effects"][0]["field"], "output_effects")

    def test_the_tolerance_decides_what_moved(self):
        a, b = base(), base()
        b["tracks"][0]["events"][0]["start"] = 0.004
        self.assertEqual(project_compare.compare(a, b)["summary"]["events_moved"], 1)
        self.assertTrue(project_compare.compare(a, b, tolerance=0.01)["same"])

    def test_the_tool_answers_with_the_comparison(self):
        instances = vegas_mcp.Instances()
        instances.call = lambda name, args: {"ok": True, "result": {"a": base(), "b": base(), "read_a": "x.veg",
                                                                    "read_b": "y.veg", "restored": "reopened z.veg"}}
        result = vegas_mcp.call_tool(instances, "compare_projects", {"a": "open", "b": "y.veg"})
        text = result["content"][0]["text"]
        self.assertFalse(result["isError"])
        self.assertIn('"same": true', text)
        self.assertIn("reopened z.veg", text)
        self.assertNotIn("pan_crop", text)  # the dumps themselves are not passed on


if __name__ == "__main__":
    unittest.main()
