#!/usr/bin/env python3
r"""Runs one scripted in-game test and writes a report: launch, steps, verdict.

Launches Day of Defeat the way DoD Studio does (HLAE + the goldsrc-hooks DLL,
windowed, `-condebug`), optionally plays a demo, then runs a list of steps:
console commands over the hook's remote pipe (#413), waits, screenshots of the
game window, and checks against the game's console log and the hook log. It
always ends the game it started, and only that one.

## Refuses to run unless

- Steam is signed into the one account in-game tests may use, named in a
  local file outside the repo: %APPDATA%\dod-studio\game_probe.json,
  `{"allowed_steam_account_id": <32-bit account id>, "label": "<any name>"}`.
  No file, no test. Checked from the registry right before launch; the
  account id is SteamID64 minus 76561197960265728.
- no `hl.exe` is running already (the user's own game is never touched);
- the install is one of the two movie installs (PRE or POST-Anniversary for
  Movies), never the stock Half-Life one.

## Steps (`--step`, repeatable, run in order)

    wait <seconds>              sleep, but stop early if the game exits
    waitfor <regex> [seconds]   wait until the regex appears in the console or
                                hook log since launch (default 60 s)
    cmd <console command>       send it over the remote pipe
    shot <name>                 a frame recorded by HLAE, saved as <run>/<name>.png
    window                      note the game window's size, minimised and foreground state
    clip <name> [secs] [fps]    record HLAE frames for a few seconds into <run>/<name>/
                                and a contact sheet <run>/<name>.png (default 2 s, 30 fps);
                                demo seconds while a demo plays
    grab <name>                 screen capture of the game window (VGUI panels included);
                                needs focus first
    focus                       bring the game window to the front (25th Anniversary
                                frames are black while it's behind other windows)
    key <name>                  press and release one key in the game window: esc,
                                enter, tab, space, backquote, backspace, up, down,
                                left, right, f1..f12, or a letter
                                or digit. Focuses the game first, and refuses unless
                                the game's window really is in front
    click <x> <y> [2]           left-click at a point in the game window, in the same
                                pixels as a `grab` image (client area, top left 0 0);
                                a trailing 2 double-clicks. Same focus check as `key`
    drag <x1> <y1> <x2> <y2>    hold the left button at one point and release it at
                                another (moving or resizing a window), same pixels
    expect <regex>              check the regex appears in either log (or in
                                the events pipe's lines, once listening)
    expect_not <regex>          check it doesn't
    listen_events               read the game's events pipe from here on, as
                                Studio does during a batch (#434)
    expect_exit <seconds>       check the game exits on its own within the
                                time; only expect/expect_not may follow
    ffwd <demo seconds> [secs]  fast-forward the demo to that demo time, then
                                back to normal speed (gives up after `secs`, default 300)
    spectate <index> [tries]    step the in-eye camera (+attack) to the player
                                with that entity index; reads the animation fix's
                                log, so it needs dodstudio_hltv_show_viewmodel_animations 1

Examples:

    python goldsrc-hooks/tools/game_probe.py --install pre --demo ktps8w1-m00cat_soul_lenn_h2 ^
        --step "waitfor Playing demo from ktps8w1-m00cat" --step "wait 8" ^
        --step "cmd dodstudio_deathmsg players" --step "wait 1" ^
        --step "expect STEAM_0:1:6155141" --step "shot kill-feed"

    # A first-demo crash: start the demo at launch, as a batch does (#546),
    # with a hook DLL built elsewhere.
    python goldsrc-hooks/tools/game_probe.py --install pre --at-launch ^
        --demo ktps8w1-stealth_soul_lenn_h1 --dll <path to dodstudio_goldsrc_hooks.dll> ^
        --step "waitfor Playing demo from dodstudio_probe" --step "wait 20" --step "shot after"

    python goldsrc-hooks/tools/game_probe.py --check     # only the refusal checks

A demo started from the menu goes through the remote pipe, since the engine
cuts its launch command line at every '-'; `--at-launch` plays a copy named
dodstudio_probe.dem instead, deleted afterwards.

The report (report.md + report.json) and screenshots go to
local/game-probe/<timestamp>/. Exit code 0 when every expect passed and the
game neither crashed nor exited early; 1 otherwise; 2 when it refused to run.
Needs Pillow for screenshots.
"""

