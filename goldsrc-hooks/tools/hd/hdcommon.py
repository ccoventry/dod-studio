"""Shared by every HD build script: where the game is, where scratch files
go, and the hashing/naming that must match goldsrc-hooks/src/texture_hires.rs
byte for byte.

The game folder is the Half-Life folder that holds dod/. In order: HD_GAME
(build_all.py --game sets it), else the hl.exe DoD Studio is set to launch
(its settings.json), else the one Steam install with a dod/ folder -- several
are an error that lists them.
"""
import atexit, fnmatch, glob, hashlib, os, re, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))

# Which maps get HD map textures and skies (see hd_maps.example.txt). No file
# means every map.
MAP_LIST = os.environ.get("HD_MAPS") or os.path.join(HERE, "hd_maps.txt")

# Largest replacement side: texture_hires raises the engine's upload ceiling
# to 1024x1024 (see its module doc).
CAP = 1024

# World textures never drawn, or drawn by a different path (sky): not built.
# The hook's TOOL_TEXTURES list must match this one.
SKIP = {"aaatrigger", "clip", "origin", "null", "skip", "hint", "bevel", "sky", "black"}


# fnv1a32's results, keyed by a BLAKE2b digest of the same bytes, kept on
# disk between runs: 16-byte digest + 4-byte hash per record, append-only.
FNV_CACHE = os.path.join(tempfile.gettempdir(), "dodstudio_hd_work", "fnv1a32.cache")
_fnv_known = None
_fnv_new = []


def fnv1a32(*parts):
    """FNV-1a, 32-bit -- texture_hires.rs's `fnv1a32` -- of `parts` joined.

    The byte loop is pure Python: hashing every texture of 133 maps took
    about 50 s, once per style (#383). So each result is remembered under a
    BLAKE2b digest of the same bytes (C, and fast), in memory and in
    FNV_CACHE, and only bytes never seen before pay for the loop. The key is
    the content itself, so the cache can't go stale: a changed texture is a
    different key."""
    global _fnv_known
    if _fnv_known is None:
        _fnv_known = _load_fnv_cache()
        atexit.register(_save_fnv_cache)
    digest = hashlib.blake2b(digest_size=16)
    for p in parts:
        digest.update(p)
    key = digest.digest()
    h = _fnv_known.get(key)
    if h is None:
        h = fnv1a32_uncached(*parts)
        _fnv_known[key] = h
        _fnv_new.append(key + h.to_bytes(4, "little"))
    return h


def fnv1a32_uncached(*parts):
    """The FNV-1a byte loop itself."""
    h = 0x811C9DC5
    for p in parts:
        for b in p:
            h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF
    return h


def _load_fnv_cache():
    try:
        with open(FNV_CACHE, "rb") as f:
            data = f.read()
    except OSError:
        return {}
    # A record cut short by a killed run is dropped, not misread.
    data = data[: len(data) - len(data) % 20]
    return {data[i : i + 16]: int.from_bytes(data[i + 16 : i + 20], "little") for i in range(0, len(data), 20)}


def _save_fnv_cache():
    if not _fnv_new:
        return
    try:
        os.makedirs(os.path.dirname(FNV_CACHE), exist_ok=True)
        with open(FNV_CACHE, "ab") as f:
            f.write(b"".join(_fnv_new))
        _fnv_new.clear()
    except OSError:
        pass  # only a cache: the next run recomputes


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


def map_patterns(path=None):
    """The patterns in hd_maps.txt, lowercased, or None when there's no such
    file (build every map). `*` matches any run of characters and `?` one, as
    in a Windows folder search; a name without either matches only itself."""
    path = path or MAP_LIST
    if not os.path.exists(path):
        return None
    patterns = []
    for raw in open(path, encoding="utf-8"):
        line = raw.split("#", 1)[0].strip().lower()
        if line.endswith(".bsp"):
            line = line[:-4]
        if line:
            patterns.append(line)
    return patterns


def select_maps(names, patterns):
    """(the names `patterns` allows, the patterns that matched nothing).
    `patterns` None allows everything."""
    if patterns is None:
        return list(names), []
    chosen = [n for n in names if any(fnmatch.fnmatchcase(n.lower(), p) for p in patterns)]
    unused = [p for p in patterns if not any(fnmatch.fnmatchcase(n.lower(), p) for n in names)]
    return chosen, unused


def all_maps(game):
    """Every map in dod/maps, as hd_maps.txt narrows it (all of them without
    the file). Patterns that match no map are reported, since a typo there
    would otherwise quietly build nothing."""
    names = sorted(os.path.basename(f)[:-4] for f in glob.glob(os.path.join(game, "dod", "maps", "*.bsp")))
    chosen, unused = select_maps(names, map_patterns())
    for p in unused:
        print(f"  {os.path.basename(MAP_LIST)}: {p!r} matches no map in dod/maps", flush=True)
    return chosen


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
