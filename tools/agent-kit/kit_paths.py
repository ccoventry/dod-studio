"""Where the agent kit finds things: the repo, the movie installs, Studio's folders.

Nothing here is a user's own path. Installs come from the environment, with
the Steam library's default folder as the fallback:

    DS_STEAM_COMMON   the Steam library's steamapps\\common (default: Steam's default)
    DS_PRE_INSTALL    the pre-Anniversary movie install (default: <common>\\Half-Life - PRE-Anniversary for Movies)
    DS_POST_INSTALL   the 25th Anniversary movie install (default: <common>\\Half-Life - POST-Anniversary for Movies)
    DS_STUDIO_APPDATA Studio's settings folder (default: %APPDATA%\\dod-studio)

The stock `Half-Life` install is never one of them: the user plays on VAC
servers with it, and nothing is injected there.
"""
import os
import subprocess
from pathlib import Path

DEFAULT_STEAM_COMMON = Path(r"C:\Program Files (x86)\Steam\steamapps\common")
INSTALL_FOLDERS = {
    "pre": "Half-Life - PRE-Anniversary for Movies",
    "post": "Half-Life - POST-Anniversary for Movies",
}
KIT_DIR = Path(__file__).resolve().parent
REPO = KIT_DIR.parents[1]


def steam_common(env=os.environ):
    """The Steam library's `steamapps\\common` folder."""
    return Path(env.get("DS_STEAM_COMMON") or DEFAULT_STEAM_COMMON)


def install_dir(name, env=os.environ):
    """The movie install called `name` ("pre" or "post"); KeyError for any other name."""
    folder = INSTALL_FOLDERS[name]
    override = env.get(f"DS_{name.upper()}_INSTALL")
    return Path(override) if override else steam_common(env) / folder


def studio_appdata(env=os.environ):
    """Studio's own settings folder (the user's: read it, never write it)."""
    override = env.get("DS_STUDIO_APPDATA")
    return Path(override) if override else Path(env.get("APPDATA", "")) / "dod-studio"


def main_checkout(repo=REPO):
    """The main checkout of the repo this kit runs from, also from a worktree.

    Test DLLs and review files live in the main checkout's `local\\`, never a
    worktree's, so tools that look for them start here. Falls back to `repo`
    when git is not on PATH."""
    try:
        common = subprocess.run(
            ["git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return repo
    return Path(common).parent
