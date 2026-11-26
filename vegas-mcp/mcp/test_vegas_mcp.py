"""Tests for the VEGAS MCP server's own logic, no VEGAS needed (#1050).

    python -I -m unittest discover -s vegas-mcp/mcp -p "test_*.py"
"""

import base64
import ctypes
import io
import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import bridge_pipe  # noqa: E402
import desktops  # noqa: E402
import snapshot_content  # noqa: E402
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
            # This python's PID is running but is not VEGAS (a crashed VEGAS's
            # PID reused after a reboot, say): still an orphan.
            reused = "%08x.autosave.veg" % os.getpid()
            for name in (reused, "7ffffff0.autosave.veg", "7ffffff0.autosave.veg.bak", "notapid.autosave.veg"):
                open(os.path.join(folder, name), "w").close()
            found = [os.path.basename(p) for p in vegas_instances.orphan_autosaves(folder)]
            self.assertEqual(found, sorted([reused, "7ffffff0.autosave.veg"]))


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
        for outside in ("D:\\tests\\vegas2", "D:\\tests", "D:\\tests\\vegas\\..\\other", "C:\\Users\\name\\Videos",
                        "D:\\tests\\vegas\\.. \\other", "D:\\tests\\vegas\\run. "):
            with self.assertRaises(vegas_instances.VegasError, msg=outside):
                vegas_instances.checked_work_folder(outside)

    def test_without_a_server_root_only_the_default_folder_is_allowed(self):
        os.environ.pop(vegas_instances.ROOT_VARIABLE, None)
        default = vegas_instances.default_root()
        self.assertEqual(vegas_instances.checked_work_folder(os.path.join(default, "run1")), os.path.join(default, "run1"))
        with self.assertRaises(vegas_instances.VegasError):
            vegas_instances.checked_work_folder("C:\\Users\\name\\Documents\\project")


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
        # Clean-up looks in an empty folder, never the real VEGAS autosave folder.
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        launched = vegas_instances.Launched(child)
        with tempfile.TemporaryDirectory() as folder:
            self.assertIn("ended VEGAS PID %d" % child.pid, launched.end(folder))
        self.assertTrue(launched.exited())

    def test_a_launched_vegas_that_died_is_cleaned_up_by_its_handle_not_its_pid(self):
        # The handle Launched holds keeps the PID from being reused, so the
        # clean-up can never reach a new process (the user's VEGAS) on that PID.
        child = subprocess.Popen([sys.executable, "-c", "pass"])
        child.wait()
        instances = vegas_mcp.Instances()
        instances.launched[child.pid] = vegas_instances.Launched(child)
        instances.current = child.pid
        real_end, real_cleanup = vegas_instances.end_bridge_pid, vegas_instances.cleanup
        vegas_instances.end_bridge_pid = lambda pid: self.fail("a launched VEGAS must not be ended by PID")
        cleaned = []
        vegas_instances.cleanup = lambda pid, folder=None: cleaned.append(pid) or "stubbed"
        try:
            instances.end_all()
        finally:
            vegas_instances.end_bridge_pid, vegas_instances.cleanup = real_end, real_cleanup
        self.assertEqual(instances.launched, {})
        self.assertEqual(cleaned, [child.pid])


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

    def test_a_line_that_is_not_an_object_is_an_error_reply(self):
        sent = []
        real = vegas_mcp.send
        vegas_mcp.send = sent.append
        try:
            vegas_mcp.serve_lines(vegas_mcp.Instances(), ["[1, 2]\n", "\"text\"\n"])
        finally:
            vegas_mcp.send = real
        self.assertEqual([m["error"]["code"] for m in sent], [-32700, -32700])

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


class Protocol(unittest.TestCase):
    def test_a_supported_version_is_answered_as_asked(self):
        for v in vegas_mcp.SUPPORTED_PROTOCOLS:
            self.assertEqual(vegas_mcp.negotiated_protocol(v), v)

    def test_an_unknown_version_gets_the_servers_own(self):
        for v in ("1999-01-01", "2099-12-31", None, 5):
            self.assertEqual(vegas_mcp.negotiated_protocol(v), vegas_mcp.PROTOCOL)

    def test_initialize_over_json_rpc(self):
        sent = []
        real = vegas_mcp.send
        vegas_mcp.send = sent.append
        try:
            vegas_mcp.serve_lines(vegas_mcp.Instances(), [json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                                                                      "params": {"protocolVersion": "2031-01-01"}})])
        finally:
            vegas_mcp.send = real
        self.assertEqual(sent[0]["result"]["protocolVersion"], vegas_mcp.PROTOCOL)


