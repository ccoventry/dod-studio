import tempfile
import unittest
from pathlib import Path

import studio_live as sl

PATHS_RS = """use std::path::PathBuf;

fn appdata_base() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}
"""


class PatchTests(unittest.TestCase):
    def test_the_env_check_goes_first_in_appdata_base(self):
        patched = sl.patched_paths_source(PATHS_RS)
        body = patched.split("fn appdata_base() -> PathBuf {")[1]
        self.assertLess(body.index("DS_LIVE_APPDATA"), body.index("dirs::config_dir"))

    def test_patching_twice_changes_nothing(self):
        once = sl.patched_paths_source(PATHS_RS)
        self.assertEqual(sl.patched_paths_source(once), once)

    def test_crlf_files_keep_crlf(self):
        patched = sl.patched_paths_source(PATHS_RS.replace("\n", "\r\n"))
        self.assertNotIn("\n", patched.replace("\r\n", ""))

    def test_a_moved_function_is_refused_not_guessed(self):
        with self.assertRaises(sl.Refused):
            sl.patched_paths_source("fn something_else() {}")


class SeedTests(unittest.TestCase):
    def test_seeded_settings_clear_recents_and_keep_the_game_behind(self):
        s = sl.seeded_settings({"recent_projects": ["a"], "keep_game_behind": False, "x": 1}, "C:/hl.exe",
                               {"capture_mode": "obs"})
        self.assertEqual(s["recent_projects"], [])
        self.assertTrue(s["keep_game_behind"])
        self.assertEqual((s["hl_path"], s["x"], s["capture_mode"]), ("C:/hl.exe", 1, "obs"))

    def test_the_users_settings_object_is_not_changed(self):
        user = {"recent_projects": ["a"]}
        sl.seeded_settings(user)
        self.assertEqual(user, {"recent_projects": ["a"]})

    def test_set_values_parse_as_json_or_stay_strings(self):
        self.assertEqual(sl.parse_set(["a=true", "b=[1]", "c=plain"]), {"a": True, "b": [1], "c": "plain"})

    def test_a_set_without_a_key_is_refused(self):
        with self.assertRaises(sl.Refused):
            sl.parse_set(["=1"])


class LiveDirTests(unittest.TestCase):
    def test_the_users_settings_folder_and_its_parent_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            user = Path(tmp) / "Roaming" / "dod-studio"
            for bad in (user, user.parent, user / "sub"):
                with self.assertRaises(sl.Refused):
                    sl.check_live_dir(bad, user)

    def test_a_scratch_folder_is_fine(self):
        with tempfile.TemporaryDirectory() as tmp:
            user = Path(tmp) / "Roaming" / "dod-studio"
            live = Path(tmp) / "scratch" / "live"
            self.assertEqual(sl.check_live_dir(live, user), live.resolve())


if __name__ == "__main__":
    unittest.main()
