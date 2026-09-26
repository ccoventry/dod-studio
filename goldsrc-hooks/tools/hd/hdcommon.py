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

# The user's own lists, read from the install they describe (see user_file):
# which maps get HD map textures and skies (hd_maps.example.txt; no file means
# every map), and extra styles (my_styles.example.txt, read by styles.py).
MAP_LIST = "hd_maps.txt"
MY_STYLES = "my_styles.txt"

# Largest replacement side, a power of two from 1024 to 4096 (build_all.py
# --cap, HD_CAP). The hook lets the engine take up to 4096x4096 (see
# texture_hires.rs's module doc), and gl_max_size in the game decides what
# is shown; 1024 is the size everything was built at before there was a
# choice. The scripts upscale 4x whatever the cap, so it only matters for
# originals over 256 a side: detail textures (mostly 512), a few large map
# textures and model skins.
CAPS = (1024, 2048, 4096)


def _cap():
    raw = os.environ.get("HD_CAP", "1024")
    if not raw.isdigit() or int(raw) not in CAPS:
        sys.exit(f"HD_CAP={raw!r}: one of {', '.join(map(str, CAPS))}")
    return int(raw)


CAP = _cap()

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


def tga_size(path):
    """(width, height) from a TGA's header, or None when there's no file."""
    try:
        with open(path, "rb") as f:
            head = f.read(18)
    except OSError:
        return None
    if len(head) < 18:
        return None
    return int.from_bytes(head[12:14], "little"), int.from_bytes(head[14:16], "little")


def built(path, w, h):
    """Whether `path` is already built at least `w` x `h`: what every build
    step skips. A file from a smaller cap is rebuilt, so raising the cap
    replaces only the files it enlarges."""
    size = tga_size(path)
    return size is not None and size[0] >= w and size[1] >= h


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


# Lists whose old-place note has been given, so it's given once per run.
old_place_noted = set()


def user_file(name, env):
    """Where one of the user's own lists is read from: the path in `env` if
    set, else <game>/dod/dodstudio_hd/<name>, beside the files it shapes.

    That folder belongs to one install and survives a re-clone of the repo;
    this folder does neither (#385). A copy left here from before still works
    while the install has none, with a note saying where to move it."""
    if os.environ.get(env):
        return os.environ[env]
    path = os.path.join(hd_dir(), name)
    old = os.path.join(HERE, name)
    if os.path.exists(path) or not os.path.exists(old):
        return path
    if name not in old_place_noted:
        old_place_noted.add(name)
        print(f"note: reading {old}; move it to {path}, where it belongs now", file=sys.stderr, flush=True)
    return old


def map_list():
    """The hd_maps.txt this run reads (which may not exist)."""
    return user_file(MAP_LIST, "HD_MAPS")


def map_patterns(path=None):
    """The patterns in hd_maps.txt, lowercased, or None when there's no such
    file (build every map). `*` matches any run of characters and `?` one, as
    in a Windows folder search; a name without either matches only itself."""
    path = path or map_list()
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
        print(f"  {MAP_LIST}: {p!r} matches no map in dod/maps", flush=True)
    return chosen


def save_output(img, path):
    """Saves a finished HD file so it only ever appears whole.

    Every build step skips a file that already exists, so a build stopped
    mid-save (Cancel in DoD Studio, a closed window) must not leave half a
    file under the real name: it would count as built forever. The image goes
    to `<path>.part` first and is renamed into place."""
    part = path + ".part"
    img.save(part, format=os.path.splitext(path)[1][1:].upper() or None)
    os.replace(part, path)


def clear_partial(out_dir):
    """Removes `.part` files a stopped build left in `out_dir`."""
    for name in os.listdir(out_dir) if os.path.isdir(out_dir) else ():
        if name.endswith(".part"):
            try:
                os.remove(os.path.join(out_dir, name))
            except OSError:
                pass


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