import argparse
import threading
import ctypes
import json
import os
import re
import subprocess
import sys
import time
import winreg
from ctypes import wintypes
from datetime import datetime
from pathlib import Path


STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
INSTALLS = {
    "pre": STEAM / "Half-Life - PRE-Anniversary for Movies",
    "post": STEAM / "Half-Life - POST-Anniversary for Movies",
}
APPDATA = Path(os.environ.get("APPDATA", "")) / "dod-studio"
# Names the one Steam account in-game tests may run on. Kept out of the repo
# on purpose: which account runs hooked sessions is nobody else's business.
ACCOUNT_FILE = APPDATA / "game_probe.json"
REPO = Path(__file__).resolve().parents[2]


class Refused(Exception):
    pass


# ── Refusal checks ───────────────────────────────────────────────────────────

def steam_account_id():
    """Steam's signed-in account id, 0 when nobody is, None when Steam never ran."""
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam\ActiveProcess") as key:
            value, _ = winreg.QueryValueEx(key, "ActiveUser")
            return int(value)
    except OSError:
        return None


def allowed_account():
    """(account id, label) from the local account file; refuses without it."""
    try:
        data = json.loads(ACCOUNT_FILE.read_text(encoding="utf-8"))
        account = int(data["allowed_steam_account_id"])
    except (OSError, ValueError, KeyError, TypeError):
        raise Refused(
            f"no allowed test account: create {ACCOUNT_FILE} with "
            '{"allowed_steam_account_id": <id>, "label": "<name>"}'
        ) from None
    if account <= 0:
        raise Refused(f"{ACCOUNT_FILE}: allowed_steam_account_id must be a positive account id")
    return account, str(data.get("label") or "the allowed test account")


def check_account():
    """Refuses unless Steam is signed into the allowed account. Returns its label."""
    allowed, label = allowed_account()
    account = steam_account_id()
    if account != allowed:
        who = {None: "Steam has never run", 0: "nobody is signed into Steam"}.get(
            account, "another Steam account is signed in"
        )
        raise Refused(f"in-game tests run only on {label}; {who}")
    return label


def pids_named(name):
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
        capture_output=True, text=True,
    ).stdout
    return {int(line.split('","')[1]) for line in out.splitlines() if line.startswith('"')}


