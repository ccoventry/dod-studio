# VEGAS bridge and MCP server (R&D, #1050)

Lets Claude (or any script) drive VEGAS Pro 21: open or start a project, put clips on the timeline, set event speed, add Media FX or Event FX with parameters, render a range to a file, save to a new path. Not built by CI or shipped with Studio.

Two parts:

- **`extension/`**: a VEGAS application extension (`DodStudioVegasBridge.dll`, C# against VEGAS's own `ScriptPortal.Vegas.dll`). VEGAS loads it at start; it listens on the named pipe `\\.\pipe\dodstudio-vegas` (current user only) for one JSON command per line and runs each on VEGAS's UI thread inside an undo block, so Edit > Undo takes back one call.
- **`mcp/vegas_mcp.py`**: an MCP server (stdio, Python standard library only) that forwards tool calls to that pipe. `python vegas_mcp.py call <cmd> '<json args>'` runs one command from a shell.

## Install (no admin)

    powershell -ExecutionPolicy Bypass -File vegas-mcp\build.ps1 -Install

builds with the `csc.exe` that ships with .NET Framework 4.x and copies the DLL to `%APPDATA%\VEGAS Pro\21.0\Application Extensions`. Restart VEGAS; **Tools > Extensions > DoD Studio bridge** confirms it loaded. `-Uninstall` removes it. On a PC that enforces WDAC it is signed with the `LocalRustDev` certificate when the user has one.

Register the MCP server with Claude Code (the user does this, not an agent):

    claude mcp add vegas -- python C:\path\to\dod-studio\vegas-mcp\mcp\vegas_mcp.py

## Tools

| Tool | What it does |
|---|---|
| `ping`, `project_info` | Versions; the open project's settings, tracks, events (time, speed, media, FX with parameter values) and media pool |
| `new_project`, `open_project` | Start a project (size, fps, pixel format) or open a `.veg`. Both refuse to drop unsaved changes unless `discard_changes` |
| `save_as` | Save to a **new** `.veg` path. Overwrites only a file this bridge session saved, with `overwrite` |
| `add_media`, `set_event` | Put a file or subclip on the timeline; change an event's speed (`playback_rate` 0.2 = 5x slow), start, length, offset, resample |
| `create_subclip` | A subclip of a file (it has its own Media FX chain) |
| `list_effects`, `add_effect`, `set_effect_params` | Find video FX; add one to an event or a media's FX chain; set OFX parameters by name or label |
| `blend_by_speed` | Frame Blend (#1051) that follows each event's speed, see below |
| `list_render_templates`, `render` | Find templates; render a range to a new file (never overwrites) |
| `run_script` | Run a VEGAS script file for anything else |

## Safety

VEGAS reopens the last project when it starts. The bridge never saves in place, never overwrites a file it did not write in this session, and never renders over a file. To keep a test away from a real project, start VEGAS on a test project: `vegas210.exe D:\some-test-folder\test.veg`.

## Frame Blend by speed (#1051)

VEGAS never tells an OFX effect an event's speed, and an Event FX only sees frames at the project rate (measured in #1051), so Frame Blend works only as a Media FX. `blend_by_speed` does the per-event part at edit time: every event on the track gets its own subclip covering exactly the media it plays, with Frame Blend as that subclip's Media FX and Frames = capture fps / project fps x speed (5 at normal speed on 60 fps, 2 at 2.5x slow, none at 5x slow). It adds the subclip as a new take and keeps the old one, so switching back undoes it. Run it again after changing an event's speed.

## Gotchas found

- Project edits from an extension must sit in an `UndoBlock`; without one every change fails with `E_UNEXPECTED`.
- VEGAS fixes its project-matching render templates ("Default Template") the first time templates are listed in a session. Listed before the timeline had video, they stay audio-only until VEGAS restarts, so `render` refuses a template without a video stream.
- A "Hint" box (OK only) appeared during the very first bridge render and held it until OK was pressed (not seen again in three later sessions, so probably shown once). `render` presses OK on Hint boxes of its own VEGAS while it runs and reports how many; that path has not been exercised since.