class TimeLimits(unittest.TestCase):
    def test_defaults(self):
        self.assertEqual(vegas_mcp.time_limit("ping", {}), vegas_mcp.DEFAULT_TIMEOUT_SECONDS)
        self.assertEqual(vegas_mcp.time_limit("render", {}), 3600)
        self.assertEqual(vegas_mcp.time_limit("open_project", {}), 600)

    def test_timeout_seconds_wins(self):
        self.assertEqual(vegas_mcp.time_limit("render", {"timeout_seconds": 5}), 5)
        self.assertEqual(vegas_mcp.time_limit("ping", {"timeout_seconds": "2.5"}), 2.5)

    def test_import_edl_waits_longer_than_its_own_wait(self):
        self.assertEqual(vegas_mcp.time_limit("import_edl", {"wait_seconds": 900}), 900 + vegas_mcp.IMPORT_EDL_MARGIN_SECONDS)

    def test_a_bad_limit_is_an_error(self):
        for bad in (0, -1, "soon", float("nan")):
            with self.assertRaises(vegas_mcp.BridgeUnavailable, msg=bad):
                vegas_mcp.time_limit("ping", {"timeout_seconds": bad})

    def test_every_bridge_tool_takes_timeout_seconds_and_no_local_one_does(self):
        for t in vegas_mcp.TOOLS:
            has = "timeout_seconds" in t["inputSchema"]["properties"]
            self.assertEqual(has, t["name"] not in vegas_mcp.LOCAL_NAMES, t["name"])


class FakePipe:
    """Stands in for a bridge pipe: records what is written, and readline
    blocks until a reply is pushed, the way a pipe waits on VEGAS."""

    CLOSED = object()

    def __init__(self):
        self.written = []
        self.replies = queue.Queue()

    def write(self, data):
        self.written.append(json.loads(data.decode("utf-8")))

    def readline(self):
        line = self.replies.get()
        if line is FakePipe.CLOSED:
            raise ValueError("I/O operation on closed file")
        return line

    def reply(self, request_id, ok=True):
        self.replies.put(json.dumps({"id": request_id, "ok": ok, "result": "done"}).encode("utf-8") + b"\n")

    def close(self):
        self.replies.put(FakePipe.CLOSED)


def fake_bridge():
    b = bridge_pipe.Bridge(4242)
    pipe = FakePipe()

    def reopen():
        b.pipe = pipe

    b._open = reopen
    return b, pipe


