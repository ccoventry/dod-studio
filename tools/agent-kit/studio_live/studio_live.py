r"""A throwaway Studio for live tests: its own ports, its own settings folder, started and
stopped by PID, never the user's Studio or the user's settings.

    studio_live.py patch <worktree>
        Makes the worktree's Studio read its settings folder from DS_LIVE_APPDATA
        (native/src/shared/paths.rs) and marks that file skip-worktree so the
        change is never committed. Setting APPDATA does not move Studio's
        settings: `dirs::config_dir()` asks Windows directly.
    studio_live.py build <worktree> --vite-port 5222 [--target-dir DIR]
        A debug build whose page loads from that Vite port.
    studio_live.py seed <live-dir> [--install pre|post] [--set key=<json>]...
        <live-dir>\dod-studio\settings.json from the user's settings (only read),
        with recent projects cleared, update checks and the setup prompt off,
        the game kept behind, and the chosen install's hl.exe.
    studio_live.py start <worktree> <live-dir> --vite-port 5222 --cdp-port 9322 [--exe EXE]
        Starts Vite and the Studio exe with CDP on that port; PIDs and the
        user's settings.json hash go in <live-dir>\state.json.
    studio_live.py stop <live-dir>
        Closes Studio (WM_CLOSE on its own windows, then its PID), stops Vite's
        PID tree, and fails loudly (exit 3) if the user's settings.json changed.
    studio_live.py status <live-dir>

Then drive it with `node cdp.cjs --port 9322 -e "return document.title"` and
answer native file dialogs with `dlg.ps1`.

Nothing starts at import time. Windows only past argument parsing.
"""
import argparse
import hashlib
import json
import os
import re
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import kit_paths  # noqa: E402

PATHS_RS = Path("native/src/shared/paths.rs")
LIVE_ENV = "DS_LIVE_APPDATA"
APPDATA_FN = re.compile(r"fn appdata_base\(\) -> PathBuf \{(\r?\n)")
CREATE_NO_WINDOW = 0x08000000
WM_CLOSE = 0x0010
EXIT_SETTINGS_CHANGED = 3
# Seeded into every throwaway Studio: nothing that reaches out, nothing that
# pops up over the page the test drives, and the game stays behind the user's windows.
SEED_OVERRIDES = {
    "recent_projects": [],
    "auto_check_updates": False,
    "setup_dismissed": True,
    "keep_game_behind": True,
}


class Refused(Exception):
    pass


# ── Pure parts ───────────────────────────────────────────────────────────────

def patched_paths_source(text):
    """paths.rs with `appdata_base` reading DS_LIVE_APPDATA first; unchanged when
    already patched. Raises Refused when the function is not where it was."""
    if LIVE_ENV in text:
        return text
    match = APPDATA_FN.search(text)
    if not match:
        raise Refused(f"{PATHS_RS}: `fn appdata_base() -> PathBuf {{` not found; patch it by hand")
    nl = match.group(1)
    insert = (f"    // tools/agent-kit/studio_live: a throwaway Studio's settings folder. Never committed.{nl}"
              f"    if let Some(dir) = std::env::var_os(\"{LIVE_ENV}\") {{{nl}"
              f"        return PathBuf::from(dir);{nl}"
              f"    }}{nl}")
    return text[:match.end()] + insert + text[match.end():]


def seeded_settings(user_settings, hl_exe=None, extra=None):
    """The throwaway Studio's settings: the user's, with SEED_OVERRIDES, the chosen hl.exe and `extra` on top."""
    settings = dict(user_settings)
    settings.update(SEED_OVERRIDES)
    if hl_exe:
        settings["hl_path"] = str(hl_exe)
    settings.update(extra or {})
    return settings


def parse_set(pairs):
    """`key=<json>` pairs into a dict; a value that isn't JSON is taken as a string."""
    out = {}
    for pair in pairs:
        key, sep, value = pair.partition("=")
        if not sep or not key:
            raise Refused(f"--set wants key=<json>, got {pair!r}")
        try:
            out[key] = json.loads(value)
        except ValueError:
            out[key] = value
    return out


def check_live_dir(live_dir, user_appdata):
    """Refuses a live folder that is, or holds, the user's own settings folder."""
    live = Path(live_dir).resolve()
    user = Path(user_appdata).resolve()
    if live == user or live == user.parent or user.is_relative_to(live) or live.is_relative_to(user):
        raise Refused(f"{live} overlaps the user's settings folder {user}; pick a scratch folder")
    return live


