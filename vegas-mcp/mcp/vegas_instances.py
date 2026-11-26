"""Starting, finding and ending the VEGAS processes the bridge runs in (#1050).

The bridge extension loads into every VEGAS, the user's own editing VEGAS
too, and listens only in one started here: launch() sets
DODSTUDIO_VEGAS_BRIDGE=1 and DODSTUDIO_VEGAS_ROOT (the one folder it may
write in) in the new VEGAS's environment. Each bridge VEGAS has its own pipe,
dodstudio-vegas-<pid>, so several can run beside the user's.

VEGAS starts with -SCRIPT: and a script that does nothing. Started that way
it opens an empty untitled project instead of its last one, which is often
the user's (measured on VEGAS Pro 21 build 315: the recent-projects list was
unchanged afterwards too).

A VEGAS this process started is ended through its own process handle
(Launched), which also keeps its PID from being reused; any other is ended
only when it is running VEGAS with a bridge pipe (end_bridge_pid), so the
user's VEGAS is never ended. Never with vegas.Exit(): that asks "cancel the
background operation?" on screen while peaks build. VEGAS deletes its autosave
(%LOCALAPPDATA%\\VEGAS Pro\\21.0\\<pid as 8 hex digits>.autosave.veg) only on
a clean exit, and a leftover one makes the next VEGAS ask to recover it, so
end() deletes that PID's autosave, and closes a crash's Problem Report box for
that PID, which would otherwise stay on screen.

Standard library only.
"""

import ctypes
import ctypes.wintypes as wt
import glob
import os
import subprocess
import tempfile
import time

PIPE_DIR = "\\\\.\\pipe\\"
PIPE_PREFIX = "dodstudio-vegas-"
FLAG_VARIABLE = "DODSTUDIO_VEGAS_BRIDGE"
ROOT_VARIABLE = "DODSTUDIO_VEGAS_ROOT"
DEFAULT_EXE = "C:\\Program Files\\VEGAS\\VEGAS Pro 21.0\\vegas210.exe"
START_SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "start_vegas.cs")

PROCESS_TERMINATE = 0x0001
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
SYNCHRONIZE = 0x00100000
STILL_ACTIVE = 259
WAIT_OBJECT_0 = 0
# tasklist and the PowerShell that closes a Problem Report take about a second.
HELPER_TIMEOUT_SECONDS = 30

_k32 = ctypes.WinDLL("kernel32", use_last_error=True) if os.name == "nt" else None
if _k32 is not None:
    _k32.OpenProcess.restype = wt.HANDLE
    _k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
    _k32.CloseHandle.argtypes = [wt.HANDLE]
    _k32.GetExitCodeProcess.argtypes = [wt.HANDLE, ctypes.POINTER(wt.DWORD)]
    _k32.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]
    _k32.WaitForSingleObject.argtypes = [wt.HANDLE, wt.DWORD]
    _k32.WaitForSingleObject.restype = wt.DWORD
    _k32.QueryFullProcessImageNameW.argtypes = [wt.HANDLE, wt.DWORD, wt.LPWSTR, ctypes.POINTER(wt.DWORD)]


class VegasError(Exception):
    pass


def vegas_exe():
    return os.environ.get("DODSTUDIO_VEGAS_EXE") or DEFAULT_EXE


def default_root():
    return os.environ.get(ROOT_VARIABLE) or os.path.join(tempfile.gettempdir(), "dodstudio-vegas")


def pipe_path(pid):
    return PIPE_DIR + PIPE_PREFIX + str(pid)


def pid_from_pipe_name(name):
    """The PID in a 'dodstudio-vegas-<pid>' pipe name, else None."""
    if not name.startswith(PIPE_PREFIX):
        return None
    rest = name[len(PIPE_PREFIX):]
    return int(rest) if rest.isdigit() else None


def bridge_pids():
    """PIDs of the VEGAS processes whose bridge pipe is open right now."""
    try:
        names = os.listdir(PIPE_DIR)
    except OSError:
        return []
    return sorted(p for p in (pid_from_pipe_name(n) for n in names) if p is not None)


def autosave_files(pid, folder=None):
    """VEGAS's autosave files for one process: <pid as 8 hex digits>.autosave.veg and its .bak."""
    folder = folder or os.path.join(os.environ.get("LOCALAPPDATA", ""), "VEGAS Pro", "21.0")
    return sorted(glob.glob(os.path.join(folder, "%08x.autosave.veg*" % pid)))


def _open(pid, access):
    h = _k32.OpenProcess(access, False, pid)
    return h or None


def is_running(pid):
    h = _open(pid, PROCESS_QUERY_LIMITED_INFORMATION)
    if not h:
        return False
    try:
        code = wt.DWORD()
        return bool(_k32.GetExitCodeProcess(h, ctypes.byref(code))) and code.value == STILL_ACTIVE
    finally:
        _k32.CloseHandle(h)


