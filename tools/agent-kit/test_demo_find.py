import json
import tempfile
import unittest
from pathlib import Path

import demo_find as df

M00CAT = "76561197972576011"


def t(viewdemo, real):
    return {"real_offset": {"secs": int(real), "nanos": int(round(real % 1 * 1e9))},
            "viewdemo_offset": {"secs": int(viewdemo), "nanos": int(round(viewdemo % 1 * 1e9))}, "frame_index": 1}


def kill(killer, victim, weapon, viewdemo, real, headshot="no", teamkill=False):
    return {"time": t(viewdemo, real), "weapon": weapon, "killer": killer, "victim": victim,
            "teamkill": teamkill, "headshot": headshot}


def entry(name, recorder_slot=2, demo_type="POV", map_name="dod_anzio", kills=(), signons=("dod_anzio",),
          damage=None, length=1200.0):
    players = [
        {"id": M00CAT, "name": "dicE[: :]m00cat :D", "connection": {"Connected": {"client_id": 2}}},
        {"id": "76561197960000002", "name": "icyHOT ~ chi", "connection": {"Connected": {"client_id": 5}}},
        {"id": "76561197960000003", "name": "gone", "connection": "Disconnected"},
    ]
    return {
        "size_bytes": 100, "modified_unix_secs": 1,
        "file_info": {"path": f"X:/dod/{name}", "name": name, "size_bytes": 100},
        "analysis": {
            "demo_info": {"map_name": map_name, "demo_type": demo_type, "playback_time": length, "damage": damage,
                          "no_camera_data": False},
            "state": {"players": players, "pov_player_index": recorder_slot if demo_type == "POV" else None,
                      "kill_positions": list(kills), "signon_maps": list(signons), "map_changed": False,
                      "hltv_name": "Philly - HLTV" if demo_type == "HLTV" else None},
        },
    }


def args(*argv):
    return df.parser().parse_args(list(argv))


class ClockTests(unittest.TestCase):
    def test_seconds_read_as_the_demo_bar(self):
        self.assertEqual(df.bar_time(992.38), "16:32:38")
        self.assertEqual(df.bar_time(5.0), "0:05:00")

    def test_lengths_take_seconds_or_minutes(self):
        self.assertEqual(df.parse_length("1200"), 1200)
        self.assertEqual(df.parse_length("20:00"), 1200)
        self.assertEqual(df.parse_length("1:00:00"), 3600)


class PlayerTests(unittest.TestCase):
    def test_steam_ids_convert_and_names_match_by_part(self):
        self.assertEqual(df.steam64("STEAM_0:1:6155141"), "76561197972576011")
        self.assertIsNone(df.steam64("m00cat"))
        self.assertTrue(df.player_matches("STEAM_0:1:6155141", M00CAT, "x"))
        self.assertTrue(df.player_matches("M00CAT", "id", "dicE[: :]m00cat :D"))
        self.assertFalse(df.player_matches("chi", "id", "dicE[: :]m00cat :D"))

    def test_the_recorder_is_the_player_in_the_recorded_slot(self):
        state = entry("a.dem")["analysis"]["state"]
        self.assertEqual(df.recorder_of(state, "POV"), (M00CAT, "dicE[: :]m00cat :D"))
        self.assertEqual(df.recorder_of(state, "HLTV"), (None, None))


class SummaryTests(unittest.TestCase):
    def test_kills_carry_both_clocks_and_names(self):
        demo = df.summarize(entry("a.dem", kills=[kill(M00CAT, "76561197960000002", "Garand", 851.5, 20.25, "yes")]))
        k = demo["kills"][0]
        self.assertAlmostEqual(k["viewdemo"], 851.5)
        self.assertAlmostEqual(k["file"], 20.25)
        self.assertEqual((k["killer_name"], k["victim_name"], k["headshot"]), ("dicE[: :]m00cat :D", "icyHOT ~ chi", "yes"))

    def test_world_kills_multi_map_and_damage_are_read(self):
        demo = df.summarize(entry("a.dem", kills=[kill(None, M00CAT, "Unknown", 1, 1)],
                                  signons=("dod_anzio", "dod_caen"), damage={"reason": "message length too long"}))
        self.assertEqual(demo["kills"][0]["killer_name"], "world")
        self.assertTrue(demo["multi_map"])
        self.assertEqual(demo["damage_reason"], "message length too long")