class TimedCalls(unittest.TestCase):
    def test_a_call_that_replies_in_time(self):
        b, pipe = fake_bridge()
        threading.Timer(0.05, lambda: pipe.reply(1)).start()
        self.assertEqual(b.call("ping", {}, 5)["result"], "done")
        self.assertIsNone(b.busy())

    def test_past_its_limit_the_call_is_an_error_and_the_vegas_busy(self):
        b, pipe = fake_bridge()
        t = time.monotonic()
        with self.assertRaises(bridge_pipe.BridgeTimeout) as caught:
            b.call("run_script", {"path": "x.cs"}, 0.2)
        self.assertLess(time.monotonic() - t, 2)
        self.assertIn("run_script", str(caught.exception))
        self.assertIn("run_script", b.busy())
        # The next call is refused, not sent: its reply would be read as the old one's.
        with self.assertRaises(bridge_pipe.BridgeBusy) as caught:
            b.call("ping", {}, 5)
        self.assertIn("still running run_script", str(caught.exception))
        self.assertEqual([w["cmd"] for w in pipe.written], ["run_script"])

    def test_the_late_reply_clears_the_mark_and_is_reported_once(self):
        b, pipe = fake_bridge()
        with self.assertRaises(bridge_pipe.BridgeTimeout):
            b.call("run_script", {}, 0.1)
        pipe.reply(1)
        deadline = time.monotonic() + 2
        while b.busy() and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertIsNone(b.busy())
        threading.Timer(0.05, lambda: pipe.reply(2)).start()
        self.assertTrue(b.call("ping", {}, 5)["ok"])
        self.assertEqual([w["id"] for w in pipe.written], [1, 2])
        late = b.take_late()
        self.assertIn("run_script", late)
        self.assertIn("ok", late)
        self.assertIsNone(b.take_late())

    def test_ending_the_vegas_clears_the_mark(self):
        b, pipe = fake_bridge()
        with self.assertRaises(bridge_pipe.BridgeTimeout):
            b.call("render", {}, 0.1)
        b.close()  # what end does once that VEGAS is gone
        self.assertIsNone(b.busy())
        self.assertIsNone(b.pipe)

    def test_a_second_caller_waits_its_turn(self):
        b, pipe = fake_bridge()
        results = []
        first = threading.Thread(target=lambda: results.append(b.call("snapshot", {}, 5)))
        first.start()
        while not pipe.written:
            time.sleep(0.01)
        second = threading.Thread(target=lambda: results.append(b.call("ping", {}, 5)))
        second.start()
        time.sleep(0.1)
        self.assertEqual(len(pipe.written), 1)  # not sent while the first is out
        pipe.reply(1)
        while len(pipe.written) < 2:
            time.sleep(0.01)
        pipe.reply(2)
        first.join(5)
        second.join(5)
        self.assertEqual([r["id"] for r in results], [1, 2])

    def test_a_reply_for_another_request_is_not_taken_for_this_ones(self):
        b, pipe = fake_bridge()
        threading.Timer(0.05, lambda: pipe.reply(99)).start()
        with self.assertRaises(bridge_pipe.BridgeUnavailable) as caught:
            b.call("ping", {}, 5)
        self.assertIn("unreadable", str(caught.exception))

    def test_the_timeout_argument_never_reaches_vegas(self):
        instances = vegas_mcp.Instances()
        b, pipe = fake_bridge()
        instances.bridges[4242] = b
        instances.launched[4242] = object()
        instances.current = 4242
        real = vegas_mcp.vegas_instances.bridge_pids
        vegas_mcp.vegas_instances.bridge_pids = lambda: [4242]
        try:
            threading.Timer(0.05, lambda: pipe.reply(1)).start()
            instances.call("ping", {"timeout_seconds": 3})
        finally:
            vegas_mcp.vegas_instances.bridge_pids = real
        self.assertEqual(pipe.written[0]["args"], {})


class ServerKeepsReading(unittest.TestCase):
    def test_a_local_tool_is_answered_while_a_bridge_call_is_stuck(self):
        sent = []
        stuck = threading.Event()
        instances = vegas_mcp.Instances()

        def slow_call(cmd, args):
            stuck.wait(5)
            return {"ok": True, "result": "late"}

        def record(msg):
            sent.append(msg)
            if msg.get("id") == 2:
                stuck.set()  # the second answer came while the first call was still waiting

        instances.call = slow_call
        instances.describe = lambda: {"bridge_vegas": []}
        real = vegas_mcp.send
        vegas_mcp.send = record
        try:
            vegas_mcp.serve_lines(instances, [
                json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "ping", "arguments": {}}}),
                json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "instances", "arguments": {}}}),
            ])
        finally:
            vegas_mcp.send = real
        self.assertEqual([m["id"] for m in sent], [2, 1])


@unittest.skipUnless(os.name == "nt", "Windows processes and desktops")
class HiddenDesktop(unittest.TestCase):
    def test_a_process_on_a_hidden_desktop_is_ended_by_its_handle_and_the_desktop_closed(self):
        desk = vegas_instances.new_desktop()
        self.assertIn(desk.name, desktops.desktop_names())
        proc = desktops.start('"%s" -c "import time; time.sleep(30)"' % sys.executable, dict(os.environ),
                              vegas_instances.SW_SHOWMINNOACTIVE, desk)
        launched = vegas_instances.Launched(proc, desk)
        proc.kill()
        with tempfile.TemporaryDirectory() as folder:
            self.assertIn("PID %d" % proc.pid, launched.end(folder))  # a second kill of a dying process is no error
        self.assertTrue(launched.exited())
        self.assertIsNone(desk.handle)

    def test_the_environment_block_is_sorted_and_double_terminated(self):
        block = desktops.environment_block({"b": "2", "A": "1"})
        self.assertEqual(ctypes.wstring_at(ctypes.addressof(block), 9), "A=1\0b=2\0\0")


