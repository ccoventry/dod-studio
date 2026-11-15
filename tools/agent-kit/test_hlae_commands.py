import unittest
from pathlib import Path

import hlae_commands as hc

DLL = (b"MZ\x00junk\x00mirv_camexport_mode\x00__mirv_debug\x00mirv_recordmovie_start\x00"
       b"xmirv_not\x00mirv_camexport_mode\x00mirv_partial" + "FileVersion\x00\x002.192.4.0\x00".encode("utf-16-le"))


class ScanTests(unittest.TestCase):
    def test_nul_terminated_mirv_names_are_found_once_without_debug_names(self):
        self.assertEqual(hc.names_in(DLL), ["mirv_camexport_mode", "mirv_recordmovie_start"])

    def test_the_file_version_is_read(self):
        self.assertEqual(hc.file_version(DLL), "2.192.4.0")
        self.assertIsNone(hc.file_version(b"no version here"))


class FileTests(unittest.TestCase):
    def test_the_names_file_round_trips(self):
        text = hc.render(["mirv_a", "mirv_b"], "2.192.4.0", "ab" * 32)
        self.assertTrue(text.startswith("# HLAE"))
        self.assertEqual(hc.parse_names_file(text), ["mirv_a", "mirv_b"])

    def test_the_tracked_file_holds_the_camexport_mode_name(self):
        self.assertIn("mirv_camexport_mode", hc.parse_names_file(hc.NAMES_FILE.read_text(encoding="utf-8")))

    def test_the_dll_defaults_to_the_env_var(self):
        self.assertEqual(hc.default_dll({"DS_HLAE_DLL": "X:/hlae/AfxHookGoldSrc.dll"}),
                         Path("X:/hlae/AfxHookGoldSrc.dll"))


class CheckTests(unittest.TestCase):
    NAMES = ["mirv_camexport_mode", "mirv_camexport_timeinfo", "mirv_recordmovie_start"]

    def test_a_known_name_is_found(self):
        self.assertEqual(hc.check("mirv_camexport_mode", self.NAMES), (True, []))

    def test_a_missing_name_gets_its_nearest(self):
        found, near = hc.check("mirv_camexport_mod", self.NAMES)
        self.assertFalse(found)
        self.assertEqual(near[0], "mirv_camexport_mode")


if __name__ == "__main__":
    unittest.main()