class QueryTests(unittest.TestCase):
    def setUp(self):
        chi = "76561197960000002"
        self.demos = [
            df.summarize(entry("a.dem", kills=[kill(M00CAT, chi, "Garand", 100, 10), kill(chi, M00CAT, "K98", 110, 20)])),
            df.summarize(entry("b.dem", recorder_slot=5, kills=[kill(chi, M00CAT, "Garand", 200, 30, "yes")])),
            df.summarize(entry("c.dem", recorder_slot=5, kills=[kill(chi, M00CAT, "ScopedK98", 300, 40)])),
            df.summarize(entry("d.dem", demo_type="HLTV", map_name="dod_caen", kills=[kill(chi, M00CAT, "K98", 1, 1)])),
        ]

    def names(self, *argv):
        return [d["name"] for d, _ in df.query(self.demos, args(*argv))]

    def test_map_type_and_weapon_filter_demos(self):
        self.assertEqual(self.names("--map", "anzio", "--weapon", "garand"), ["a.dem", "b.dem"])
        self.assertEqual(self.names("--hltv"), ["d.dem"])

    def test_an_exact_weapon_name_does_not_match_its_scoped_cousin(self):
        self.assertEqual(self.names("--weapon", "k98"), ["a.dem", "d.dem"])
        self.assertEqual(self.names("--weapon", "scoped"), ["c.dem"])

    def test_by_recorder_keeps_the_recorders_own_kills(self):
        self.assertEqual(self.names("--weapon", "garand", "--by-recorder"), ["a.dem", "b.dem"])
        self.assertEqual(self.names("--weapon", "k98", "--by-recorder"), [])

    def test_per_player_keeps_one_demo_per_recorder(self):
        self.assertEqual(self.names("--pov", "--per-player", "1"), ["a.dem", "b.dem"])

    def test_player_kills_headshots_and_victims(self):
        self.assertEqual(self.names("--player", "chi", "--weapon", "garand", "--headshot", "yes"), ["b.dem"])
        self.assertEqual(self.names("--victim", "chi"), ["a.dem"])

    def test_the_same_file_in_two_installs_counts_once(self):
        copy = dict(self.demos[0], path="Y:/dod/a.dem")
        self.demos.append(copy)
        self.assertEqual(self.names("--weapon", "garand"), ["a.dem", "b.dem"])
        self.assertEqual(self.names("--weapon", "garand", "--all-copies"), ["a.dem", "b.dem", "a.dem"])

    def test_missing_files_are_left_out_unless_asked(self):
        self.demos[0]["on_disk"] = False
        self.assertNotIn("a.dem", self.names())
        self.assertIn("a.dem", self.names("--include-missing"))

    def test_min_kills_counts_the_killers_kills(self):
        self.assertEqual(self.names("--player", "chi", "--min-kills", "1", "--map", "anzio"), ["a.dem", "b.dem", "c.dem"])
        self.assertEqual(self.names("--player", "chi", "--min-kills", "2"), [])


class CacheTests(unittest.TestCase):
    def test_the_newest_schema_folder_with_entries_is_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            for version, has in ((12, True), (16, True), (17, False)):
                folder = Path(tmp) / f"v{version}"
                folder.mkdir()
                if has:
                    (folder / "x.json").write_text(json.dumps(entry("a.dem")), encoding="utf-8")
                    (folder / "x.players.json").write_text("{}", encoding="utf-8")
            folder = df.schema_dir(tmp)
            self.assertEqual(folder.name, "v16")
            demos, bad = df.load(folder)
            self.assertEqual((len(demos), bad, demos[0]["on_disk"]), (1, 0, False))


if __name__ == "__main__":
    unittest.main()
