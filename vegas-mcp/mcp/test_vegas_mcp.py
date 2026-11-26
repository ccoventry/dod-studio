"""Tests for the VEGAS MCP server's own logic, no VEGAS needed (#1050).

    python -I -m unittest discover -s vegas-mcp/mcp -p "test_*.py"
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import vegas_instances  # noqa: E402
import vegas_mcp  # noqa: E402


class PipeNames(unittest.TestCase):
    def test_one_pipe_per_pid(self):
        self.assertEqual(vegas_instances.pipe_path(9112), "\\\\.\\pipe\\dodstudio-vegas-9112")

    def test_pid_from_pipe_name(self):
        self.assertEqual(vegas_instances.pid_from_pipe_name("dodstudio-vegas-9112"), 9112)
        # The prototype's fixed name, and anything else, is not a bridge VEGAS.
        self.assertIsNone(vegas_instances.pid_from_pipe_name("dodstudio-vegas"))
        self.assertIsNone(vegas_instances.pid_from_pipe_name("dodstudio-vegas-abc"))
        self.assertIsNone(vegas_instances.pid_from_pipe_name("dodstudio-game"))


class Autosave(unittest.TestCase):
    def test_only_that_pids_files(self):
        with tempfile.TemporaryDirectory() as folder:
            for name in ("00002a3c.autosave.veg", "00002a3c.autosave.veg.bak", "00002a3d.autosave.veg", "00000000.restored.veg"):
                open(os.path.join(folder, name), "w").close()
            found = [os.path.basename(p) for p in vegas_instances.autosave_files(0x2A3C, folder)]
            self.assertEqual(found, ["00002a3c.autosave.veg", "00002a3c.autosave.veg.bak"])

    def test_eight_hex_digits(self):
        with tempfile.TemporaryDirectory() as folder:
            open(os.path.join(folder, "0001e240.autosave.veg"), "w").close()
            self.assertEqual(len(vegas_instances.autosave_files(123456, folder)), 1)


@unittest.skipUnless(os.name == "nt", "Windows processes")
class EndRefusesOtherProcesses(unittest.TestCase):
    def test_a_process_without_a_bridge_pipe_is_left_alone(self):
        # This test's own python has no dodstudio-vegas-<pid> pipe, like the user's VEGAS.
        with self.assertRaises(vegas_instances.VegasError):
            vegas_instances.end(os.getpid())

    def test_a_process_that_is_not_vegas_is_left_alone(self):
        with self.assertRaises(vegas_instances.VegasError):
            vegas_instances.end(os.getpid(), launched=True)
        self.assertTrue(vegas_instances.is_running(os.getpid()))
        self.assertFalse(vegas_instances.is_vegas(os.getpid()))


class Tools(unittest.TestCase):
    def test_names_are_unique(self):
        names = [t["name"] for t in vegas_mcp.TOOLS]
        self.assertEqual(len(names), len(set(names)))

    def test_local_tools_are_handled_here(self):
        self.assertEqual(vegas_mcp.LOCAL_NAMES, {"launch", "end", "instances", "use"})

    @unittest.skipUnless(os.name == "nt", "named pipes")
    def test_a_bridge_call_with_no_bridge_vegas_is_an_error_not_a_crash(self):
        instances = vegas_mcp.Instances()
        instances.current = os.getpid()  # no pipe for this PID
        result = vegas_mcp.call_tool(instances, "ping", {})
        self.assertTrue(result["isError"])
        self.assertIn("gone", result["content"][0]["text"])

    def test_a_bridge_vegas_started_elsewhere_is_never_picked_without_use(self):
        real = vegas_mcp.vegas_instances.bridge_pids
        vegas_mcp.vegas_instances.bridge_pids = lambda: [4242]
        try:
            instances = vegas_mcp.Instances()
            with self.assertRaises(vegas_mcp.BridgeUnavailable) as caught:
                instances.target()
            self.assertIn("only with use", str(caught.exception))
            instances.launched.add(4242)
            self.assertEqual(instances.target(), 4242)
        finally:
            vegas_mcp.vegas_instances.bridge_pids = real

    @unittest.skipUnless(os.name == "nt", "named pipes")
    def test_use_refuses_a_vegas_without_a_bridge(self):
        result = vegas_mcp.call_tool(vegas_mcp.Instances(), "use", {"pid": os.getpid()})
        self.assertTrue(result["isError"])


if __name__ == "__main__":
    unittest.main()
