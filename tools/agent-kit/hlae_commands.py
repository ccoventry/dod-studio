r"""HLAE's GoldSrc console names (`mirv_*` commands and cvars), checkable without the game.

    hlae_commands.py --check mirv_camexport_mode [more names]
        Whether each name is in hlae_commands.txt (beside this script, tracked); the
        nearest names for one that isn't. Exit 1 when any is missing.
    hlae_commands.py --grep camexport
        Every name with that text in it.
    hlae_commands.py --dump [--dll PATH]
        Re-reads the names out of AfxHookGoldSrc.dll and rewrites hlae_commands.txt (do
        it after an HLAE update, and commit the file).
    --dll with --check / --grep reads the DLL instead of the text file.

The DLL: --dll, else DS_HLAE_DLL, else AfxHookGoldSrc.dll beside the HLAE.exe Studio's
settings name (`hlae_path` in %APPDATA%\dod-studio\settings.json, only read). The names
come from a strings scan, the same one goldsrc-hooks/tools/console_names.py runs for
Studio's console autocomplete: NUL-terminated `mirv_[a-z0-9_]+`, not HLAE's `__mirv_*`
debug names. Only the names are copied, nothing else of HLAE's. A scan can't tell a
command from a cvar, and a name HLAE builds at run time is missing; `mirv_` sub-commands
(`mirv_camexport mode`) are arguments, not names, unless HLAE also registers the joined
name.
"""
import argparse
import difflib
import hashlib
import json
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kit_paths  # noqa: E402

NAMES_FILE = kit_paths.KIT_DIR / "hlae_commands.txt"
NAME = re.compile(rb"(?<![A-Za-z0-9_])(mirv_[a-z0-9_]+)\x00")
FILE_VERSION = "FileVersion\x00".encode("utf-16-le")


# ── Pure parts ───────────────────────────────────────────────────────────────

def names_in(data):
    """The sorted `mirv_*` names in a DLL's bytes."""
    return sorted({m.decode() for m in NAME.findall(data)})


def file_version(data):
    """The PE version resource's FileVersion string, or None."""
    at = data.find(FILE_VERSION)
    if at < 0:
        return None
    i = at + len(FILE_VERSION)
    while i < len(data) - 1 and data[i:i + 2] == b"\x00\x00":
        i += 2
    end = i
    while end < len(data) - 1 and data[end:end + 2] != b"\x00\x00":
        end += 2
    text = data[i:end].decode("utf-16-le", "replace").strip()
    return text or None


def render(names, version, sha):
    head = [
        "# HLAE's GoldSrc console names (mirv_* commands and cvars), one per line.",
        f"# From AfxHookGoldSrc.dll {version or '(no version)'}, sha256 {sha[:16]}, by tools/agent-kit/hlae_commands.py --dump.",
        "# Names only. Regenerate after an HLAE update.",
    ]
    return "\n".join(head + names) + "\n"


def parse_names_file(text):
    return [line.strip() for line in text.splitlines() if line.strip() and not line.startswith("#")]


def check(name, names):
    """(found, nearest names when not found)."""
    if name in names:
        return True, []
    close = difflib.get_close_matches(name, names, n=5, cutoff=0.6)
    parts = [n for n in names if name.replace("mirv_", "") in n][:5]
    return False, list(dict.fromkeys(close + parts))[:6]


# ── Files ────────────────────────────────────────────────────────────────────

def default_dll(env=os.environ):
    """DS_HLAE_DLL, else beside the HLAE.exe Studio's settings name; None if neither."""
    if env.get("DS_HLAE_DLL"):
        return Path(env["DS_HLAE_DLL"])
    settings = kit_paths.studio_appdata(env) / "settings.json"
    try:
        hlae = json.loads(settings.read_text(encoding="utf-8")).get("hlae_path")
    except (OSError, ValueError):
        return None
    return Path(hlae).parent / "AfxHookGoldSrc.dll" if hlae else None


def names_from(args):
    if args.dll or args.dump:
        dll = Path(args.dll) if args.dll else default_dll()
        if not dll or not dll.exists():
            raise SystemExit(f"no AfxHookGoldSrc.dll at {dll} (pass --dll or set DS_HLAE_DLL)")
        data = dll.read_bytes()
        return names_in(data), data, dll
    if not NAMES_FILE.exists():
        raise SystemExit(f"{NAMES_FILE} is missing: run --dump first")
    return parse_names_file(NAMES_FILE.read_text(encoding="utf-8")), None, NAMES_FILE


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--check", nargs="+", metavar="NAME")
    p.add_argument("--grep")
    p.add_argument("--dump", action="store_true", help="rewrite hlae_commands.txt from the DLL")
    p.add_argument("--dll")
    args = p.parse_args(argv)
    if not (args.check or args.grep or args.dump):
        p.error("--check NAME, --grep TEXT or --dump")
    names, data, source = names_from(args)
    if args.dump:
        version, sha = file_version(data), hashlib.sha256(data).hexdigest()
        exe = source.parent / "HLAE.exe"
        if not version and exe.exists():
            # AfxHookGoldSrc.dll carries no version resource; HLAE.exe beside it does.
            version = f"HLAE.exe {file_version(exe.read_bytes()) or '?'}"
        NAMES_FILE.write_text(render(names, version, sha), encoding="utf-8", newline="\n")
        print(f"{len(names)} names from {source} ({version or 'no version'}) -> {NAMES_FILE}")
    if args.grep:
        hits = [n for n in names if args.grep.lower() in n]
        print("\n".join(hits) if hits else f"no name with {args.grep!r} in it ({source})")
    missing = 0
    for name in args.check or []:
        found, near = check(name, names)
        if found:
            print(f"{name}: yes")
        else:
            missing += 1
            print(f"{name}: NOT in {source.name}" + (f"; nearest: {', '.join(near)}" if near else ""))
    return 1 if missing else 0


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.exit(main())