def file_hash(path):
    """sha256 of a file, or None when it does not exist."""
    try:
        return hashlib.sha256(Path(path).read_bytes()).hexdigest()
    except FileNotFoundError:
        return None


# ── Windows and processes ────────────────────────────────────────────────────

def port_free(port):
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", port)) != 0


def wait_for_url(url, timeout_s):
    end = time.time() + timeout_s
    while time.time() < end:
        try:
            with urllib.request.urlopen(url, timeout=2):
                return True
        except OSError:
            time.sleep(0.5)
    return False


def pid_alive(pid):
    out = subprocess.run(["tasklist", "/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    return f'"{pid}"' in out


def close_windows_of(pid):
    """Posts WM_CLOSE to every top-level window `pid` owns (the title bar's X)."""
    import ctypes
    from ctypes import wintypes
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    found = []

    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    user32.EnumWindows(proc(visit), 0)
    for hwnd in found:
        user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
    return len(found)


def kill_tree(pid):
    """Stops `pid` and its children: only a process this kit started."""
    subprocess.run(["taskkill", "/F", "/T", "/PID", str(pid)], capture_output=True)


def spawn(args, cwd, env, log):
    """Starts a process with no console window, output to `log`."""
    with open(log, "ab") as out:
        return subprocess.Popen(args, cwd=str(cwd), env=env, stdout=out, stderr=subprocess.STDOUT,
                                stdin=subprocess.DEVNULL, creationflags=CREATE_NO_WINDOW).pid


# ── Commands ─────────────────────────────────────────────────────────────────

def cmd_patch(args):
    path = Path(args.worktree) / PATHS_RS
    text = path.read_text(encoding="utf-8")
    new = patched_paths_source(text)
    if new != text:
        path.write_bytes(new.encode("utf-8"))
    subprocess.run(["git", "-C", str(args.worktree), "update-index", "--skip-worktree", str(PATHS_RS)], check=True)
    print(f"{path} reads {LIVE_ENV} (skip-worktree set: it will not be committed)")


def cmd_build(args):
    env = {**os.environ, "TAURI_CONFIG": json.dumps({"build": {"devUrl": f"http://localhost:{args.vite_port}"}})}
    if args.target_dir:
        env["CARGO_TARGET_DIR"] = args.target_dir
    if LIVE_ENV not in (Path(args.worktree) / PATHS_RS).read_text(encoding="utf-8"):
        raise Refused(f"run `patch {args.worktree}` first: this build would use the user's settings")
    started = time.time()
    code = subprocess.run(["cargo", "build", "-p", "desktop-studio"], cwd=args.worktree, env=env).returncode
    print(f"build exit {code} after {time.time() - started:.0f} s")
    return code


def cmd_seed(args):
    user_file = kit_paths.studio_appdata() / "settings.json"
    live = check_live_dir(args.live_dir, kit_paths.studio_appdata())
    user = json.loads(user_file.read_text(encoding="utf-8"))
    hl_exe = kit_paths.install_dir(args.install) / "hl.exe" if args.install else None
    target = live / "dod-studio" / "settings.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(seeded_settings(user, hl_exe, parse_set(args.set)), indent=2), encoding="utf-8")
    print(f"seeded {target} (user settings sha256 {file_hash(user_file)[:16]}, only read)")


def default_exe(worktree):
    target = Path(os.environ.get("CARGO_TARGET_DIR") or Path(worktree) / "target")
    return target / "debug" / "desktop-studio.exe"


def cmd_start(args):
    user_file = kit_paths.studio_appdata() / "settings.json"
    live = check_live_dir(args.live_dir, kit_paths.studio_appdata())
    worktree = Path(args.worktree)
    exe = Path(args.exe) if args.exe else default_exe(worktree)
    if LIVE_ENV not in (worktree / PATHS_RS).read_text(encoding="utf-8"):
        raise Refused(f"{worktree / PATHS_RS} does not read {LIVE_ENV}: run `patch` and rebuild")
    if not exe.is_file():
        raise Refused(f"no Studio build at {exe}: run `build` (or pass --exe)")
    if not (live / "dod-studio" / "settings.json").is_file():
        raise Refused(f"no settings in {live}: run `seed {live}` first")
    for port in (args.vite_port, args.cdp_port):
        if not port_free(port):
            raise Refused(f"port {port} is in use; pick another")
    state = {"user_settings_sha256": file_hash(user_file), "vite_port": args.vite_port,
             "cdp_port": args.cdp_port, "exe": str(exe), "started": time.strftime("%Y-%m-%d %H:%M:%S")}
    npx = "npx.cmd" if os.name == "nt" else "npx"
    state["vite_pid"] = spawn([npx, "vite", "--port", str(args.vite_port), "--strictPort"],
                              worktree / "studio", os.environ.copy(), live / "vite.log")
    if not wait_for_url(f"http://localhost:{args.vite_port}/", 60):
        kill_tree(state["vite_pid"])
        raise Refused(f"Vite never answered on {args.vite_port}; see {live / 'vite.log'}")
    env = {**os.environ, LIVE_ENV: str(live),
           "WEBVIEW2_USER_DATA_FOLDER": str(live / "webview"),
           "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS": f"--remote-debugging-port={args.cdp_port}"}
    state["studio_pid"] = spawn([str(exe)], worktree / "studio" / "src-tauri", env, live / "studio.log")
    (live / "state.json").write_text(json.dumps(state, indent=2), encoding="utf-8")
    cdp_up = wait_for_url(f"http://127.0.0.1:{args.cdp_port}/json/version", 90)
    print(f"Studio PID {state['studio_pid']}, Vite PID {state['vite_pid']}, CDP "
          f"{'up' if cdp_up else 'NOT up'} on {args.cdp_port}; state in {live / 'state.json'}")
    return 0 if cdp_up else 1


def cmd_stop(args):
    live = Path(args.live_dir)
    state = json.loads((live / "state.json").read_text(encoding="utf-8"))
    studio = state.get("studio_pid")
    if studio and pid_alive(studio):
        close_windows_of(studio)
        end = time.time() + 15
        while time.time() < end and pid_alive(studio):
            time.sleep(0.5)
        if pid_alive(studio):
            kill_tree(studio)
    if state.get("vite_pid"):
        kill_tree(state["vite_pid"])
    before = state.get("user_settings_sha256")
    after = file_hash(kit_paths.studio_appdata() / "settings.json")
    print(f"stopped; user settings.json sha256 before {str(before)[:16]} after {str(after)[:16]}")
    if before != after:
        print("FAIL: the user's settings.json changed while the throwaway Studio ran. Report it; do not "
              "assume it was this Studio (the user's own Studio may have saved), but check.")
        return EXIT_SETTINGS_CHANGED
    return 0


def cmd_status(args):
    state = json.loads((Path(args.live_dir) / "state.json").read_text(encoding="utf-8"))
    for key in ("studio_pid", "vite_pid"):
        pid = state.get(key)
        print(f"{key} {pid}: {'running' if pid and pid_alive(pid) else 'gone'}")
    print(f"CDP port {state['cdp_port']}, Vite port {state['vite_port']}")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("patch")
    s.add_argument("worktree")
    s = sub.add_parser("build")
    s.add_argument("worktree")
    s.add_argument("--vite-port", type=int, required=True)
    s.add_argument("--target-dir")
    s = sub.add_parser("seed")
    s.add_argument("live_dir")
    s.add_argument("--install", choices=sorted(kit_paths.INSTALL_FOLDERS))
    s.add_argument("--set", action="append", default=[], metavar="KEY=JSON")
    s = sub.add_parser("start")
    s.add_argument("worktree")
    s.add_argument("live_dir")
    s.add_argument("--vite-port", type=int, required=True)
    s.add_argument("--cdp-port", type=int, required=True)
    s.add_argument("--exe")
    s = sub.add_parser("stop")
    s.add_argument("live_dir")
    s = sub.add_parser("status")
    s.add_argument("live_dir")
    args = p.parse_args(argv)
    commands = {"patch": cmd_patch, "build": cmd_build, "seed": cmd_seed, "start": cmd_start,
                "stop": cmd_stop, "status": cmd_status}
    try:
        return commands[args.cmd](args) or 0
    except Refused as why:
        print(f"REFUSED: {why}")
        return 2


if __name__ == "__main__":
    sys.exit(main())