def image_path(pid):
    h = _open(pid, PROCESS_QUERY_LIMITED_INFORMATION)
    if not h:
        return None
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wt.DWORD(len(buf))
        if not _k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            return None
        return buf.value
    finally:
        _k32.CloseHandle(h)


def is_vegas(pid):
    path = image_path(pid)
    return bool(path) and os.path.basename(path).lower().startswith("vegas") and path.lower().endswith(".exe")


def vegas_pids():
    """Every running VEGAS (the user's included), from tasklist."""
    out = subprocess.run(
        ["tasklist", "/FO", "CSV", "/NH", "/FI", "IMAGENAME eq vegas*"],
        capture_output=True, text=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        timeout=HELPER_TIMEOUT_SECONDS,
    ).stdout
    pids = []
    for line in out.splitlines():
        cells = [c.strip('"') for c in line.split('","')]
        if len(cells) > 1 and cells[1].isdigit():
            pids.append(int(cells[1]))
    return sorted(pids)


def is_inside(path, folder):
    """Whether path is folder itself or below it (both made absolute, any case)."""
    path = os.path.normcase(os.path.abspath(path))
    folder = os.path.normcase(os.path.abspath(folder))
    return path == folder or path.startswith(folder.rstrip(os.sep) + os.sep)


def checked_work_folder(requested):
    """The work folder launch may give a VEGAS: never a drive root, and inside
    DODSTUDIO_VEGAS_ROOT when the server has one, so a caller cannot widen the
    folder the bridge writes and opens projects in to the user's own files."""
    allowed = os.environ.get(ROOT_VARIABLE)
    root = os.path.abspath(requested or default_root())
    if os.path.dirname(root.rstrip(os.sep)) == root.rstrip(os.sep) or os.path.splitdrive(root)[1] in ("", os.sep):
        raise VegasError("the work folder cannot be a drive root (%s)" % root)
    if allowed and not is_inside(root, allowed):
        raise VegasError("the work folder %s is outside this server's %s %s" % (root, ROOT_VARIABLE, os.path.abspath(allowed)))
    return root


def orphan_autosaves(folder=None):
    """Autosaves left by a VEGAS that is no longer running: a crashed session's
    recovery file. A new VEGAS offers to restore it, and the bridge's dialog
    answerer must never be the one to answer that, so launch refuses while
    one exists."""
    folder = folder or os.path.join(os.environ.get("LOCALAPPDATA", ""), "VEGAS Pro", "21.0")
    found = []
    for path in glob.glob(os.path.join(folder, "*.autosave.veg")):
        stem = os.path.basename(path).split(".")[0]
        try:
            pid = int(stem, 16)
        except ValueError:
            continue
        if not is_running(pid):
            found.append(path)
    return sorted(found)


class Launched:
    """A VEGAS this process started. Holding its Popen keeps the process handle
    open, so Windows cannot give its PID to another process (the user's next
    VEGAS) while this record exists, and ending it goes through that handle."""

    def __init__(self, proc):
        self.proc = proc
        self.pid = proc.pid

    def exited(self):
        return self.proc.poll() is not None

    def end(self):
        """Terminate it through its own handle, then clean up after it."""
        if not self.exited():
            self.proc.kill()  # TerminateProcess on the handle Popen holds
            try:
                self.proc.wait(END_WAIT_SECONDS)
            except subprocess.TimeoutExpired:
                raise VegasError("VEGAS PID %d did not exit within %d s of being ended" % (self.pid, END_WAIT_SECONDS))
            what = "ended VEGAS PID %d" % self.pid
        else:
            what = "VEGAS PID %d had already exited" % self.pid
        return what + "; " + cleanup(self.pid)


END_WAIT_SECONDS = 15
# VEGAS takes 7-14 s to open its pipe on this PC; this allows a slow disk.
DEFAULT_WAIT_SECONDS = 120
STARTUP_POLL_SECONDS = 0.25
SW_SHOWMINNOACTIVE = 7
SW_SHOWNOACTIVATE = 4


