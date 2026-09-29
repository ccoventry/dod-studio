# DoD Studio: what it does today

Written from the `dev` branch at `9eb40a5` (2026-09-27), then refreshed against `dev` at `19602ab` (2026-09-29) for #436, #454–#458, #461, #462, #471, #472, #414, #418 and #421. Everything here was read from the code, not from older docs. Open pull requests are not counted; where one changes a fact below, it is named.

When this file and the code disagree, the code wins. Fix this file in the same PR that changes the behaviour.

Contents:

1. [The short version](#1-the-short-version)
2. [The desktop app, page by page](#2-the-desktop-app-page-by-page)
3. [How a capture batch works](#3-how-a-capture-batch-works)
4. [Render Studio](#4-render-studio)
5. [The companion DLL (goldsrc-hooks)](#5-the-companion-dll-goldsrc-hooks)
6. [HD textures](#6-hd-textures)
7. [Demo analysis](#7-demo-analysis)
8. [Command-line tools](#8-command-line-tools)
9. [Where things are stored](#9-where-things-are-stored)
10. [Pre-Anniversary vs 25th Anniversary](#10-pre-anniversary-vs-25th-anniversary)
11. [Known gaps and inconsistencies](#11-known-gaps-and-inconsistencies)

---

## 1. The short version

DoD Studio turns Day of Defeat 1.3 demos into highlight clips.

1. **Scan** a folder of `.dem` files. Every kill streak becomes a highlight row.
2. **Pick** highlights and press Start Capture Batch.
3. The app **patches** copies of the demos with timed console commands, then launches `hl.exe` through HLAE. The game fast-forwards between highlights and records each one. This is **HLAE Game Capture**; OBS can record instead.
4. **Render Studio** finds the recorded takes and transcodes them with FFmpeg.

Around that pipeline sit a demo analyzer (scoreboards, streaks, chat, rounds), a duplicate-demo auditor, an HD texture builder, and a companion DLL that fixes and hides things inside the game.

Two game installs matter. The **pre-Anniversary for Movies** install ("PRE") is the one every hook was written against. The **25th Anniversary** install is supported by decision, but on `dev` several hooks still work only on PRE (section 10). Never use the stock Steam install for any of this; see `docs/vac_safety.md`.

---

## 2. The desktop app, page by page

The app is Tauri v2 with a plain-JS frontend in `studio/`. Top-level tabs, in order: **Studio**, **Demo Auditor**, **Demo Analyzer**, **HD Textures**. The Studio tab has three sub-tabs: **Capture**, **Render**, **Configuration**.

Every visible label comes from `studio/src/strings.js`, which overwrites the fallback text in `index.html` at startup.

### 2.1 Always present

- **File menu:** New Session (Ctrl+N), Load Session (Ctrl+O), Save Session (Ctrl+S). A session is a JSON project file holding the queue, highlight statuses, notes and the take index. There is no project autosave.
- **Help menu:** Check for Updates, View Logs (reveals today's activity log), About.
- **Updates:** two channels, Stable (from `main`) and Experimental (from `dev`). Switching channel counts as an update, so it can downgrade. The startup check is skipped in local and debug builds.
- **Unsaved-changes prompt** on close. F5 and Ctrl+R are swallowed. Ctrl+W is swallowed and does nothing.
- **Toasts** bottom-right, **OS notifications** with six per-kind switches, and an uncaught-error net that logs to the activity log and shows at most three toasts per session.
- No drag-and-drop anywhere in the app. File and folder picking always uses the native dialog.

### 2.2 Studio → Capture

**Directory Scan & Management.** Add Demo Files or Add Folder, then the scan runs with a set number of parallel workers (Configuration → Pipeline, 1–8, default 2). The status line shows `Scanning n / N — <demo>` from the start. Each demo is fully analysed, and the result also warms the analyzer cache. Cancel Scan stops it.

- **Known demos are skipped.** A demo already in the queue whose file is unchanged is not parsed again. Unchanged means the same path and the same size plus hash of the first 64 KiB. The toast counts these separately. A changed file is rescanned, and the rescan keeps its statuses, selection, notes and kill ranges.
- **Adding a folder always scans it**, including a folder that was added before. A folder that no longer exists gives a "Not found" error.
- **Unreadable demos are named.** A warning toast lists each one with a reason: too short to be a demo, not a Half-Life demo (bad header), or corrupt partway through.

**Master Demo Queue.** One row per demo with counts: Highlights, Selected, Pending, Captured, Rendered. Counts include only the recording player's streaks. A search box filters by name, path or map, and every bulk action works on the visible rows only:

- **Clear Untracked** removes demos with no statuses, notes or narrowed ranges.
- **Clear Selected** and **Clear All** ask to save first when tracked work would be lost.
- A **Maps needed** banner lists maps that are missing or a different build than the demo expects, with a Download button that fetches from the KTP mirror, verifies the checksum, and never overwrites a map in place. HLTV demos are skipped because their map cannot be verified.

**Highlight Details.** One row per kill streak of the recording player, at or above **Min Kills** (default 1). Columns: select, kill range (narrowable), kills, time, duration, status (None / Pending / Captured / Rendered), notes, weapon timeline.

Status can be changed by hand in either direction. A status set by hand shows a small ✎ and a toast with **Undo**. A verified capture or render replaces a hand-set status; a Rendered that a real render set is never knocked back to Captured.

Buttons:

- **Launch Preview** patches a `<stem>_preview.dem` with a bookmark per highlight and opens it with `viewdemo`.
- **Generate All Previews** does the patching for every demo without launching.
- **View Match Telemetry** jumps to the Demo Analyzer on that demo.
- **Launch Game (HLAE)** starts the game with no demo. It is hidden outside local and debug builds.
- **Advanced Diagnostics** is a collapsed canvas timeline of the streaks with their pre/post-roll margins.

If a game the hook DLL is in is already open, Launch Preview sends it `viewdemo <stem>_preview` over a local pipe (section 5.1) instead of relaunching. Otherwise, if `hl.exe` or HLAE is already running, a dialog offers Force Relaunch, Copy View Command, or Cancel.

**Start Capture Batch** sits in the pinned footer. It is disabled while any of these is true: a banned or too-long command is present, no highlight is selected, no Destination is set, or the required disk space exceeds what is free. The footer shows free and required space and a progress bar. When the batch ends (or is cancelled), the app checks every planned take folder on disk, flips covered highlights to Captured, and writes `dodstudio_take.json` with the capture FPS into each take.

### 2.3 Studio → Render

Covered in section 4.

### 2.4 Studio → Configuration

Eight tabs. Every field saves to `settings.json` as soon as it changes.

| Tab | What is on it |
|---|---|
| **Paths** | Half-Life executable, HLAE executable, FFmpeg override, GoldSrc Hooks DLL override. Each shows "no file at this path" style warnings. `hl.exe` inside Steam's own `Half-Life` folder gets a VAC warning. An **HLAE FFmpeg** row reports whether HLAE can find an FFmpeg and offers **Point HLAE at FFmpeg**, which writes `<HLAE>\ffmpeg\ffmpeg.ini` (never over one it did not write, with a UAC retry). |
| **Output Format** | Width and height (1280×720), Capture FPS (300), **Capture Mode** (Frame sequence, Video, OBS). Video mode adds a capture codec (Ut Video default, FFV1, x264 lossless, uncompressed). OBS mode adds host, port, password, OBS path, Launch OBS, **Test Connection**, and OBS Capture FPS (120). Test Connection is not read-only: it creates or repairs a `[DoD-Studio]` profile and scene in OBS and switches to them. |
| **Timing** | Initial Delay (3.0 s), Pre-roll (2.0 s), Start Lead (0), Stop Trail (0), Post-roll (0.6 s). FF Speed (0.05) is shown but locked; its value is the `host_framerate` used for fast-forward. Every timing field can be 0. A banner warns when pre/post-roll is shorter than the batch needs; it never clamps. A live timeline table illustrates the result. |
| **Pipeline** | Flush Decals Between Clips (on), Save Local Patched Copy, Auto-clear Logs, Auto-clear Previews, Auto-clear Temp Demos, **Clear Previews...** (lists stale `_preview.dem` files the app made and deletes the ones you tick), and **Demo Scan Workers** (1–8, default 2, with a hint of about 1.2 GB per worker against this PC's RAM). |
| **Commands** | **Initial Commands** (run once at demo load; first-run defaults `r_decals 256` and `mirv_fov 90`; Import Config reads a `.cfg`), and **Scheduled Commands** (each runs a number of seconds before or after a highlight). Warning banners explain what will not take effect, what is refused, and what your own game configs set. See 3.5 for the tiers. |
| **Destinations** | Folders that captures are written to. Render Studio scans the same folders. |
| **Render Settings** | Codec, custom FFmpeg args, source FPS, max concurrent renders (1–8, default 2), export drives. |
| **Notifications** | Six switches: patching started/finished, demo loading, fast-forward to clip, captures done, renders done, errors. |

### 2.5 Demo Auditor

Finds byte-identical duplicate demos under one folder. Files are keyed by size plus a hash of the first 64 KiB. Each duplicate group keeps its first file and pre-ticks the rest for deletion. The footer shows duplicates found and wasted space.

### 2.6 Demo Analyzer

- **Explorer sidebar:** Pinned, Recent and Local quick links, a drive/folder tree, optional per-folder demo counts, a resizable width.
- **Demos table:** the selected folder's demos (not recursive), filterable by text, type, map and date, sortable. The type column here is a filename guess ("hltv" in the name).
- **Report**, seven sub-tabs:
  - **Summary:** file, map, server, who recorded it, demo type, match type (public, clan pre-game, clan incomplete, clan full), durations.
  - **Scoreboard:** by team, with POV, reconnected and pre-existing-stats badges and a partial-recording warning.
  - **Player Details:** Steam links, score, kills, deaths, lifespans, weapon breakdown, kill streaks with weapon filters.
  - **Team Details**, **Timeline** (team score chart), **Rounds**, **Chat Log** (with team, alive/dead and system-message filters).

There is no export from the page. The CLI can export Markdown or JSON (section 8).

### 2.7 HD Textures

Covered in section 6.

---

## 3. How a capture batch works

### 3.1 Capture modes

| Mode | What records | What lands in a take |
|---|---|---|
| **Frame sequence** (default) | HLAE `mirv_recordmovie`, one BMP per frame | `take0000/<stream>/00000.bmp…` plus `sound.wav` |
| **Video** | HLAE pipes frames to an FFmpeg it launches itself | `take0000/<stream>/video.avi` plus `sound.wav`. Needs HLAE to find an FFmpeg (the Paths row). |
| **OBS** | OBS Studio over obs-websocket v5, started and stopped from console-log markers | `take0000/all/video.<ext>` with audio muxed in, no wav |

There is no fourth mode and no Separate HUD setting. The alpha launch flags are sent on every launch, so typing `mirv_movie_separate_hud 1` into Initial Commands still produces a usable HUD alpha stream.

### 3.2 Planning

For each demo and recording player, highlights are sorted and grouped into **blocks**. Two highlights share one recording when their windows overlap once the start lead, stop trail and a **take separation** are added. The separation is 1.0 s for HLAE (a guess; issue #9) and 2.0 s for OBS (measured). Highlights that do not share a recording but whose speed-change windows collide stay separate takes but skip the fast-forward between them.

Each block gets a disk estimate, and blocks are packed onto the Destinations with first-fit-decreasing. A drive must keep 15 GiB free.

### 3.3 Patching

Nothing in your original demo changes. The app writes:

- `dod/dodstudio_primer.dem`: a copy of the first demo that plays for 500 frames and then chains to the first real job. It exists because the first demo of a game session renders too dark (issue #365; PR #397 fixes the cause on PRE).
- `dod/dodstudio_chain_NN.dem`: one patched copy per demo and player. Each carries:
  - your Initial Commands plus the app's own (`mirv_movie_fps`, OBS frame pacing, and `r_decals` when the decal flush needs it and you did not set it);
  - per block: return to normal speed, sync audio, point `mirv_movie_filename` at the block's folder, start recording, stop recording, fast-forward again;
  - your Scheduled Commands, anchored to the first or last kill;
  - a director text label per highlight, `echo` markers the app reads back from `qconsole.log`, and a breadcrumb every 5000 frames;
  - at the end, `playdemo` of the next chain file, or the exit trigger on the last one.
- `dod/dodstudio_helper.cfg`: the aliases those commands call.

Injected commands are standalone `ConsoleCommand` frames, written after `DemoStart` and never inside an existing network message. Each is limited to 63 bytes by the demo file format.

**Decal flush.** With Flush Decals on, a pre-pass rewrites a scratch copy of each demo so wall decals from outside the recorded windows are removed, and bursts of harmless decals clear the ring before each block. A failure here never fails the batch; the unmodified demo is used and the reason is logged.

### 3.4 Launching and running

Every launch (batch, preview, Launch Game) uses one command line:

    hlae.exe -customLoader -noGui -autoStart
      -hookDllPath <HLAE>\AfxHookGoldSrc.dll
      [-hookDllPath dodstudio_goldsrc_hooks.dll]
      -programPath <hl.exe>
      -cmdLine "-game dod -insecure -windowed -w W -h H -gl -32bpp
                -afxRenderMode standard -afxForceAlpha8 1 -condebug <extra>"

A batch adds `+exec dodstudio_helper.cfg +playdemo dodstudio_primer`. The hook DLL is added only if the file exists (section 5.1). `-demoedit` (PR #401) and `-addons` (PR #412) are not on `dev` yet.

While the game runs, the app reads its markers and turns them into status lines and notifications. They come from the game's events pipe (section 5.1) once it connects, and from `qconsole.log` until then, or throughout for a game without the hook DLL. The batch ends when:

- `BATCH_COMPLETE` arrives over the events pipe (any mode), OBS mode sees it in the log, or the exit trigger folder appears (HLAE creates it on the last `mirv_movie_filename` call; the fallback);
- you cancel (the app kills `hl.exe`);
- the game closes on its own ("closed manually or crashed");
- in OBS mode, markers stop arriving for too long, or OBS disconnects.

On failure or cancel the copied demos, junctions and scratch files are removed. Takes are never deleted.

### 3.5 Command tiers

Commands you type into Initial or Scheduled Commands are checked twice: once for the warning banner, and again when the batch starts. The lists live in `native/src/patch/cfg_scan.rs`; read it rather than trusting this summary.

Every command must also fit a demo's 64-byte command field, which holds 63 characters. A longer one is refused the same way as a banned command, with a red row under its field.

| Tier | Commands | What happens |
|---|---|---|
| Banned everywhere | `mirv_recordmovie_start`, `mirv_recordmovie_stop`, `mirv_movie_ffmpeg`, `host_framerate`, `r_drawentities`, `cl_lw` | Refused; Start is disabled |
| Banned when scheduled | `r_decals`, `mirv_fov`, `gl_widescreenfov`, `mirv_movie_filename` | Fine in Initial Commands, refused in Scheduled |
| Mid-demo hazards | the pipeline-owned commands, as a warning | Shadowed with a warning, not refused |
| No effect | `exec`, `quit` everywhere; `mirv_movie_filename` in Initial | Reported as doing nothing |
| Fatal config values | `cl_lw` not 1; `r_drawentities` not 1 while `sv_cheats` is on | Reported from your game configs; DoD's client quits over these |

Your game's own `.cfg` files are read, never written.

---

## 4. Render Studio

**Scan for Takes** walks the Destinations and stages every renderable take as a Queued row. A take is renderable when it has at least one stream folder and audio, either a `.wav` beside it or an audio track inside its video. A muted OBS recording is therefore not renderable.

Frame-sequence and video takes with HUD streams become two rows: the full picture, and a HUD-only row rendered as ProRes 4444 with alpha.

| Codec | FFmpeg video | Container | Audio |
|---|---|---|---|
| ProRes 422 HQ (default) | `prores_ks -profile:v 3`, 10-bit 4:2:2 | `.mov` | PCM |
| DNxHR HQ | `dnxhd -profile:v dnxhr_hq` | `.mov` | PCM |
| HuffYUV | `huffyuv` | `.avi` | PCM |
| Uncompressed | `rawvideo` | `.avi` | PCM |
| H.264 (software) | `libx264 -crf 16` | `.mp4` | AAC 192k |
| H.264 (NVENC) | `h264_nvenc -cq 15` | `.mp4` | AAC 192k |
| Custom | your arguments, unvalidated | `.mkv` | PCM |
| Skip (OBS takes only) | no FFmpeg; the file is copied | source | — |

Each row shows its own codec and FPS snapshot, status, speed, progress and size, with Cancel, Reset, Remove, View Log and Open Folder actions. The footer has Start Render Batch, Cancel All, Reset All and Remove All (Not Rendering).

- **Scheduling:** up to the max-concurrent limit; each job reserves space on the first export drive with room.
- **Crash recovery:** a lockfile (`.render_autosave.json`) is written at queue time and after each finished job. On the next start, a dialog offers to recover the batch. Recovered rows are stubs until rescanned. Per-job codec choices are lost on recovery until PR #395 lands.
- **FFmpeg:** the override path, else `local/tools/ffmpeg.exe` beside the app, else `ffmpeg` on PATH.

---

## 5. The companion DLL (goldsrc-hooks)

### 5.1 How it gets into the game

`goldsrc-hooks/` builds `dodstudio_goldsrc_hooks.dll` for 32-bit Windows. HLAE injects it alongside its own DLL on every launch the app makes. The app looks for it in this order: the path set in Configuration → Paths, the copy bundled with the installed app, then beside `hlae.exe`. If none exists the game still launches, and a warning is logged.

At load the DLL hooks two imports of `hw.dll` (`GetProcAddress`, `LoadLibraryA`). That lets it catch `client.dll` loading and swap four client functions for its own wrappers. From then on it runs a small amount of work every frame. `client.dll` does not reload between demos, so this happens once per game session.

It logs to `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks_YYYYMMDD.log`, and a crash handler records any crash as `module+offset` with a stack trail.

**Commands from Studio.** The DLL serves a local named pipe, `\\.\pipe\dodstudio-hl-<pid>`, and runs each line Studio writes to it as a console command on the next frame. Only the same Windows user on the same machine can write to it. Launch Preview uses it today. `GOLDSRC_HOOKS_REMOTE=0` turns it off.

**Events to Studio.** A second pipe, `\\.\pipe\dodstudio-hl-<pid>-events`, carries the pipeline's `[dod-studio]` markers from the game as the engine runs each `echo` (the DLL wraps `echo` through the engine's command list, with no per-build address). Markers from before Studio connects are sent when it does. `GOLDSRC_HOOKS_EVENTS=0` turns it off; Studio then reads `qconsole.log` as before (issue #434, step 1).

### 5.2 Console commands

Every name starts `dodstudio_`. None is saved into `config.cfg`. `docs/dodstudio_commands.md` is the user-facing reference; this table is what the code registers on `dev` (15 cvars, 10 commands).

| Name | Kind | Default | What it does | Works on |
|---|---|---|---|---|
| `dodstudio_allow_shaders` | cvar | 0 | Lets map surfaces be drawn through the engine's own GLSL shaders (`platform/gl_shaders`) during demo playback. Needs `gl_use_shaders 1`; `gl_reloadshaders` recompiles edits live. World surfaces only, not models. | Anniversary only (PRE has no shaders) |
| `dodstudio_clear_decals` | command | — | Wipes every decal from the world right now | PRE only (PR #398 adds Anniversary) |
| `dodstudio_deathmsg` | command | — | Kill feed: `max`, `offset`, `block`, `fake` | both |
| `dodstudio_debug_hd_misses` | command | — | Lists textures that kept their original this session, and why | PRE only |
| `dodstudio_debug_log_spectator_target` | cvar | 0 | Logs spectator mode and target changes (#206 diagnostic) | both |
| `dodstudio_debug_log_texture_loads` | cvar | 0 | One log line per HD-eligible texture load | PRE only |
| `dodstudio_debug_log_weapon_model` | cvar | 0 | Logs the spectated player's third-person weapon model | both |
| `dodstudio_debug_msglog` | command | — | Hex-dumps chosen DoD user messages to the log | both |
| `dodstudio_debug_status` | command | — | Prints the state of every fix and setting | both |
| `dodstudio_ex_interp_max` | cvar | 100 | Raises the engine's `ex_interp` ceiling (51–1000) | PRE (PR #398 adds Anniversary) |
| `dodstudio_hd_enabled` | cvar | 1 if `dod\dodstudio_hd` exists | HD texture replacement on or off | PRE only (PR #399 adds Anniversary) |
| `dodstudio_hd_style` | cvar | `ultrasharp` | Which HD style folder to read | PRE only |
| `dodstudio_hide_crosshair` | cvar | 0 | Hides the POV and spectator crosshair | both |
| `dodstudio_hide_hand_signals` | cvar | 0 | Replaces hand-signal animations with the player's normal pose | both |
| `dodstudio_hide_hudelement` | command | — | Hides one of ten HUD elements: `crosshair`, `deathnotice`, `icons`, `menu`, `message`, `objectives`, `saytext`, `statusbar`, `train`, `vgui2print` | both |
| `dodstudio_hide_scoreboard` | cvar | 0 | Stops `+showscores` opening the scoreboard | both |
| `dodstudio_hide_sprite` | command | — | Hides map sprites by model path (`env_sprite` only) | both |
| `dodstudio_hltv_gunshot_attenuation` | cvar | 0.3 | How far gunshots carry while the gunshots fix is on | both |
| `dodstudio_hltv_gunshots_fix` | cvar | 0 | Makes distant gunshots audible while spectating | both |
| `dodstudio_hltv_show_viewmodel_animations` | cvar | 0 | Animates the spectated player's first-person gun (levels 0–4) | both |
| `dodstudio_match_pov_crosshair` | cvar | 0 | Draws the spectator crosshair in the POV style from `cl_xhair_style` | both |
| `dodstudio_mute_voice_commands` | cvar | 0 | Silences voice-command sounds; the chat line stays | both |
| `dodstudio_objectives` | command | — | Moves the objective icons and timer (`offset`, `xoffset`, `timer`) | both |
| `dodstudio_overviewmap` | command | — | Places and sizes the full and mini overview map | both |
| `dodstudio_reload_demo` | command | — | Plays the last `playdemo`/`viewdemo` demo again from the start | both (wraps the engine's own commands through the SDK's command list, no per-build address) |

Two fixes have no console name and are on by default: the **temp-entity crash fix** (DoD's own NULL-sprite crash, `GOLDSRC_HOOKS_TEMPENT_FIX=0` turns it off) and the **hull-trace guard** (the #384 crash after a `playdemo` map change, PRE only, `GOLDSRC_HOOKS_HULL_TRACE_GUARD=0` turns it off).

Not compiled on `dev`: `spectator_bars.rs` (both approaches failed live; issue #328).

### 5.3 Offline verification

`goldsrc-hooks/tools/verify_*.py` re-derive every patched address from the DLL on disk with `pefile` and `capstone`. `survey_client_dll.py` and `survey_hw_dll.py` regenerate the survey docs. `crash_report.py` groups crashes from the hook logs against known causes. They default to the PRE install's paths.

---

## 6. HD textures

**In the game:** the DLL replaces textures as the engine uploads them, from `dod\dodstudio_hd\<type>\<style>\`, where type is `world`, `models`, `sprites`, `detail` or `sky`. An `overrides` folder per type wins over any style. Replacements are capped at 1024 pixels a side. The engine's own `gl_max_size` (default 256) still clamps each side, so set it to 512 or 1024 to see the gain. PR #423 raises the cap to 4096 and lets `gl_max_size` decide.

**The page** (HD Textures tab):

- **What's built:** a table of files and size per style and type, with a Refresh.
- **Use it in the game:** the two lines to put in `movie.cfg` for the chosen style, with Copy.
- **Tools:** finds or downloads the Real-ESRGAN upscaler, its models, and a private Python 3.12 with numpy, Pillow and SciPy. You can also point it at your own copies.
- **Build:** tick styles and asset types, then Build. It runs `goldsrc-hooks/tools/hd/build_all.py` with live step progress; Cancel keeps finished files.

**Built-in styles:** `ultrasharp` (default), `remacri`, `siax`, `generalv3`, `x4plus` (all Real-ESRGAN models), `plain` (Lanczos plus sharpening) and `blend` (x4plus and plain mixed). Your own styles go in `dod\dodstudio_hd\my_styles.txt`, and `hd_maps.txt` limits which maps are built.

Command-line only: `compare.py` (a side-by-side sheet of styles), `setup_tools.py`, the per-type scripts, `--also` and `--extra-models`. Batched writing (`HD_BATCH`, PR #404), the misses view, custom-style form, comparison, and map picker (PRs #390, #391, #392, #422) are not on `dev` yet.

---

## 7. Demo analysis

### 7.1 What is parsed

`dem-patch/` (a fork of the `dem` crate) reads the demo file into frames and engine messages. The fork adds malformed-input safety (errors, not aborts), nom 8, and cancellable writing. `dod/` decodes DoD's own user messages. Of 63 names it knows, the analyzer asks for 26; everything else is decoded and dropped.

### 7.2 What is computed

| Output | How |
|---|---|
| Players | From userinfo. Identity is SteamID64, else `*fid`, else slot. Stats survive reconnects by folding the old session into an accumulated total. |
| Scoreboard | Score, kills and deaths from the score messages; sorted by team, then score, kills, deaths. |
| Kill streaks | Each death closes the victim's streak. Team kills and world kills give no credit. A grenade kill after the thrower died joins their previous streak. A round reset closes every streak. |
| Weapon breakdown | Kills and team kills per weapon per player. |
| Lifespans | Spawn to death, per player. |
| Rounds | Start, end, winner, and the winner's kills that round. |
| Team score timeline | Every team-score change. |
| Chat | Say and team chat with dead/team tags, plus system messages translated from the game's own strings. |
| Clan match | Detected from the clan timer, wave respawn time, or a reset followed by a start with everyone at 0. |
| Partial recording | "Started late" and "ended early" flags, from time-left and round state. |
| British | Allies become British the first time anyone plays a British class. |
| Map change | Once there is gameplay, a new level ends the analysis, even the same map loaded again; before that, the warm-up is discarded. |
| Demo type | "HLTV" if any HLTV or director message appears, else "POV". PR #395 stops patched previews counting as HLTV. |

Analyses are cached as JSON in `%APPDATA%\dod-studio\analyzer_cache\v2\`, keyed by path and invalidated by size and modified time.

### 7.3 Highlights

A highlight is any streak with at least one kill, for every connected player. The Master Queue and Highlight Details then keep only the recording player's streaks, and Min Kills filters further. HLTV demos appear in the queue but show no highlights of their own (issue #247).

### 7.4 Web analyzer

`web-analyzer/` compiles the same analysis to WebAssembly for a static page at `https://ccoventry.github.io/dod-studio/`. It deploys on every push to `main`. It shows the same seven report tabs from a hand-copied renderer (issue #238). It takes one file at a time and has no export (issue #102).

---

## 8. Command-line tools

| Binary | What it does |
|---|---|
| `preview_cli` | Drag demos or folders onto it; writes `<stem>_preview.dem` bookmark files into a `previews` folder. `--player` picks one player in an HLTV demo. |
| `dod-studio-cli` | `analyze <demos>` prints a Markdown or JSON match report. `patch-streak` is an older standalone patcher. |
| `dod-studio-dump` | Header, frame and message counts, first commands and sounds of one demo. |
| `dod-studio-inspect` | Library statistics across folders: maps, message frequency, duplicates. |
| `check_maps` | Per-demo map status against a maps folder, with optional download. |
| `check_cfgs` | What your game configs set, and what Initial Commands would override. |
| `strip_decals` | Runs the decal flush on one demo with every knob exposed. |
| `hl-demo-auditor` | The duplicate finder as a CLI; writes a Markdown report. |
| `dod-benchmark` | Times each stage of the analyzer load path. |

`native/examples/` and `analysis/examples/` hold R&D probes built only with `--examples`.

---

## 9. Where things are stored

| What | Where |
|---|---|
| Settings | `%APPDATA%\dod-studio\settings.json`. The OBS password is stored in plain text. |
| Activity log | `%APPDATA%\dod-studio\logs\activity_YYYYMMDD.md`, 30 days kept |
| Hook DLL log | `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks_YYYYMMDD.log`, 30 days kept |
| Analyzer cache | `%APPDATA%\dod-studio\analyzer_cache\v2\` (no eviction) |
| Render lockfile | `%APPDATA%\dod-studio\.render_autosave.json` |
| Capture manifests | `%APPDATA%\dod-studio\manifests\<session_id>.json`. Written as `planned` when a batch starts and rewritten as `complete` or `cancelled` with each block's verdict. The newest 50 are kept. |
| HD tools | `%APPDATA%\dod-studio\hd_tools\` |
| Project files | wherever you save them |
| Patched demos, helper cfg | `<game>\dod\` (`dodstudio_primer.dem`, `dodstudio_chain_NN.dem`, `dodstudio_helper.cfg`) |
| Previews | `<game>\dod\<stem>_preview.dem` plus a hidden `.dodstudio_preview` marker |
| Takes | `<Destination>\<session>\dodstudio_chain_NN_bK\take0000\...` |
| HD files | `<game>\dod\dodstudio_hd\` |
| HLAE FFmpeg link | `<HLAE>\ffmpeg\ffmpeg.ini` |
| OBS | a `[DoD-Studio]` profile and scene inside your OBS |

`DOD_STUDIO_LOG_DIR` redirects both logs. An old `%APPDATA%\dod-tools` folder is migrated once, without overwriting.

---

## 10. Pre-Anniversary vs 25th Anniversary

The app itself has no build detection. The install used is whatever `hl.exe` you point Paths at.

In the DLL, each module finds its code by a byte pattern and refuses loudly if the pattern is missing or appears twice, so the wrong build fails safe. `client.dll` is byte-identical across the stock, PRE and Anniversary installs, so every module that patches only `client.dll` works on both. The `hw.dll` modules were written against PRE.

| On `dev` | Modules |
|---|---|
| Both builds | everything in `client.dll`: kill feed, crosshair, spectator crosshair, scoreboard, voice mute, HUD elements, hand signals, objectives, overview map, map sprites, message log, viewmodel animations, gunshots, temp-entity fix |
| PRE only | decal clear, HD textures, hull-trace guard |
| Anniversary only | world shaders (`dodstudio_allow_shaders`); the PRE engine has no shader path |
| Unstated | `ex_interp` ceiling (would refuse loudly on a mismatch) |

Open PRs close most of the gap: #393 (hull guard), #396 (ESC keeps GameUI windows open on Anniversary), #398 (decals, `ex_interp`), #399 (HD textures). The first-load lighting fix in #397 is PRE only.

---

## 11. Known gaps and inconsistencies

Each of these is true on `dev` today. Items with an issue number are tracked; the rest are not yet.

**Behaviour**

- The uncaught-error toast says details went to `crash_log.md`; they go to the activity log.
- Three different HLTV tests disagree: the analyzer's (any director message), the scanner's (a header string that never matches, #247), and the Explorer's (the filename).
- A recovered render batch has stub rows. A job resumed without a rescan can fail with "no audio source".
- The analyzer computes POV stats (shots, reloads, scoped kills) and admits five objective messages, but nothing displays or uses either (#192).
- `web-analyzer` is never built for WebAssembly in CI; a break shows up only at deploy.
- The analyzer cache is never pruned, and old `v1` folders are not removed.

**Dead code**

- `.autosave.json` is deleted after a batch but never written (#20).
- `DOD_BATCH_DONE` is polled but never created.
- `dod_quit.cfg` is written but never executed.
- `hlcr_config.json` load and save have no caller.
- The WASM gates in `native/` guard nothing, since no WASM build uses `native`.

**Naming and docs drift**

- The installer and first window title still say "DoD Tools Studio", with identifier `com.dodtools.studio` (#258).
- `goldsrc-hooks/README.md` still describes the gunshots fix as "full volume with no distance attenuation"; the code only lowers attenuation.
- `goldsrc-hooks/src/engine.rs` still says `client.dll` reloads between demos; it does not.
