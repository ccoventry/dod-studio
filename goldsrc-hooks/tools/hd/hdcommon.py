"""Shared by every HD build script: where the game is, where scratch files
go, and the hashing/naming that must match goldsrc-hooks/src/texture_hires.rs
byte for byte.

The game folder is the Half-Life folder that holds dod/. In order: HD_GAME
(build_all.py --game sets it), else the hl.exe DoD Studio is set to launch
(its settings.json), else the one Steam install with a dod/ folder -- several
are an error that lists them.
"""
import glob, os, re, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))

# Largest replacement side: texture_hires raises the engine's upload ceiling
# to 1024x1024 (see its module doc).
CAP = 1024

# World textures never drawn, or drawn by a different path (sky): not built.
# The hook's TOOL_TEXTURES list must match this one.
SKIP = {"aaatrigger", "clip", "origin", "null", "skip", "hint", "bevel", "sky", "black"}


def fnv1a32(*parts):
    """FNV-1a, 32-bit -- texture_hires.rs's `fnv1a32`."""
    h = 0x811C9DC5
    for p in parts:
        for b in p:
            h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def file_stem_name(name):
    """texture_hires.rs's `file_stem_name`: lowercase, Windows-forbidden
    characters to `_`."""
    return "".join("_" if c in '<>:"/\\|?*' or ord(c) < 32 else c.lower() for c in name)


def pot(n):
    """Next power of two, capped at CAP."""
    p = 1
    while p < n:
        p *= 2
    return min(p, CAP)


def _steam_libraries():
    roots = []
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam") as k:
            roots.append(winreg.QueryValueEx(k, "SteamPath")[0])
    except OSError:
        pass
    roots.append(r"C:\Program Files (x86)\Steam")
    libs = []
    for root in roots:
        vdf = os.path.join(root, "steamapps", "libraryfolders.vdf")
        if os.path.exists(vdf):
            libs += re.findall(r'"path"\s+"([^"]+)"', open(vdf, encoding="utf-8", errors="replace").read())
        libs.append(root)
    seen = {}
    for p in libs:
        p = os.path.normpath(p.replace("\\\\", "\\"))
        seen.setdefault(os.path.normcase(p), p)
    return list(seen.values())


def _studio_game():
    """The folder of the hl.exe DoD Studio launches, from its settings."""
    try:
        import json
        cfg = os.path.join(os.environ.get("APPDATA", ""), "dod-studio", "settings.json")
        hl = json.load(open(cfg, encoding="utf-8")).get("hl_path")
        game = os.path.dirname(hl) if hl else None
        return game if game and os.path.isdir(os.path.join(game, "dod")) else None
    except (OSError, ValueError, AttributeError):
        return None


def game_root():
    """The Half-Life folder holding dod/."""
    env = os.environ.get("HD_GAME")
    if env:
        if not os.path.isdir(os.path.join(env, "dod")):
            sys.exit(f"HD_GAME={env!r} has no dod folder; point it at the Half-Life folder that does")
        return env
    studio = _studio_game()
    if studio:
        return studio
    found = sorted({os.path.normcase(os.path.dirname(d)): os.path.dirname(d)
                    for lib in _steam_libraries()
                    for d in glob.glob(os.path.join(lib, "steamapps", "common", "*", "dod"))}.values())
    if len(found) == 1:
        return found[0]
    listing = "\n  ".join(found) or "(none found)"
    sys.exit("Which game folder? Set HD_GAME (or build_all.py --game) to the Half-Life folder "
             f"DoD Studio launches. Found:\n  {listing}")


def dod_dir():
    return os.path.join(game_root(), "dod")


def hd_dir():
    """dod/dodstudio_hd, where texture_hires looks."""
    return os.path.join(dod_dir(), "dodstudio_hd")


def work_dir(name):
    """A fresh scratch folder with in/ and out/ (HD_WORK overrides)."""
    import shutil
    work = os.environ.get("HD_WORK") or os.path.join(tempfile.gettempdir(), "dodstudio_hd_work", name)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(os.path.join(work, "in"))
    os.makedirs(os.path.join(work, "out"))
    return work


def extra_installs():
    """Other Half-Life folders whose models and sprites should get HD
    copies too (HD_ALSO, separated by ';'): e.g. a stock install whose
    files differ from the movie install's."""
    return [p for p in os.environ.get("HD_ALSO", "").split(os.pathsep) if p]
