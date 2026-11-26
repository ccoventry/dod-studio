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

end() stops only a VEGAS that has a bridge pipe or that this process started,
and checks the process is VEGAS first, so the user's VEGAS is never ended.
It ends it by PID, never with vegas.Exit(): that asks "cancel the background
operation?" on screen while peaks build. VEGAS deletes its autosave
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
    ).stdout
    pids = []
    for line in out.splitlines():
        cells = [c.strip('"') for c in line.split('","')]
        if len(cells) > 1 and cells[1].isdigit():
            pids.append(int(cells[1]))
    return sorted(pids)


def launch(root=None, wait_seconds=120, minimised=True):
    """Start a VEGAS for the bridge and wait for its pipe. Returns a dict with its pid.

    minimised=False shows it (still without taking focus), to watch it work."""
    root = os.path.abspath(root or default_root())
    os.makedirs(root, exist_ok=True)
    exe = vegas_exe()
    if not os.path.isfile(exe):
        raise VegasError("VEGAS not found at %s (set DODSTUDIO_VEGAS_EXE)" % exe)
    others = vegas_pids()
    bridged = bridge_pids()
    env = dict(os.environ)
    env[FLAG_VARIABLE] = "1"
    env[ROOT_VARIABLE] = root
    si = subprocess.STARTUPINFO()
    si.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    # SW_SHOWMINNOACTIVE (7) or SW_SHOWNOACTIVATE (4): never take focus from the user's window.
    si.wShowWindow = 7 if minimised else 4
    # A command-line string, so the script path is quoted after the colon the
    # way VEGAS reads it (-SCRIPT:"C:\a b\start_vegas.cs").
    command = '"%s" -SCRIPT:"%s"' % (exe, START_SCRIPT)
    proc = subprocess.Popen(command, env=env, startupinfo=si, close_fds=True)
    deadline = time.monotonic() + wait_seconds
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            cleaned = cleanup(proc.pid)
            raise VegasError("VEGAS (PID %d) exited during start-up, exit code %s; %s" % (proc.pid, proc.returncode, cleaned))
        if proc.pid in bridge_pids():
            # A VEGAS with a bridge pipe was started by a launcher (another
            # session's, say), so only the ones without one are the user's.
            return {
                "pid": proc.pid,
                "pipe": pipe_path(proc.pid),
                "work_folder": root,
                "other_vegas": others,
                "users_vegas": [p for p in others if p not in bridged],
                "other_bridge_vegas": [p for p in others if p in bridged],
            }
        time.sleep(0.25)
    end(proc.pid, launched=True)
    raise VegasError(
        "VEGAS (PID %d) started but its bridge pipe never opened in %d s: is DodStudioVegasBridge.dll in "
        "%%APPDATA%%\\VEGAS Pro\\21.0\\Application Extensions? Ended that VEGAS." % (proc.pid, wait_seconds)
    )


def close_problem_reports(pid):
    """Close VEGAS's crash 'Problem Report' box for one crashed VEGAS, unsent.

    It is ErrorReportClient.exe with VEGASApp<pid>.erpt on its command line,
    and its parent is not that VEGAS (frag-editor, 2026-11-24), so it is found
    by that PID, which leaves the user's own reports alone.
    """
    script = (
        "Get-CimInstance Win32_Process -Filter \"Name='ErrorReportClient.exe'\" | "
        "Where-Object { $_.CommandLine -match 'VEGASApp%d\\.erpt' } | "
        "ForEach-Object { Stop-Process -Id $_.ProcessId -Force; $_.ProcessId }" % pid
    )
    out = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
        capture_output=True, text=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    ).stdout
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


def end(pid, launched=False):
    """End one bridge VEGAS by PID and clean up after it. Refuses any other process."""
    if not launched and not is_running(pid):
        # Not ours to clean up: an exited VEGAS's autosave may be the user's
        # recovery file.
        return "VEGAS PID %d has already exited; nothing done" % pid
    if not launched and pid not in bridge_pids():
        raise VegasError("PID %d has no bridge pipe: not a VEGAS started for the bridge, so it is left alone" % pid)
    if is_running(pid):
        if not is_vegas(pid):
            raise VegasError("PID %d is not VEGAS (%s): left alone" % (pid, image_path(pid)))
        h = _open(pid, PROCESS_TERMINATE | SYNCHRONIZE)
        if not h:
            raise VegasError("cannot open VEGAS PID %d to end it (error %d)" % (pid, ctypes.get_last_error()))
        try:
            _k32.TerminateProcess(h, 1)
            if _k32.WaitForSingleObject(h, 15000) != WAIT_OBJECT_0:
                raise VegasError("VEGAS PID %d did not exit within 15 s of being ended" % pid)
        finally:
            _k32.CloseHandle(h)
        what = "ended VEGAS PID %d" % pid
    else:
        what = "VEGAS PID %d had already exited" % pid
    return what + "; " + cleanup(pid)
