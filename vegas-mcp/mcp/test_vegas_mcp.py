"""Tests for the VEGAS MCP server's own logic, no VEGAS needed (#1050).

    python -I -m unittest discover -s vegas-mcp/mcp -p "test_*.py"
"""

import io
import json
import os
import subprocess
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


    def test_an_orphan_autosave_is_one_whose_vegas_is_not_running(self):
        with tempfile.TemporaryDirectory() as folder:
            mine = "%08x.autosave.veg" % os.getpid()  # a running process: not an orphan
            for name in (mine, "7ffffff0.autosave.veg", "7ffffff0.autosave.veg.bak", "notapid.autosave.veg"):
                open(os.path.join(folder, name), "w").close()
            found = [os.path.basename(p) for p in vegas_instances.orphan_autosaves(folder)]
            self.assertEqual(found, ["7ffffff0.autosave.veg"])


class WorkFolder(unittest.TestCase):
    def setUp(self):
        self.saved = os.environ.get(vegas_instances.ROOT_VARIABLE)

    def tearDown(self):
        if self.saved is None:
            os.environ.pop(vegas_instances.ROOT_VARIABLE, None)
        else:
            os.environ[vegas_instances.ROOT_VARIABLE] = self.saved

    def test_a_drive_root_is_refused(self):
        os.environ.pop(vegas_instances.ROOT_VARIABLE, None)
        for root in ("C:\\", "C:/", "\\\\server\\share"):
            with self.assertRaises(vegas_instances.VegasError, msg=root):
                vegas_instances.checked_work_folder(root)

    def test_launch_stays_inside_the_servers_root(self):
        os.environ[vegas_instances.ROOT_VARIABLE] = "D:\\tests\\vegas"
        self.assertEqual(vegas_instances.checked_work_folder("D:\\tests\\vegas\\run1"), "D:\\tests\\vegas\\run1")
        self.assertEqual(vegas_instances.checked_work_folder(None), "D:\\tests\\vegas")
        for outside in ("D:\\tests\\vegas2", "D:\\tests", "D:\\tests\\vegas\\..\\other", "C:\\Users\\name\\Videos"):
            with self.assertRaises(vegas_instances.VegasError, msg=outside):
                vegas_instances.checked_work_folder(outside)


@unittest.skipUnless(os.name == "nt", "Windows processes")
class EndRefusesOtherProcesses(unittest.TestCase):
    def test_a_process_that_is_not_vegas_is_left_alone(self):
        # This test's own python is running but is not VEGAS, like any other program.
        with self.assertRaises(vegas_instances.VegasError):
            vegas_instances.end_bridge_pid(os.getpid())
        self.assertTrue(vegas_instances.is_running(os.getpid()))

    def test_an_exited_process_is_left_alone(self):
        child = subprocess.Popen([sys.executable, "-c", "pass"])
        child.wait()
        pid = child.pid
        del child  # handle closed: the PID is free again
        self.assertIn("nothing done", vegas_instances.end_bridge_pid(pid))

    def test_a_launched_vegas_is_ended_through_its_own_handle(self):
        # A stand-in child: Launched.end must end exactly this process, by its handle.
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        launched = vegas_instances.Launched(child)
        self.assertIn("ended VEGAS PID %d" % child.pid, launched.end())
        self.assertTrue(launched.exited())

    def test_a_launched_vegas_that_died_is_cleaned_up_by_its_handle_not_its_pid(self):
        # The handle Launched holds keeps the PID from being reused, so the
        # clean-up can never reach a new process (the user's VEGAS) on that PID.
        child = subprocess.Popen([sys.executable, "-c", "pass"])
        child.wait()
        instances = vegas_mcp.Instances()
        instances.launched[child.pid] = vegas_instances.Launched(child)
        instances.current = child.pid
        real = vegas_instances.end_bridge_pid
        vegas_instances.end_bridge_pid = lambda pid: self.fail("a launched VEGAS must not be ended by PID")
        try:
            instances.end_all()
        finally:
            vegas_instances.end_bridge_pid = real
        self.assertEqual(instances.launched, {})


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
            instances.launched[4242] = object()
            self.assertEqual(instances.target(), 4242)
        finally:
            vegas_mcp.vegas_instances.bridge_pids = real

    def test_after_its_vegas_goes_a_call_never_switches_to_another(self):
        real = vegas_mcp.vegas_instances.bridge_pids
        vegas_mcp.vegas_instances.bridge_pids = lambda: [4242]  # B has gone, A (4242) is left
        try:
            instances = vegas_mcp.Instances()
            instances.launched[4242] = object()
            instances.current = 4343
            for _ in range(2):  # the first call reports it gone; the retry must not land in A
                with self.assertRaises(vegas_mcp.BridgeUnavailable):
                    instances.target()
            instances.use({"pid": 4242})
            self.assertEqual(instances.target(), 4242)
        finally:
            vegas_mcp.vegas_instances.bridge_pids = real

    @unittest.skipUnless(os.name == "nt", "named pipes")
    def test_use_refuses_a_vegas_without_a_bridge(self):
        result = vegas_mcp.call_tool(vegas_mcp.Instances(), "use", {"pid": os.getpid()})
        self.assertTrue(result["isError"])

    def test_bad_arguments_are_an_error_reply_not_the_end_of_the_server(self):
        instances = vegas_mcp.Instances()
        for name, args in (("use", {"pid": "abc"}), ("use", {}), ("end", {"pid": "x1"}), ("launch", {"wait_seconds": "soon"})):
            result = vegas_mcp.call_tool(instances, name, args)
            self.assertTrue(result["isError"], (name, args))
        self.assertEqual(instances.launched, {})

    def test_non_ascii_arguments_arrive_intact(self):
        sent = []
        real = vegas_mcp.send
        vegas_mcp.send = sent.append
        try:
            line = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                               "params": {"name": "use", "arguments": {"pid": "café"}}}, ensure_ascii=False)
            vegas_mcp.serve_lines(vegas_mcp.Instances(), io.TextIOWrapper(io.BytesIO(line.encode("utf-8")), encoding="utf-8"))
        finally:
            vegas_mcp.send = real
        self.assertIn("café", sent[0]["result"]["content"][0]["text"])


if __name__ == "__main__":
    unittest.main()
