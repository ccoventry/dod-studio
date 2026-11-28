// strings/capture_config.js: the app's text for the capture configuration and what it checks before a capture.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

import { DEFAULT_AGR_FPS } from '../agr_rate.js';

export const CAPTURE_CONFIG_STRINGS = {
  // ── Half-Life Preview Detector modal ─────────────────────────────────────
  PROCESS_DETECTOR_MODAL: {
    TITLE: 'Half-Life Preview Detector',
    BODY: 'The Half-Life engine is already running (hl.exe / hlae.exe). Launching a new preview now can corrupt the capture session — close the running instance first, or force a relaunch.',
    // A batch fails differently and later: GoldSrc refuses the second instance
    // outright, but not until every demo in the queue has been patched, so the
    // work is already done by the time the error box appears.
    TITLE_BATCH: 'Day of Defeat Is Already Running',
    BODY_BATCH: 'The Half-Life engine is already running (hl.exe / hlae.exe), and this batch can\'t run in it (the activity log says why). Day of Defeat allows only one instance. Close the running game first, or force a relaunch to close it now.',
    FORCE_RELAUNCH_BUTTON: 'Force Relaunch',
    COPY_VIEW_COMMAND_BUTTON: 'Copy View Command',
    CANCEL_BUTTON: 'Cancel',
  },
  // ── Export Configuration & Batch Capture Pipeline ────────────────────────
  CAPTURE_CONFIG: {
    PANEL_TITLE: 'Export Configuration & Batch Capture Pipeline',
    // Grouped by what a field does, not by what it historically sat next to.
    // Capture FPS moved out of Timing — it is the recording rate and has
    // nothing to do with when anything happens — and joined resolution, which
    // is the other half of "what the video is".
    TAB_PATH_ROUTING: 'Paths',
    TAB_OUTPUT_FORMAT: 'Output Format',
    TAB_TIMING_OPTIONS: 'Timing',
    TAB_PIPELINE: 'Pipeline',
    TAB_CAPTURE_OUTPUT: 'Destinations',
    TAB_CUSTOM_COMMANDS: 'Commands',
    TAB_RENDER_OUTPUT: 'Render Settings',
    TAB_PLAYERS: 'Players',
    TAB_NOTIFICATIONS: 'Notifications',
    HLAE_EXEC_LABEL: 'HLAE Executable:',
    HLAE_EXEC_PLACEHOLDER: 'Path to hlae.exe',
    HL_EXEC_LABEL: 'Half-Life Executable:',
    HL_EXEC_PLACEHOLDER: 'Path to hl.exe',
    // install_picker.js (#931): the Half-Life installs found on this PC.
    INSTALL_BUILD: { anniversary: '25th Anniversary', pre_anniversary: 'pre-Anniversary' },
    installOption: (name, build) => (build ? `${name} (${build})` : name),
    installHiddenOption: (text) => `${text} · hidden`,
    installOther: (path) => `Other: ${path}`,
    INSTALL_NONE: 'No install picked',
    INSTALL_NO_MATCH: 'No install matches',
    INSTALL_HIDE: 'Hide',
    INSTALL_HIDE_TITLE: 'Hide from this list. "Show hidden installs" at the end brings it back.',
    INSTALL_UNHIDE: 'Show',
    INSTALL_UNHIDE_TITLE: 'Put it back on the list',
    showHiddenInstalls: (count) => `Show hidden installs (${count})`,
    HIDE_HIDDEN_INSTALLS: 'Hide hidden installs again',
    INSTALL_PICKER_TITLE: 'The Half-Life installs found in your Steam libraries. Picking one sets the Half-Life Executable and the Game Folder; nothing else changes.',
    // Game Folder (#402): game_folder_ui.js.
    GAME_FOLDER_LABEL: 'Game Folder:',
    GAME_FOLDER_TIP:
        "The folder beside hl.exe that DoD Studio starts with -game, and where it finds your configs, maps and demos. Almost always dod; pick another if you keep several versions of DoD side by side (dod_movies, dod_1_2, ...).",
    GAME_FOLDER_EMPTY: 'Give the name of a folder beside hl.exe, such as dod.',
    GAME_FOLDER_DOTS: 'Give just the name of a folder beside hl.exe, without "..".',
    GAME_FOLDER_OPTION: "A folder name can't start with - or +: the game would read it as another launch option.",
    gameFolderBadChar: (c) => (/\s/.test(c)
      ? "A folder name can't contain spaces here: the game would split it on the launch line."
      : `Give just the name of a folder beside hl.exe: "${c}" can't be part of it.`),
    gameFolderNotFoundOption: (name) => `${name} (not found beside hl.exe)`,
    gameFolderMissing: (name) =>
        `There's no "${name}" folder beside hl.exe with a liblist.gam and a cl_dlls/client.dll, so the game can't start it. Pick one from the list.`,
    GAME_FOLDER_NO_LIST: 'Set the Half-Life Executable to a real hl.exe first: the game folders are listed from beside it.',
    // #985: menu_scale_setting.js. The box shows what's in the install.
    MENU_SCALE_LABEL: 'Size the Game\'s Menus Like the Pre-Anniversary Engine',
    MENU_SCALE_TITLE:
        "The 25th Anniversary engine draws the main menu, Options, the console and DoD Studio's in-game window bigger the higher your resolution (twice as big at 1440p). Tick to draw them at the pre-Anniversary size instead: more room, smaller text. DoD Studio puts its own copy of the menu scheme in <game folder>_addon\\resource; your game's own files aren't changed. It works when DoD Studio starts the game (it adds -addons). Starting the game from Steam keeps the big menus unless you add -addons to its launch options. Untick to remove the copy.",
    MENU_SCALE_PRE_NOTE: "This install runs the pre-Anniversary engine, which already draws the menus at this size.",
    MENU_SCALE_NO_INSTALL: 'Set the Half-Life Executable first.',
    menuScaleForeign: (path) => `${path} isn't DoD Studio's, so it's left alone. Move it away to use this setting.`,
    menuScaleFailed: (err) => `Couldn't change the menu size: ${err}`,
    // #925: a copy of dod lacks <folder>_<language>.txt, so VGUI2 text shows
    // raw keys. Studio only says what to do; it never writes the folder.
    languageFileMissing: (folder, needed, renameFrom) =>
      `${folder} has no resource\\${needed}, so in-game text shows as codes like #Dod_Spec_Allies_Score. `
      + `In ${folder}\\resource, rename ${renameFrom} to ${needed}.`,
    languageFileMissingNoSource: (folder, needed, language) =>
      `${folder} has no resource\\${needed}, so in-game text shows as codes like #Dod_Spec_Allies_Score. `
      + `Copy dod\\resource\\dod_${language}.txt into ${folder}\\resource and name it ${needed}.`,
    LANGUAGE_FILE_SHOW: 'Show me the file',
    LANGUAGE_FILE_OPEN_FOLDER: 'Open the folder',
    // HLAE spawns its own FFmpeg for `mirv_movie_ffmpeg` and does not consult
    // the app's FFmpeg setting, so this is reported separately from it.
    HLAE_FFMPEG_LABEL: 'HLAE FFmpeg:',
    HLAE_FFMPEG_LINK_BUTTON: 'Point HLAE at FFmpeg',
    HLAE_FFMPEG_UNKNOWN: 'Set the HLAE executable above to check.',
    // Shown under the field that caused it, so a typo is obvious while you are
    // still looking at the box you typed it into. Nothing is blocked — Start
    // Capture Batch has its own guard.
    CAPTURE_MODE_FRAMES: 'Frame sequence',
    CAPTURE_MODE_FRAMES_TITLE:
        'HLAE writes every frame as its own bitmap. What this pipeline has always done, and what Render Studio was built around.',
    CAPTURE_MODE_VIDEO: 'Video',
    CAPTURE_MODE_VIDEO_TITLE:
        'HLAE pipes frames straight to FFmpeg as one lossless video per take. Same picture, roughly half the disk, and far fewer files. Needs the HLAE FFmpeg row above to be set.',
    CAPTURE_MODE_LABEL: 'Capture Mode:',
    CAPTURE_MODE_TITLE:
        'How frames get onto disk. Frame sequence and Video are both HLAE, deterministic and capable of any frame rate. OBS records the screen in real time instead, which is faster to a finished file but captures whatever actually rendered.',
    CAPTURE_MODE_AGR: 'AGR for Blender (no video)',
    CAPTURE_MODE_AGR_TITLE:
        'HLAE records no video. Each clip is saved as one .agr file holding every player, weapon and the camera, for rebuilding the clip in Blender. A few MB per clip rather than gigabytes. The sound of each clip is saved beside its .agr as a .wav of the same name, so the two line up.',
    AGR_FPS_LABEL: 'AGR FPS:',
    AGR_FPS_PLACEHOLDER: `e.g. ${DEFAULT_AGR_FPS}`,
    AGR_FPS_TITLE:
        `How many positions per second each .agr file records. Blender can only show motion the file contains, so for slow motion use at least the output frame rate times the slow-down (300 for 5x at 60 fps). A 30 second clip is about 7 MB at 30, 27 MB at 120 and 67 MB at 300, and a high rate makes Blender renders far slower. Required in AGR mode: Studio won't start an AGR capture without it, and it never follows Capture FPS.`,
    CAPTURE_MODE_OBS: 'OBS (real time)',
    CAPTURE_MODE_OBS_TITLE:
        'OBS records the game window while DoD Studio tells it when each clip starts and stops. HLAE records nothing. Output is a finished, playable file with audio already in it — but capture runs at real time, so frames drop if the machine cannot keep up, and high capture rates are not possible.',
    // ── OBS connection ──────────────────────────────────────────────────────
    OBS_SECTION_TITLE: 'OBS Connection',
    OBS_EXE_PATH_LABEL: 'OBS Path:',
    OBS_EXE_PATH_PLACEHOLDER: 'Path to obs64.exe',
    OBS_LAUNCH_BUTTON: 'Launch OBS',
    OBS_LAUNCHING: 'Launching…',
    obsLaunchFailed: (err) => `Could not launch OBS: ${err}`,
    OBS_HOST_LABEL: 'Host:',
    OBS_PORT_LABEL: 'Port:',
    OBS_PASSWORD_LABEL: 'Password:',
    OBS_PASSWORD_PLACEHOLDER: 'From Tools → WebSocket Server Settings',
    OBS_PASSWORD_TITLE:
        'The obs-websocket password, if OBS has authentication enabled. Use the Copy button in OBS rather than retyping it.',
    OBS_TEST_BUTTON: 'Test Connection',
    OBS_TESTING: 'Connecting…',
    OBS_LAUNCHING_AND_CONNECTING: 'Launching OBS and waiting for it to be ready…',
    OBS_UNREACHABLE: 'Could not reach OBS.',
    obsConnectedSummary: (obsVersion, websocketVersion) =>
        `Connected — OBS ${obsVersion} (obs-websocket ${websocketVersion})`,
    // Read-only — DoD Studio always targets its own fixed profile/scene, there
    // is nothing here for the user to change.
    obsUsingSummary: (profile, scene) => `Using OBS profile "${profile}", scene "${scene}"`,
    obsCanvasSummary: (canvas, output, fps) => `Canvas ${canvas}, output ${output} @ ${Math.round(fps)} fps`,
    obsRecordingToSummary: (directory) => `Recording to ${directory}`,
    obsMissingRequests: (requests) => `This OBS is missing: ${requests.join(', ')} — capture cannot run.`,
    OBS_ALREADY_RECORDING: 'OBS is already recording — stop it before starting a batch.',
    OBS_ALREADY_STREAMING: 'OBS is streaming — DoD Studio will not drive its recorder.',
    obsTestFailed: (err) => `OBS test failed: ${err}`,
    OBS_CAPTURE_FPS_LABEL: 'OBS Capture FPS:',
    OBS_CAPTURE_FPS_TITLE:
        "OBS's own live recording rate (the game draws at twice it) — separate from Capture FPS above, which is non-real-time for the other two modes. Set above what your machine can sustain and OBS drops frames instead of falling behind, making the clip choppy or unusable. At 2560x1440 on an RTX 3070 Ti, 120 holds with every OBS Quality choice but \"OBS's own\" (the encoder about two-thirds busy); 240 does not. Test a short capture first and watch for dropped-frame warnings before committing to a value.",
    OBS_CAPTURE_FPS_WARNING:
        'Too high for your machine and OBS drops frames — test a short capture first.',
    OBS_QUALITY_LABEL: 'OBS Quality:',
    OBS_QUALITY_TITLE:
        "How OBS encodes the clips on an NVIDIA card (NVENC). Each choice is a fast single-pass setup that kept up with 120 fps at 2560x1440 on an RTX 3070 Ti; higher choices look closer to HLAE's lossless frames and make bigger files. Sizes at 1440p, 120 fps: Small ~1.7 GB a minute, Standard ~3 GB, High ~4.5 GB, Max ~5.3 GB. Max keeps full colour detail (sharper red and coloured text), but some editors can't open 4:4:4 H.264: use Studio's render step for those clips. \"OBS's own\" is OBS's Simple-mode preset, which used the whole encoder at 120 fps there and dropped frames in some runs. On an AMD or Intel card OBS's own settings are used whatever is picked.",
    OBS_QUALITY_SMALL: 'Small (~1.7 GB/min)',
    OBS_QUALITY_STANDARD: 'Standard (~3 GB/min)',
    OBS_QUALITY_HIGH: 'High (~4.5 GB/min)',
    OBS_QUALITY_MAX: 'Max, full colour (~5.3 GB/min)',
    OBS_QUALITY_OBS_SIMPLE: "OBS's own (may drop frames at 120 fps)",
    OBS_ENABLE_HINT:
        'OBS 28+: enable this under Tools → WebSocket Server Settings (the checkbox, not the Connect Info panel).',
    OBS_PROVISION_HINT:
        'DoD Studio manages its own OBS profile/scene ([DoD-Studio]) — your own setup is never touched.',
    // ── Orphaned recording left by a previous run ───────────────────────────
    OBS_ORPHAN_TITLE: 'OBS is still recording',
    obsOrphanPrompt: (directory) =>
        `OBS is still recording into a DoD Studio take folder:\n\n${directory}\n\nA previous session ended without stopping it — a crash, a force-quit or a power cut. It will keep recording until the drive fills.\n\nStop it and keep the clip?`,
    OBS_ORPHAN_STOP: 'Stop and keep',
    OBS_ORPHAN_LEAVE: 'Leave it',
    obsOrphanRecovered: (video) => `Stopped OBS and kept the recording: ${video}`,
    OBS_ORPHAN_GONE: 'OBS had already stopped recording.',
    obsOrphanFailed: (err) => `Could not stop the orphaned OBS recording: ${err}`,
    CAPTURE_CODEC_LABEL: 'Capture Codec:',
    // Says why the list is short, so the absence of H.264/HEVC reads as a
    // decision rather than an omission. The sizes are transcode measurements,
    // deliberately described as such — how each one behaves while competing
    // with the game for cores during a live capture is not measured.
    CAPTURE_CODEC_TITLE:
        'All lossless: the render pass always re-encodes, so a lossy capture would cost quality for nothing. Ut Video is the only one built for real-time and is the safe default. The others are smaller per frame but heavier to encode, and the capture slows down if the encoder cannot keep up.',
    CODEC_UTVIDEO: 'Ut Video (fastest, recommended)',
    CODEC_FFV1: 'FFV1 (smaller, slower)',
    CODEC_X264_LOSSLESS: 'x264 lossless (smallest, slowest)',
    CODEC_RAWVIDEO: 'Uncompressed (no CPU cost, huge)',
    // The tooltip above already hedges this, but a hover-only warning is easy
    // to miss — put it where it stays visible regardless of which option is
    // picked, since "unverified" doesn't change once you've stopped hovering.
    CAPTURE_CODEC_UNVERIFIED_HINT:
        'Only Ut Video has been proven in a real capture. The others are sized from a transcode with every core free — during a live capture they compete with the game, and that ranking is likely to change.',
    // Turning it on without HLAE having an FFmpeg produces a capture that runs
    // and records nothing, so it is worth saying before the batch rather than
    // after it.
    FFMPEG_CAPTURE_UNAVAILABLE:
        "Capture to video is on, but HLAE has no FFmpeg — the capture would run and produce no video. Sort the HLAE FFmpeg row above first.",
    AGR_SOUND_UNAVAILABLE:
        "AGR mode records each clip's sound through HLAE's FFmpeg, and HLAE has none — the .agr files would have no sound beside them. Sort the HLAE FFmpeg row above first.",
    PATH_NOT_FOUND: "There's no file at this path — check it for a typo.",
    PATH_IS_A_FOLDER: "That's a folder, not the program itself. Pick the .exe inside it.",
    wrongExe: (found, expected) => `That's ${found}, not ${expected}: DoD Studio can't start the game with it. Pick ${expected}.`,
    // #373. Shown under Half-Life Executable only; see usesPlayFolder.
    HL_PATH_PLAY_INSTALL:
        "This is Steam's own Half-Life with its dod folder, the one you play online with. That works. A copy of the dod folder for movies (dod_movies, say, picked as the Game Folder below) is highly recommended: it keeps your movie configs, models and sounds out of the game you play online. Either way, don't join a game server from a game DoD Studio started.",
    HLAE_FFMPEG_BUNDLED: (path) => `Installed in HLAE's own folder (${path}).`,
    HLAE_FFMPEG_LINKED: (target) => `Pointed at ${target}.`,
    // Both halves of the pipeline encoding with the same FFmpeg build was the
    // whole reason for writing an ini instead of copying the binary, so a
    // divergence is worth stating rather than leaving to be discovered.
    HLAE_FFMPEG_DIVERGED: (target, app) =>
        `Pointed at ${target}, but Render Studio uses ${app}. Capture and render would use different FFmpeg builds — re-point HLAE unless that's deliberate.`,
    // ffplay.exe and ffprobe.exe live beside ffmpeg.exe and are one misclick
    // apart in a file picker, so this is worth naming rather than letting it
    // through to a capture that records nothing.
    // Checks the file the capture pipeline actually passes as -hookDllPath,
    // rather than what the exe calls itself. Advisory: nothing is blocked, since
    // an unusual install layout should not stop someone who knows it works.
    HLAE_FFMPEG_NO_HOOK_DLL: (dll) =>
        `AfxHookGoldSrc.dll isn't beside the HLAE Executable above (expected ${dll}). Capture needs that file — either the path isn't HLAE, or the DLL is missing or quarantined by antivirus.`,
    HLAE_FFMPEG_BAD_OVERRIDE: (why) =>
        `The FFmpeg Override Path above isn't FFmpeg: ${why}. Pick ffmpeg.exe — HLAE can't record with anything else.`,
    HLAE_FFMPEG_STALE: (target) =>
        `HLAE's ffmpeg.ini points at ${target}, which isn't there. Direct-to-video capture will produce no video until that path is fixed or the ini is deleted.`,
    HLAE_FFMPEG_MISSING:
        "HLAE has no FFmpeg of its own, so direct-to-video capture would run and produce no video. This is separate from Render Studio's FFmpeg.",
    HLAE_FFMPEG_LINKED_OK: (ini) => `Wrote ${ini}. HLAE can now find FFmpeg.`,
    HLAE_FFMPEG_LINK_FAILED: (err) => `Could not point HLAE at FFmpeg: ${err}`,
    // HLAE can live anywhere — it ships as a zip as well as an installer — so a
    // protected location like Program Files is one real possibility among
    // several, and needs a route through rather than a raw OS error.
    HLAE_FFMPEG_ELEVATE_TITLE: 'Administrator rights needed',
    HLAE_FFMPEG_ELEVATE_PROMPT: (ini) =>
        `${ini} is inside a protected folder, so Windows won't let DoD Studio write there directly.\n\nContinue and Windows will ask for permission, then write a two-line file pointing HLAE at your FFmpeg. Nothing else is changed, and an existing ffmpeg.ini is never replaced.`,
    HLAE_FFMPEG_ELEVATE_CONFIRM: 'Ask Windows for permission',
    HLAE_FFMPEG_ELEVATE_REFUSED: 'Permission was declined, so nothing was written.',
    FFMPEG_OVERRIDE_LABEL: 'FFmpeg Override Path:',
    FFMPEG_OVERRIDE_PLACEHOLDER: 'Optional path to ffmpeg.exe',
    GOLDSRC_HOOKS_LABEL: 'GoldSrc Hooks DLL:',
    GOLDSRC_HOOKS_PLACEHOLDER: 'Optional path to dodstudio_goldsrc_hooks.dll (blank = bundled default)',
    // hook_dll_picker.js: the test builds in the checkout's local\ folder.
    HOOK_DLL_BUNDLED: 'Bundled (none in a local dev build)',
    HOOK_DLL_BUNDLED_TITLE: 'The DLL an installed DoD Studio ships with. A local dev build has none, so the game starts without the hook.',
    HOOK_DLL_CHECKOUT: "This checkout's build (target\\…\\release)",
    hookDllOther: (name) => `Other: ${name}`,
    hookDllFolderHint: (folder) => `Test builds: put dodstudio_goldsrc_hooks_<name>.dll in ${folder}, or dodstudio_goldsrc_hooks.dll in its own folder under local\\. Then Refresh.`,
    HOOK_DLL_NO_MATCH: 'No test DLL matches',
    HOOK_DLL_REFRESH: 'Refresh',
    HOOK_DLL_REFRESH_TITLE: 'List the test DLLs in local\\ again',
    BROWSE_BUTTON: 'Browse',
    WIDTH_LABEL: 'Width:',
    HEIGHT_LABEL: 'Height:',
    DECAL_FLUSH_LABEL: 'Flush Decals Between Clips',
    DECAL_FLUSH_TITLE:
      'Clear bullet holes and blood off the walls between one clip and the next, so a later capture does not inherit the damage from an earlier one. Off captures the walls exactly as the engine leaves them. How many decals the engine keeps is a separate thing — set r_decals in Initial Commands.',
    SAVE_LOCAL_PATCHED_LABEL: 'Save Local Patched Copy',
    HOOK_RUNS_BATCH_LABEL: 'Seek Between Clips',
    HOOK_RUNS_BATCH_TITLE:
      'Play the original demos and jump straight to each clip, instead of patching a copy of every demo and fast-forwarding through it. Demos on other drives play where they are, with nothing copied (an older hook DLL copies them into the game folder for the batch and removes them afterwards). Needs the DoD Studio hook DLL; a batch that cannot run this way is patched as before. Turn off to patch every batch.',
    KEEP_GAME_BEHIND_LABEL: 'Keep the Game Behind Your Windows While Capturing',
    KEEP_GAME_BEHIND_TITLE:
      'The game captures behind your other windows and leaves your mouse alone. Off: the game comes to the front while it records each clip, so you can watch. Full speed either way. Only a capture batch does this: reviewing highlights and Launch Preview leave the window alone. Needs the DoD Studio hook DLL.',
    PRE_ROLL_LABEL: 'Pre-roll (s):',
    PRE_ROLL_HINT: 'Time between the game reaching the clip and capture starting.',
    POST_ROLL_LABEL: 'Post-roll (s):',
    POST_ROLL_HINT: 'Time between capture stopping and the game moving on to the next clip.',
    START_LEAD_LABEL: 'Start Lead (s):',
    START_LEAD_HINT: 'Time between capture starting and the first kill.',
    STOP_TRAIL_LABEL: 'Stop Trail (s):',
    STOP_TRAIL_HINT: 'Time between the last kill and capture stopping.',
    // Gaps between kills inside a clip (#831): kill_gaps_setting.js.
    KILL_GAPS_LABEL: 'Gaps between kills over',
    KILL_GAPS_UNIT: 's:',
    KILL_GAPS_KEEP: 'Keep',
    KILL_GAPS_FAST_FORWARD: 'Fast-forward',
    KILL_GAPS_SPEED_UNIT: 'x',
    KILL_GAPS_HINT:
      "Fast-forward speeds up a long wait between two kills inside the clip, from 2 s after one to 4 s before the next. It's part of the clip (no undo in the edit), and its sound is choppy. Needs Seek Between Clips.",
    INITIAL_DELAY_LABEL: 'Initial Delay (s):',
    INITIAL_DELAY_HINT: 'Time between the demo loading and the game moving to the first clip.',
    // Timing diagram (#150) — a visual timeline under the fields above,
    // showing how they relate to each other and to the recording window.
    timingDiagramInitialDelayNote: (v) => `This timeline starts after Initial Delay (${v} s) has already passed — a once-per-demo wait, not part of the per-clip cycle below.`,
    TIMING_DIAGRAM_COL_TIME: 'Relative Time',
    TIMING_DIAGRAM_COL_EVENT: 'Event',
    // Signed offset from the first kill (0.0s) — matches the pre-Tauri egui
    // build's "+{n} sec" / "{n} sec" formatting.
    timingDiagramTime: (v) => (v >= 0 ? `+${v.toFixed(1)} s` : `${v.toFixed(1)} s`),
    timingDiagramPreRollEvent: (v) => `Normal speed, Pre-roll starts — Pre-roll is ${v} s`,
    timingDiagramRecordStartEvent: (v) => `Recording starts — Start Lead is ${v} s`,
    TIMING_DIAGRAM_FIRST_KILL_EVENT: 'First kill (anchor)',
    TIMING_DIAGRAM_LAST_KILL_EVENT: 'Last kill (anchor — illustrative gap, not a real streak length)',
    timingDiagramRecordStopEvent: (v) => `Recording stops — Stop Trail is ${v} s`,
    timingDiagramPostRollEvent: (v) => `Post-roll ends, on to the next clip — Post-roll is ${v} s`,
    CAPTURE_FPS_LABEL: 'Capture FPS:',
    // Worth stating outright. This used to sit under Timing Options, and the
    // adjacency invited exactly the confusion that cost real time: the demo's
    // own tickrate is a different number entirely, and conflating the two is
    // how a "3 second" margin turned out to be 0.6.
    CAPTURE_FPS_TITLE:
      'Frames per second written to the recorded video. Nothing to do with the demo\'s own tickrate, which is a property of how the demo was recorded and is not adjustable here. Not used in OBS mode — see OBS Capture FPS in the OBS Connection section below.',
    OUTPUT_DIR_PLACEHOLDER: 'Capture output directory path...',
    // The chip at the end of each Capture Output row (#25).
    OUTPUT_ROW_CHECKING: 'Checking…',
    OUTPUT_ROW_OK: 'Found',
    OUTPUT_ROW_WILL_BE_CREATED: 'Will be created',
    OUTPUT_ROW_WILL_BE_CREATED_TITLE: 'This folder does not exist yet, but its drive does, so it is created when a capture first writes to it. Check the spelling if you meant an existing folder.',
    OUTPUT_ROW_DRIVE_MISSING: 'Drive not found',
    OUTPUT_ROW_DRIVE_MISSING_TITLE: 'This folder is on a drive that is not connected, so captures cannot be written here.',
    OUTPUT_ROW_NOT_FULL_PATH: 'Needs a full path',
    OUTPUT_ROW_NOT_FULL_PATH_TITLE: 'Start the path with a drive letter, for example D:/captures.',
    OUTPUT_ROW_MALFORMED: 'Not a valid path',
    OUTPUT_ROW_MALFORMED_TITLE: 'This path has characters Windows does not allow in a folder name.',
    OUTPUT_ROW_NOT_A_FOLDER: 'That is a file',
    OUTPUT_ROW_NOT_A_FOLDER_TITLE: 'This path points at a file, not a folder.',
    // The line under a row that cannot be used (#25): what is wrong with that path.
    OUTPUT_ROW_DRIVE_MISSING_DETAIL: "This drive isn't connected, so nothing can be captured here. Connect it, or remove this folder.",
    OUTPUT_ROW_NOT_FULL_PATH_DETAIL: 'Start with a drive letter, for example D:/captures.',
    OUTPUT_ROW_MALFORMED_DETAIL: "This has characters Windows doesn't allow in a folder name.",
    OUTPUT_ROW_NOT_A_FOLDER_DETAIL: 'This is a file, not a folder. Pick the folder to write captures into.',
    OUTPUT_ROW_OK_TITLE: 'This folder exists.',
    OUTPUT_ROW_CHECKING_TITLE: 'Checking this folder.',
    ADD_DIRECTORY_BUTTON: 'Add Directory',
    DESTINATIONS_HELP_TEXT: 'Captures are written here, and Render Studio scans these same locations for takes to render.',
    AUTO_CLEAR_LOGS_LABEL: 'Auto-clear Logs',
    AUTO_CLEAR_PREVIEWS_LABEL: 'Auto-clear Previews',
    AUTO_CLEAR_TEMP_DEMOS_LABEL: 'Auto-clear Temp Demos',
    // .dem files in Explorer (#528, file_types_settings.js).
    DEM_OPEN_WITH_LABEL: 'Show DoD Studio in Open With for .dem Files',
    DEM_OPEN_WITH_TITLE: "Adds DoD Studio to the Open With menu when you right-click a demo in Explorer. Whatever opens a demo when you double-click it stays as it is. Untick to take DoD Studio back out.",
    DEM_OPEN_TARGET_LABEL: 'Open .dem Files In:',
    DEM_OPEN_TARGET_TITLE: 'Where a demo you open with DoD Studio from Explorer goes.',
    DEM_OPEN_TARGET_ANALYZER: 'Demo Analyzer',
    DEM_OPEN_TARGET_QUEUE: "Studio's Queue",
    demOpenWithOtherCopy: (exe) =>
      `Open With opens another copy of DoD Studio: ${exe}. Untick and tick again to make it open this one.`,
    demOpenWithFailed: (err) => `Couldn't change Open With for .dem files: ${err}`,
    SCAN_WORKERS_LABEL: 'Demo Scan Workers:',
    SCAN_WORKERS_TITLE: 'How many demos a scan reads at once. More is faster up to about 4, but each one holds the analysis of a whole demo in memory.',
    scanWorkersHint: (totalGb) => totalGb
      ? `≈1.2 GB per worker; this PC has ${totalGb} GB`
      : '≈1.2 GB per worker',
    CLEAR_PREVIEWS_BUTTON: 'Clear Previews...',
    NOTIFY_PATCHING_LABEL: 'Patching Started/Complete',
    NOTIFY_PATCHING_TITLE: 'One notification when patching begins, one when your demos are ready and capture is about to start. Not per-demo — decal clearing makes patching take real time now, but a toast per demo patched would be noise.',
    NOTIFY_DEMO_LOADING_LABEL: 'Demo Loading',
    NOTIFY_DEMO_LOADING_TITLE: 'Fires each time a new demo starts playing during capture, showing which demo and how many clips are on it. Automatically skipped when Fast-Forward to Clip is also on, since that notification covers the same ground with more detail.',
    NOTIFY_BETWEEN_CLIPS_LABEL: 'Fast-Forward to Clip',
    NOTIFY_BETWEEN_CLIPS_TITLE: 'Fires as playback starts fast-forwarding toward each clip, including the first one in a demo.',
    NOTIFY_CAPTURES_DONE_LABEL: 'Captures Done',
    NOTIFY_CAPTURES_DONE_TITLE: 'Fires once when the whole capture batch finishes.',
    NOTIFY_RENDERS_DONE_LABEL: 'Renders Done',
    NOTIFY_RENDERS_DONE_TITLE: 'Fires once when the whole render batch finishes.',
    NOTIFY_ERROR_LABEL: 'Errors',
    NOTIFY_ERROR_TITLE: 'Fires immediately if a patch, capture, or render step fails.',
    // #442: named sets of both command lists. Row laid out like Render
    // Output's preset row (#108).
    PROFILE_LABEL: 'Profile:',
    PROFILE_TITLE: 'A saved set of Initial and Scheduled Commands. Pick one to replace both lists below with it. Shows — when the lists match no profile, and "(edited)" once you change them after picking one. Built-in profiles can be picked or duplicated, not changed.',
    PROFILE_NONE: '—',
    profileEditedOption: (name) => `${name} (edited)`,
    PROFILE_NAME_PLACEHOLDER: 'Name for these commands',
    PROFILE_SAVE_CHANGES_BUTTON: 'Save Changes',
    PROFILE_SAVE_CHANGES_TITLE: 'Save your changes to both lists back into the picked profile.',
    PROFILE_SAVE_BUTTON: 'Save Profile',
    PROFILE_SAVE_TITLE: 'Save both lists below under the name typed here, or over the picked profile when no name is typed.',
    PROFILE_RENAME_BUTTON: 'Rename',
    PROFILE_RENAME_TITLE: 'Rename the picked profile to the name typed here.',
    PROFILE_DELETE_BUTTON: 'Delete',
    PROFILE_DELETE_TITLE: 'Delete the picked profile. The lists below stay as they are.',
    PROFILE_UNDO: 'Undo',
    profileAppliedToast: (name) => `Replaced both command lists with the "${name}" profile.`,
    profileDeletedToast: (name) => `Deleted the "${name}" profile.`,
    profileNameTakenToast: (name) => `There is already a profile named "${name}".`,
    // Built-in profiles (#911): shipped with the app, kept up to date, locked.
    PROFILE_BUILTIN_CHIP: 'Built-in',
    PROFILE_BUILTIN_TITLE: "Comes with DoD Studio and stays up to date with it, so it can't be changed. Duplicate it to make your own copy.",
    PROFILE_BUILTIN_LOCKED_TITLE: "Built-in profiles can't be changed. Duplicate it to make your own copy.",
    profileBuiltinNameTitle: (name) => `"${name}" is a built-in profile. Type another name.`,
    PROFILE_DUPLICATE_BUTTON: 'Duplicate',
    PROFILE_DUPLICATE_TITLE: 'Make a copy of the picked profile that is yours to change.',
    profileDuplicatedToast: (name) => `Made "${name}", your own copy. Save Changes now saves into it.`,
    builtinProfileNotice: (names) => `Built-in profiles now stay up to date and can't be changed. You had changed your copy, so it's kept as ${names.map((n) => `"${n}"`).join(' and ')}.`,
    PROFILE_NOTICE_OK: 'OK',
    INIT_COMMANDS_LABEL: 'Initial Commands (run once at demo load):',
    INIT_COMMANDS_INFO_TITLE:
      "If you already exec a movie config from your config.cfg or autoexec, you don't need to add anything here. If you exec one manually instead, add exec and its name here (it runs with Seek Between Clips on), or use Import Config below to pull its lines in as Initial Commands. Only a capture batch runs these: Launch Preview and Launch Game don't.",
    ADD_INIT_COMMAND_BUTTON: '+ Add Initial Command',
    IMPORT_CFG_BUTTON: 'Import Config...',
    importedCfgToast: (n) => `Imported ${n} command(s) from the config.`,
    IMPORTED_CFG_EMPTY_TOAST: 'That config had no commands to import.',
    // Both lists on this tab are custom commands; only one is scheduled, so
    // that is what the label says. Paired with INIT_COMMANDS_LABEL's "once at
    // demo load", the two read as the distinction they actually are.
    CUSTOM_COMMANDS_LABEL: 'Scheduled Commands (run relative to each highlight):',
    ADD_CUSTOM_COMMAND_BUTTON: '+ Add Scheduled Command',
    START_CAPTURE_BUTTON: 'Start Capture Batch',
    CANCEL_BATCH_BUTTON: 'Cancel Batch',
    STATUS_WAITING: 'Status: Waiting...',
    INIT_COMMAND_PLACEHOLDER: 'e.g. mirv_fx_wh_enable 1',
    CUSTOM_COMMAND_PLACEHOLDER: 'Command',
    CUSTOM_COMMAND_RELATION_OPTIONS: ['Before', 'After'],
    footerRequiredSpace: (gb) => `Required: ${gb} GB`,
    REQUIRED_SPACE_DEFAULT: 'Required: 0.00 GB',
  },
  // ── capture_pane.js runtime text (toasts, warnings, confirms) ───────────
  // Under the Configuration field that blocks Start Capture (capture_field_errors.js).
  CAPTURE_FIELDS: {
    HL_PATH_EMPTY: 'Choose your Half-Life install: Start Capture needs hl.exe.',
    HLAE_PATH_EMPTY: 'Set the path to hlae.exe: Start Capture needs it.',
    OUTPUT_DIR_EMPTY: 'Add a Capture Output folder: Start Capture needs somewhere to write.',
    AGR_FPS_EMPTY: 'Set AGR FPS: AGR captures need their own rate.',
  },
  // ── main.js: sessions, settings dialogs, scan status, Clear actions ─────
  // Map library warnings. A demo names the map it was recorded on and stamps
  // that map's build alongside it, so "missing" and "wrong build" are different
  // problems: one cannot be played at all, the other plays and is quietly wrong.
  MAPS: {
    BANNER_TITLE: 'Maps needed',
    MISSING_LABEL: 'missing',
    WRONG_BUILD_LABEL: 'different version',
    UNREADABLE_LABEL: 'unreadable',
    DOWNLOAD_BUTTON: 'Download',
    DOWNLOAD_ALL_BUTTON: 'Download all',
    DISMISS_BUTTON: 'Dismiss',
    DOWNLOADING: 'Downloading…',
    // "Downloading dod_anzio (1 of 3): 2.1 of 6 MB"; no count for one map.
    downloadingLine: (map, index, count, bytes) =>
      `Downloading ${map}${count > 1 ? ` (${index} of ${count})` : ''}${bytes ? `: ${bytes}` : '…'}`,
    checkingLine: (map) => `Checking ${map}…`,
    NO_GAME_PATH: 'Set the hl.exe path in Configuration to check demo maps.',
    demoCount: (n) => (n === 1 ? '1 demo' : `${n} demos`),
    missingSummary: (maps, demos) =>
      `${maps === 1 ? '1 map' : `${maps} maps`} needed by ${demos === 1 ? '1 demo' : `${demos} demos`}`,
    installedToast: (map) => `Installed ${map}`,
    alreadyCorrectToast: (map) => `${map} was already the right build`,
    replacedNote: (path) => `Previous map kept at ${path}`,
    downloadFailedToast: (map, err) => `Could not install ${map}: ${err}`,
  },
  // Studio's map builds (#1547): old builds of a map kept in dod_addon and put
  // in place for the demos recorded on them. Shown on the Maps needed banner.
  MAP_BUILDS: {
    ADD_BUTTON: 'Add a build…',
    ADD_BUTTON_TITLE:
      'Pick a .bsp of this map. DoD Studio keeps a copy with its map builds and plays each demo on the build it was recorded on. Your installed map stays as it is.',
    useCopyButton: (file) => `Use ${file}`,
    useCopyTitle: (path) =>
      `${path} is the build these demos were recorded on. DoD Studio copies it to its map builds; the file stays where it is.`,
    PICK_TITLE: 'Pick a build of the map (.bsp)',
    MAP_FILES_FILTER_NAME: 'Map files',
    ADDING: 'Adding…',
    addingLine: (map) => `Adding a build of ${map}`,
    addedToast: (map, build) => `Added build ${build} of ${map}: Studio plays these demos on it when it starts the game for them`,
    otherBuildToast: (map, build, wanted) =>
      `That file is build ${build} of ${map}, not ${wanted}, the one these demos were recorded on. It is kept for demos recorded on ${build}.`,
    addFailedToast: (map, err) => `Could not add the build of ${map}: ${err}`,
    libraryNote: (map, build, count) =>
      `${map}: ${count === 1 ? '1 demo plays' : `${count} demos play`} on build ${build} from DoD Studio's map builds when DoD Studio starts the game for ${count === 1 ? 'it' : 'them'}`,
    NOTE_HELP:
      "The installed map is another version. DoD Studio puts the right one in dod_addon\\maps before it starts the game and takes it out once the game closes. A game that is already open keeps the version it has, and a game plays one version of a map: a batch mixing versions of one map keeps the installed one when any demo wants it.",
    // A file of the user's own in dod_addon\maps is what the game plays.
    addonFileLabel: 'a file in dod_addon\\maps is in the way',
    addonFileTitle: (path) =>
      `The game plays ${path}, a file DoD Studio didn't put there, so DoD Studio can't use its map builds for this map. Move it away to use them.`,
    // The confirm before a batch mixing versions of one map.
    mixedTitle: (count) => (count === 1 ? '1 demo won\'t play on its map version' : `${count} demos won't play on their map version`),
    mixedMessage: (count) =>
      `A game plays one version of each map, and ${count === 1 ? 'this demo was' : 'these demos were'} recorded on another version than the rest of the batch uses. ${count === 1 ? 'It plays' : 'They play'} on the installed version, so some walls, doors or water may look wrong. Capture ${count === 1 ? 'it' : 'them'} in a batch of ${count === 1 ? 'its' : 'their'} own to play ${count === 1 ? 'it' : 'them'} right.`,
    START_ANYWAY: 'Start anyway',
  },
  // The game's own config files setting cvars this app reads. Advisory only —
  // nothing in this app writes to a config file.
  CFG: {
    BANNER_TITLE: "Your game's config files set values this app reads:",
    ADVICE:
      'These are set outside the app, so it cannot see them when it plans a capture. Either remove them from your configs, or state them in Initial Commands below so the pipeline works from the same values the engine does. Nothing here changes your config files.',
    location: (file, line) => `set in ${file}, line ${line}`,
    // Rule 1 of #216: one cvar, different values in more than one place.
    CONFLICT_TITLE: 'These settings are given different values:',
    CONFLICT_ADVICE:
      'They run in order: your config files, then Initial Commands (with the ones DoD Studio adds last), then Scheduled Commands before each clip. The last one wins, so the others never apply. If DoD Studio sets the winning value, change that setting instead. Nothing here changes your config files.',
    conflictRow: (cvar, values, effective) => `${cvar}: ${values} — in effect: ${effective}`,
    stated: (value, source) => `${value} (${source})`,
    sourceConfig: (file, line) => `${file}, line ${line}`,
    sourceExec: (file, line) => `${file}, line ${line}, run by Initial Commands`,
    SOURCE_INITIAL: 'Initial Commands',
    sourceApp: (setting) => `DoD Studio, from ${setting}`,
    sourceBefore: (secs) => `Scheduled, ${secs} s before`,
    sourceAfter: (secs) => `Scheduled, ${secs} s after`,
    // Rule 2 of #216: an After with no Before for the same cvar.
    ASYMMETRIC_TITLE: 'These Scheduled Commands change a value for the rest of the batch:',
    ASYMMETRIC_ADVICE:
      'Scheduled Commands run around every clip, and nothing puts this value back. So the first clip records at one value and every clip after it at another. Add a Before command for the same setting with the value each clip should start from.',
    asymmetricRow: (cvar, baseline, baselineSource, after, afterSource) =>
      `${cvar}: the first clip records at ${baseline} (${baselineSource}), every later clip at ${after} (${afterSource})`,
    // Which setting owns a value the pipeline appends for itself, so the advice
    // can name the control rather than leaving the user to hunt for it.
    SETTING_FOR_CVAR: {
      mirv_movie_fps: 'Output Format → Capture FPS',
      r_decals: 'Pipeline → Flush Decals Between Clips',
    },
    UNKNOWN_SETTING: 'its own setting',
    BANNED_TITLE: 'These commands are not allowed:',
    BANNED_ADVICE:
      'Too dangerous to run at all — remove them. Start Capture Batch stays disabled while any are present.',
    // Why each is banned, and where to go instead when there's a real setting
    // for it.
    BANNED_REASONS: {
      mirv_movie_ffmpeg: 'set Capture Mode to Video in the Output Format tab instead',
      host_framerate: 'fast-forwarding is handled by the app; this will break the automation',
      mirv_recordmovie_start: 'the app schedules this itself; a manual one will break the automation',
      mirv_recordmovie_stop: 'the app schedules this itself; a manual one will break the automation',
      mirv_movie_filename: 'set the save location in the Destinations tab instead',
      // Only ever reported for Scheduled Commands (it is fine in Initial
      // Commands), like the three below.
      mirv_agr: 'set Capture Mode to AGR in the Output Format tab instead -- it saves one file per clip',
      // Scheduled only, like mirv_agr (#831).
      dem_speed: "the app sets the demo's speed itself for Fast-forward gaps between kills -- set that in the Timing tab instead",
      r_drawentities:
        "the engine resets this to 1 by itself, so it does nothing -- and if cheats are on instead, DoD's client closes the game",
      cl_lw: "DoD's client quits the game outright if this is not 1 -- there is no other value",
      // Fine as Initial Commands — that's how the decal flush is meant to be
      // configured — but banned_scheduled only ever reports these three when
      // they show up as Scheduled Commands instead, so the reason is always
      // about the mid-demo change, never about the cvar itself being wrong.
      r_decals: "can't change mid-demo — set it in Initial Commands instead",
      mirv_fov: "can't change mid-demo — set it in Initial Commands instead",
      gl_widescreenfov: "can't change mid-demo — set it in Initial Commands instead",
    },
    TOO_LONG_TITLE: 'These commands are too long to fit in a demo:',
    TOO_LONG_ADVICE:
      'Each command must be under 64 bytes — split it into shorter ones. Start Capture Batch stays disabled while any are present.',
    tooLongRow: (command, bytes) => `${command} — ${bytes} bytes`,
    bannedRowDetailed: (command, reason) => (reason ? `${command} — not allowed: ${reason}` : `${command} — not allowed`),
    HAZARD_TITLE: 'These Scheduled Commands fight a value DoD Studio sets itself:',
    HAZARD_ADVICE:
      "mirv_movie_fps is already pinned every capture from Output Format's own Capture FPS setting, so a scheduled one just fights it. fps_max, fps_override and gl_vsync are set by the fast-forward between clips and put back to your Initial Commands (or config) value at every clip's pre-roll, so a scheduled one lasts only until the next clip. Not dangerous, just short-lived — set them in Initial Commands instead.",
    hazardRow: (command) => `${command} — runs during playback`,
    DECAL_DEFAULT_TITLE: 'No r_decals value is set anywhere:',
    DECAL_DEFAULT_ADVICE:
      "The engine will use its default, 256, for the decal ring. That's a safe value on most maps — state r_decals in Initial Commands if you want a different one.",
    decalDefaultRow: (ring) => `r_decals — defaulting to ${ring}`,
    DECAL_NOOP_TITLE: 'Flush Decals Between Clips has nothing to clear:',
    DECAL_NOOP_ADVICE:
      "r_decals is 0, so the flush's sweep finds an empty ring every clip — real work for no effect. State a nonzero r_decals in Initial Commands, or turn off Flush Decals Between Clips in the Pipeline tab.",
    DECAL_NOOP_ROW: 'r_decals 0 — clears nothing',
    FATAL_TITLE: 'These config values will quit the game:',
    FATAL_ADVICE:
      "DoD's own client checks these whenever the HUD is on screen, and for most cvars it just forces the right value back silently. For these it also closes the game outright rather than merely correcting course. Nothing here changes your config files -- open the file named above and remove the line, or give it the value DoD requires. Setting it in Initial Commands instead is not a way round this: the app refuses these there, for the same reason.",
    fatalRow: (cvar, value, required, file, line) =>
      `${cvar} ${value} — DoD requires ${required}, set in ${file}, line ${line}`,
    // #478: the engine rewrites config.cfg on quit.
    CONFIG_WRITABLE_TITLE: 'Your config.cfg is saved over when the game closes:',
    CONFIG_WRITABLE_ROW:
      'config.cfg is not read-only, so the game writes its current settings into it on quit, including values your Initial and Scheduled Commands set.',
    CONFIG_WRITABLE_ADVICE:
      'To keep your own values, make config.cfg read-only (right-click it, Properties, tick Read-only). The trade-off: settings you change inside the game, like binds and options, stop being saved too. DoD Studio never changes this file.',
    NOOP_TITLE: 'These commands have no effect:',
    NOOP_ADVICE:
      'The pipeline (or the engine itself) always overrides or drops these before they could ever apply — not wrong, just wasted keystrokes.',
    NOOP_REASONS: {
      mirv_movie_filename: 'the pipeline sets this itself before every clip is captured',
    },
    noopRow: (command, reason, source) => {
      const isPlainSource = source === 'Initial Commands' || source === 'Scheduled Commands';
      return isPlainSource ? `${command} — ${reason}` : `${command} — ${reason} (${source})`;
    },
    // #555: the hook's console names went from dodstudio_ to ds_.
    RENAMED_TITLE: 'These DoD Studio commands have new names:',
    RENAMED_ADVICE:
      "DoD Studio's in-game commands now start with ds_ instead of dodstudio_. The old names do nothing in the game any more, so change these lines to the new name. DoD Studio never changes your config files.",
    renamedRow: (oldName, newName, source) => {
      const isPlainSource = source === 'Initial Commands' || source === 'Scheduled Commands';
      return isPlainSource ? `${oldName} → ${newName}` : `${oldName} → ${newName} (${source})`;
    },
    // #679: the engine's filter for commands a demo carries.
    FILTERED_TITLE: 'The game drops these commands without running them:',
    FILTERED_ADVICE:
      "Seek Between Clips (Pipeline tab) is off, so Initial and Scheduled Commands reach the game inside a patched copy of the demo, and the game filters every command a demo carries. With no message, it drops any command whose name contains bind, _set, kill, exit, quit, retry, _restart, writecfg, motdfile or motd_write, any name starting with connect, any alias, and any line containing exec. Turn Seek Between Clips on and DoD Studio's hook runs them all. A config file the game runs at start-up isn't filtered.",
    // An Initial Command `exec <file>` the game refuses or won't find.
    MISSING_EXEC_TITLE: "The game won't run the file these Initial Commands exec:",
    MISSING_EXEC_ADVICE:
      'The name must end in .cfg (or .rc), with no other dot and no backslash; the game adds no extension. It looks in dod, then dod_addon, then dod_downloads, and prints "couldn\'t exec" when the file is in none of them. Check the name, or move the file into one of those folders.',
    filteredRow: (command, rule, pattern) => {
      const why = {
        nameContains: `its name contains '${pattern}'`,
        nameStartsWith: `its name starts with '${pattern}'`,
        lineStartsWith: `it starts with '${pattern}'`,
        lineContains: `it contains '${pattern}'`,
        wordStartsWith: `it has a word starting with '${pattern}'`,
      }[rule];
      return why ? `${command} — dropped: ${why}` : `${command} — dropped`;
    },
  },
  // Pre-roll and post-roll are load-bearing: playback returns to real time one
  // pre-roll before recording, so everything that must happen at normal speed
  // has to fit inside it.
  ROLLS: {
    BANNER_TITLE: 'These timings are shorter than this capture needs:',
    ADVICE:
      'Playback only returns to real time one pre-roll before recording starts. Anything that has to happen at normal speed — the engine flushing its audio buffers after the fast-forward, the decal sweep, a Scheduled Command — has to fit inside that window, or it happens while the engine is still racing through frames.',
    tooShort: (name, have, need, binding) =>
      `<code>${name} ${have.toFixed(1)} s</code> — needs at least <code>${need.toFixed(1)} s</code> for ${binding}`,
  },
  // ── killfeed_layout_warning.js: the even kill feed can't draw (#1339) ──
  // The warning itself comes from the backend (native messages/killfeed_layout.rs).
  KILLFEED_LAYOUT: {
    LINE_TITLE: "The hook DLL said so while the batch ran: clips captured while this shows have the game's own kill-feed layout, not the even one. The activity log has the hook's own words.",
  },
  // ── crash_map_warnings.js: maps a session crashed on (#207) ────────────
  CRASH_MAPS: {
    title: (count) => `${count} demo${count === 1 ? ' is' : 's are'} on a map the game crashed on`,
    message: (maps) => `A capture on ${maps === 1 ? 'this map' : 'these maps'} ended in a crash before, for a reason the demo file can't show. Start the batch anyway?`,
    demos: (names) => (names.length <= 2 ? names.join(', ') : `${names.slice(0, 2).join(', ')} and ${names.length - 2} more`),
    seen: (count, date, build) => `Seen ${count === 1 ? 'once' : `${count} times`}, last on ${date}${build ? `, ${build} build` : ''}.`,
    START_ANYWAY: 'Start anyway',
  },
  // ── packet_entity_limit.js: demos over the engine's limit (#207) ───────
  ENTITY_LIMIT: {
    BADGE: 'trimmed copy',
    badgeTitle: (peak, limit) => `Up to ${peak} entities in one snapshot. This install's game closes at more than ${limit}, so captures and previews play a trimmed copy: fog, invisible collision hulls and a few far decorations are left out of every snapshot. The original file is unchanged.`,
    ANNIVERSARY_HINT: 'The 25th Anniversary engine allows 1024 and plays the original: set its hl.exe in Configuration → Paths for these.',
  },
  // ── command_suggest.js: the Commands tab's type-ahead (#215) ────────────
  COMMAND_SUGGEST: {
    OWNED_BY_STUDIO: "DoD Studio sets this itself, so it's refused here.",
    GAME_QUITS_OVER: "DoD quits the game if this isn't 1, so it's refused here.",
    SCHEDULED_BANNED: "Initial Commands only: it's refused as a Scheduled Command.",
    droppedFromDemo: (pattern) => `Does nothing while Seek Between Clips is off: the game drops any command from a demo with '${pattern}' in it.`,
    NOOP_IN_INIT: 'Does nothing here: DoD Studio sets it before anything reads it.',
    HAS_A_SETTING: 'DoD Studio has a setting for this; typing it here is flagged.',
    FAST_FORWARD_RESTORED: "Lasts until the next clip: the fast-forward between clips sets this, and every clip's pre-roll puts your Initial Commands value back.",
    describe: (source, kind, builds) => {
      const what = source === 'hlae' ? 'HLAE' : source === 'dodstudio' ? 'DoD Studio' : kind === 'cvar' ? 'setting' : 'command';
      const where = builds === 'pre' ? ', pre-Anniversary only' : builds === 'post' ? ', 25th Anniversary only' : '';
      return `${what}${where}`;
    },
  },
  // ── ipc_bridge.js: error-toast prefixes wrapping backend errors ─────────
  // Checked before DoD Studio starts the game (steam_guard.js).
  STEAM: {
    NOT_RUNNING_TITLE: "Steam isn't running",
    NOT_RUNNING_MESSAGE: "Day of Defeat needs Steam running and signed in. Without it the game closes straight away with an authentication error. Start Steam now? The launch carries on once you're signed in.",
    START_STEAM: 'Start Steam',
    CANCEL: 'Cancel',
    WAITING_FOR_SIGN_IN: 'Waiting for Steam to sign in. The launch carries on once it has.',
    STILL_WAITING: 'Still waiting for Steam to sign in.',
    WAIT_CANCELLED: 'Cancelled. Nothing was launched.',
    // Beside Start Capture Batch when the Steam check stopped it.
    BATCH_NOT_STARTED_STATUS: "Status: Not started — Steam wasn't running and signed in.",
    NOT_SIGNED_IN: "Steam still isn't signed in after 2 minutes, so nothing was launched. Sign in, then try again.",
  },
  // ── running_game_guard.js: a running game started with other launch
  // settings (#666) ──────────────────────────────────────────────────────
  RUNNING_GAME: {
    TITLE: 'Day of Defeat is running with other settings',
    CLOSE_AND_RESTART: 'Close it and start again',
    CANCEL: 'Cancel',
    CLOSING: 'Closing Day of Defeat…',
    RUNNING_EXE: 'Running now',
    STUDIO_EXE: "DoD Studio's Half-Life Executable",
    // The hook DLL rows (#1143): path, then its build id when it has one.
    RUNNING_HOOK: 'Hook DLL in the game',
    STUDIO_HOOK: 'Hook DLL DoD Studio is set to',
    hookLine: ({ hook_dll, hook_build }) => `${hook_dll} (${hook_build ? `build ${hook_build}` : 'no build id'})`,
    // Beside Start Capture Batch when the user kept the running game.
    BATCH_NOT_STARTED_STATUS: 'Status: Not started — Day of Defeat is running with other launch settings.',
    /** "DoD is running from X at 1920×1080; DoD Studio is set to Y at
     *  3440×1440", naming only what differs. `check` is check_running_game's
     *  mismatch answer. */
    message({ running, wanted, differs }) {
      const install = differs.includes('install');
      // The Game folder setting, `-game` (#402).
      const folder = differs.includes('game_folder');
      const resolution = differs.includes('resolution');
      // The hook DLL from another path, or another build at its path (#1143).
      const hookDll = differs.includes('hook_dll');
      const hookBuild = differs.includes('hook_build');
      const unreadable = differs.includes('hook_unreadable');
      const size = (s) => `${s.width}×${s.height}`;
      const folderName = (s) => `the "${s.game_folder}" game folder`;
      const hookFrom = (s) => `the hook DLL from "${s.hook_label}"`;
      const build = (s) => (s.hook_build ? `hook build ${s.hook_build}` : 'an older hook build with no build id');
      const now = [
        install && `from "${running.install}"`,
        folder && `with ${folderName(running)}`,
        resolution && `at ${size(running)}`,
        hookDll && `with ${hookFrom(running)}`,
        hookBuild && `with ${build(running)}`,
      ].filter(Boolean).join(' ');
      const unread = 'DoD Studio could not read which hook DLL the running Day of Defeat has, so it can\'t tell whether it is the one DoD Studio is set to.';
      if (!now) return `${unread} Close it and start again?`;
      const set = [
        install && `"${wanted.install}"`,
        folder && (install ? `with ${folderName(wanted)}` : folderName(wanted)),
        resolution && (install || folder ? `at ${size(wanted)}` : size(wanted)),
        hookDll && (install || folder || resolution ? `with ${hookFrom(wanted)}` : hookFrom(wanted)),
        hookBuild && (install || folder || resolution ? `with ${build(wanted)}` : build(wanted)),
      ].filter(Boolean).join(' ');
      const also = unreadable ? ` ${unread}` : '';
      return `Day of Defeat is running ${now}; DoD Studio is set to ${set}.${also} The game only takes these when it starts. Close it and start again?`;
    },
    // Switching installs in Configuration > Paths while a game runs (#931).
    SWITCH_TITLE: 'Day of Defeat is running',
    CLOSE_AND_SWITCH: 'Close it and switch',
    switchMessage: ({ running }) =>
      `Day of Defeat is running from "${running.install}". Close it to switch installs? Nothing is switched if you cancel.`,
  },
  // ── OBS capture test (#646, obs_capture_test.js) ──────────────────────────
  OBS_TEST: {
    TEST_BUTTON: 'Test OBS capture',
    TEST_BUTTON_TITLE:
        "Records about a minute of a demo that comes with DoD Studio, through a normal batch at your OBS Capture FPS and OBS Quality, then says whether this PC keeps up: frames OBS skipped or missed, frames the game didn't draw in time, and whether the clip has sound. The game opens for about a minute and a half; the clip is deleted afterwards.",
    LADDER_BUTTON: 'Find my highest steady FPS',
    LADDER_BUTTON_TITLE:
        'Runs the same test at 60, 120, 144, 165, 180, 200 and 240 fps at your OBS Quality, stopping once a rate clearly fails, and says the highest rate this PC captures with no dropped or repeated frames. On the bundled demo it recommends one step below that, for maps that are busier to encode. About a minute and a half per rate. Your OBS Capture FPS stays as it is unless you choose to use the result.',
    LONG_LABEL: '1-minute clips',
    LONG_TITLE: 'Longer clips catch a PC that slows down as it warms up. Each rate takes about 40 s longer.',
    STOP_BUTTON: 'Stop test',
    DEMO_LABEL: 'Test demo:',
    DEMO_BUNDLED: 'the one that comes with DoD Studio',
    DEMO_PICK: 'Use another demo…',
    DEMO_PICK_TITLE: "Test on one of your own demos instead: the test records the stretch of it with the most kills. A real match's own lag shows up as repeated frames, so the bundled demo's result is the fairer one.",
    DEMO_RESET: 'Use the bundled demo',
    DEMO_DIALOG_TITLE: 'Pick a demo for the OBS capture test',
    DEMO_NOTE:
        "The test plays a stretch of a real 6v6 match on dod_anzio, with grenades and fights, recorded without lag. A map with more detail on screen gives OBS's encoder more to do: if you play other maps, test one of your own demos too.",
    // Step lines, each shown while that step runs.
    CHECKING_OBS: 'Checking OBS…',
    CHECKING_GAME: 'Checking the game is closed…',
    PREPARING_CLIP: 'Getting the test clip ready…',
    ladderStep: (n, total, rate) => `Rate ${n} of up to ${total}, ${rate} fps. `,
    recording: (rate, status) => `Capturing at ${rate} fps: ${status || 'starting…'}`,
    // No count until the clip's frame total is known, rather than "frame 0 of 0".
    readingClip: (done, total) => (total > 0
      ? `Reading the clip: frame ${done.toLocaleString()} of ${total.toLocaleString()}`
      : 'Reading the clip…'),
    DELETING: 'Deleting the test clip…',
    RESTORING: 'Putting OBS back to your OBS Capture FPS…',
    // Results.
    LEVEL_STEADY: 'Sustainable',
    LEVEL_BORDERLINE: 'Borderline',
    LEVEL_NOT_STEADY: 'Not sustainable',
    CAUSE_ENCODER: "OBS's encoder couldn't keep up",
    CAUSE_RENDERING: "OBS's rendering couldn't keep up",
    CAUSE_GAME: "the game didn't draw new frames fast enough",
    verdictLine: (rate, level, quality) => `${rate} fps at OBS Quality ${quality}: ${level}.`,
    causeLine: (cause) => `What lost frames: ${cause}.`,
    numbersLine: (skipped, missed, repeated) =>
        `${skipped} skipped by OBS's encoder, ${missed} missed by OBS's rendering, ${repeated} of the moving frames repeated in the file (the game late, or OBS filling in a skipped frame).`,
    adviceFps: (rate) => `Change one setting: lower OBS Capture FPS to ${rate}.`,
    adviceQuality: (quality) => `Change one setting: lower OBS Quality to ${quality}, or lower OBS Capture FPS.`,
    ADVICE_NONE: 'Nothing to change: OBS kept every frame.',
    gameLine: (avg, low1, low01, slower, rate) =>
        `The game itself: ${avg} fps average, 1% low ${low1} fps, 0.1% low ${low01} fps; ${slower} of its frames took longer than one ${rate} fps frame.`,
    GAME_UNMEASURED: "The game's own frame rate wasn't measured: the hook DLL in use is older than this test.",
    soundOk: (peak) => `Sound: present, loudest moment ${peak} dB.`,
    SOUND_MISSING: "Sound: the clip has no sound track. Check OBS's [DoD-Studio] Game Audio source.",
    SOUND_SILENT: "Sound: the clip is silent. Check that OBS's [DoD-Studio] Game Audio source isn't muted.",
    COUNTERS_UNREAD: "OBS's own frame counters couldn't be read, so only the clip itself was checked.",
    ladderBest: (best, recommended, quality) =>
        `Steady up to ${best} fps at OBS Quality ${quality} on this demo. Recommended: ${recommended} fps (one step lower, for busier maps).`,
    ladderBestEdge: (rate, quality) =>
        `Steady only at ${rate} fps at OBS Quality ${quality} on this demo. Recommended: ${rate} fps, but it's at the edge: busier maps may not hold it.`,
    ladderBestOwn: (rate, quality) =>
        `Steady up to ${rate} fps at OBS Quality ${quality} on your demo (its busiest stretch). Recommended: ${rate} fps.`,
    LADDER_MARGIN_WHY:
        "Why lower: the bundled demo is lighter to encode than maps with more detail (on an RTX 3070 Ti it held 200 fps at High, where a dod_harrington match failed at 165). Test one of your own demos for a tighter number.",
    LADDER_NONE: 'No rate held steady, not even 60 fps.',
    ladderFirstFail: (rate, level, cause) => `${rate} fps: ${level}, ${cause}.`,
    ladderAllPassed: (rate) => `Every rate up to ${rate} fps held.`,
    applyButton: (rate) => `Use ${rate} fps`,
    APPLY_TITLE: 'Sets OBS Capture FPS to this rate.',
    applied: (rate) => `OBS Capture FPS is now ${rate}.`,
    COL_FPS: 'OBS FPS',
    COL_RESULT: 'Result',
    COL_SKIPPED: 'Skipped (encoder)',
    COL_MISSED: 'Missed (rendering)',
    COL_REPEATED: 'Repeated',
    COL_GAME: 'Game fps (1% low)',
    COL_SOUND: 'Sound',
    SOUND_CELL_OK: 'yes',
    SOUND_CELL_MISSING: 'none',
    SOUND_CELL_SILENT: 'silent',
    // Refusals and endings.
    BATCH_RUNNING: 'A capture batch is running. Test OBS once it has finished.',
    GAME_OPEN: 'Close the game first: the test starts its own.',
    OBS_NOT_CONNECTED: "OBS isn't reachable. Check the OBS Connection settings above, then test again.",
    NO_TAKE: "OBS recorded nothing for the test clip. Check OBS's own window for why.",
    batchEnded: (status) => `The test clip's batch ended without a clip: ${status}`,
    failed: (err) => `The test stopped: ${err}`,
    STOPPED: 'Test stopped.',
  },
};
