# VEGAS bridge and MCP server (R&D, #1050)

Lets Claude (or any script) drive VEGAS Pro 21: open or start a project, put clips on the timeline, set event speed, add Media FX or Event FX with parameters, render a range to a file, save to a new path. Not built by CI or shipped with Studio.

Two parts:

- **`extension/`**: a VEGAS application extension (`DodStudioVegasBridge.dll`, C# against VEGAS's own `ScriptPortal.Vegas.dll`). VEGAS loads it into every VEGAS, but it listens only in a VEGAS started for it (see Safety), on the named pipe `\\.\pipe\dodstudio-vegas-<pid>` (current user only), for one JSON command per line, and runs each on VEGAS's UI thread inside an undo block, so Edit > Undo takes back one call.
- **`mcp/vegas_mcp.py`**: an MCP server (stdio, Python standard library only). Its `launch` tool starts its own VEGAS beside the user's, on a hidden desktop, and the other tools go to that VEGAS's pipe. `mcp/vegas_instances.py` starts, finds and ends those VEGAS processes, `mcp/desktops.py` makes the hidden desktop and starts VEGAS on it, and `mcp/bridge_pipe.py` is one pipe connection with a time limit on each call.

From a shell:

    python vegas_mcp.py launch [work-folder]              # start a bridge VEGAS (hidden, runs until end); prints its pid
    python vegas_mcp.py call --pid N <cmd> [json-args]    # one command (in PowerShell 5.1, pass the JSON from a variable or file)
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
| `launch`, `end` | Start a bridge VEGAS (beside the user's, on a hidden desktop; later tools go to it); end one by PID and remove its autosave, also while a call to it is stuck |
| `instances`, `use` | Every VEGAS, which have a bridge, which is busy with a call past its time limit; send later tools to another bridge VEGAS |
| `ping`, `project_info` | Versions, pid, work folder, dialogs answered; the open project's settings, tracks, events (time, speed, media, FX with parameter values) and media pool |
| `new_project`, `open_project` | Start a project (size, fps, pixel format) or open a `.veg` (one outside the work folder is copied in and the copy opened). Both refuse to drop unsaved changes unless `discard_changes` |
| `import_edl` | Open an EDL Text File (Studio's Export VEGAS EDL or Export Project to VEGAS) the way VEGAS does, as a new project; then size/fps and Select None (for the nameplate script). Reports how many events are offline |
| `save_as` | Save to a **new** `.veg` path. Overwrites only a file this bridge session saved, with `overwrite` |
| `add_media`, `set_event` | Put a file or subclip on the timeline (a new track goes after the existing ones, so no index moves; the reply gives its index); change an event's speed (`playback_rate` 0.2 = 5x slow), start, length, offset, resample |
| `create_subclip` | A subclip of a file (it has its own Media FX chain) |
| `list_effects`, `add_effect`, `set_effect_params` | Find video FX; add one to an event or a media's FX chain; set OFX parameters by name or label |
| `blend_by_speed` | Frame Blend (#1051) that follows each event's speed, see below |
| `list_render_templates`, `render` | Find templates; render a range to a new file (never overwrites); `files` lists what was written, an Image Sequence's numbered files too |
| `snapshot` | See the project at a time (or up to 10 `times`): the composited frame at Best quality and full size, saved as a PNG under `<work folder>\snapshots` (a fresh name each time) and returned inline as an image, so Claude can check its own edits. `width` scales it down (default 1280, 0 = full size, never up). Leaves the cursor, preview settings and project as they were, see below |
| `run_script` | Run a C# VEGAS script from the work folder for anything else; compiled first as VEGAS compiles it (C# 5, no System.Core), so a compile error comes back instead of leaving a box open |

Every reply carries `dialogs` when VEGAS raised any during the call, with how each was answered, and `earlier_call` once a call that ran past its time limit has finished.

Every bridge tool takes `timeout_seconds`: how long the server waits for VEGAS. The default is 120 s, 600 s for `open_project` and `import_edl` (at least `wait_seconds` + 60), and an hour for `render`. Past it the call is an error that names the command and the dialog-like windows open in that VEGAS, and the VEGAS is marked busy: a later call to it is refused at once (its reply would be read as the old one's) until the late reply comes or `end` ends it.

## Safety

The user edits in VEGAS on the same PC, and their VEGAS comes first.

- **Off in every VEGAS the user starts.** VEGAS loads the extension into all of them, so it listens only when its environment has `DODSTUDIO_VEGAS_BRIDGE=1` and a `DODSTUDIO_VEGAS_ROOT` that exists, which only `launch` sets. A VEGAS started from the Start menu, a shortcut or a `.veg` opens no pipe, so nothing can reach the project being edited.
- **One pipe per VEGAS** (`dodstudio-vegas-<pid>`), so bridge VEGAS run beside the user's and beside each other, and a command never reaches a VEGAS it was not meant for: tools go only to a VEGAS this server started unless `use` names another (one started by another session or script is theirs), and when the current one goes, the server says so instead of switching.
- **The work folder.** `save_as`, `render`, `snapshot` and `run_script` refuse paths outside `DODSTUDIO_VEGAS_ROOT`; `open_project` copies a project from elsewhere into `<work folder>\copies` and opens the copy, and `import_edl` takes only EDL text files (`.txt`, `.edl`), so the user's own `.veg` files are never opened or saved. `launch`'s `work_folder` must be inside the server's `DODSTUDIO_VEGAS_ROOT` (or, when that is unset, inside `%TEMP%\dodstudio-vegas`), is never a drive root, and no folder name in it may end in a space or dot. Media is read from anywhere. It is a guard rail, not a sandbox: a script `run_script` runs can write anywhere, VEGAS writes `.sfk` peak files beside the media it imports, and a junction inside the folder leads out.
- **Unseen.** Started minimised and without activation, VEGAS still put its start-up splash and its render progress window (WPF, unowned, shown even in a minimised VEGAS) over the user's full-screen game. So `launch` starts each VEGAS on a desktop of its own (`CreateDesktop` in the user's window station, named `dodstudio-vegas-<server pid>-<n>`; `mcp/desktops.py`), which is never shown: every window of that VEGAS, its splash, dialogs and any process it starts are there. Measured: no window of a bridge VEGAS visible on the user's desktop through launch, snapshot, renders (CPU and a failing NVENC one) and every dialog test; snapshots still match an Image Sequence render pixel for pixel. Window messages pass only within a desktop, so the dialog answerer, which runs inside VEGAS, still reaches its boxes; a tool outside that sends a VEGAS window messages must move a thread onto that desktop first (`SetThreadDesktop`). The desktop goes when the last process on it has. `minimised: false` starts it on the user's desktop instead, shown without focus, to watch it work.
- **Starting.** `launch` starts `vegas210.exe -SCRIPT:mcp\start_vegas.cs` (a script that does nothing), minimised and without focus. Started with a script, VEGAS opens an empty untitled project instead of its last one (often the user's), and its recent-projects list is left as it was (both measured on build 315). It refuses to start while an `*.autosave.veg` exists whose PID is not a running VEGAS (a crashed session's recovery file), since the new VEGAS would offer to restore it; only VEGAS's default autosave folder is checked, not a custom one set in its preferences. The extension clears the two variables once read, so nothing VEGAS starts inherits them.
- **Ending.** A VEGAS this server started is ended through the process handle it kept from starting it, which also stops Windows giving its PID to another process (the user's next VEGAS) meanwhile. Any other (`end` from the shell, or one reached with `use`) is ended only when it is running VEGAS with a bridge pipe, all checked through the handle that ends it. Never with `vegas.Exit()`, which asks "cancel the background operation?" on screen while peaks build. VEGAS deletes `%LOCALAPPDATA%\VEGAS Pro\21.0\<pid as 8 hex digits>.autosave.veg` only on a clean exit, and a leftover one makes the next VEGAS ask to recover it, so ending deletes that PID's autosave (and `.bak`), and closes a crash's Problem Report box for that PID. Each tool call runs on a thread of its own, so the server always reads stdin: when Claude Code closes it, the server ends every VEGAS it started at once, a stuck call or not (measured: 0.1 s). A server killed outright cannot, so a VEGAS it launched watches it (`extension/OwnerWatch.cs`): the launch passes `DODSTUDIO_VEGAS_OWNER_PID` and `DODSTUDIO_VEGAS_OWNER_START` (its creation time), read and cleared like the other two; the bridge opens a handle to that process (which keeps its PID from being reused) and checks the start time, and when it exits, deletes its own autosave and terminates its own process (measured: gone 0.1 s after the server was killed, autosave removed). VEGAS does not hold its autosave open between saves, and a VEGAS that ends itself leaves it, so it is deleted first. The shell's `launch` passes no owner, since it exits at once.
- **Dialogs** (`extension/DialogAnswerer.cs`), in the bridge VEGAS's own windows only, and only the active box: a Windows dialog box that is modal (enabled, its owner disabled, the owner's enabled popup) or has no owner (enabled and shown); a WinForms form inside its `ShowDialog`, owned or not (a script's too, from any thread). Missing media gets "Ignore all missing files and leave them offline" and OK; the first-render "Hint" box OK; "save changes?" No; any other box still open after 4 s Cancel, else No, else OK, else Close, with its text in the reply (a failed NVENC render's "An error occurred while creating the media file" box gets OK). Left alone: a dialog with a progress bar (a render's), VEGAS's offer to restore an autosave, a box still open after 3 presses (reported), and WPF windows (VEGAS's render progress is one, and nothing outside a WPF window tells a modal box from it): a call held up by one runs into its time limit, and `end` ends that VEGAS. English VEGAS only. A script that asks questions with message boxes therefore gets those answers: run it in a VEGAS of your own instead.
- **Errors.** A tool call that fails for any reason returns an error; it never ends the server (which would end its VEGAS). A call past its time limit returns an error too, and its VEGAS refuses calls until it has replied (see Tools). After the current VEGAS goes, bridge tools are refused until `launch` or `use`, so a retried command never lands in another VEGAS. A bridge command that fails part-way is rolled back (its undo block is cancelled).
- Nothing saves in place: `save_as` never overwrites a file it did not write in this session, and `render` and `snapshot` never overwrite a file.

## Snapshots

`snapshot` uses `Vegas.SaveSnapshot`, which renders the frame at the preview's quality and size, so each call sets the preview to Best at full size, takes its frames, and puts the preview and the edit cursor back. The frame is the one a render gives, pixel for pixel (checked against an Image Sequence PNG render of the same frames). A frame takes about 0.1 to 0.5 s (a black gap is fastest, a 2560x1440 ProRes clip scaled down slowest); a call that has to change the preview size waits about 0.3 s more for VEGAS to apply it. The snapshot leaves no undo step and does not mark the project modified (the preview settings are not undoable edits). Each setting goes back on its own, also when the call fails part-way; a call is every frame or none, so a failure deletes the PNGs it had already written. Files are named `<project>_<time>s_<when>_<pid>.png` and claimed with `FileMode.CreateNew`, so two bridge VEGAS sharing a work folder never take each other's names. `folder` may be the work folder itself or any folder inside it.

The MCP result is the reply as text, then one image block per frame. The inline copy is the PNG when it is at most 1 MB (a scaled-down or simple frame), else a JPEG of the same pixels (quality 90, 0.3 MB for a 1280 px frame of a game capture, whose PNG is 2.5 MB), since every image in a conversation is sent again with each later request. A frame wider than 2000 px (`width: 0` on a 4K project) goes inline scaled to 2000 px wide, and says `inline_width`; its PNG on disk keeps the full size. Each frame says `inline_as`, or `not_inline` with why (over 4 MB in one call: the bridge stops encoding inline copies there); `inline: false` sends the text only. From the shell (`call ... snapshot`) the reply shows the paths, not the base64.

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
- Two bridge VEGAS started at once were both ready in 12 s; one on a hidden desktop is ready in 4 to 5 s.
- `Vegas.SaveSnapshot(path, format, time)` works in a minimised VEGAS, but moves the edit cursor to `time`, renders at the preview's quality and size (Draft / Quarter gives a 720p project as 160x90), and throws `ArgumentOutOfRangeException` past the end of the project. Its PNG is RGBA, transparent where no event is, so `snapshot` flattens it onto black.
- `Project.AddVideoTrack()` inserts the new track at index 0, moving every other track down one; `AddAudioTrack()` appends. `new VideoTrack(project, project.Tracks.Count, name)` then `Tracks.Add` appends, which `add_media` does.
- An Image Sequence render to `f.png` writes `f_000000.png`, `f_000001.png` and on, never `f.png`; `render` lists those and refuses a path whose numbered files exist.
- VEGAS runs scripts in their own AppDomain ("Script Domain"; the extension is in "VEGAS Domain"), so `Control.FromHandle` cannot see a script's WinForms forms. A WinForms button clicks only when the mouse is over it at mouse-up, so `BM_CLICK` misses in a hidden form, but its form turns a `WM_COMMAND` (BN_CLICKED) from it back into a click from any AppDomain, which the dialog answerer sends.
- Bad media (a text file named `.mp4`, an `.xyz`) makes `add_media` fail with `E_FAIL` and no box; a garbage `.veg` makes `OpenProject` return false, no box.
- VEGAS writes its autosave a few seconds after the first edit, not at start, and does not hold it open.
- The preview settings (`Project.Preview`) are not undoable edits: changing them adds no undo step and leaves `IsModified` alone. From an extension the quality still needs an `UndoBlock` (`E_UNEXPECTED` without one); the size does not. A new quality applies at once, but a new preview size only after VEGAS's own message loop has run, not inside a command and not with `Application.DoEvents`, so `snapshot` takes its frames in a later step of the same command (`LaterStep` in `extension/UiQueue.cs`).
