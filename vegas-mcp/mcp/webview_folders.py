"""A WebView2 user data folder of its own for each bridge VEGAS (#1050).

VEGAS's Hub runs in WebView2 with its user data in %LOCALAPPDATA%\\VEGAS
Pro\\21.0\\WebView2, the same folder for every VEGAS. WebView2 lets only one
browser process use a folder, and one started with other settings (here: on
another desktop) is refused with ERROR_BUSY. So with a bridge VEGAS on its
hidden desktop holding that folder, the user's VEGAS opened beside it showed
"VEGAS Hub: WebView2 failed initialization ... The requested resource is in
use (0x800700AA)" on their screen. WebView2 takes the folder from the
WEBVIEW2_USER_DATA_FOLDER environment variable over the one the program
asks for, so launch gives each bridge VEGAS a folder under
%TEMP%\\dodstudio-vegas-webview2 and the user's folder is never opened.

Each folder is removed once its VEGAS has gone. A folder left by a server
that was killed outright is removed by a later launch: each folder holds
the PID of its VEGAS (vegas.pid), and one whose VEGAS is not running goes.

Standard library only.
"""

import os
import secrets
import shutil
import tempfile
import time

VARIABLE = "WEBVIEW2_USER_DATA_FOLDER"
PID_FILE = "vegas.pid"
# Every folder new_folder makes starts with this; the sweep touches nothing else.
PREFIX = "vegas-"
# A folder made by a launch that has not written its PID yet is left alone this long.
UNCLAIMED_GRACE_SECONDS = 600
# WebView2's processes go a moment after their VEGAS; removal retries this long.
REMOVE_WAIT_SECONDS = 5.0
REMOVE_POLL_SECONDS = 0.25


def parent():
    return os.path.join(tempfile.gettempdir(), "dodstudio-vegas-webview2")


def new_folder(root=None):
    """A fresh, empty folder for one VEGAS's WebView2 data."""
    path = os.path.join(root or parent(), "%s%d-%s" % (PREFIX, os.getpid(), secrets.token_hex(4)))
    os.makedirs(path)
    return path


def claim(path, pid):
    """Record which VEGAS uses the folder, so a later sweep knows when it has gone."""
    with open(os.path.join(path, PID_FILE), "w") as f:
        f.write(str(pid))


def remove(path, wait_seconds=REMOVE_WAIT_SECONDS):
    """Remove a folder whose VEGAS has gone; its WebView2 processes may hold
    files a moment longer. Returns what it did."""
    deadline = time.monotonic() + wait_seconds
    while True:
        shutil.rmtree(path, ignore_errors=True)
        if not os.path.exists(path):
            return "removed its WebView2 folder"
        if time.monotonic() >= deadline:
            return "left its WebView2 folder %s (still in use)" % path
        time.sleep(REMOVE_POLL_SECONDS)


def sweep(is_vegas, root=None):
    """Remove the folders of bridge VEGAS that are no longer running (a server
    killed outright never removed its own). is_vegas(pid) says whether a PID
    is a running VEGAS. Only folders new_folder made are touched. Returns the
    folders removed."""
    root = root or parent()
    try:
        names = os.listdir(root)
    except OSError:
        return []
    removed = []
    for name in names:
        path = os.path.join(root, name)
        if not name.startswith(PREFIX) or not os.path.isdir(path):
            continue
        pid = claimed_pid(path)
        if pid is None:
            try:
                fresh = time.time() - os.path.getmtime(path) < UNCLAIMED_GRACE_SECONDS
            except OSError:
                continue
            if fresh:
                continue  # a launch in progress
        elif is_vegas(pid):
            continue
        if remove(path, wait_seconds=0) == "removed its WebView2 folder":
            removed.append(path)
    return removed


def claimed_pid(path):
    """The PID claim() wrote in a folder, or None while it has none: no file,
    an empty one (claim's write not finished) or one that is not a PID."""
    try:
        with open(os.path.join(path, PID_FILE)) as f:
            text = f.read().strip()
    except OSError:
        return None
    try:
        pid = int(text)
    except ValueError:
        return None
    return pid if pid > 0 else None
