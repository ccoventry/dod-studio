import tempfile
import unittest
from pathlib import Path

import game_lock
from game_lock import BUSY, OK, REFUSED, decide_acquire, decide_release


class AcquireTests(unittest.TestCase):
    def test_a_free_lock_with_no_game_and_the_right_account_is_taken(self):
        self.assertEqual(decide_acquire(None, "me", False, None)[0], OK)

    def test_the_wrong_steam_account_is_refused_before_anything_else(self):
        code, message = decide_acquire(None, "me", False, "another Steam account is signed in")
        self.assertEqual(code, REFUSED)
        self.assertIn("another Steam account", message)

    def test_another_owner_holding_it_is_busy(self):
        self.assertEqual(decide_acquire("them", "me", False, None)[0], BUSY)

    def test_re_acquiring_while_your_own_game_still_runs_is_busy(self):
        code, message = decide_acquire("me", "me", True, None)
        self.assertEqual(code, BUSY)
        self.assertIn("your own earlier game", message)

    def test_a_game_nobody_holds_is_busy(self):
        self.assertEqual(decide_acquire(None, "me", True, None)[0], BUSY)

    def test_re_acquiring_your_own_lock_with_no_game_is_fine(self):
        self.assertEqual(decide_acquire("me", "me", False, None)[0], OK)


class ReleaseTests(unittest.TestCase):
    def test_release_waits_for_the_game_to_exit(self):
        self.assertEqual(decide_release("me", "me", True, False)[0], BUSY)

    def test_force_releases_with_a_game_running(self):
        self.assertEqual(decide_release("me", "me", True, True)[0], OK)

    def test_another_owners_lock_is_never_released(self):
        self.assertEqual(decide_release("them", "me", False, True)[0], BUSY)


class LockFileTests(unittest.TestCase):
    def test_the_lock_file_is_created_once_and_names_its_owner(self):
        with tempfile.TemporaryDirectory() as tmp:
            lock = Path(tmp) / "sub" / "game.lock"
            self.assertTrue(game_lock.take_lock(lock, "me"))
            self.assertFalse(game_lock.take_lock(lock, "them"))
            self.assertEqual(game_lock.read_holder(lock), "me")

    def test_the_default_lock_file_is_outside_any_checkout(self):
        path = game_lock.default_lock_file({"LOCALAPPDATA": "X:/appdata"})
        self.assertEqual(path, Path("X:/appdata") / "dod-studio-agents" / "game.lock")

    def test_ds_game_lock_overrides_the_default(self):
        self.assertEqual(game_lock.default_lock_file({"DS_GAME_LOCK": "Y:/l"}), Path("Y:/l"))


if __name__ == "__main__":
    unittest.main()