def launch(root=None, wait_seconds=DEFAULT_WAIT_SECONDS, minimised=True):
    """Start a VEGAS for the bridge and wait for its pipe. Returns (info dict, Launched).

    minimised=False shows it (still without taking focus), to watch it work."""
    wait_seconds = float(wait_seconds)
    root = checked_work_folder(root)
    exe = vegas_exe()
    if not os.path.isfile(exe):
        raise VegasError("VEGAS not found at %s (set DODSTUDIO_VEGAS_EXE)" % exe)
    orphans = orphan_autosaves()
    if orphans:
        raise VegasError(
            "not starting a bridge VEGAS: %s is the autosave of a VEGAS that did not exit cleanly, and a new VEGAS "
            "would offer to restore it. Open VEGAS yourself to recover or discard it first." % ", ".join(orphans)
        )
    try:
        os.makedirs(root, exist_ok=True)
    except OSError as e:
        raise VegasError("cannot create the work folder %s: %s" % (root, e))
    others = vegas_pids()
    bridged = bridge_pids()
    env = dict(os.environ)
    env[FLAG_VARIABLE] = "1"
    env[ROOT_VARIABLE] = root
    si = subprocess.STARTUPINFO()
    si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    # Never take focus from the user's window.
    si.wShowWindow = SW_SHOWMINNOACTIVE if minimised else SW_SHOWNOACTIVATE
    # A command-line string, so the script path is quoted after the colon the
    # way VEGAS reads it (-SCRIPT:"C:\a b\start_vegas.cs").
    command = '"%s" -SCRIPT:"%s"' % (exe, START_SCRIPT)
    launched = Launched(subprocess.Popen(command, env=env, startupinfo=si, close_fds=True))
    try:
        deadline = time.monotonic() + wait_seconds
        while time.monotonic() < deadline:
            if launched.exited():
                raise VegasError("VEGAS (PID %d) exited during start-up, exit code %s; %s"
                                 % (launched.pid, launched.proc.returncode, cleanup(launched.pid)))
            if launched.pid in bridge_pids():
                # A VEGAS with a bridge pipe was started by a launcher (another
                # session's, say), so only the ones without one are the user's.
                return {
                    "pid": launched.pid,
                    "pipe": pipe_path(launched.pid),
                    "work_folder": root,
                    "other_vegas": others,
                    "users_vegas": [p for p in others if p not in bridged],
                    "other_bridge_vegas": [p for p in others if p in bridged],
                }, launched
            time.sleep(STARTUP_POLL_SECONDS)
        raise VegasError(
            "VEGAS (PID %d) started but its bridge pipe never opened in %d s: is DodStudioVegasBridge.dll in "
            "%%APPDATA%%\\VEGAS Pro\\21.0\\Application Extensions?" % (launched.pid, wait_seconds)
        )
    except BaseException:
        # Whatever went wrong after the start, that VEGAS is ours and must not stay.
        launched.end()
        raise


def close_problem_reports(pid):
    """Close VEGAS's crash 'Problem Report' box for one crashed VEGAS, unsent.

    It is ErrorReportClient.exe with VEGASApp<pid>.erpt on its command line,
    and its parent is not that VEGAS (measured 2026-11-24), so it is found
    by that PID, which leaves the user's own reports alone.
    """
    script = (
        "Get-CimInstance Win32_Process -Filter \"Name='ErrorReportClient.exe'\" | "
        "Where-Object { $_.CommandLine -match 'VEGASApp%d\\.erpt' } | "
        "ForEach-Object { Stop-Process -Id $_.ProcessId -Force; $_.ProcessId }" % pid
    )
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
            capture_output=True, text=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            timeout=HELPER_TIMEOUT_SECONDS,
        ).stdout
    except subprocess.TimeoutExpired:
        return []
    return [int(x) for x in out.split() if x.isdigit()]


def cleanup(pid):
    """After a bridge VEGAS is gone: its Problem Report box and its autosave. Returns what it did."""
    done = []
    for report in close_problem_reports(pid):
        done.append("closed its Problem Report (PID %d) unsent" % report)
    time.sleep(0.5)  # let VEGAS's file handles go
    for path in autosave_files(pid):
        try:
            os.remove(path)
            done.append("removed its autosave %s" % os.path.basename(path))
        except OSError as e:
            done.append("could not remove its autosave %s: %s" % (path, e))
    return "; ".join(done) or "nothing left behind"


def end_bridge_pid(pid):
    """End a bridge VEGAS this process did not start (the CLI's end, or one
    reached with use), by PID. It must be running, be VEGAS and have a bridge
    pipe, all checked through the one handle that then ends it, so the PID
    cannot change hands in between. Anything else is left alone, an exited
    one too: its autosave may be the user's recovery file."""
    h = _open(pid, PROCESS_TERMINATE | SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION)
    if not h:
        return "VEGAS PID %d is not running; nothing done" % pid
    try:
        code = wt.DWORD()
        if not _k32.GetExitCodeProcess(h, ctypes.byref(code)) or code.value != STILL_ACTIVE:
            return "VEGAS PID %d has already exited; nothing done" % pid
        buf = ctypes.create_unicode_buffer(1024)
        size = wt.DWORD(len(buf))
        image = buf.value if _k32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)) else ""
        if not (os.path.basename(image).lower().startswith("vegas") and image.lower().endswith(".exe")):
            raise VegasError("PID %d is not VEGAS (%s): left alone" % (pid, image or "unknown"))
        if pid not in bridge_pids():
            raise VegasError("PID %d has no bridge pipe: not a VEGAS started for the bridge, so it is left alone" % pid)
        if not _k32.TerminateProcess(h, 1):
            raise VegasError("cannot end VEGAS PID %d (error %d)" % (pid, ctypes.get_last_error()))
        if _k32.WaitForSingleObject(h, END_WAIT_SECONDS * 1000) != WAIT_OBJECT_0:
            raise VegasError("VEGAS PID %d did not exit within %d s of being ended" % (pid, END_WAIT_SECONDS))
    finally:
        _k32.CloseHandle(h)
    return "ended VEGAS PID %d; %s" % (pid, cleanup(pid))