def shot(n_bytes, mime_type="image/jpeg", path="D:\\work\\snapshots\\a.png"):
    """A snapshot reply entry whose inline copy decodes to n_bytes."""
    data = base64.b64encode(b"\0" * n_bytes).decode("ascii")
    return {"path": path, "inline": {"mime_type": mime_type, "data": data}}


class SnapshotImages(unittest.TestCase):
    def test_an_inline_copy_becomes_an_image_block_and_leaves_the_text(self):
        shots = [shot(300, "image/png"), shot(300, "image/jpeg")]
        blocks = snapshot_content.image_blocks(shots)
        self.assertEqual([(b["type"], b["mimeType"]) for b in blocks], [("image", "image/png"), ("image", "image/jpeg")])
        self.assertEqual(base64.b64decode(blocks[0]["data"]), b"\0" * 300)
        self.assertEqual([s.get("inline_as") for s in shots], ["image/png", "image/jpeg"])
        self.assertFalse(any("inline" in s for s in shots))

    def test_the_images_past_the_total_cap_are_left_out_and_say_why(self):
        shots = [shot(300) for _ in range(3)]
        blocks = snapshot_content.image_blocks(shots, max_total_bytes=700)
        self.assertEqual(len(blocks), 2)
        self.assertIn("fewer times", shots[2]["not_inline"])
        self.assertNotIn("inline", shots[2])

    def test_a_copy_narrower_than_its_frame_says_its_width(self):
        wide = shot(100)
        wide["width"] = 3840
        wide["inline"]["width"] = 2000
        same = shot(100)
        same["width"] = 1280
        same["inline"]["width"] = 1280
        snapshot_content.image_blocks([wide, same])
        self.assertEqual(wide.get("inline_width"), 2000)
        self.assertNotIn("inline_width", same)

    def test_a_copy_the_bridge_left_out_keeps_its_reason(self):
        shots = [{"path": "x.png", "inline": {"left_out": "over 1000000 bytes even as a JPEG"}}]
        self.assertEqual(snapshot_content.image_blocks(shots), [])
        self.assertIn("even as a JPEG", shots[0]["not_inline"])

    def test_a_missing_or_strange_copy_is_left_out(self):
        shots = [{"path": "a.png"}, shot(10, "image/gif"), "not a dict"]
        self.assertEqual(snapshot_content.image_blocks(shots), [])
        self.assertEqual([s.get("not_inline") is not None for s in shots[:2]], [True, True])

    def call_snapshot(self, arguments, reply):
        instances = vegas_mcp.Instances()
        instances.call = lambda name, args: reply
        return vegas_mcp.call_tool(instances, "snapshot", arguments)

    def test_a_snapshot_result_is_its_text_then_its_images(self):
        result = self.call_snapshot({"time": 1}, {"ok": True, "result": {"snapshots": [shot(100)]}})
        self.assertFalse(result["isError"])
        self.assertEqual([c["type"] for c in result["content"]], ["text", "image"])
        self.assertIn("D:\\\\work\\\\snapshots\\\\a.png", result["content"][0]["text"])
        self.assertNotIn("AAAA", result["content"][0]["text"])

    def test_inline_false_sends_only_the_text(self):
        result = self.call_snapshot({"time": 1, "inline": False}, {"ok": True, "result": {"snapshots": [{"path": "a.png"}]}})
        self.assertEqual([c["type"] for c in result["content"]], ["text"])
        self.assertNotIn("not_inline", result["content"][0]["text"])

    def test_dialogs_still_reach_the_text_beside_the_images(self):
        reply = {"ok": True, "result": {"snapshots": [shot(100)]}, "dialogs": ["Hint: OK"]}
        result = self.call_snapshot({"time": 1}, reply)
        self.assertEqual([c["type"] for c in result["content"]], ["text", "image"])
        self.assertIn("dialogs_answered", result["content"][0]["text"])

    def test_a_failed_snapshot_is_an_error_with_no_images(self):
        result = self.call_snapshot({"time": 99}, {"ok": False, "error": "time 99 s is past the end of the project"})
        self.assertTrue(result["isError"])
        self.assertEqual([c["type"] for c in result["content"]], ["text"])


if __name__ == "__main__":
    unittest.main()
