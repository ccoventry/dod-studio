# Agent kit

Dev-only helpers that Claude agents (and anyone testing by hand) use to test DoD Studio: share the game,
play demos to the end, drive a throwaway Studio, check frames, read the activity log, see what open PRs
wait on. Nothing a player or the app runs, so their output stays plain prints. Nothing here launches the
game or Studio at import time.

Paths: no user paths are built in. Installs come from `DS_PRE_INSTALL` / `DS_POST_INSTALL` (or
`DS_STEAM_COMMON`), falling back to Steam's default library; Studio's folder from `DS_STUDIO_APPDATA`,
else `%APPDATA%\dod-studio` (only read). See `kit_paths.py`. Run the Python tools with `python -I`.

Tests: `python -m unittest discover -s .github/scripts -p "test_*.py"` runs them (CI does, through
`.github/scripts/test_agent_kit.py`).

| Tool | What it is for | One command |
| --- | --- | --- |
| `game_lock.py` | One game at a time across agents and worktrees. Checks the allowed Steam account (game_probe's `%APPDATA%\dod-studio\game_probe.json`), refuses while any `hl.exe` runs, your own included, and refuses `release` until your game has exited. Lock file: `%LOCALAPPDATA%\dod-studio-agents\game.lock`, or `--lock` / `DS_GAME_LOCK`. Exit 0 go, 1 busy, 2 refused. | `python -I tools/agent-kit/game_lock.py acquire my-topic --wait 5` |
| `play_through.py` | Plays demos to the end on PRE, POST or both (game_probe, launched behind the user's windows, fast-forwarded) and reports the last demo-clock stamp, clock jumps, the end line (Host_EndGame, Host_Error, or the clean end), crashes and known-bad console lines; JSON plus one line per run. Takes and releases the game lock per run, and moves `.bmp` snapshots a demo writes into the install out to the run folder. `--dry-run` prints the plan. | `python -I tools/agent-kit/play_through.py ktps8w7-element_over_harr_h1 --install both --dll <abs path to DLL>` |
| `demo_gaps` (Rust, `native/examples/demo_gaps.rs`) | First look at a damaged or salvaged demo: frame walk straight from the bytes with each section's clock, jumps, holes (where the walk stops, by byte), level changes (signons with their time), the directory, and whether the strict parser reads it. | `cargo run --release -p native --example demo_gaps -- a.dem` |
| `studio_live/studio_live.py` | A throwaway Studio on its own Vite and CDP ports and its own settings folder: `patch` (the worktree reads `DS_LIVE_APPDATA`, skip-worktree so it is never committed), `build`, `seed` (from the user's settings, only read), `start`, `stop` (fails with exit 3 if the user's settings.json changed). | `python -I tools/agent-kit/studio_live/studio_live.py start <worktree> <live dir> --vite-port 5222 --cdp-port 9322` |
| `studio_live/cdp.cjs` | Runs JavaScript in that Studio's page over CDP and prints the result; `--watch` logs the capture footer and toasts as they change. | `node tools/agent-kit/studio_live/cdp.cjs --port 9322 -e "return document.title"` |
| `studio_live/dlg.ps1` | Answers a native file dialog Studio opened (WM_SETTEXT + BM_CLICK, no focus). Edit ids: 1152 folder picker, 1148 Open, 1001 Save. | `powershell -ExecutionPolicy Bypass -File tools/agent-kit/studio_live/dlg.ps1 -Title "Select Demo Folder" -Path D:\tests\demos -EditId 1152` |
| `frames.py` | Frame checks: black rows/columns at each edge, crop a HUD region (kill feed, crosshair, chat, scoreboard, objectives) at any resolution, contact sheet of N frames from a take folder or video, diff two frames, and the stopwatch every comparison video gets (`clock`, was `add_clock.py`). | `python -I tools/agent-kit/frames.py edges frame.png` |
| `activity_log.py` | Studio's activity log for a day: filter by tag (`--tag capture --tag HLAE`), regex, time or last session; `--jobs` pairs each job's start and end line with its duration. | `python -I tools/agent-kit/activity_log.py --date yesterday --jobs --tag capture` |
| `pr_board.py` | Open PRs oldest first: CI, behind/conflicts, draft, stacked base, a `local\test-<n>` DLL in the main checkout, and the body's "Test done" and needs-the-user lines. Needs `gh`. | `python -I tools/agent-kit/pr_board.py` |
| `BRIEF.template.md` | The rules brief for an unattended multi-agent pass, with the session paths as placeholders. | copy to `local\unattended\BRIEF.md` and fill in |

## A throwaway Studio, start to finish

    python -I tools/agent-kit/studio_live/studio_live.py patch <worktree>
    python -I tools/agent-kit/studio_live/studio_live.py build <worktree> --vite-port 5222 --target-dir <worktree>-target
    python -I tools/agent-kit/studio_live/studio_live.py seed <scratch>\live --install post
    python -I tools/agent-kit/studio_live/studio_live.py start <worktree> <scratch>\live --vite-port 5222 --cdp-port 9322 --exe <worktree>-target\debug\desktop-studio.exe
    node tools/agent-kit/studio_live/cdp.cjs --port 9322 --studio <worktree>\studio -e "document.querySelector('#add-folder-btn').click(); return 'ok'"
    powershell -ExecutionPolicy Bypass -File tools/agent-kit/studio_live/dlg.ps1 -Title "Select Demo Folder" -Path <folder> -EditId 1152
    python -I tools/agent-kit/studio_live/studio_live.py stop <scratch>\live

The build needs the hook DLL at `<worktree>\target\i686-pc-windows-msvc\release\` (CLAUDE.md, build traps)
and `studio\node_modules` copied into the worktree. A click that opens a native dialog blocks the
evaluating call until the dialog closes: click from a `setTimeout` (as above) and answer the dialog after.

## Related tools elsewhere

- `goldsrc-hooks/tools/game_probe.py`: one scripted in-game test; `play_through.py` drives it.
- `goldsrc-hooks/tools/crash_report.py`: every game crash in the hook logs, grouped, known ones named.
- `goldsrc-hooks/tools/crosshair_frames.py`: whether a crosshair is drawn, frame by frame.
- `native/examples/demo_integrity.rs`, `hl-demo-auditor --integrity`: deltas built on frames a demo lacks.
- PR #1375 (unmerged) adds `analysis::repair` with its own frame walk and a `repair_compare` example; once
  it merges, `demo_gaps` could use that walk instead of its own.
