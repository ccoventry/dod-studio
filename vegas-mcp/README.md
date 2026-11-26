# VEGAS bridge and MCP server (R&D, #1050)

Lets Claude (or any script) drive VEGAS Pro 21: open or start a project, put clips on the timeline, set event speed, add Media FX or Event FX with parameters, render a range to a file, save to a new path. Not built by CI or shipped with Studio.

Two parts:

- **`extension/`**: a VEGAS application extension (`DodStudioVegasBridge.dll`, C# against VEGAS's own `ScriptPortal.Vegas.dll`). VEGAS loads it into every VEGAS, but it listens only in a VEGAS started for it (see Safety), on the named pipe `\\.\pipe\dodstudio-vegas-<pid>` (current user only), for one JSON command per line, and runs each on VEGAS's UI thread inside an undo block, so Edit > Undo takes back one call.
- **`mcp/vegas_mcp.py`**: an MCP server (stdio, Python standard library only). Its `launch` tool starts its own VEGAS beside the user's, and the other tools go to that VEGAS's pipe. `mcp/vegas_instances.py` starts, finds and ends those VEGAS processes.

From a shell:

    python vegas_mcp.py launch [work-folder]              # start a bridge VEGAS; prints its pid
    python vegas_mcp.py call [--pid N] <cmd> '<json args>' # one command
    python vegas_mcp.py instances                         # every VEGAS, and which have a bridge
    python vegas_mcp.py end <pid>                         # end it and remove its autosave

Tests that need no VEGAS: `python -I -m unittest discover -s vegas-mcp/mcp -p "test_*.py"` (not run by CI).

## Install (no admin)

    powershell -ExecutionPolicy Bypass -File vegas-mcp\build.ps1 -Install

builds with the `csc.exe` that ships with .NET Framework 4.x and copies the DLL to `%APPDATA%\VEGAS Pro\21.0\Application Extensions`. In a VEGAS you start yourself, **Tools > Extensions** shows "DoD Studio bridge (off in this VEGAS)". `-Uninstall` removes it. On a PC that enforces WDAC it is signed with the `LocalRustDev` certificate when the user has one.

Register the MCP server with Claude Code (the user does this, not an agent), with the work folder the bridge may write in:

    claude mcp add vegas -e DODSTUDIO_VEGAS_ROOT=D:\some-test-folder\vegas -- python C:\path\to\dod-studio\vegas-mcp\mcp\vegas_mcp.py

Without `DODSTUDIO_VEGAS_ROOT` the work folder is `%TEMP%\dodstudio-vegas`; `launch` takes `work_folder` too. `DODSTUDIO_VEGAS_EXE` points at another `vegas210.exe`.

## Tools

| Tool | What it does |
|---|---|
| `launch`, `end` | Start a bridge VEGAS (minimised, beside the user's; later tools go to it); end one by PID and remove its autosave |
| `instances`, `use` | Every VEGAS and which have a bridge; send later tools to another bridge VEGAS |
| `ping`, `project_info` | Versions, pid, work folder, dialogs answered; the open project's settings, tracks, events (time, speed, media, FX with parameter values) and media pool |
| `new_project`, `open_project` | Start a project (size, fps, pixel format) or open a `.veg` (one outside the work folder is copied in and the copy opened). Both refuse to drop unsaved changes unless `discard_changes` |
| `import_edl` | Open an EDL Text File (Studio's Export VEGAS EDL or Export Project to VEGAS) the way VEGAS does, as a new project; then size/fps and Select None (for the nameplate script). Reports how many events are offline |
| `save_as` | Save to a **new** `.veg` path. Overwrites only a file this bridge session saved, with `overwrite` |
| `add_media`, `set_event` | Put a file or subclip on the timeline; change an event's speed (`playback_rate` 0.2 = 5x slow), start, length, offset, resample |
| `create_subclip` | A subclip of a file (it has its own Media FX chain) |
| `list_effects`, `add_effect`, `set_effect_params` | Find video FX; add one to an event or a media's FX chain; set OFX parameters by name or label |
| `blend_by_speed` | Frame Blend (#1051) that follows each event's speed, see below |
| `list_render_templates`, `render` | Find templates; render a range to a new file (never overwrites) |
| `run_script` | Run a C# VEGAS script from the work folder for anything else; compiled first as VEGAS compiles it (C# 5, no System.Core), so a compile error comes back instead of leaving a box open |

Every reply carries `dialogs` when VEGAS raised any during the call, with how each was answered.

## Safety

The user edits in VEGAS on the same PC, and their VEGAS comes first.

- **Off in every VEGAS the user starts.** VEGAS loads the extension into all of them, so it listens only when its environment has `DODSTUDIO_VEGAS_BRIDGE=1` and a `DODSTUDIO_VEGAS_ROOT` that exists, which only `launch` sets. A VEGAS started from the Start menu, a shortcut or a `.veg` opens no pipe, so nothing can reach the project being edited.
- **One pipe per VEGAS** (`dodstudio-vegas-<pid>`), so bridge VEGAS run beside the user's and beside each other, and a command never reaches a VEGAS it was not meant for: tools go only to a VEGAS this server started unless `use` names another (one started by another session or script is theirs), and when the current one goes, the server says so instead of switching.
- **The work folder.** `save_as`, `render` and `run_script` refuse paths outside `DODSTUDIO_VEGAS_ROOT`; `open_project` copies a project from elsewhere into `<work folder>\copies` and opens the copy, so the user's own `.veg` files are never opened or saved. Media is read from anywhere.
- **Starting.** `launch` starts `vegas210.exe -SCRIPT:mcp\start_vegas.cs` (a script that does nothing), minimised and without focus (`minimised: false` shows it, still without focus, to watch it work). Started with a script, VEGAS opens an empty untitled project instead of its last one (often the user's), and its recent-projects list is left as it was (both measured on build 315).
- **Ending.** `end` stops only a process that has a bridge pipe (or that this server started) and is VEGAS, by PID: never the user's. It never calls `vegas.Exit()`, which asks "cancel the background operation?" on screen while peaks build. VEGAS deletes `%LOCALAPPDATA%\VEGAS Pro\21.0\<pid as 8 hex digits>.autosave.veg` only on a clean exit, and a leftover one makes the next VEGAS ask to recover it, so `end` deletes that PID's autosave (and `.bak`), and closes a crash's Problem Report box for that PID. When Claude Code closes the server, it ends every VEGAS it started.
- **Dialogs** (`extension/DialogAnswerer.cs`), in the bridge VEGAS's own windows only: missing media gets "Ignore all missing files and leave them offline" and OK; the first-render "Hint" box OK; "save changes?" No; any other modal box still open after 4 s Cancel, else No, else OK, else Close, with its text in the reply. A dialog with a progress bar (a render's) is left alone. A script that asks questions with message boxes therefore gets those answers: run it in a VEGAS of your own instead.
- Nothing saves in place: `save_as` never overwrites a file it did not write in this session, and `render` never overwrites a file.

## Frame Blend by speed (#1051)

VEGAS never tells an OFX effect an event's speed, and an Event FX only sees frames at the project rate (measured in #1051), so Frame Blend works only as a Media FX. `blend_by_speed` does the per-event part at edit time: every event on the track gets its own subclip covering exactly the media it plays, with Frame Blend as that subclip's Media FX and Frames = capture fps / project fps x speed (5 at normal speed on 60 fps, 2 at 2.5x slow, none at 5x slow). It adds the subclip as a new take and keeps the old one, so switching back undoes it. Run it again after changing an event's speed.

## Gotchas found

- Project edits from an extension must sit in an `UndoBlock`; without one every change fails with `E_UNEXPECTED`.
- An EDL opened with `OpenFile` (what File > Open of an EDL does) **replaces** the open project, asking "save changes to Untitled?" when it has any, and `OpenFile` returns before the events exist. `import_edl` clears the dirty state first and pumps messages until the timeline fills (#1303's test).
- `project_info` gives each event its file (`media_path`, a subclip's parent), `offline` and `selected`, and each media `offline`, so an import can be checked for files VEGAS did not find.
- VEGAS fixes its project-matching render templates ("Default Template") the first time templates are listed in a session. Listed before the timeline had video, they stay audio-only until VEGAS restarts, so `render` refuses a template without a video stream.
- Commands run on VEGAS's UI thread from a WinForms timer (`extension/UiQueue.cs`), not `Control.Invoke`: inside an Invoke callback (and `run_script` from one), `OpenProject` of a project with missing media never returns once its box is answered, idle in VEGAS's own wait loop; started from a WM_TIMER, or from a `-SCRIPT` start, it returns in about a second.
- Windows hides the windows a minimised window owns, so a minimised bridge VEGAS's dialogs are hidden. A push button in a hidden dialog ignores `BM_CLICK`, so the answerer sends the dialog the button's `WM_COMMAND` instead (radio buttons still take `BM_CLICK`).
- VEGAS 21's MP4 reader (`dp4_reader`) crashed once in `add_media` of a 300 fps clip while three bridge VEGAS were reading media (`std::terminate` during its own exception); the same call passed every other time. `end` closes the crash's Problem Report box.
- A "Hint" box (OK only) appeared during the very first bridge render and held it until OK was pressed (not seen again in later sessions, so probably shown once per PC). The dialog answerer presses OK on it; that path has not been exercised since.
- Two bridge VEGAS started at once were both ready in 12 s.
