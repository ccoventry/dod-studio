"""name_hw_dll.py's pure parts: merging per-.so maps and checking a merged map.

usage: python -m unittest test_name_hw_dll   (from this folder; skipped without pefile/capstone/pyelftools)
"""
import unittest

try:
    from name_hw_dll import check_merged, merge_maps
except ImportError:  # the matcher's third-party libraries are not installed
    check_merged = merge_maps = None


@unittest.skipIf(merge_maps is None, "needs pefile, capstone and pyelftools")
class MergeTests(unittest.TestCase):
    def test_first_listed_wins_and_disagreements_are_reported(self):
        merged, disagree = merge_maps([{1: "A", 2: "B"}, {2: "C", 3: "D"}])
        self.assertEqual(merged, {1: ("A", 0), 2: ("B", 0), 3: ("D", 1)})
        self.assertEqual(disagree, [(2, ["B", "C"])])

    def test_same_name_twice_is_not_a_disagreement(self):
        _merged, disagree = merge_maps([{1: "A"}, {1: "A"}])
        self.assertEqual(disagree, [])

    def test_clone_suffixes_are_not_a_disagreement(self):
        _merged, disagree = merge_maps([{1: "Host_Map.part.29"}, {1: "Host_Map"}])
        self.assertEqual(disagree, [])

    def test_known_function_with_another_linux_name_or_a_clone_suffix_is_a_hit(self):
        merged = {10: ("PM_RecursiveHullCheck.part.0", 0), 20: ("_PM_PlayerTrace", 0)}
        rows = check_merged(merged, {"PM_RecursiveHullCheck": 10, "PM_PlayerTraceEx": 20})
        self.assertEqual([v for _n, _r, v in rows], ["hit", "hit"])

    def test_check_merged_verdicts(self):
        merged = {10: ("Hit", 0), 20: ("Elsewhere", 0), 30: ("Other", 0)}
        rows = check_merged(merged, {"Hit": 10, "Elsewhere": 25, "Renamed": 30, "Missing": 40})
        self.assertEqual({n: v for n, _r, v in rows},
                         {"Hit": "hit", "Elsewhere": "wrong", "Renamed": "wrong", "Missing": "unnamed"})


if __name__ == "__main__":
    unittest.main()
