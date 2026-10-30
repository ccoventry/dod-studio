# DoD Studio: what it does today

Written from the `dev` branch on 2026-09-27, refreshed against `dev` on 2026-10-07 (up to #638), and again after v0.2.0 shipped on 2026-10-22 (up to #921). Everything here was read from the code, not from older docs. Open pull requests are not counted; where one changes a fact below, it is named.

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

1. **Scan** a folder of `.dem` files. Every kill streak becomes a highlight row: the recording player's in a POV demo, every player's in an HLTV demo.
2. **Pick** highlights (optionally after **Review highlights** plays them in the game) and press Start Capture Batch.
3. The app launches `hl.exe` through HLAE and hands DoD Studio's hook DLL a schedule: the game plays the original demos, seeks between highlights and records each one (**Seek Between Clips**, on by default). With it off, the app **patches** copies of the demos with timed console commands instead and the game fast-forwards between highlights. This is **HLAE Game Capture**; OBS can record instead.
4. **Render Studio** finds the recorded takes and transcodes them with FFmpeg.

Around that pipeline sit a demo analyzer (scoreboards, streaks, chat, rounds, flags, a kill map), a demo auditor (duplicates, renames, multi-map splits), an HD texture builder, an overview maker, a Blender page, and a companion DLL that fixes and hides things inside the game and adds DoD Studio's own window there.

Two game builds are supported: the **pre-Anniversary** engine ("PRE", as in a pre-Anniversary for Movies install) and the **25th Anniversary** engine. Every hook works on both, except the first-load lighting fix, which only PRE needs, and world shaders, which only the Anniversary engine has (section 10). Never use the stock Steam install for any of this; see `docs/vac_safety.md`.

---

## 2. The desktop app, page by page

The app is Tauri v2 with a plain-JS frontend in `studio/`. Top-level tabs, in order: **Studio**, **Demo Auditor**, **Demo Analyzer**, **HD Textures**, **Overviews**, **Blender**. The Studio tab has three sub-tabs: **Highlights**, **Render**, **Configuration** (#117).

Every visible label comes from `studio/src/strings.js`, which overwrites the fallback text in `index.html` at startup.

### 2.1 Always present

- **File menu:** New Session (Ctrl+N), Load Session (Ctrl+O), Save Session (Ctrl+S). A session is a JSON project file holding the queue, highlight statuses, notes, the take index and the Teams list's names and merges. There is no project autosave.
- **Project files are `.dodstudio`** (#528): JSON inside. A first save defaults to `.dodstudio`; Load Session also opens older `.json` projects, and saving one writes back to that same `.json`. Once a `.json` project has loaded (after the missing-demo check and the rescan offer), Studio offers to switch it: **Switch to .dodstudio** renames the file in its folder (never over an existing `.dodstudio`; unsaved changes are saved into it) and Open Recent follows; **Keep as .json** is remembered for that file (`kept_json_projects` in settings). No offer for a read-only file, a folder Studio can't write to, or a `.autosave.json`. Double-clicking a `.dodstudio` file opens it in DoD Studio like Load Session (the unsaved-changes prompt, the missing-demo check, the rescan offer); with Studio already open, that window opens it. The installer registers `.dodstudio` (`bundle.fileAssociations`).
- **`.dem` files in Explorer** (Settings → Pipeline, #528): *Show DoD Studio in Open With for .dem Files* adds DoD Studio to a demo's Open With menu, per user, without changing what a double-click opens; unticking removes it, and so does uninstalling. *Open .dem Files In* sends a demo opened that way to the Demo Analyzer (default) or Studio's queue.
- **Help menu:** Check for Updates, What's New, View Logs (reveals today's activity log), Setup…, About.
- **What's New** (#1013, also from About): this version's release notes, which the release workflows put in the build so they read offline (a local build has none), then the other versions from GitHub's release list, each folded, stable ones and, on the Experimental channel, the experimental build. The list is read once per session while update checks are on, and saved in the app's data folder (`release_notes_cache.json`); with checks off the window shows the saved copy and a button to read GitHub. Each version shows its grouped notes; the list of every pull request stays folded under "Everything else". The first start after an update shows a toast with a button to open it, and the update window shows the pending update's notes before you install it.
- **Setup** (#763): a window on first launch walks through what DoD Studio needs (a movie copy of Half-Life, HLAE, FFmpeg, output folders, demos); Help → Setup… opens it again, and a checklist stands in for the empty Master Demo Queue on the Highlights tab while something Start needs is missing. Its Make a movie copy… button copies the dod folder to a new one for movies (demos only if asked, logs never), gives it the language files it needs and picks it as the Game Folder (#939).
- **Updates:** an **Update <version> available** button appears beside the Help menu when a new version is out, and says **Restart to update** once it is downloaded (#869). Two channels, Stable (from `main`) and Experimental (from `dev`). Switching channel counts as an update, so it can downgrade. The startup check is skipped in local and debug builds, and a local build's Check for Updates only reports versions, never installs. The window title of a local build names the git branch and dev-server port.
- **Unsaved-changes prompt** on close. Closing while a capture batch runs asks first too; the game keeps capturing without Studio. F5 and Ctrl+R are swallowed. Ctrl+W is swallowed and does nothing.
- **Toasts** bottom-right, **OS notifications** with six per-kind switches, and an uncaught-error net that logs to the activity log and shows at most three toasts per session.
- No drag-and-drop anywhere in the app. File and folder picking always uses the native dialog.

### 2.2 Studio → Highlights

**Directory Scan & Management.** Add Demo Files or Add Folder, then the scan runs with a set number of parallel workers (Configuration → Pipeline, 1–8, default 2). The status line shows `Scanning n / N — <demo>` from the start. The demos the analyzer cache has go first, then the rest largest first (#687); the line then adds `(n from the analyzer cache)` and a time left measured in bytes. Each demo is fully analysed, and the result also warms the analyzer cache. Cancel Scan stops it. Before any demo is read, a folder (a whole drive too) is listed with `Looking for demos: N folders read, M demos found` on the line; Cancel Scan there ends the scan with nothing added and the folder not remembered. A folder + Add Folder scans goes into the Demo Analyzer's Recent folders, not the pinned ones; pin it from there (#533). Once, after updating, Studio lists the folders older builds pinned and lets you untick the ones you don't want. **Include subfolders** beside + Add Folder (on by default, saved with the settings) decides whether a folder's subfolders are read too (#531). The walk never follows a junction or symlink, reads no folder twice, matches `.dem` in any case, and stops at 6 levels down or 50,000 files and folders; when it stops there, a toast and the activity log say so. Demos and folders dragged from Windows onto Directory Scan & Management or the Master Demo Queue are added as the buttons would (#530): both sections are outlined while you drag, files and folders in one drop scan together, dropped folders go into Recent and follow Include subfolders, anything that is neither a `.dem` nor a folder is named in a toast and skipped, a drop during a scan is refused with a toast, and one anywhere else on the page does nothing. The Recycle Bin and System Volume Information are never walked, so deleted demos don't come back (#843).

**Demo folders the game can play (#376).** Below the scan buttons, + Link Folder to Game links a folder picked on any drive into the game folder of the Half-Life Executable setting, as a junction at `dod\demos\<name>` (the folder's name, made safe for the console: letters, digits, `_`, `-`). The game then plays its demos in place: `viewdemo "demos/<name>/<demo>"`. A folder that holds the game install or sits inside it, a whole drive, a folder that is itself a link, and a network path are refused with the reason. The list shows each link with its console line; Remove unlinks it and leaves the folder and its demos alone. Both actions write an activity-log line. Every folder scan in the app skips junctions and symlinks and stops at a depth cap (`native::sys::folder_walk`; the ones that look for demos or takes also skip `$RECYCLE.BIN` and `System Volume Information`, #843), so a link back up the tree cannot make a scan loop. Why a link and not a path: `docs/goldsrc_viewdemo.md`, "Demos on another drive".

- **Known demos are skipped.** A demo already in the queue whose file is unchanged is not parsed again. Unchanged means the same path and the same size plus hash of the first 64 KiB. The toast counts these separately. A changed file is rescanned, and the rescan keeps its statuses, selection, notes and kill ranges.
- **Adding a folder always scans it**, including a folder that was added before. A folder that no longer exists gives a "Not found" error.
- **Unreadable demos are named.** A warning toast lists each one with a reason: too short to be a demo, not a Half-Life demo (bad header), or corrupt partway through.

**Master Demo Queue.** One row per demo with counts: Highlights, Selected, **Status** (its Pending, Captured and Rendered highlights as coloured counts) and **Review** (not reviewed, Keep, Maybe and Skip, #824). Counts cover the recording player's highlights in a POV demo and every player's in an HLTV demo. The **Filters** row under the toolbar (#127) holds them all: a search box (with an × and Esc to clear it) filters by name, path or map, a player box shows only the POV demos one player recorded (#174), quick filters narrow the list to demos with kills, a multi-kill or a headshot (#1128), or to POV demos only, and **Tracked only** shows only the demos with the bookmark badge, the ones Clear Untracked keeps: a highlight with a status, a review mark, a note, a clip name or a narrowed kill range (the demo open in Highlight Details stays listed; dimmed while no demo is tracked). **Group by** puts the demos under a header line per match, player, map, folder, date or demo type (#837). Every bulk action works on the visible rows only:

- **Review highlights** (#623) plays every highlight of the ticked demos (with none ticked, every demo in the list, #821) in the game, one after another, at normal speed. A box beside it picks which marks a review plays: All, Unmarked, Keep, Maybe or Skip. While each plays, a box on the game screen says which it is, whose and how many kills, and lists its kills (time on the demo bar, victim, weapon) with the next one to come in amber behind a `>` (#1140); the box lists the answer keys (#660), which answer while it plays, ending it early (#1141), or at its end, where the review pauses: **Y** Yes, **M** Maybe, **N** No, **R** Replay, **B** Back, **Space** Next, **S** Stop (one key per Review tab button), and **ESC** opens the DoD Studio window's Review tab for the kill range and note. For a second after each highlight starts the keys are greyed and do nothing, so a double press can't answer the next one unseen. The keys work only during a review highlight, never while typing in the console, chat or a box, and take no bind (`ds_ui_review_keys 0` opens the Review tab at each end instead; `ds_ui_review_info 0` hides the box while a highlight plays). **Yes** marks the row Keep, **Maybe** marks it Maybe and **No** marks it Skip in the Review column (#44); each saves the kill range and note typed on the Review tab, and none touches Status. For an HLTV demo, an **HLTV** box picks whose highlights play, with the camera on each player in first person (#825): by default the players missing their POV demo for that half, or the players with a POV demo in the queue, everyone, or players and whole teams you pick (#839, saved with the project); an order box plays them in match order or one player at a time (#829). While a review runs, the game's demo bar, Load Demo and the in-game window's seek controls are locked, so a stray click can't seek or load another demo (#847). **Tick Yes for capture** (on by default) also ticks each Yes row. **Fast-forward gaps over N s** (on, 8 s by default, #665) plays the stretch between two kills further apart than that at 4x, from 2 s after one kill to 4 s before the next. Replay, Back, Next and Stop move around; a second review starts at the first highlight not yet answered. Studio uses the running game it started, or starts one. A running game started with another install, resolution or hook DLL than Studio has now is not reused: Studio asks to close it and start again (#666, #1143). Highlights found before demo-player times were kept need a rescan first. The review controls (which marks it plays, the order, Tick Yes, the fast-forward) are saved with the settings and come back after a restart.
- **Saved Review marks** (#848): every highlight's Review mark, note and Kill Range is also saved outside the project, in `review_library.json` in the app data folder, project or not, so a review with no project open isn't lost. It is keyed on the demo's contents (size + head hash), so a moved, renamed or copied demo still matches, and the latest mark of a highlight wins. When a scan adds a demo with saved marks, they are found again by their kills (as a rescan carries work) and put on its highlights that have no mark, note or Kill Range of their own: at once with no project file open, after asking with one open. Opening a project offers nothing; its demos already hold what it saved.
- **Clear Untracked** removes demos with no statuses, notes, narrowed ranges or review answers.
- **Clear Selected** and **Clear All** ask to save first when tracked work would be lost.
- **Teams** lists the clan tags found in the queue's demos (issue #445), with how many demos each is in. A tag is the longest start or end that at least 60% and at least 3 of a side's player names share, kept to letters, digits and spaces. Each row takes a display name and a **Same team as** pick that merges it into another row (× on the merged tag splits it out). Names and merges are saved in the project file. Demos from a project saved before this have no teams until **Read Their Teams** scans them again. These feed the clip-name `{team_name}` and `{opponent}` placeholders (#441).
- A demo that has moved, or was deleted, is marked missing; **Locate…** points the row at where it is now, and a project that is loaded again looks for moved demos on its own. Start Capture Batch refuses while a demo with picked highlights is missing.
- A demo that recorded **more than one map** (#217) gets a **2 maps · Split** button by its name. The game's `viewdemo` stops at the first map change, so nothing past it can be captured, and its highlights cover one map only. Split writes every map of a minute or more as a demo of its own next to the original (the Demo Auditor's Split Maps), scans them into the queue and removes the original's row; the file is kept. Each highlight's picks, Review mark, note, status, clip name and Kill Range move to the same highlight (same player, same kills) in the first map's demo, which is the only map the original's highlights come from; a toast counts what moved, and any that couldn't be matched are listed with their work in the activity log. Start Capture Batch refuses picks in such a demo and offers the same split. Demos from a project saved before the map list existed are scanned again.
- A **Maps needed** banner lists maps that are missing or a different build than the demo expects, with a Download button that fetches from the KTP mirror, verifies the checksum, and never overwrites a map in place. HLTV demos are skipped because their map cannot be verified.

**Highlight Details.** One row per kill streak of the recording player (in an HLTV demo, of every player, with a **Player** box to show one, #765). Columns: select, kill range (narrowable), kills, **HS** (headshots among the kills in the range, #1128: told from the helmet that flies off in the demo, in POV and HLTV demos alike; `1+?` when some kills can't be told, two kills on one frame, a British victim, whose helmet may not pop, or in an HLTV demo any kill with no helmet seen, since the HLTV proxy loses about half of them; a demo scanned before this shows a dash until it is scanned again; clip names have `{headshots}`), player (in an HLTV demo), time, duration, review (Keep / Maybe / Skip; Skip unticks the row and locks it out of every capture batch), status (None / Pending / Captured / Rendered, each in its own colour), notes (which wrap and can be resized), weapon timeline. Time and Dur. use the demo player's clock.

Status can be changed by hand in either direction. A status set by hand shows a small ✎ and a toast with **Undo**. A verified capture or render replaces a hand-set status; a Rendered that a real render set is never knocked back to Captured.

Buttons:

- **Launch Preview** opens the demo with `viewdemo`. With the hook DLL and the demo on the game's drive (or on any drive, by its full path, with a hook DLL from #779 on), the game plays the original in place and opens the in-game window's Highlights tab, which lists the highlights with Go (#434); nothing is written. Otherwise it patches a `<stem>_preview.dem` with a bookmark per highlight. Your Initial and Scheduled Commands don't run in a preview, nor in Launch Game: only a capture batch runs them.
- **Launch Game**, beside it, starts the game with no demo, in every build.
- **View Match Telemetry** jumps to the Demo Analyzer on that demo.

If a game the hook DLL is in is already open, Launch Preview sends it the same `viewdemo` line over a local pipe (section 5.1) instead of relaunching. Otherwise, if `hl.exe` or HLAE is already running, a dialog offers Force Relaunch, Copy View Command, or Cancel. Before either, a game started with another install or resolution than Studio has now gets its own question, naming what differs: close it and start again, or Cancel to do nothing. Launch Game, Start Capture Batch and Review highlights ask the same (#666); a Seek Between Clips batch runs in an open game Studio started instead of asking to relaunch it (section 3.3, #725). The hook DLL counts too (#1143): HLAE passes it on its own command line and exits after injecting, so Studio reads which file the game loaded off its module list, and which build from the id the build bakes into the DLL (`goldsrc-hooks/build.rs`: version and commit, plus `dirty` and the build time for a build with uncommitted changes), in the game's copy and in the file on disk (`native::sys::hook_build`). Another build asks the same question, naming both builds and folders; so does a game whose DLLs can't be read. The same build counts as the same DLL whichever folder it came from; builds from before ids are told apart by path. A game with no hook DLL is left to the "already running" handling, as before. The hook writes its id to its log at start (`goldsrc-hooks build ...`).

The game never loads the hook DLL Studio ships or the one the Hooks DLL setting picks, only a copy of it (#1146): `%LOCALAPPDATA%\dod-studio\hooks\<build id>_<hash>\`, one folder per build, with `dodstudio_hook_source.txt` beside it naming the file it came from (the prompt above names that folder). Windows keeps a loaded DLL from being overwritten or deleted, so this way a test DLL can be rebuilt in place, and Studio updated, with the game open. Each launch removes the copies no running game has loaded and none was launched from in the last 10 minutes (`native::sys::hook_copy`). If the copy can't be made, the game loads the file itself, and the activity log says so.

A **capture summary** strip above Start Capture Batch says in one line how the batch is about to run (mode, codec, size and FPS, the command profile in use and whether it was edited since, Scheduled Commands, decal flush, what the finish step will save the clips as, Destinations); each part links to its setting.

**Start Capture Batch** sits in the pinned footer. It is disabled while any of these is true: a banned or too-long command is present, no highlight is selected, no Destination is set, the hl.exe or HLAE path is not set, or the required disk space exceeds what is free. The footer shows free and required space and a progress bar. Before the game starts, the app checks that Steam is running and signed in, and offers to start it. If the game or HLAE shows an error box, the app reads it and says what it means. When the batch ends (or is cancelled), the app checks every planned take folder on disk, flips covered highlights to Captured, and writes `dodstudio_take.json` with the capture FPS into each take.

### 2.3 Studio → Render

Covered in section 4.

### 2.4 Studio → Configuration

Nine tabs. Every field saves to `settings.json` as soon as it changes.

| Tab | What is on it |
|---|---|
| **Paths** | Half-Life executable (a list of the Half-Life installs found in Steam's libraries, from the registry's `SteamPath` and `libraryfolders.vdf`, each labelled with its engine build, pre-Anniversary or 25th Anniversary, read from its `hw.dll`; Browse still picks any `hl.exe`. Picking an install sets `hl.exe` and the Game Folder only: the same folder name when the new install has it, else `dod`. Each row's **Hide** takes it off the list, **Show hidden installs** at the end brings hidden ones back; while the game runs, a switch offers to close it first. On a PC with one install, Setup fills it in, #931), **Game Folder**, HLAE executable, FFmpeg override, GoldSrc Hooks DLL override (only a debug build or one made on this PC shows and uses it; an installed release always runs the hook DLL it ships with, whatever is saved). Game Folder is the folder beside `hl.exe` that launches name with `-game` (`dod` unless changed, #402): a list of the folders there holding a `liblist.gam` and a `cl_dlls/client.dll` (#768), or a text box when none can be listed; a name with a path separator, `..` or a space is refused. It is for an install that keeps several DoD versions side by side (`dod_movies`, `dod_1_2`, ...); Studio reads configs, maps and demos and writes its helper files there, and the `_addon`/`_downloads` folders it uses are that folder's (`dod_movies_addon`). The path rows each show "no file at this path" style warnings. `hl.exe` inside Steam's own `Half-Life` folder, with its own `dod` as the Game Folder, gets a note recommending a copy of the dod folder for movies, and the row reports whether this `hl.exe` gets 2 GB or 4 GB of memory. An **HLAE FFmpeg** row reports whether HLAE can find an FFmpeg and offers **Point HLAE at FFmpeg**, which writes `<HLAE>\ffmpeg\ffmpeg.ini` (never over one it did not write, with a UAC retry). |
| **Output Format** | Width and height (a new install starts at 1920×1080 when the primary screen is at least that big, else 1280×720), Capture FPS (120), **Capture Mode** (Frame sequence, Video, OBS, AGR for Blender). AGR mode adds an AGR FPS (Capture FPS when empty). Video mode adds a capture codec (Ut Video default, FFV1, x264 lossless, uncompressed). OBS mode adds host, port, password, OBS path, Launch OBS, **Test Connection**, and OBS Capture FPS (120). Test Connection is not read-only: it creates or repairs a `[DoD-Studio]` profile and scene in OBS and switches to them. |
| **Timing** | Initial Delay (3.0 s; shown only with Seek Between Clips off, since the hook waits for the demo itself), Pre-roll (5 s), Start Lead (5 s), Stop Trail (5 s), Post-roll (1 s). The fast-forward a batch falls back to when it can't skip straight to a clip uses a fixed step of `host_framerate 0.05`; it isn't a setting. Every timing field can be 0. A banner warns when pre/post-roll is shorter than the batch needs; it never clamps. A live timeline table illustrates the result. **Gaps between kills over N s: Keep / Fast-forward Nx** (#831; Keep by default, 8 s and 4x, Review's own defaults): with Fast-forward, a clip plays each wait longer than N s between two of its kills sped up, from 2 s after one kill to 4 s before the next (Review's margins), using the demo player's own speed (`dem_speed`, scheduled as `sys_ramp_on` / `sys_ramp_off` by `native::patch::builder::kill_gaps`). The take comes out exactly that much shorter, every frame after the gap matches a 1x take, and the sound stays in step, but the speed-up is baked into the clip and the sound inside it is choppy. It needs Seek Between Clips: a batch that is patched keeps its gaps and says so in the activity log. The disk estimate bills the shorter take, and the Last Batch panel labels such a take "(gap at 4x)". |
| **Pipeline** | Flush Decals Between Clips (on), **Seek Between Clips** (on; off patches every batch, see 3.3), **Keep the Game Behind Your Windows While Capturing** (on: the game records at full speed behind your windows or minimised, without taking focus or the mouse; off, it comes to the front so you can watch, #722), Save Local Patched Copy, Auto-clear Logs, Auto-clear Previews, Auto-clear Temp Demos (on), **Clear Previews...** (lists stale `_preview.dem` files the app made and deletes the ones you tick), and **Demo Scan Workers** (1–8, default 2, with a hint of about 1.2 GB per worker against this PC's RAM). |
| **Commands** | **Initial Commands** (run once at demo load; first-run defaults `r_decals 256` and `mirv_fov 90`; Import Config reads a `.cfg`), a **Profile** list of saved sets of both (with the built-in **Clean clips**: Match POV on, the scoreboard, voice menus, HLTV messages, spectator bars and warm-up text hidden, the sniper scope shown, and every other clip-look setting at the game's own. Built-in profiles stay up to date with the app and can't be changed, renamed or deleted; **Duplicate** makes your own copy of any profile. A changed copy of Clean clips from before it was built in is kept as "Clean clips (mine)", with a one-time notice), and **Scheduled Commands** (each runs a number of seconds before or after a highlight). Typing the start of a console name lists every name it could be, and says so when Studio refuses or overrides one. Warning banners explain what will not take effect, what is refused, and what your own game configs set (including a `config.cfg` that is not read-only), and an Initial Command `exec <file>` is followed into that file, read-only, the way the game finds it (`dod`, then `dod_addon`, then `dod_downloads`; only a `.cfg` or `.rc` name runs). See 3.6 for the tiers. |
| **Destinations** | Folders that captures are written to. Render Studio scans the same folders. **After a clip is finished** (keep, Recycle Bin, or delete permanently, optionally only when space is needed before a capture or render; see 4) sits here too (#1094). |
| **Render Settings** | Codec, custom FFmpeg args, source FPS (only for a take that doesn't record its capture FPS), max concurrent renders (1–8, default 2), named **Presets** of these settings (#499), export drives (none: Start Render Batch stays off and says to add one), **When a batch finishes** (see 4) with a finish codec per capture mode, and the **Clip Name Template** the finished files are named from, with the same grouped placeholder chips as Rename Demos (#441, #761). |
| **Players** | Each player's spray and crosshair in a demo (#428). **Use in the game** (on) sends `ds_player_overrides` when Studio starts the game, a review or a batch, and to a running game when it changes. One row per player from the queue's demos (or a typed SteamID or name): a spray PNG from the game's `logos` folder (or **From the demo**: the spray the demo saved, else the fallback), and a crosshair, a sprite from its `sprites` folder and a 64x64 tile picked from a 4x4 preview of it. Players not listed: sprays in POV demos their own (the default), one fallback PNG or none; in HLTV demos one fallback PNG (the default) or none, since an HLTV demo never records the picture; the crosshair the game's own (the default) or one sprite and tile. Mirrored to `%APPDATA%\dod-studio\goldsrc_hooks_players.txt` for the hook DLL, which applies it while a demo plays (`ds_player_spray` in `dodstudio_commands.md`). **Spray size** (64 by default, 96 or 128) is the size on the wall of every spray built from a PNG, in every demo, with or without HD textures. A POV fallback of their own spray with none saved is no spray, never a bullet hole. In a POV demo the crosshair needs `cl_xhair_style` 1-16. |
| **Notifications** | Six switches: patching started/finished, demo loading, fast-forward to clip (these three off by default, since they fire per demo or per clip), captures done, renders done, errors. |

### 2.5 Demo Auditor

One folder picker, then a tab per tool:

- **Duplicates:** finds byte-identical duplicate demos under the folder, plus any folders added with **Add Folder…**, or under every fixed drive with **All drives** (#844). All of them are keyed in one pass, so a copy on one drive pairs with its original on another; the Recycle Bin, System Volume Information, `.git` and `node_modules` folders are skipped, and so is the Windows folder when a drive root is walked (#888). Folders are listed and files keyed several at a time. Files are keyed by size plus a hash of the first 64 KiB, and files sharing a key are split again by a hash of their last 64 KiB, so two cuts of one recording don't group. A demo whose size no other demo has is never opened. Each demo's key is remembered between scans (`audit_keys.json` in the app data folder, valid while its size and modified time are unchanged), so a second scan opens only new or changed demos. Each group keeps one copy: one in the install's game folder or the project's queue first, then the shortest path. The other copies start ticked, except ones the game folder or the queue uses. **Move to Recycle Bin** is the delete. Since the key reads only the first 64 KiB, each ticked copy is first compared byte for byte with the copy its group keeps, and one that differs is not deleted: it's reported, and the table marks it "Not an exact copy" (unticked, not counted as wasted space) until another copy is kept. Its whole-file hash and the kept copy's are remembered with their keys, so later scans list it apart instead of grouping it again. Picking another copy to keep moves no row; the group says "Kept copy changed by you" with a **Reset** back to the audit's pick. **Delete Permanently…**, a second button with its own confirm, is hidden in 0.2.0. A bar and a step line show the folder listing, then the key pass, then the check and the delete. The footer shows duplicates found and wasted space. **Show demos backed up in archives (.zip, .7z, .rar)** (off by default, saved with the settings, #929) also reads the `.dem` files inside `.zip`, `.7z` and `.rar` files, read only: nothing is extracted or changed. An archive is a backup, not a copy to clean up: a loose demo an archive holds (a whole-file match: the loose demo is read in full once, remembered in `audit_keys.json`, and its hash compared with the archive entry's hash over every decompressed byte; a header's CRC-32 alone never counts) gets a "backed up in pack.7z" chip ("backed up in 3 archives" when several hold it, every archive and entry in its tooltip), and is never offered for deletion because of it. An archived copy never joins a group or becomes the kept copy, so groups of loose copies work as they do with the box off. A section below the groups, "Backed up in archives", lists every backed-up loose demo, including ones with no loose copy, and the footer counts them. An entry is decompressed only when a loose demo has its size, and every such entry is read, so a damaged archive whose header lists the same size and CRC-32 never shows as a backup. Each archive's entries and hashes are remembered in `audit_keys.json` while its size and modified time hold. In a solid `.7z` or `.rar`, reaching one entry decompresses the entries before it in its block, so the first audit of a big solid pack takes longer; blocks holding nothing needed are skipped. An archive inside an archive isn't opened; a corrupt or encrypted archive (or entry), a part of a split `.rar`, and a 7z block asking for a dictionary over 1.5 GiB are skipped with a line in the activity log. Rar reading uses RARLAB's freeware UnRAR library (`docs/third_party_notices.md`).
- **Split Maps (#624):** lists every demo under the folder that recorded more than one map, with each map's start and length. Ticked maps are written as demos of their own next to the original, which is never changed. A map under a minute starts unticked (usually the next map loading as the recording stopped). The list comes from a remembered answer, the analyzer cache's `signon_maps`, or a quick byte scan, in that order (`native::demo_maps_scan`); the split itself is `native::demo_split`.

**Rename Demos** (#469) renames the same folder's demos, subfolders too, in place: one template for POV demos (default `{name}_{kills}k_v_{opponent}_{map}`) and one for HLTV demos (default `{allies}_v_{axis}_{map}_{date}`), on the clip names' template engine. Team names come from the project's Teams list. `{name}` is the recording player without their team's tag (`{full_name}` keeps it), `{faction}` / `{enemy_faction}` are the sides; these and the other recording-player placeholders are refused in the HLTV template, where their chips are greyed out and say why. The chips sit in labelled rows (Player, Sides, Teams, Demo, Format). Placeholder and modifier names work in any case (`{MAP}` = `{map}`). Modifiers: `:lower`, `:upper`, and `:first`, which keeps a value's first word (`{name:first}` gives `m00cat` for "m00cat :D"); clip names share them. A word after `|` is used when a placeholder has no value, such as the team names in a pug (`{team_name|mix}`; without one, `unknown`); the `|fallback` chip inserts it and its tooltip lists the placeholders that take one. New names keep letters, digits, `-` and `_` only (runs of `_` collapse, none at either end), and **Lowercase the whole name** lower-cases them all. **List Demos** reads the demos the analyzer cache has first (`analysis::cache::is_fresh`), then parses the rest largest first, 4 at a time, with a bar and a time left measured in bytes. A preview lists every demo first; a clash gets `_2`, and a name already in the folder is never taken. **Undo Last Rename** reverses the newest batch, from a log in `%APPDATA%\dod-studio\demo_rename_logs`. A demo's analyzer cache entry moves with it.

### 2.6 Demo Analyzer

- **Explorer sidebar:** Pinned, Recent and Local quick links, a drive/folder tree, optional per-folder demo counts, a resizable width.
- **Demos table:** the selected folder's demos (not recursive), filterable by text, type, map, date and player (a demo the player recorded or played in, #437), sortable. The type column reads the demo's first 4 KB: an HLTV proxy's connect message ends "(HLTV)" (#566); only a file too short to say falls back to "hltv" in the name.
- **Cache all:** analyses every demo of the folder into the analyzer cache in the background, two at a time, skipping the ones already there (#569). Those go first, then the rest largest first, with a time left measured in bytes (#687). Opening one afterwards takes ~15 ms, and the in-game Highlights tab and the player filters read the same cache. The button turns into Stop while it runs, with a bar of the count. After a cache-format change it first upgrades the analyses an older version cached ("Upgrading cached analyses: 812 of 1,486", milliseconds each) and parses only the ones it can't upgrade.
- **A demo that recorded more than one map** (#217): the report covers only the first map, up to the next map change, as the game's `viewdemo` stops there too (cache 7; before, a first map with no gameplay was skipped). A notice above the tabs lists the maps (from the analysis's `signon_maps`) and offers **Split now**: the same split as the Demo Auditor's Split Maps, on this demo, keeping every map of a minute or more. The demos written appear next to the original, which is kept, and the first one opens.
- **A damaged demo** (a server crash, a cut file) is analysed up to where it breaks, and the page says how far it was read (#815).
- **Report**, nine sub-tabs:
  - **Summary:** file, map, server, who recorded it, demo type, match type (public, clan pre-game, clan incomplete, clan full), durations.
  - **Scoreboard:** by team, with POV, reconnected and pre-existing-stats badges and a partial-recording warning.
  - **Player Details:** Steam links (SteamID64, classic and SteamID3, each copyable), score, kills, deaths, lifespans, weapon breakdown, kill streaks with weapon filters.
  - **Team Details**, **Timeline** (team score chart), **Rounds**, **Flags** (per team: captures, how many were taken from the other team, and the capture attempts it blocked; per flag: who holds it at the end and how often it changed hands; every capture with its cappers; each player's capture credits, cap breaks (enemy captures they stopped by killing a capper) and objective points), **Chat Log** (with team, alive/dead and system-message filters).
  - **Kill Map** (#448): every death placed on the map's overview picture (from the game's `dod\overviews` folder), with the players, weapon and distance on hover, and an engagement-distance table per weapon.

There is no export from the page. The CLI can export Markdown or JSON (section 8).

### 2.7 HD Textures

Covered in section 6.

### 2.8 Overviews

Makes the image the game draws for its overview map (`overviews/<map>.tga` or `.bmp`, with its `.txt`) from the map itself (#371). `native::overview` works out every floor a player can reach from the spawns and groups it into areas. The page draws them in the colours of a hand-made overview and lets you:

- recolour an area or a single piece of floor;
- hide an area;
- rename flags;
- add labels and drag them;
- turn flags, capture zones, spawns, white stairs and water on or off.

Edits are saved per map as you go. **Save overview** writes a TGA (full colour, the background left transparent) or an 8-bit BMP at the game's 1024x768. It goes into `dod_addon/overviews` (read when the game runs with `-addons`) or `dod/overviews`. A user's own overview there is copied to `overviews/dodstudio_backup` first. The `.txt` uses the game's own placement maths, so the game's player icons land on the right spot.

The game itself shows at most 1024x768 (its loader reads the image into a fixed 3 MB buffer and cuts it into 128-pixel tiles). Ticking **Also save a high-quality copy** writes `<map>_hd.tga` at 4096x3072 beside it. With DoD Studio's hook DLL in the game (`goldsrc-hooks/src/overview_hd.rs`, both builds), each tile is re-uploaded from that copy at 512x512.

### 2.9 Blender

Rebuilds a highlight recorded in **AGR for Blender** capture mode (3.1) in Blender, with the real map, HD textures, lighting and the recorded camera, and renders it (#403). It needs Blender 4.4 with Blender Source Tools and afx-blender-scripts, and the game's models decompiled with Crowbar (`blender/README.md`). Pick the `.agr`, the models folder, the map and a texture style, then run the four steps in order: **Import**, **Build scene** (EEVEE previews or one Cycles frame), **Render** (Quick 720p or Final 1080p) and **Encode MP4**. Every step writes into one work folder beside the `.agr` (`native::blender`).

---

## 3. How a capture batch works

### 3.1 Capture modes

| Mode | What records | What lands in a take |
|---|---|---|
| **Frame sequence** (default) | HLAE `mirv_recordmovie`, one BMP per frame | `take0000/<stream>/00000.bmp…` plus `sound.wav` |
| **Video** | HLAE pipes frames to an FFmpeg it launches itself | `take0000/<stream>/video.avi` plus `sound.wav`. Needs HLAE to find an FFmpeg (the Paths row). |
| **OBS** | OBS Studio over obs-websocket v5, started and stopped from console-log markers | `take0000/all/video.<ext>` with audio muxed in, no wav |
| **AGR for Blender** | HLAE `mirv_agr`, no video: every drawn model's bones and the camera, at the AGR FPS (#450) | one `.agr` file per clip, for the Blender page (2.9) |

There is no Separate HUD setting. The alpha launch flags are sent on every launch, so typing `mirv_movie_separate_hud 1` into Initial Commands still produces a usable HUD alpha stream.

### 3.2 Planning

For each demo and recording player, highlights are sorted and grouped into **blocks**. Two highlights share one recording when their windows overlap once the start lead, stop trail and a **take separation** are added. The separation is 0.5 s for HLAE and 2.0 s for OBS, both measured: HLAE takes about 0.28 s to finalise a take and loses frames from a restart sooner than that (#9). Highlights that do not share a recording but whose speed-change windows collide stay separate takes but skip the fast-forward between them.

Each block gets a disk estimate, and blocks are packed onto the Destinations with first-fit-decreasing. A drive must keep 15 GiB free.

### 3.3 Seek Between Clips, and patching

By default (**Seek Between Clips**, on the Pipeline tab) the batch plays your original demos: the hook DLL loads each one with `viewdemo`, runs the batch's commands on the demo player's clock and seeks to each clip instead of fast-forwarding to it (`%APPDATA%\dod-studio\dodstudio_batch.txt` holds the plan; `ds_capture_batch` in `dodstudio_commands.md`). Nothing is patched. Two things happen first when they are needed, each with a status line and an activity-log line:

- A highlight from a project saved before the demo player's clip times existed is read again from its demo (from the analyzer cache when it has it) to get them.
- A demo on the game's drive is played where it is, by a path relative to `dod\`. A demo on another drive is played where it is too, by its full path (`viewdemo "D:/folder/demo"`, `ds_demo_other_drives`, #776, #779), and the batch turns that setting on before it starts; the activity log names each such demo. Only with a hook DLL from before #779, which can't do that, is such a demo copied as it is into `<game>\dod\` as `dodstudio_src_NN_<stem>.dem`, after a free-space check (the copies plus 1 GB), played from there and removed when the batch ends, is cancelled or fails. A path too long for `viewdemo` (over 240 characters), with a `"` or `;` in it, or with any character outside ASCII (`viewdemo` lower-cases the name byte by byte, which breaks `ü` and the like, on the game's drive too) is copied the same way, under a plain name.

A batch that cannot run this way (no hook DLL, or a demo whose kills still have no clip times) is patched as below, and the activity log says why. Turning the setting off patches every batch.

**In the game that is already open (#725).** With Day of Defeat open, a Seek Between Clips batch runs in that game instead of asking to relaunch it, when it is one Studio started: the same install, game folder, resolution and hook DLL build (#666, #1143, #1146), a hook DLL in it at all (read off the game's loaded DLLs, `native::sys::modules`), HLAE in it (`AfxHookGoldSrc.dll`, from the same list), and its command pipe answers. Not in OBS mode, which starts the game with its sound through waveOut. Before the batch the game gets `ds_demo_review stop`, `ds_capture_batch stop`, `disconnect` (a review, an earlier batch or a demo stops) and `exec dodstudio_helper.cfg` (the batch's aliases, which a launch gets on its command line); then the usual early lines and the batch. At the end, and on Cancel, the game is left open at its menu (`ds_capture_batch stop`, `sys_record_stop`, `mirv_recordmovie_stop`, `sys_normal_speed`, `disconnect`: a Cancel mid-clip or mid-fast-forward leaves no AGR recording, `host_framerate` or `r_norefresh` behind) instead of closed; a game that fails (an error box) is still closed. What a fresh launch has and a reused game may not: console settings the user changed in it stay (the batch's Initial Commands still run per demo), and `ds_capture_run_in_background` and `ds_capture_stay_behind` stay as the batch set them. Any other open game still gets the Force Relaunch question, and the activity log says why it wasn't used (`[launch] The open game can't take the batch`). A batch that turns out to need patching after all stops with a message to close the game (`patch::batch_reuse`).

### 3.4 Patching

Nothing in your original demo changes. The app writes:

- `dod/dodstudio_primer.dem`, only when the batch launches without the hook DLL: a copy of the first demo that plays for 500 frames and then chains to the first real job. It exists because the first demo of a game session renders too dark. With the hook DLL there is no primer (#365): its `lightmap_gamma` fixes the lighting on PRE (#397; the 25th Anniversary build never had the bug), and the first demo is sent over its pipe after `config.cfg`/`movie.cfg` have run (#705).
- `dod/dodstudio_chain_NN.dem`: one patched copy per demo and player. Each carries:
  - your Initial Commands plus the app's own (`mirv_movie_fps`, OBS frame pacing, and `r_decals` when the decal flush needs it and you did not set it);
  - per block: return to normal speed, sync audio, point `mirv_movie_filename` at the block's folder, start recording, stop recording, fast-forward again;
  - your Scheduled Commands, anchored to the first or last kill;
  - a director text label per highlight, `echo` markers the app reads back from `qconsole.log`, and a breadcrumb every 5000 frames;
  - at the end, `playdemo` of the next chain file, or the exit trigger on the last one.
- `dod/dodstudio_helper.cfg`: the aliases those commands call.

Injected commands are standalone `ConsoleCommand` frames, written after `DemoStart` and never inside an existing network message. Each is limited to 63 bytes by the demo file format.

**Decal flush.** With Flush Decals on, a pre-pass rewrites a scratch copy of each demo so wall decals from outside the recorded windows are removed, and bursts of harmless decals clear the ring before each block. A failure here never fails the batch; the unmodified demo is used and the reason is logged.

### 3.5 Launching and running

Every launch (batch, preview, Launch Game) uses one command line:

    hlae.exe -customLoader -noGui -autoStart
      -hookDllPath <HLAE>\AfxHookGoldSrc.dll
      [-hookDllPath dodstudio_goldsrc_hooks.dll]
      -programPath <hl.exe>
      -cmdLine "-game <folder> -insecure -addons -demoedit -windowed -w W -h H
                -afxForceAlpha8 1 -condebug <extra>"

A batch adds `+exec dodstudio_helper.cfg`. With the hook DLL, the first demo follows over its pipe once the game is up (5 s after the pipe answers): `ds_capture_batch` with the batch plan under Seek Between Clips, else `playdemo dodstudio_chain_01`; without it, `+playdemo dodstudio_primer` goes on the launch line (#365). The hook DLL is added only if the file exists (section 5.1). `<folder>` is the **Game Folder** setting (Configuration → Paths), `dod` unless changed (#402). `-addons` (PR #412) is on every launch, right after `-game`, and so is `-demoedit` (PR #401). `-afxForceAlpha8 1` is the only HLAE alpha flag: a bisect showed `-gl`, `-32bpp` and `-afxRenderMode standard` change nothing in a separate-HUD capture (#69).

While the game runs, the app reads its markers and turns them into status lines and notifications. They come from the game's events pipe (section 5.1) once it connects, and from `qconsole.log` until then, or throughout for a game without the hook DLL. The batch ends when:

- `BATCH_COMPLETE` arrives over the events pipe (any mode), OBS mode sees it in the log, or the exit trigger folder appears (HLAE creates it on the last `mirv_movie_filename` call; the fallback);
- you cancel (the app kills `hl.exe`);
- the game closes on its own ("closed manually or crashed");
- in OBS mode, markers stop arriving for too long, or OBS disconnects.

On failure or cancel the copied demos, junctions and scratch files are removed. Takes are never deleted.

**When Studio closes or crashes mid-batch** (#545, #20), the game keeps capturing and quits itself at the end when no Studio reads its events pipe. The batch's manifest (section 9) stays at `planned`, and that is the recovery record: it holds each highlight's id, its source demo and the project file. On the next start Studio checks each planned batch's takes on disk and marks the found ones Captured, in the project file or the open queue. While a game is open, the check waits for it to close, a toast says the batch may still be capturing, and Start Capture Batch refuses until then (a new batch would run in that open game and stop the old one). When some clips have no take (the game closed or crashed too), Studio asks **Capture the rest?**: **Capture them now** opens the batch's project, ticks just those highlights and runs Start Capture Batch with its usual checks.

### 3.6 Command tiers

Commands you type into Initial or Scheduled Commands are checked twice: once for the warning banner, and again when the batch starts. The lists live in `native/src/patch/cfg_scan.rs`; read it rather than trusting this summary. `docs/command_tiers.md` explains each tier.

Every command must also fit a demo's 64-byte command field, which holds 63 characters. A longer one is refused the same way as a banned command, with a red row under its field.

| Tier | Commands | What happens |
|---|---|---|
| Banned everywhere | `mirv_recordmovie_start`, `mirv_recordmovie_stop`, `mirv_movie_ffmpeg`, `host_framerate`, `r_norefresh`, `r_drawentities`, `cl_lw` | Refused; Start is disabled |
| Banned when scheduled | `r_decals`, `mirv_fov`, `gl_widescreenfov`, `mirv_movie_filename` | Fine in Initial Commands, refused in Scheduled |
| Mid-demo hazards | the pipeline-owned commands, as a warning; `fps_override`, `fps_max`, `gl_vsync` when scheduled (the fast-forward sets them and every clip's pre-roll puts the Initial Commands value back) | Shadowed with a warning, not refused |
| No effect | what the game's demo filter drops (`exec`, `alias`, `quit`, ...), in Initial and Scheduled alike, only while Seek Between Clips is off (with it on, the hook DLL runs them); `mirv_movie_filename`, `ds_capture_run_in_background` and `ds_capture_stay_behind` in Initial | Reported as doing nothing |
| Fatal config values | `cl_lw` not 1; `r_drawentities` not 1 while `sv_cheats` is on | Reported from your game configs; DoD's client quits over these |

Your game's own `.cfg` files are read, never written.

---

## 4. Render Studio

**Scan for Takes** walks the Destinations and stages every renderable take as a Queued row. A take is renderable when it has at least one stream folder and audio, either a `.wav` beside it or an audio track inside its video. A muted OBS recording is therefore not renderable.

Scan works at any time, renders included (#772): it merges into the list. Queued and Rendering rows stay exactly as they are, finished, failed and cancelled rows are re-read from their take, new takes are added, and rows whose take folder is gone are dropped unless rendering. A new Queued row joins a batch that is already rendering. The toast says what changed ("3 new takes, 2 refreshed, 1 gone").

Frame-sequence and video takes with HUD streams become two rows: the full picture, and a HUD-only row rendered as ProRes 4444 with alpha.

| Codec | FFmpeg video | Container | Audio |
|---|---|---|---|
| ProRes 422 (default) | `prores_ks -profile:v standard`, 10-bit 4:2:2 | `.mov` | PCM |
| ProRes 422 HQ | `prores_ks -profile:v hq`, 10-bit 4:2:2 | `.mov` | PCM |
| ProRes 422 LT | `prores_ks -profile:v lt`, 10-bit 4:2:2 | `.mov` | PCM |
| ProRes 4444 | `prores_ks -profile:v 4444`, 10-bit 4:4:4, no alpha | `.mov` | PCM |
| DNxHR HQ | `dnxhd -profile:v dnxhr_hq` | `.mov` | PCM |
| HuffYUV | `huffyuv` | `.avi` | PCM |
| Uncompressed | `rawvideo` | `.avi` | PCM |
| H.264 (software) | `libx264 -crf 16` | `.mp4` | AAC 192k |
| H.264 (NVENC) | `h264_nvenc -cq 15` | `.mp4` | AAC 192k |
| Custom | your arguments, unvalidated | `.mkv` | PCM |
| Skip (OBS takes only) | no FFmpeg; the file is copied | source | — |

Which ProRes to pick: on a 300 fps 2560x1440 capture, 422 came out at 67% of HQ's size and LT at 47%, with VMAF against the lossless capture of 99.3 (HQ), 98.6 (422) and 97.2 (LT), and no difference visible at 3x zoom. 422 suits most videos and is the default for a new install; LT gives up a little detail in fast motion and fine textures for half the size. 4444 keeps full colour detail at about 1.5x HQ (measured 1.47x), only worth it for heavy colour grading or small coloured text drawn into the picture; it has no transparency, since the HUD stream is always its own 4444 file with alpha whichever ProRes is picked. The Codec picker shows a one-line hint for each. Saved settings that say `prores` (HQ, the default before) stay HQ, and any other saved codec stays too; only a settings file with no codec in it gets 422. Before the first render of a ProRes profile finishes, its space reservation borrows another profile's measured size through these ratios, and Combine Clips re-encodes rather than copying clips of different ProRes profiles into one file.

Each row shows its own codec and the FPS its frames are read at, "as captured" (the take's `dodstudio_take.json`) or "Render Settings" (a take without one), then status, speed, progress and size, with Cancel, Reset, Remove, View Log, Open Folder and Delete Take actions. A header click sorts the table, and a whole-batch progress bar sits above it. A scan stages a take whose stream was rendered before (a finished render in its history, #438, with the file still there) as Finished rather than Queued, so Start Render Batch (N) renders every Queued row and nothing twice; Retry queues a finished one again, and Cancel or Remove keeps a row out. Every row has a tick box (#70), ticked to begin with; the header tick box or Select unrendered sets them together, and the ticks survive a re-scan that adds takes. The other footer buttons act on the ticked rows and say on how many: Cancel (N), Retry (N), Remove (N) and Delete Take folders (N), plus Clear Queue, which empties the whole list and asks first when anything is rendering. **Export Marker List…** (next to Scan for Takes) saves a CSV with one row per captured or rendered highlight: its demo, player, kills, place in the demo, take and label, for lining clips up in an editor.

- **Scheduling:** up to the max-concurrent limit; each job reserves space on the first export drive with room (with none, or every one full, the job is refused rather than written anywhere else), and the reservation learns each codec's real size per frame once there is a render to learn from.
- **Crash recovery:** a lockfile (`.render_autosave.json`) is written at queue time and after each finished job. On the next start, a dialog offers to recover the batch (not when every clip in it had finished, #864). Each recovered row keeps its scanned take and its own codec, custom arguments and FPS (#85), and Recover starts the batch again (#818); only a lockfile written by an older version recovers stubs that need a rescan.
- **Combine Clips…** (next to Scan for Takes, #588) joins finished renders, or files you add, into one video.
- **FFmpeg:** the override path, else `local/tools/ffmpeg.exe` beside the app, else `ffmpeg` on PATH.
- **Take pictures (#1069):** each row's Picture column shows the middle frame of its take, a bigger copy on hover, and a click opens nine frames spread across the take with their times, so a black, frozen or wrong take shows before it renders. The pictures are made in the background, one take at a time, with a "Making take pictures: 3 of 12" bar under the table, and cached in the take's own `dodstudio_preview\` folder, so each is made once and goes to the Recycle Bin with the take. A take re-captured into the same folder gets new pictures. Delete Take pauses the pass while it runs.
- **Take checks (#1084):** under each picture, a chip says whether the take is good: `OK`, or `⚠ 2` with each warning in its tooltip and listed on the contact sheet, with its time in the clip and whether capturing again helps or the demo itself is the reason. It finds a black picture (0.5 s or more), a frozen picture (2 s or more without a pixel changing: the demo may have no camera data there), no sound, silent sound (peak under -50 dB) and clipping sound (0.1% or more of it at full scale). Shorter holds (0.1-2 s), the demo itself lagging, are a note, not a warning. One FFmpeg pass per take reads every frame (about 25-35 s for a 19 s, 300 fps HLAE take read from disk), after the picture pass and never while a render runs, with a "Checking takes: 1 of 4" bar; the result is kept as `checks.json` in the same `dodstudio_preview\` folder. It also holds the take to its plan: a frame count more than a tenth (at least half a second) off the length the batch planned (on the clock Seek Between Clips records on), and any kill the highlight lists that falls outside the video (more than 0.1 s past either end), are capture warnings. The plan is kept in the take's `dodstudio_take.json`; a take captured before that, or from a demo whose kills have no demo-player times (a scan from before them), shows a note that its length and kills weren't checked, never a warning. A video that stops before its sound is #1105's partial take and is shown on the row already.
- **Delete Take (#644):** sends a take's folder to the Recycle Bin after a confirm naming the folder and its size, and takes its rows out of the list. It is off while the take (either half of a HUD pair) is rendering, with the tooltip saying which row is busy; deleting a queued take pulls its render from the queue, and the confirm says so. The backend only deletes a folder the scanner recognises as a take, inside a Destinations folder, and leaves `dodstudio_take.json` (the render history) in place. **After a clip is finished** (below) deletes takes automatically.
- **Finishing clips after a batch (#440):** with **When a batch finishes** set to *Finish clips automatically* (off by default), a capture batch that completes queues and starts its own verified takes here, through the same Scan/Start path, and the Capture footer shows "Finishing clips: 3 of 12". OBS clips default to Skip (a copy); Video and Frame sequence clips default to the Codec above, and a frame sequence is encoded at the batch's own capture FPS. A cancelled batch is not finished, nothing is finished without an export drive, and a busy Render tab (running, or staged but not started) is waited for rather than replaced.
- **Deleting takes automatically (#644):** **After a clip is finished** keeps the take (default), moves it to the Recycle Bin, or deletes it permanently once the render batch that finished it ends; turning permanent deletion on asks first. **Only when space is needed** (permanent only, since the Recycle Bin frees nothing until emptied) instead waits until a capture or render batch is about to start without room on its drives, then deletes the oldest finished takes there until it fits: a capture needs its estimated size with 15 GiB left on each Destinations drive, a render batch the reservations of its first jobs plus 1 GiB. If even every finished take isn't enough, none is deleted. Only a take whose every stream's last render finished, with the clip file still there, and that isn't queued or rendering, is ever eligible, and the delete runs Delete Take's own check. Each one goes to the activity log (`[take-cleanup]`) with its size, where it went and why, and the Render tab's take-delete bar shows the steps.

---

## 5. The companion DLL (goldsrc-hooks)

### 5.1 How it gets into the game

`goldsrc-hooks/` builds `dodstudio_goldsrc_hooks.dll` for 32-bit Windows. HLAE injects it alongside its own DLL on every launch the app makes. The app looks for it in this order: the path set in Configuration → Paths, the copy bundled with the installed app, then beside `hlae.exe`. If none exists the game still launches, and a warning is logged.

At load the DLL hooks two imports of `hw.dll` (`GetProcAddress`, `LoadLibraryA`). That lets it catch `client.dll` loading and swap four client functions for its own wrappers. From then on it runs a small amount of work every frame. `client.dll` does not reload between demos, so this happens once per game session.

It logs to `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks_YYYYMMDD.log`, and a crash handler records any crash as `module+offset` with a stack trail.

**Refuses to join a server, except an HLTV proxy.** While the DLL is loaded, `connect` and `listen` first ask the address what it is, and the game only joins an HLTV proxy that says VAC is off. Anything else is refused, with a console message and a hook-log line (issue #451): joining a VAC-secured server with it loaded is a ban risk. `connect local` (what `map` runs) still works. `GOLDSRC_HOOKS_ALLOW_CONNECT=1` turns it off. See `docs/vac_safety.md`.

**Commands from Studio.** The DLL serves a local named pipe, `\\.\pipe\dodstudio-hl-<pid>`, and runs each line Studio writes to it as a console command on the next frame. Only the same Windows user on the same machine can write to it. Launch Preview, Review highlights, the Players settings and a batch's first demo use it. `GOLDSRC_HOOKS_REMOTE=0` turns it off.

**Events to Studio.** A second pipe, `\\.\pipe\dodstudio-hl-<pid>-events`, carries the pipeline's `[dod-studio]` markers from the game as the engine runs each `echo` (the DLL wraps `echo` through the engine's command list, with no per-build address). Markers from before Studio connects are sent when it does. `GOLDSRC_HOOKS_EVENTS=0` turns it off; Studio then reads `qconsole.log` as before (issue #434, step 1).

**Batch end without Studio.** If a batch's `BATCH_COMPLETE` marker goes by and no Studio is reading the events pipe five seconds later (Studio was closed mid-batch), the game quits itself instead of sitting there (issue #545). A connected Studio still ends the game as before.

**In-game window.** `ds_ui_panel` opens DoD Studio's own window inside the game, with Playback, Demos, Highlights, Console, Settings, Commands, Studio and Review tabs. It reads the analyzer cache for the Highlights tab, and every tab shows how far the demo and its highlights have loaded (#804). The Demos tab browses every drive, and with the hook DLL `viewdemo "D:/folder/demo"` plays a demo from any drive with no link or copy (`ds_demo_other_drives`, #778, #780). The game launches with `-addons` so the hook's own main menu can load from `dod_addon`. See `docs/dodstudio_commands.md`.

**Menu size on the 25th Anniversary engine (#985).** That engine draws the menus, Options, the console and the in-game window bigger at higher resolutions (screen height / 720, so 2x at 1440p). Configuration › Paths › *Size the Game's Menus Like the Pre-Anniversary Engine* (off by default, shown only for an Anniversary install) writes a copy of the game's `TrackerScheme.res` (the game folder's own, else `valve`'s) with `ProportionalBaseWidthHD`/`HeightHD` raised to 7680x4320 into `<game>_addon\resource\`; the engine clamps that base to the screen, so the menus draw at 1x, as on the pre-Anniversary engine. It needs `-addons` (Studio's launches pass it). Unticking removes the copy; a `TrackerScheme.res` there that isn't Studio's is never touched, nor are the game's own files. The copy is refreshed from the game's scheme each time Studio reads the setting.

### 5.2 Console commands

The DLL is how the app and a person reach into a running game: kill-feed, HUD and spectator controls, HD textures, demo seeking, and Studio's own in-game window. Every name starts `ds_` and none is saved into `config.cfg`. The names, defaults and what each does are in [`docs/dodstudio_commands.md`](dodstudio_commands.md), which is what the code registers on `dev`. Which of them work on which game build is in section 10.

Four fixes have no console name and are on by default: the **temp-entity crash fix** (DoD's own NULL-sprite crash, `GOLDSRC_HOOKS_TEMPENT_FIX=0` turns it off), the **hull-trace guard** (the #384 crash after a `playdemo` map change, both builds, `GOLDSRC_HOOKS_HULL_TRACE_GUARD=0` turns it off), and the **sprite-blend upload fix** (`gl_spriteblend 0` at the first sprite load no longer darkens sprites for the session, #467, both builds, `GOLDSRC_HOOKS_SPRITEBLEND_FIX=0` turns it off), and the **first-demo pmove guard** (a session's first demo sending `InitHUD` in its first packets no longer crashes while DoD places the map's models, #546, both builds, `GOLDSRC_HOOKS_PMOVE_GUARD=0` turns it off).

### 5.3 Offline verification

`goldsrc-hooks/tools/verify_*.py` re-derive every patched address from the DLL on disk with `pefile` and `capstone`. `survey_client_dll.py` and `survey_hw_dll.py` regenerate the survey docs. `crash_report.py` groups crashes from the hook logs against known causes. They default to the PRE install's paths.

---

## 6. HD textures

**In the game:** the DLL replaces textures as the engine uploads them, from `dod_addon\dodstudio_hd\<type>\<style>\` (`dod\dodstudio_hd` for an install built before #415, while `dod_addon` has none), where type is `world`, `models`, `sprites` (HUD icons too, #733), `decals` (#729), `detail` or `sky`. It works on both builds (#399). An `overrides` folder per type wins over any style. Replacements go up to 4096 pixels a side (#423), and the engine's own `gl_max_size` (default 256) decides what each is shown at, so set it to 1024 or more to see the gain; the game and the HD page warn when it shrinks HD files (#680). Player sprays (and crosshairs) follow Configuration > Players (#428): each listed player's spray, in POV and HLTV demos alike, is a PNG from the game's `logos` folder, and everyone else gets the fallback for the demo type; `ds_player_spray <name>` makes one of those PNGs your own spray for live play. See [`dodstudio_commands.md`](dodstudio_commands.md).

**The page** (HD Textures tab):

- **What's built:** a table of files and size per style and type, with a Refresh.
- **Use it in the game:** the lines to put in `movie.cfg` for the chosen style, with Copy, how much of the game the style covers and which maps it has textures for, with a warning when a map being built has none in that style (#426).
- **Compare styles:** the original and each built style side by side at 1:1 pixels, from your own maps (#392).
- **Tools:** finds or downloads the Real-ESRGAN upscaler, its models, and a private Python 3.12 with numpy, Pillow and SciPy. You can also point it at your own copies.
- **Maps to build:** every map, or only the maps and name patterns you pick, saved to `hd_maps.txt` (#422).
- **Build:** tick styles and asset types and a largest size (1024, 2048 or 4096), then Build. It runs `goldsrc-hooks/tools/hd/build_all.py` with live step progress; files already built are skipped, so Cancel keeps finished files and a later Build carries on.
- **Your own styles:** a form for a style of your own (an AI model, a plain enlargement with your sharpening, or a mix of two styles), saved to `my_styles.txt` (#391).
- **Textures that kept their original:** after `ds_debug_hd_misses` in the game, reads the hook log and lists by map what no HD file replaced (#390).

**Built-in styles:** `ultrasharp` (default), `remacri`, `siax`, `generalv3`, `x4plus` (all Real-ESRGAN models), `plain` (Lanczos plus sharpening) and `blend` (x4plus and plain mixed). Your own styles go in `dodstudio_hd\my_styles.txt`, and `hd_maps.txt` limits which maps are built.

Command-line only: `setup_tools.py`, the per-type scripts, `--also` and `--extra-models`. The page's comparison runs `compare.py`, and the scripts write their files batch by batch (`HD_BATCH`, #404).

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
| Demo type | "HLTV" if any HLTV message or camera-moving director command appears, else "POV". DoD Studio's own bookmarks and injected commands don't count, so a patched preview stays POV (#351). |
| Kill positions | Where both players stood at each kill, and the distance between them, from a replay of the entity snapshots (#448). The recording player's own position comes from its client data. On a POV demo, enemies out of the recorder's view have no position. |

Analyses are cached as JSON in `%APPDATA%\dod-studio\analyzer_cache\v11\`, keyed by path and invalidated by size and modified time.

### 7.3 Highlights

A highlight is one life's kills by one player, at least one kill, for every connected player (`analysis::highlights`, shared by the Highlights page and the in-game Highlights tab, #573). A POV demo lists only the recording player's; an HLTV demo lists every player's, each kept in the slot the player had when the kills happened, so players who left before the end still show (#247, #744). Capturing an HLTV highlight follows that player in first person (#706). Each scanned demo saves the highlight-detection version (`HIGHLIGHTS_VERSION`), and opening a project offers to rescan demos scanned by an older one, keeping their marks, notes, status and kill ranges (#835).

### 7.4 Web analyzer

`web-analyzer/` compiles the same analysis to WebAssembly for a static page at `https://ccoventry.github.io/dod-studio/`. It deploys on every push to `main`. It shows the same report tabs from a hand-copied renderer (issue #238). It analyses several demos at once and exports JSON (#498).

---

## 8. Command-line tools

| Binary | What it does |
|---|---|
| `preview_cli` | Drag demos or folders onto it; writes `<stem>_preview.dem` bookmark files into a `previews` folder. `--player` picks one player in an HLTV demo. |
| `dod-studio-cli` | `analyze <demos>` prints a Markdown or JSON match report. `stats <demos>` prints league stats as JSON: teamkills, suicides, objective points, cap credits, every flag capture and cap blocks. `patch-streak` is an older standalone patcher. |
| `dod-studio-dump` | Header, frame and message counts, first commands and sounds of one demo. |
| `dod-studio-inspect` | Library statistics across folders: maps, message frequency, duplicates. |
| `check_maps` | Per-demo map status against a maps folder, with optional download. |
| `check_cfgs` | What your game configs set, and what Initial Commands would override. |
| `strip_decals` | Runs the decal flush on one demo with every knob exposed. |
| `hl-demo-auditor` | The duplicate finder as a CLI; writes a Markdown report. `--integrity` checks demos for deltas built on frames the file doesn't have (#739). |
| `dod-benchmark` | Times each stage of the analyzer load path. |

`native/examples/` and `analysis/examples/` hold R&D probes built only with `--examples`.

---

## 9. Where things are stored

| What | Where |
|---|---|
| Settings | `%APPDATA%\dod-studio\settings.json`. The OBS password is stored in plain text. |
| Activity log | `%APPDATA%\dod-studio\logs\activity_YYYYMMDD.md`, 30 days kept |
| Hook DLL log | `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks_YYYYMMDD.log`, 30 days kept |
| Analyzer cache | `%APPDATA%\dod-studio\analyzer_cache\v11\` (the number is the cache schema version; an entry an older version left is upgraded into the current folder when it can be, `analysis::cache::migrations`; older version folders that can't be upgraded from are deleted once a week old; entries within a version are never evicted) |
| Render lockfile | `%APPDATA%\dod-studio\.render_autosave.json` |
| Capture manifests | `%APPDATA%\dod-studio\manifests\<session_id>.json`. Written as `planned` when a batch starts and rewritten as `complete`, `cancelled` or `error` with each block's verdict, or `unwatched` once a batch that ran while Studio was closed has been caught up on (section 3.5). The newest 50 are kept. |
| Players table for the hook DLL | `%APPDATA%\dod-studio\goldsrc_hooks_players.txt`, rewritten from Configuration > Players |
| HD tools | `%APPDATA%\dod-studio\hd_tools\` |
| Blender | `%APPDATA%\dod-studio\blender\` (the chosen `blender.exe`); each take's work folder beside its `.agr` |
| Project files | wherever you save them |
| Patched demos, helper cfg | `<game>\dod\` (`dodstudio_chain_NN.dem`, `dodstudio_helper.cfg`, and `dodstudio_primer.dem` without the hook DLL) |
| Batch plan, copies of demos from another drive (hook DLL from before #779 only) | `%APPDATA%\dod-studio\dodstudio_batch.txt`; `<game>\dod\dodstudio_src_NN_<stem>.dem` for the batch only |
| Previews | `<game>\dod\<stem>_preview.dem` plus a hidden `.dodstudio_preview` marker |
| Takes | `<Destination>\<session>\dodstudio_chain_NN_bK\take0000\...` |
| HD files | `<game>\dod_addon\dodstudio_hd\` (or `<game>\dod\dodstudio_hd\` from before #415) |
| Menu size (#985) | `<install>\dod_addon\resource\TrackerScheme.res`, Anniversary installs only, while the setting is on |
| Overview edits | `%APPDATA%\dod-studio\overviews\<map>.json` |
| Saved overviews | `<install>\dod_addon\overviews\` or `<install>\dod\overviews\`; a user's own copied to `overviews\dodstudio_backup\` first |
| HLAE FFmpeg link | `<HLAE>\ffmpeg\ffmpeg.ini` |
| OBS | a `[DoD-Studio]` profile and scene inside your OBS |

`DOD_STUDIO_LOG_DIR` redirects both logs. An old `%APPDATA%\dod-tools` folder is migrated once, without overwriting.

---

## 10. Pre-Anniversary vs 25th Anniversary

The app itself has no build detection. The install used is whatever `hl.exe` you point Paths at.

In the DLL, each module finds its code by a byte pattern and refuses loudly if the pattern is missing or appears twice, so the wrong build fails safe. `client.dll` is byte-identical across the stock, PRE and Anniversary installs, so every module that patches only `client.dll` works on both. A module that touches `hw.dll` (or another engine DLL) carries a table per build, checked offline by its `tools/verify_*` script for both.

| On `dev` | Modules |
|---|---|
| Both builds | everything in `client.dll`: kill feed, crosshair, spectator crosshair, scoreboard, voice mute, HUD elements, hand signals, HLTV text, objectives, overview map, map sprites, message log, viewmodel animations, gunshots, temp-entity fix |
| Both builds | in the engine: decal clear and the `ex_interp` ceiling (#398), HD textures (#399), sprays in HLTV demos, high-quality overviews, demo seeking, faster demo loading, demos from other drives, the detail-texture fix (#718), the hull-trace guard (#393), the sprite-blend and first-demo pmove fixes, and the ESC fix that keeps GameUI windows open (#396; the PRE build never closed them) |
| PRE only | first-load lighting fix (#397; the Anniversary engine rebuilds its lightmaps on a gamma change itself, so it has no first-load bug to fix, #370) |
| Anniversary only | world shaders (`ds_map_allow_shaders`); the PRE engine has no shader path |

---

## 11. Known gaps and inconsistencies

Each of these is true on `dev` today. Items with an issue number are tracked; the rest are not yet.

**Behaviour**

- Two different HLTV tests remain: the analyzer's (an HLTV message or a camera-moving director command, 7.2) and the scanner's and Explorer's (the proxy's connect message in the first 4 KB, #566).
- The analyzer computes POV stats (shots, reloads, scoped kills), but nothing displays them (#192). Its objective data shows only on the Flags tab and in the `stats` CLI.
- Entries in the current analyzer cache version are never pruned (older version folders are, #510).

**Dead code**

- `DOD_BATCH_DONE` is removed before each batch but nothing creates or reads it.
- `dod_quit.cfg` is written but never executed.
- The WASM gates in `native/` guard nothing, since no WASM build uses `native`.