def settings():
    try:
        return json.loads((APPDATA / "settings.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def resolve_paths(args):
    install = INSTALLS[args.install]
    hl = install / "hl.exe"
    s = settings()
    hlae = Path(args.hlae or s.get("hlae_path") or "")
    dll = Path(args.dll or s.get("goldsrc_hooks_dll_path") or
               REPO / "target/i686-pc-windows-msvc/release/dodstudio_goldsrc_hooks.dll")
    for label, path in (("hl.exe", hl), ("HLAE", hlae), ("hook DLL", dll)):
        if not path.is_file():
            raise Refused(f"{label} not found: {path}")
    afx = hlae.parent / "AfxHookGoldSrc.dll"
    if not afx.is_file():
        raise Refused(f"AfxHookGoldSrc.dll not found beside HLAE: {afx}")
    return install, hl, hlae, afx, dll


def preflight(args):
    args.account_label = check_account()
    if pids_named("hl.exe"):
        raise Refused("hl.exe is already running; the probe never touches a game it didn't start")
    return resolve_paths(args)


# ── Logs ─────────────────────────────────────────────────────────────────────

class Tail:
    """Reads what a log gained since the probe started."""

    def __init__(self, path):
        self.path = path
        self.start = path.stat().st_size if path.exists() else 0

    def text(self):
        if not self.path.exists():
            return ""
        with open(self.path, "rb") as f:
            size = f.seek(0, 2)
            f.seek(self.start if size >= self.start else 0)
            return f.read().decode("utf-8", "replace")


def hook_log_path():
    return APPDATA / "logs" / f"dodstudio_goldsrc_hooks_{datetime.now():%Y%m%d}.log"


# ── The game ─────────────────────────────────────────────────────────────────

def send_commands(pid, commands, timeout=30):
    """Writes console commands to the hook's remote pipe, retrying while the
    game is still starting (the pipe opens once the engine is ready)."""
    name = rf"\\.\pipe\dodstudio-hl-{pid}"
    deadline = time.time() + timeout
    while True:
        try:
            with open(name, "wb", buffering=0) as pipe:
                pipe.write(("\n".join(commands) + "\n").encode("utf-8"))
            return None
        except OSError as e:
            if time.time() > deadline:
                return f"could not open {name}: {e}"
            time.sleep(0.5)


DEMO_TIME = re.compile(r"\[demo\s+(\d+\.\d+)\]")
SPECTATING = re.compile(r"now spectating idx (\d+)")


def demo_time(pid, hooklog):
    """The demo's clock now. The hook stamps its log lines with it and logs
    every pipe command, so any command asks the question; `wait` does nothing
    else."""
    if send_commands(pid, ["wait"], timeout=5):
        return None
    time.sleep(0.1)
    stamps = DEMO_TIME.findall(hooklog.text()[-4000:])
    return float(stamps[-1]) if stamps else None


def fast_forward(pid, hooklog, target, timeout):
    """Runs the demo at `host_framerate 0.05` until its clock reaches
    `target`, then back to real time. Returns an error or None."""
    err = send_commands(pid, ["host_framerate 0.05"])
    if err:
        return err
    end = time.time() + timeout
    try:
        while time.time() < end and alive(pid):
            now = demo_time(pid, hooklog)
            if now is not None and now >= target:
                return None
        return f"the demo never reached {target}s"
    finally:
        send_commands(pid, ["host_framerate 0"], timeout=5)


def spectate(pid, hooklog, index, tries):
    """Steps the spectator camera with +attack until the animation fix logs
    that it is in-eye on `index`. Returns an error or None."""
    for _ in range(tries):
        seen_now = SPECTATING.findall(hooklog.text())
        if seen_now and int(seen_now[-1]) == index:
            return None
        send_commands(pid, ["+attack"], timeout=5)
        time.sleep(0.1)
        send_commands(pid, ["-attack"], timeout=5)
        time.sleep(0.5)
    seen_now = SPECTATING.findall(hooklog.text())
    return f"never reached player {index} (last seen: {seen_now[-1] if seen_now else 'none'})"


user32 = ctypes.WinDLL("user32", use_last_error=True)
WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


def screenshot(pid, path, timeout=15):
    """A frame as HLAE records it, saved to `path` as a PNG.

    Records a fraction of a second with HLAE's own movie recording -- the path
    a capture batch uses, which works with the game behind other windows --
    and keeps the last frame. The engine's `snapshot` read back all black
    here, and copying the window from outside reads an OpenGL window as
    garbage."""
    try:
        from PIL import Image
    except ImportError:
        return "Pillow is not installed"
    rec = path.parent / f"_rec_{path.stem}"
    err = send_commands(pid, [f'mirv_movie_filename "{rec}"', "mirv_recordmovie_start"])
    if err:
        return err
    time.sleep(0.6)
    err = send_commands(pid, ["mirv_recordmovie_stop"])
    if err:
        return err
    deadline = time.time() + timeout
    frames = []
    while time.time() < deadline:
        frames = sorted(rec.glob("take*/**/*.bmp")) + sorted(rec.glob("take*/**/*.tga"))
        if frames:
            time.sleep(1.0)  # let the last frames finish writing
            frames = sorted(rec.glob("take*/**/*.bmp")) + sorted(rec.glob("take*/**/*.tga"))
            break
        time.sleep(0.25)
    if not frames:
        return "HLAE recorded no frames"
    try:
        Image.open(frames[-1]).convert("RGB").save(path)
    except OSError as e:
        return f"could not read {frames[-1].name}: {e}"
    import shutil
    shutil.rmtree(rec, ignore_errors=True)
    return None


def dialogs(pid):
    """The text of every message box the game has open ("Fatal Error", asserts)."""
    texts = []

    def text_of(hwnd):
        n = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        return buf.value

    def child(hwnd, parts):
        t = text_of(hwnd)
        if t and t not in ("OK", "Cancel", "&Abort", "&Retry", "&Ignore"):
            parts.append(t)
        return True

    def top(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        cls = ctypes.create_unicode_buffer(64)
        user32.GetClassNameW(hwnd, cls, 64)
        if owner.value == pid and cls.value == "#32770" and user32.IsWindowVisible(hwnd):
            parts = [text_of(hwnd)]
            cb = WNDENUMPROC(lambda h, _: child(h, parts))
            user32.EnumChildWindows(hwnd, cb, 0)
            texts.append(": ".join(p for p in parts if p))
        return True

    user32.EnumWindows(WNDENUMPROC(top), 0)
    return texts


def game_hwnd(pid):
    best = None

    def cb(hwnd, _):
        nonlocal best
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            rect = wintypes.RECT()
            user32.GetClientRect(hwnd, ctypes.byref(rect))
            if best is None or rect.right * rect.bottom > best[0]:
                best = (rect.right * rect.bottom, hwnd)
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return best[1] if best else None


def focus_window(pid):
    """Brings the game's window to the front. Only the game's own window, and
    only on request: the 25th Anniversary build draws nothing into its frames
    while it's in the background."""
    hwnd = game_hwnd(pid)
    if not hwnd:
        return "no visible window"
    # Windows only hands the foreground to the process that has input; a
    # synthetic Alt press satisfies that rule (the usual workaround).
    user32.keybd_event(0x12, 0, 0, 0)
    user32.SetForegroundWindow(hwnd)
    user32.keybd_event(0x12, 0, 2, 0)
    time.sleep(0.5)
    fg = user32.GetForegroundWindow()
    return "foreground" if fg == hwnd else f"not foreground (the foreground window is {fg:#x})"


# Virtual-key codes for the `key` step. Letters and digits map to themselves.
KEYS = {"esc": 0x1B, "enter": 0x0D, "tab": 0x09, "space": 0x20, "backquote": 0xC0,
        "backspace": 0x08, "up": 0x26, "down": 0x28, "left": 0x25, "right": 0x27,
        **{f"f{n}": 0x6F + n for n in range(1, 13)}}


def press_key(pid, name):
    """Presses one key in the game's window. The key goes wherever the
    foreground is, so this refuses unless that is the game's own window."""
    name = name.strip().lower()
    vk = KEYS.get(name) or (ord(name.upper()) if len(name) == 1 and name.isalnum() else None)
    if vk is None:
        return f"unknown key {name!r}"
    state = focus_window(pid)
    if not state.startswith("foreground"):
        return f"not pressed: {state}"
    scan = user32.MapVirtualKeyW(vk, 0)
    user32.keybd_event(vk, scan, 0, 0)
    time.sleep(0.05)
    user32.keybd_event(vk, scan, 2, 0)
    return None


def click(pid, x, y, count=1):
    """Left-clicks at client-area point (x, y) of the game's window, the
    same pixels a `grab` image has. Refuses unless the game is in front, and
    unless the point is inside its window."""
    state = focus_window(pid)
    if not state.startswith("foreground"):
        return f"not clicked: {state}"
    hwnd = game_hwnd(pid)
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    if not (0 <= x < rect.right and 0 <= y < rect.bottom):
        return f"({x}, {y}) is outside the {rect.right}x{rect.bottom} window"
    point = wintypes.POINT(x, y)
    user32.ClientToScreen(hwnd, ctypes.byref(point))
    user32.SetCursorPos(point.x, point.y)
    time.sleep(0.1)
    for _ in range(count):
        user32.mouse_event(0x0002, 0, 0, 0, 0)  # left down
        time.sleep(0.05)
        user32.mouse_event(0x0004, 0, 0, 0, 0)  # left up
        time.sleep(0.08)
    return None


def drag(pid, x1, y1, x2, y2):
    """Presses the left button at (x1, y1) and releases it at (x2, y2), in
    steps, so vgui2 sees the cursor move while the button is down."""
    state = focus_window(pid)
    if not state.startswith("foreground"):
        return f"not dragged: {state}"
    hwnd = game_hwnd(pid)
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    for x, y in ((x1, y1), (x2, y2)):
        if not (0 <= x < rect.right and 0 <= y < rect.bottom):
            return f"({x}, {y}) is outside the {rect.right}x{rect.bottom} window"

    def move(x, y):
        point = wintypes.POINT(x, y)
        user32.ClientToScreen(hwnd, ctypes.byref(point))
        user32.SetCursorPos(point.x, point.y)

    move(x1, y1)
    time.sleep(0.1)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # left down
    steps = 20
    for i in range(1, steps + 1):
        time.sleep(0.02)
        move(x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)
    time.sleep(0.1)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # left up
    return None


def record_clip(pid, folder, seconds, fps, hooklog=None):
    """Records `seconds` of HLAE frames at `fps` into `folder/` (PNGs) and
    writes `folder.png`, a contact sheet of up to 24 evenly spaced frames, for
    checking an animation rather than one instant.

    The seconds are demo seconds when a demo is playing: a recording runs as
    fast as frames can be written, which on a quick machine is several times
    real time, so a wall-clock wait records far more than was asked for."""
    try:
        from PIL import Image
    except ImportError:
        return "Pillow is not installed"
    import shutil
    rec = folder.parent / f"_rec_{folder.name}"
    err = send_commands(pid, [f"mirv_movie_fps {fps}", f'mirv_movie_filename "{rec}"', "mirv_recordmovie_start"])
    if err:
        return err
    started = demo_time(pid, hooklog) if hooklog else None
    if started is None:
        time.sleep(seconds)
    else:
        give_up = time.time() + seconds * 4 + 10
        while time.time() < give_up and alive(pid):
            now = demo_time(pid, hooklog)
            if now is not None and now - started >= seconds:
                break
    err = send_commands(pid, ["mirv_recordmovie_stop", "mirv_movie_fps 30"])
    if err:
        return err
    time.sleep(1.5)
    frames = sorted(rec.glob("take*/**/*.bmp")) + sorted(rec.glob("take*/**/*.tga"))
    if not frames:
        return "HLAE recorded no frames"
    folder.mkdir(parents=True, exist_ok=True)
    images = []
    for i, f in enumerate(frames):
        try:
            im = Image.open(f).convert("RGB")
        except OSError:
            continue
        im.save(folder / f"{i:04d}.png")
        images.append(im)
    shutil.rmtree(rec, ignore_errors=True)
    if not images:
        return "no readable frames"
    picks = images if len(images) <= 24 else [images[int(i * (len(images) - 1) / 23)] for i in range(24)]
    w, h = picks[0].size
    tw = 320
    th = int(h * tw / w)
    cols = 6
    rows = (len(picks) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * tw, rows * th), (40, 40, 40))
    for i, im in enumerate(picks):
        sheet.paste(im.resize((tw, th)), ((i % cols) * tw, (i // cols) * th))
    sheet.save(folder.parent / f"{folder.name}.png")
    return None


def screen_grab(pid, path):
    """A capture of the game window's client area as it is on screen. Unlike
    `shot`, this includes the VGUI panels HLAE leaves out of its frames, and
    it needs the window in front (`focus` first)."""
    try:
        from PIL import ImageGrab
    except ImportError:
        return "Pillow is not installed"
    hwnd = game_hwnd(pid)
    if not hwnd:
        return "no visible window"
    if user32.GetForegroundWindow() != hwnd:
        return "the game window is not in front (use focus first)"
    rect = wintypes.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = wintypes.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    box = (origin.x, origin.y, origin.x + rect.right, origin.y + rect.bottom)
    ImageGrab.grab(bbox=box).save(path)
    return None


def window_state(pid):
    """The game's biggest window: size, minimised, foreground. For telling a
    black capture from a game that isn't drawing."""
    best = None

    def cb(hwnd, _):
        nonlocal best
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            rect = wintypes.RECT()
            user32.GetClientRect(hwnd, ctypes.byref(rect))
            area = rect.right * rect.bottom
            if best is None or area > best[0]:
                best = (area, hwnd, rect.right, rect.bottom)
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    if best is None:
        return "no visible window"
    _, hwnd, w, h = best
    return (f"{w}x{h}, minimised={bool(user32.IsIconic(hwnd))}, "
            f"foreground={user32.GetForegroundWindow() == hwnd}")


def alive(pid):
    return pid in pids_named("hl.exe")


def find_new_hl(before, timeout=45):
    deadline = time.time() + timeout
    while time.time() < deadline:
        new = pids_named("hl.exe") - before
        if new:
            return min(new)
        time.sleep(0.5)
    return None


# ── Run ──────────────────────────────────────────────────────────────────────

def run(args):
    install, hl, hlae, afx, dll = preflight(args)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out = Path(args.out or REPO / "local" / "game-probe" / stamp)
    out.mkdir(parents=True, exist_ok=True)

    console = Tail(install / "qconsole.log")
    hooklog = Tail(hook_log_path())
    # The same line Studio builds (native::patch::types::build_hlae_process).
    game_cmd = (f"-game dod -insecure -windowed -w {args.width} -h {args.height} -gl -32bpp "
                "-afxRenderMode standard -afxForceAlpha8 1 -condebug")
    probe_copy = None
    if args.demo and args.at_launch:
        # `+playdemo` at launch, as a capture batch starts its primer: the demo
        # loads during engine start-up, not from the menu. The engine cuts its
        # command line at '-', so a copy with a safe name is played instead.
        source = install / "dod" / f"{args.demo}.dem"
        if not source.is_file():
            raise Refused(f"demo not found: {source}")
        probe_copy = install / "dod" / "dodstudio_probe.dem"
        import shutil
        shutil.copyfile(source, probe_copy)
        game_cmd += f" +{args.play} dodstudio_probe"
    before = pids_named("hl.exe")
    # Checked again at the last moment: the account can change while a run is set up.
    check_account()
    # SteamAppId and HLAE's own folder as the working directory, as Studio
    # does: without them the game fails Steam's authentication at start-up.
    launcher = subprocess.Popen([
        str(hlae), "-customLoader", "-noGui", "-autoStart",
        "-hookDllPath", str(afx), "-hookDllPath", str(dll),
        "-programPath", str(hl), "-cmdLine", game_cmd,
    ], cwd=str(hlae.parent), env={**os.environ, "SteamAppId": "30"})
    report = {
        "started": stamp, "install": args.install, "demo": args.demo, "dll": str(dll),
        "account": args.account_label, "steps": [], "shots": [],
    }
    pid = None
    try:
        pid = find_new_hl(before)
        report["pid"] = pid
        if pid is None:
            report["error"] = "hl.exe never started"
            return report, out
        events = []
        launched_at = time.time()

        def seen(rx):
            return bool(rx.search(console.text()) or rx.search(hooklog.text())
                        or rx.search("\n".join(events)))

        if args.demo and not args.at_launch:
            # Through the pipe, not `+playdemo` on the command line: the engine
            # splits its command line at every '-', which cuts most demo names.
            err = send_commands(pid, [f"{args.play} {args.demo}"], timeout=60)
            if err:
                report["error"] = f"could not start the demo: {err}"
                return report, out
        for step in args.step:
            kind, _, rest = step.partition(" ")
            result = {"step": step, "at_s": round(time.time() - launched_at, 1)}
            if kind == "expect_exit":
                end = time.time() + float(rest or 10)
                while time.time() < end and alive(pid):
                    time.sleep(0.25)
                result["ok"] = not alive(pid)
                report["expected_exit"] = result["ok"]
                report["steps"].append(result)
                continue
            if report.get("expected_exit") and kind in ("expect", "expect_not"):
                # Checks of the logs still work after the game has gone.
                time.sleep(0.5)
                hit = seen(re.compile(rest))
                result["ok"] = hit if kind == "expect" else not hit
                report["steps"].append(result)
                continue
            if not alive(pid):
                result["ok"] = False
                result["note"] = "game had already exited"
                report["steps"].append(result)
                break
            boxes = dialogs(pid)
            if boxes:
                result["ok"] = False
                result["note"] = "message box open: " + " | ".join(boxes)
                report["steps"].append(result)
                break
            if kind == "wait":
                end = time.time() + float(rest)
                while time.time() < end and alive(pid):
                    time.sleep(0.25)
                result["ok"] = alive(pid)
            elif kind == "waitfor":
                pattern, _, secs = rest.rpartition(" ")
                if not pattern or not re.fullmatch(r"\d+(\.\d+)?", secs):
                    pattern, secs = rest, "60"
                end = time.time() + float(secs)
                rx = re.compile(pattern)
                while time.time() < end and alive(pid):
                    if seen(rx):
                        result["ok"] = True
                        break
                    time.sleep(0.5)
                else:
                    result["ok"] = False
            elif kind == "ffwd":
                parts = rest.split()
                err = fast_forward(pid, hooklog, float(parts[0]),
                                   float(parts[1]) if len(parts) > 1 else 300)
                result["ok"] = err is None
                if err:
                    result["note"] = err
            elif kind == "spectate":
                parts = rest.split()
                err = spectate(pid, hooklog, int(parts[0]),
                               int(parts[1]) if len(parts) > 1 else 40)
                result["ok"] = err is None
                if err:
                    result["note"] = err
            elif kind == "clip":
                parts = rest.split()
                name = parts[0] if parts else "clip"
                seconds = float(parts[1]) if len(parts) > 1 else 2.0
                fps = int(parts[2]) if len(parts) > 2 else 30
                err = record_clip(pid, out / name, seconds, fps, hooklog)
                result["ok"] = err is None
                result["note"] = err or str(out / f"{name}.png")
                if err is None:
                    report["shots"].append(str(out / f"{name}.png"))
            elif kind == "grab":
                path = out / f"{rest}.png"
                result["note"] = screen_grab(pid, path)
                result["ok"] = result["note"] is None
                if result["note"] is None:
                    result["note"] = str(path)
                    report["shots"].append(str(path))
            elif kind == "focus":
                result["note"] = focus_window(pid)
                result["ok"] = result["note"].startswith("foreground")
            elif kind == "click":
                parts = rest.split()
                err = click(pid, int(parts[0]), int(parts[1]),
                            int(parts[2]) if len(parts) > 2 else 1)
                result["ok"] = err is None
                if err:
                    result["note"] = err
            elif kind == "drag":
                x1, y1, x2, y2 = (int(v) for v in rest.split())
                err = drag(pid, x1, y1, x2, y2)
                result["ok"] = err is None
                if err:
                    result["note"] = err
            elif kind == "key":
                err = press_key(pid, rest)
                result["ok"] = err is None
                if err:
                    result["note"] = err
            elif kind == "window":
                result["note"] = window_state(pid)
                result["ok"] = True
            elif kind == "listen_events":
                name = rf"\\.\pipe\dodstudio-hl-{pid}-events"

                def read_events():
                    deadline = time.time() + 30
                    while time.time() < deadline:
                        try:
                            with open(name, "rb", buffering=0) as pipe:
                                buf = b""
                                while chunk := pipe.read(4096):
                                    buf += chunk
                                    *lines, buf = buf.split(b"\n")
                                    events.extend(l.decode("utf-8", "replace").rstrip("\r") for l in lines)
                            return
                        except OSError:
                            time.sleep(0.5)

                threading.Thread(target=read_events, daemon=True).start()
                # The hook's hello line says the connection is up.
                end = time.time() + 30
                while time.time() < end and not events:
                    time.sleep(0.25)
                result["ok"] = bool(events)
                result["note"] = events[0] if events else "no hello from the events pipe"
            elif kind == "cmd":
                err = send_commands(pid, [rest])
                result["ok"] = err is None
                if err:
                    result["note"] = err
            elif kind == "shot":
                path = out / f"{rest}.png"
                err = screenshot(pid, path)
                result["ok"] = err is None
                result["note"] = err or str(path)
                if path.exists():
                    report["shots"].append(str(path))
            elif kind in ("expect", "expect_not"):
                rx = re.compile(rest)
                hit = seen(rx)
                result["ok"] = hit if kind == "expect" else not hit
            else:
                result["ok"] = False
                result["note"] = "unknown step"
            report["steps"].append(result)
        report["events"] = events
        report["alive_at_end"] = alive(pid)
        report["dialogs"] = dialogs(pid) if report["alive_at_end"] else []
    finally:
        # Only what this run started.
        if pid and alive(pid):
            subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True)
        if launcher.poll() is None:
            launcher.kill()
        time.sleep(1)
        if probe_copy:
            probe_copy.unlink(missing_ok=True)
        report["console_log"] = console.text()[-20000:]
        report["hook_log"] = hooklog.text()[-20000:]
        report["crashes"] = [l for l in report["hook_log"].splitlines() if "CRASH:" in l]
    return report, out


def verdict(report):
    if report.get("error"):
        return False
    steps_ok = all(s.get("ok") for s in report["steps"])
    running_as_expected = report.get("expected_exit") or report.get("alive_at_end", False)
    return steps_ok and not report["crashes"] and not report.get("dialogs") and running_as_expected


def write_report(report, out):
    ok = verdict(report)
    (out / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    lines = [
        f"# Game probe {report['started']}: {'PASS' if ok else 'FAIL'}",
        "",
        f"- install: {report['install']}, demo: {report['demo'] or '(none)'}",
        f"- hook DLL: `{report['dll']}`",
        f"- account: {report['account']}",
        f"- hl.exe pid: {report.get('pid')}, still running at the end: {report.get('alive_at_end')}",
    ]
    if report.get("error"):
        lines.append(f"- error: {report['error']}")
    for d in report.get("dialogs", []):
        lines.append(f"- **message box open:** {d}")
    lines += ["", "## Steps", ""]
    lines += [f"- {'ok ' if s.get('ok') else 'FAIL'} [{s.get('at_s', '?')} s] `{s['step']}`" + (f" -- {s['note']}" if s.get("note") else "")
              for s in report["steps"]]
    lines += ["", "## Crashes", ""] + ([f"    {c}" for c in report["crashes"]] or ["none"])
    lines += ["", "## Hook log (tail)", "", "```", *report["hook_log"].splitlines()[-40:], "```"]
    lines += ["", "## Console log (tail)", "", "```", *report["console_log"].splitlines()[-40:], "```"]
    (out / "report.md").write_text("\n".join(lines), encoding="utf-8")
    return ok


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--install", choices=sorted(INSTALLS), default="pre")
    p.add_argument("--demo", help="demo name as the game takes it (relative to dod/, no .dem)")
    p.add_argument("--play", choices=["playdemo", "viewdemo"], default="playdemo")
    p.add_argument("--at-launch", action="store_true",
                   help="start the demo from the launch command line (as a batch does), not from the menu")
    p.add_argument("--step", action="append", default=[])
    p.add_argument("--dll", help="hook DLL (default: Studio's setting, then the repo's release build)")
    p.add_argument("--hlae", help="HLAE.exe (default: Studio's setting)")
    p.add_argument("--width", type=int, default=1280)
    p.add_argument("--height", type=int, default=720)
    p.add_argument("--out", help="report folder (default: local/game-probe/<timestamp>)")
    p.add_argument("--check", action="store_true", help="only run the refusal checks")
    args = p.parse_args()
    try:
        if args.check:
            install, hl, hlae, afx, dll = preflight(args)
            print(f"OK to run: {args.account_label} signed in, no hl.exe running, {hl}, {dll}")
            return 0
        report, out = run(args)
    except Refused as why:
        print(f"REFUSED: {why}")
        return 2
    ok = write_report(report, out)
    print(f"{'PASS' if ok else 'FAIL'} -- report: {out / 'report.md'}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
