"""A desktop of its own for each bridge VEGAS, so the user never sees it (#1050).

Started minimised and without activation on the user's desktop, VEGAS still
showed its start-up splash over a full-screen game, and its render progress
windows too. A desktop made with CreateDesktop in the user's window station
(WinSta0) is never shown: only the input desktop is drawn on screen, and
switching to another takes SwitchDesktop, which nothing here calls. A process
started with STARTUPINFO.lpDesktop naming it has every window there, its
splash, dialogs and child processes (a crash's Problem Report) included.
Window messages pass only between threads on the same desktop, so the
bridge's dialog answerer, which runs inside VEGAS, still reaches its boxes.

subprocess cannot name a desktop (its STARTUPINFO has no lpDesktop), so the
VEGAS is started with CreateProcessW here, and Process stands in for Popen:
the same poll/wait/kill on the process handle it keeps.

A desktop lives while a handle to it is open: the launcher's (closed when it
ends that VEGAS) or the threads of a process running on it, so it goes away
with the last of them, also when the server is killed outright.

Standard library only.
"""

import ctypes
import ctypes.wintypes as wt
import os
import subprocess

GENERIC_ALL = 0x10000000
DESKTOP_READOBJECTS = 0x0001
DESKTOP_ENUMERATE = 0x0040
CREATE_UNICODE_ENVIRONMENT = 0x00000400
STARTF_USESHOWWINDOW = 0x00000001
INFINITE = 0xFFFFFFFF
WAIT_OBJECT_0 = 0
WAIT_TIMEOUT = 0x102
KILL_GRACE_MS = 2000
GW_OWNER = 4
# The windows a call stuck in VEGAS may be waiting on: Windows dialog boxes,
# WinForms forms, and shown WPF windows (a render's progress is one).
DIALOG_CLASSES = ("#32770", "WindowsForms10.Window")
WPF_CLASS = "HwndWrapper"
MAX_WINDOWS_LISTED = 6


class STARTUPINFOW(ctypes.Structure):
    _fields_ = [
        ("cb", wt.DWORD), ("lpReserved", wt.LPWSTR), ("lpDesktop", wt.LPWSTR), ("lpTitle", wt.LPWSTR),
        ("dwX", wt.DWORD), ("dwY", wt.DWORD), ("dwXSize", wt.DWORD), ("dwYSize", wt.DWORD),
        ("dwXCountChars", wt.DWORD), ("dwYCountChars", wt.DWORD), ("dwFillAttribute", wt.DWORD),
        ("dwFlags", wt.DWORD), ("wShowWindow", wt.WORD), ("cbReserved2", wt.WORD), ("lpReserved2", ctypes.c_void_p),
        ("hStdInput", wt.HANDLE), ("hStdOutput", wt.HANDLE), ("hStdError", wt.HANDLE),
    ]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("hProcess", wt.HANDLE), ("hThread", wt.HANDLE), ("dwProcessId", wt.DWORD), ("dwThreadId", wt.DWORD)]


_k32 = ctypes.WinDLL("kernel32", use_last_error=True) if os.name == "nt" else None
_u32 = ctypes.WinDLL("user32", use_last_error=True) if os.name == "nt" else None
# WINFUNCTYPE exists only on Windows; elsewhere the module still imports, so
# the unit tests that need no Windows run and the rest skip.
_ENUM_WINDOWS = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM) if os.name == "nt" else None
_ENUM_DESKTOPS = ctypes.WINFUNCTYPE(wt.BOOL, wt.LPWSTR, wt.LPARAM) if os.name == "nt" else None
if _k32 is not None:
    _k32.CreateProcessW.argtypes = [wt.LPCWSTR, wt.LPWSTR, ctypes.c_void_p, ctypes.c_void_p, wt.BOOL, wt.DWORD,
                                    ctypes.c_void_p, wt.LPCWSTR, ctypes.POINTER(STARTUPINFOW), ctypes.POINTER(PROCESS_INFORMATION)]
    _k32.CloseHandle.argtypes = [wt.HANDLE]
    _k32.WaitForSingleObject.argtypes = [wt.HANDLE, wt.DWORD]
    _k32.WaitForSingleObject.restype = wt.DWORD
    _k32.GetExitCodeProcess.argtypes = [wt.HANDLE, ctypes.POINTER(wt.DWORD)]
    _k32.TerminateProcess.argtypes = [wt.HANDLE, wt.UINT]
    _u32.CreateDesktopW.restype = wt.HANDLE
    _u32.CreateDesktopW.argtypes = [wt.LPCWSTR, wt.LPCWSTR, ctypes.c_void_p, wt.DWORD, wt.DWORD, ctypes.c_void_p]
    _u32.OpenDesktopW.restype = wt.HANDLE
    _u32.OpenDesktopW.argtypes = [wt.LPCWSTR, wt.DWORD, wt.BOOL, wt.DWORD]
    _u32.CloseDesktop.argtypes = [wt.HANDLE]
    _u32.GetProcessWindowStation.restype = wt.HANDLE
    _u32.EnumDesktopsW.argtypes = [wt.HANDLE, _ENUM_DESKTOPS, wt.LPARAM]
    _u32.EnumDesktopWindows.argtypes = [wt.HANDLE, _ENUM_WINDOWS, wt.LPARAM]
    _u32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
    _u32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
    _u32.GetClassNameW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
    _u32.IsWindowEnabled.argtypes = [wt.HWND]
    _u32.IsWindowVisible.argtypes = [wt.HWND]
    _u32.GetWindow.argtypes = [wt.HWND, wt.UINT]
    _u32.GetWindow.restype = wt.HWND


class DesktopError(OSError):
    pass


class Desktop:
    """A desktop in this process's window station that is never shown."""

    def __init__(self, name):
        self.name = name
        self.handle = _u32.CreateDesktopW(name, None, None, 0, GENERIC_ALL, None)
        if not self.handle:
            raise DesktopError("cannot create the desktop %s (error %d)" % (name, ctypes.get_last_error()))

    @property
    def path(self):
        """The name STARTUPINFO.lpDesktop takes: window station, then desktop."""
        return "WinSta0\\" + self.name

    def close(self):
        if self.handle:
            _u32.CloseDesktop(self.handle)
            self.handle = None


class Process:
    """A process started by start(), driven like a Popen through its handle."""

    def __init__(self, handle, pid):
        self._handle = handle
        self.pid = pid
        self.returncode = None

    def poll(self):
        if self.returncode is None and _k32.WaitForSingleObject(self._handle, 0) == WAIT_OBJECT_0:
            code = wt.DWORD()
            _k32.GetExitCodeProcess(self._handle, ctypes.byref(code))
            self.returncode = code.value
        return self.returncode

    def wait(self, timeout=None):
        ms = INFINITE if timeout is None else int(timeout * 1000)
        if _k32.WaitForSingleObject(self._handle, ms) == WAIT_TIMEOUT:
            raise subprocess.TimeoutExpired("process %d" % self.pid, timeout)
        return self.poll()

    def kill(self):
        if self.poll() is None and not _k32.TerminateProcess(self._handle, 1):
            # A process already being terminated refuses a second
            # TerminateProcess (access denied) until it has gone.
            error = ctypes.get_last_error()
            if _k32.WaitForSingleObject(self._handle, KILL_GRACE_MS) != WAIT_OBJECT_0:
                raise OSError("cannot end PID %d (error %d)" % (self.pid, error))

    def __del__(self):
        if self._handle:
            _k32.CloseHandle(self._handle)
            self._handle = None


def environment_block(env):
    """A CreateProcessW Unicode environment block, sorted as Windows expects."""
    lines = ["%s=%s" % (k, v) for k, v in sorted(env.items(), key=lambda kv: kv[0].upper())]
    return ctypes.create_unicode_buffer("\0".join(lines) + "\0\0")


def start(command, env, show_window, desktop=None):
    """Start `command` (a full command line) with `env`, on `desktop` (a
    Desktop) or the caller's own. Returns a Process."""
    si = STARTUPINFOW()
    si.cb = ctypes.sizeof(si)
    si.dwFlags = STARTF_USESHOWWINDOW
    si.wShowWindow = show_window
    if desktop is not None:
        si.lpDesktop = desktop.path
    pi = PROCESS_INFORMATION()
    block = environment_block(env)
    line = ctypes.create_unicode_buffer(command)  # CreateProcessW may write into it
    if not _k32.CreateProcessW(None, line, None, None, False, CREATE_UNICODE_ENVIRONMENT, block, None,
                               ctypes.byref(si), ctypes.byref(pi)):
        raise OSError("cannot start %s (error %d)" % (command, ctypes.get_last_error()))
    _k32.CloseHandle(pi.hThread)
    return Process(pi.hProcess, pi.dwProcessId)


def desktop_names():
    """Every desktop of this process's window station."""
    names = []

    def each(name, _):
        names.append(name)
        return True

    _u32.EnumDesktopsW(_u32.GetProcessWindowStation(), _ENUM_DESKTOPS(each), 0)
    return names


def windows_of(pid, only_visible=False):
    """(desktop, hwnd, title, class, owned, enabled, visible) of every top-level
    window of one process, on every desktop of this window station that can be
    read (a desktop of another bridge VEGAS's too; Winlogon cannot be)."""
    found = []
    for name in desktop_names():
        h = _u32.OpenDesktopW(name, 0, False, DESKTOP_READOBJECTS | DESKTOP_ENUMERATE)
        if not h:
            continue
        try:
            def each(hwnd, _):
                owner = wt.DWORD()
                _u32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
                if owner.value == pid:
                    visible = bool(_u32.IsWindowVisible(hwnd))
                    if visible or not only_visible:
                        title = ctypes.create_unicode_buffer(256)
                        cls = ctypes.create_unicode_buffer(128)
                        _u32.GetWindowTextW(hwnd, title, 256)
                        _u32.GetClassNameW(hwnd, cls, 128)
                        found.append((name, hwnd, title.value, cls.value, bool(_u32.GetWindow(hwnd, GW_OWNER)),
                                      bool(_u32.IsWindowEnabled(hwnd)), visible))
                return True

            _u32.EnumDesktopWindows(h, _ENUM_WINDOWS(each), 0)
        finally:
            _u32.CloseDesktop(h)
    return found


def dialog_windows(pid):
    """The enabled dialog-like windows of one process, as 'title' (class):
    what a call stuck in that VEGAS may be waiting on."""
    out = []
    for _desk, _h, title, cls, _owned, enabled, visible in windows_of(pid):
        if not enabled or not title:
            continue
        # A minimised VEGAS hides the boxes its windows own, so a Windows or
        # WinForms dialog counts hidden too; WPF keeps hidden helper windows
        # (MediaContextNotificationWindow), so a WPF one counts only shown.
        if cls.startswith(DIALOG_CLASSES) or (cls.startswith(WPF_CLASS) and visible):
            out.append("'%s' (%s)" % (title, cls))
    return out[:MAX_WINDOWS_LISTED]
