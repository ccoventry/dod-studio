// strings/workspace.js: the app's text for the workspace: sessions, the demo queue, teams and the project file.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const WORKSPACE_STRINGS = {
  WORKSPACE: {
    SCAN_PANEL_TITLE: 'Directory Scan & Management',
    ADD_FILES_BUTTON: '+ Add Demo Files',
    ADD_FOLDER_BUTTON: '+ Add Folder',
    // #531: beside + Add Folder; also what a dropped folder follows.
    INCLUDE_SUBFOLDERS_LABEL: 'Include subfolders',
    INCLUDE_SUBFOLDERS_TITLE: "Ticked: + Add Folder also reads the demos in the folder's subfolders, up to 6 levels down. Unticked: only the demos directly in the folder.",
    CANCEL_SCAN_BUTTON: 'Cancel Scan',
    CANCEL_SCAN_TITLE: 'Cancel the running directory scan',
    SCAN_STATUS_READY: 'Ready',
    MASTER_QUEUE_TITLE: 'Master Demo Queue',
    SEARCH_PLACEHOLDER: 'Search filename or map...',
    FILTER_ROW_LABEL: 'Filters',
    // #174: demos you can capture a player from (POV demos they recorded).
    PLAYER_FILTER_ALL: 'All players',
    PLAYER_FILTER_TITLE: 'Show only the POV demos this player recorded, so every highlight in the list is theirs. Each entry groups every name the player used.',
    // #54: the Master Queue's quick filters.
    KILLS_FILTER_TITLE: 'Hide demos where the recording player got no kills, no highlight of two or more kills, or no headshot. A demo scanned before headshots were read shows none until it is scanned again.',
    KILLS_FILTER_ALL: 'All demos',
    KILLS_FILTER_WITH_KILLS: 'With kills',
    KILLS_FILTER_MULTI_KILL: 'With a multi-kill',
    KILLS_FILTER_HEADSHOT: 'With a headshot',
    OWNER_ONLY_LABEL: 'POV only',
    OWNER_ONLY_TITLE: 'Hide demos where no single recording player could be found. Their highlights list every player in the match.',
    // #127: the demos with the bookmark badge (isDemoTracked).
    TRACKED_ONLY_LABEL: 'Tracked only',
    TRACKED_ONLY_TITLE: 'Show only the demos with the bookmark: a highlight with a status, a review mark, a note, a clip name or a narrowed kill range. The demo open in Highlight Details stays listed until you open another.',
    TRACKED_ONLY_NONE_TITLE: 'No demo is tracked yet. Give a highlight a status, a review mark, a note, a clip name or a narrowed kill range to use this.',
    // #685, #823: the queue grouped by match, player, map, ..., with a header line per group.
    GROUP_BY_LABEL: 'Group by:',
    GROUP_BY_TITLE: 'Show the queue in groups, each under a line with its counts. Only the order changes: '
      + 'the same demos stay shown, and Review highlights and the bulk actions use them as before.\n'
      + 'Match puts the demos of one match together: the HLTV demo and every POV of each half. '
      + 'Which match and half a demo is comes from what it recorded (the players, their sides, the kills and the chat), never from its file name. '
      + 'The same numbers feed {match} and {half} in clip names.\n'
      + 'Player groups POV demos by who recorded them. Date is the day the demo file was last changed.',
    GROUP_BY_NONE: 'None',
    GROUP_BY_MATCH: 'Match',
    GROUP_BY_PLAYER: 'Player',
    GROUP_BY_MAP: 'Map',
    GROUP_BY_FOLDER: 'Folder',
    GROUP_BY_DATE: 'Date',
    GROUP_BY_TYPE: 'Demo type',
    // The queue's Sort by dropdown (queue_sort_by.js).
    SORT_BY_LABEL: 'Sort by:',
    SORT_BY_TITLE: 'The order of the demos in the queue, inside each group when Group by is on. Only the order changes: '
      + 'the same demos stay shown, and Review highlights and the bulk actions use them as before.\n'
      + 'Order added is the order the demos went into the queue. Newest and Oldest go by the day the demo file was last changed.',
    SORT_BY_ADDED: 'Order added',
    SORT_BY_NAME: 'Name',
    SORT_BY_NEWEST: 'Newest first',
    SORT_BY_OLDEST: 'Oldest first',
    SORT_BY_HIGHLIGHTS: 'Most highlights',
    groupHeader: (label, demos, highlights) =>
      `${label} · ${demos} demo${demos === 1 ? '' : 's'}, ${highlights} highlight${highlights === 1 ? '' : 's'}`,
    GROUP_HLTV: 'HLTV demos',
    GROUP_POV: 'POV demos',
    GROUP_NO_RECORDER: 'No recording player found',
    GROUP_NO_MAP: 'Map not known',
    GROUP_NO_DATE: 'Date not known',
    GROUP_NO_TYPE: 'Demo type not known',
    groupDateTitle: (day) => `Demo files last changed on ${day}`,
    groupingStatus: (done, total) => (total > 0 ? `Grouping by match: ${done} of ${total}` : 'Grouping by match…'),
    matchHeader: (number, map, teams, halves) => [`Match ${number}`, map, teams, halves].filter(Boolean).join(' · '),
    matchTeams: (a, b) => `${a || '?'} vs ${b || '?'}`,
    matchHalfCount: (half, count) => `${half ? `h${half}` : 'half unknown'}: ${count} demo${count === 1 ? '' : 's'}`,
    UNGROUPED_HEADER: 'Not in a match: these demos could not be read',
    halfTooltip: (detail) => `What decided the half: ${detail}`,
    SEARCH_CLEAR_TITLE: 'Clear the search (Esc)',
    CLEAR_UNTRACKED_BUTTON: 'Clear Untracked',
    CLEAR_UNTRACKED_TITLE: 'Remove demos with no Pending/Captured/Rendered status, notes, or edited kill range. Tracked demos are kept. Only affects demos matching the current search.',
    CLEAR_SELECTED_BUTTON: 'Clear Selected',
    CLEAR_SELECTED_TITLE_DEFAULT: 'Check one or more rows first.',
    CLEAR_ALL_BUTTON: 'Clear All',
    CLEAR_ALL_TITLE_TOOLTIP: 'Remove every demo matching the current search — the whole queue if search is empty.',
    SELECT_ALL_CB_TITLE: 'Select/deselect all visible demos',
    TABLE_HEADER_DEMO_FILE: 'Demo File',
    TABLE_HEADER_HIGHLIGHTS: 'Highlights',
    TABLE_HEADER_SELECTED: 'Selected',
    // One cell each of coloured counts, legend letters under the name.
    TABLE_HEADER_STATUS: 'Status',
    TABLE_HEADER_STATUS_TITLE: 'How many of the demo\'s highlights are Pending / Captured / Rendered (P / C / R), each in its status colour. Hover a row for the numbers in words.',
    TABLE_HEADER_REVIEW: 'Review',
    TABLE_HEADER_REVIEW_TITLE: 'How many of the demo\'s highlights have no Review mark yet / Keep / Maybe / Skip (– / K / M / S), each in its colour. Hover a row for the numbers in words.',
    COUNT_LABELS: { Pending: 'Pending', Captured: 'Captured', Rendered: 'Rendered', Unmarked: 'not reviewed', Keep: 'Keep', Maybe: 'Maybe', Skip: 'Skip' },
    COUNT_LETTERS: { Pending: 'P', Captured: 'C', Rendered: 'R', Unmarked: '–', Keep: 'K', Maybe: 'M', Skip: 'S' },
    TABLE_HEADER_ACTIONS: 'Actions',
    TABLE_EMPTY_NO_DEMOS: "No demos scanned yet. Use '+ Add Demo Files' or '+ Add Folder' to get started.",
    TABLE_EMPTY_NO_DEMOS_IN_DIRS: 'No demos found in specified directories.',
    TABLE_EMPTY_NO_MATCH_SEARCH: 'No demos match your search and filters.',
    DEMO_LIST_FOOTER_DEFAULT: 'Loaded Demos: 0 | Total Highlights: 0',
    demoListFooter: (loaded, highlights) => `Loaded Demos: ${loaded} | Total Highlights: ${highlights}`,
    // The totals row under the queue's header (queue_totals.js).
    totalsLabel: (shown, queued) => (shown === queued
      ? `Total: ${shown} demo${shown === 1 ? '' : 's'}`
      : `Total: ${shown} of ${queued} demos`),
    TOTALS_TITLE: 'Totals of the demos listed below, with the search and filters applied. Hover Status or Review for the numbers in words.',
    // #21: a project demo that is not at its saved path.
    MISSING_BADGE: 'missing',
    missingBadgeTitle: (path) => `Not found at ${path}. Use Locate to point at where it is now, or remove it from the queue.`,
    LOCATE_DEMO_BUTTON: 'Locate…',
    LOCATE_DEMO_TITLE: 'Pick where this demo is now',
    USE_FOUND_COPY_BUTTON: 'Use found copy',
    useFoundCopyTitle: (path) => `A matching file was found at ${path}. Click to use it.`,
    REMOVE_DEMO_TITLE: 'Remove demo from queue',
    // #217: a queued demo that recorded more than one map.
    multiMapSplitButton: (count) => `${count} maps · Split`,
    multiMapSplitTitle: (maps) => `This demo recorded ${maps.length} maps (${maps.join(', ')}). `
      + 'Its highlights and captures use the first map only; the maps after it are left out. '
      + 'Click to split it: each map of a minute or more becomes a demo of its own in the queue, in place of this one. The file itself is kept.',
    MULTI_MAP_ALREADY_SPLIT: 'Already split',
    multiMapAlreadySplitTitle: (maps, files) => `This demo recorded ${maps.length} maps (${maps.join(', ')}), already split next to it: `
      + `${files.join(', ')}. This row uses the first map only.`,
    MULTI_MAP_SPLITTING: 'This demo is being split.',
    removeDemoConfirm: (name) => `Remove "${name}" from the queue? It has tracked work (a Pending/Captured/Rendered status, a note, or an edited kill range) that will be lost.`,
    trackedBadgeTooltip: (reasons) => `Tracked — has ${reasons.join(', ')}. Protected from Clear Untracked in Workspace mode.`,
    rowDeleteLog: (name, trackedNote) => `[queue] Row delete: removed "${name}"${trackedNote}`,
    TRACKED_NOTE_SUFFIX: ' (had tracked work; user confirmed)',
    REASON_STATUS: 'a Pending/Captured/Rendered status',
    REASON_NOTE: 'a note',
    REASON_CLIP_NAME: 'a typed clip name',
    REASON_REVIEW: 'a Keep/Maybe/Skip review mark',
    REASON_RANGE: 'an edited kill range',
    EMPTY_DASH: '—',
  },
  // ── Teams list (#445) — clan tags found in the project's demos ──────────
  TEAMS: {
    BUTTON: 'Teams',
    BUTTON_TITLE: "The clan tags found in this project's demos, and the team names clip names use for them",
    TITLE: 'Teams',
    INTRO: "Tags found in players' names, one per side of each demo. Type the name you want a team to go by, or pick another tag it is the same team as.",
    HEADER_TAG: 'Tag',
    HEADER_DEMOS: 'Demos',
    HEADER_SIDES: 'Played as',
    sides: (allied, axis) => `${allied} Allied, ${axis} Axis`,
    HEADER_NAME: 'Name',
    HEADER_SAME_AS: 'Same team as',
    SAME_AS_NONE: '—',
    EMPTY: 'No tags found yet. They appear once demos with tagged players are in the queue.',
    unreadNote: (count) => `${count} demo(s) in the queue were scanned before teams were read.`,
    READ_BUTTON: 'Read Their Teams',
    READ_BUTTON_TITLE: 'Scan those demos again. Statuses, notes and kill ranges are kept.',
    READING_BUTTON: 'Reading...',
    alsoTag: (tag) => `also ${tag}`,
    splitTitle: (tag) => `Split ${tag} back out into a team of its own`,
    CLOSE_BUTTON: 'Close',
  },
  // ── The project file's format, backup and newer-build notes (project_file.js, #1085) ──
  PROJECT_FILE: {
    MADE_WITH_NEWER: 'Made with a newer DoD Studio',
    NEWER_TITLE: 'This project was saved by a newer DoD Studio. Its new settings are kept when you save here, but not used.',
    NEEDS_COPY_TITLE: 'This project was saved by a newer DoD Studio that asks older builds not to save over it. Save makes a copy instead.',
    NEWER_SAVED_TOAST: 'This project was made with a newer DoD Studio: its new settings are kept, but not used here.',
    SAVE_COPY_DIALOG_TITLE: 'Save a copy',
    copyName: (stem) => `${stem} copy`,
    updatedToast: ({ clips, takesFound, takesNotFound, settingsCopied = false, snapshotDay = '' }, backup) => {
      const takes = takesFound === null ? '' : `, ${takesFound} take${takesFound === 1 ? '' : 's'} found, ${takesNotFound} not found`;
      // #1085: an old project's own settings start as the one snapshot of
      // Configuration every older project gets, or, when there is none, a
      // copy of Configuration as it was when the project opened.
      let settings = '';
      if (settingsCopied) {
        settings = snapshotDay
          ? ` Its settings are the ones Configuration had on ${snapshotDay}, the same for every older project, and are now its own.`
          : ' Its settings were copied from Configuration and are now its own.';
      }
      return `Updated: ${clips} clip${clips === 1 ? '' : 's'}${takes}.${settings} Backup: ${backup}`;
    },
    backupFailed: (path, err) => `Not saved: could not back up ${path} first: ${err}`,
    needsNewerBuild: (path) => `Not saved: ${path} needs a newer DoD Studio to save over it. Use Save a copy.`,
    newerOnDisk: (path) => `Not saved: a newer DoD Studio saved ${path} after you opened it, and saving here would erase what it added. Save a copy, or open the file again.`,
    writeFailed: (path, err) => `Not saved: could not write ${path}: ${err}`,
    CANT_CHECK: 'could not check whether the file is there',
    NO_BACKUP_NAME: 'no backup could be made without overwriting an earlier one',
    noMigrationStep: (schema) => `This build has no way to update a project from format ${schema}.`,
  },
  // A project's own settings (project_settings.js, project_install_offer.js, #1085).
  PROJECT_SETTINGS: {
    INSTALL_OFFER_TITLE: 'Switch install?',
    installOffer: (madeWith, current) => `This project was made with ${madeWith}. ${current} is picked now. Switch?`,
    SWITCH: 'Switch',
    KEEP: 'Keep the one picked',
    NO_INSTALL: 'No install',
  },
  // ── The New UI switch (new_ui.js, #1085) ──
  NEW_UI: {
    LABEL: 'Show the new pages (work in progress)',
    TITLE: "Shows the pages of the new DoD Studio as they are built, starting with Project Settings in the File menu. They are not finished. Turn this off to hide them and get today's app back; a project keeps any settings you changed there.",
    switchedLine: (on) => `[settings] New UI switch turned ${on ? 'on: the new pages show' : 'off: the new pages are hidden'}`,
  },

  // ── The Project settings page (project_settings_page.js, #1085) ──
  PROJECT_SETTINGS_PAGE: {
    MENU_ITEM: 'Project Settings…',
    MENU_TITLE: "This project's own picture, clip length, clip names, folders and clip settings.",
    UNTITLED: 'Untitled project',
    TITLE: 'Project settings',
    SIDE_NOTE: 'How this movie is made. Every clip in the project uses these. OK or Apply puts a change into the project; Save writes it to the file.',
    TAB_PROJECT: 'Project',
    TAB_CLIP_SETTINGS: 'Clip settings',
    TAB_PLAYERS: 'Players and sprays',
    TAB_PLAYERS_SOON: 'Comes in a later update. Until then, Configuration › Players sets sprays and crosshairs for every project.',
    BATCH_RUNNING: 'A batch is running. Changes apply to the next batch.',

    PICTURE: 'Picture',
    PICTURE_SUB: 'Every clip in this project',
    CAPTURE_WITH: 'Capture with',
    CAPTURE_WITH_HINT: 'Frames: one image per frame, the best picture. Video: HLAE records straight to a file and uses less disk. OBS: records in real time through the OBS connection in Configuration. AGR: models and camera only, for the Blender tab.',
    MODE_FRAMES: 'Frames',
    MODE_VIDEO: 'Video',
    MODE_OBS: 'OBS',
    MODE_AGR: 'AGR for Blender',
    SIZE: 'Size',
    SIZE_HINT: 'The game window and every take. Custom: any width and height, such as 2560×1080.',
    SIZE_1080P: '1080p',
    SIZE_1440P: '1440p',
    SIZE_4K: '4K',
    CUSTOM: 'Custom…',
    WIDTH: 'Width',
    HEIGHT: 'Height',
    BY: '×',
    SMOOTHNESS: 'Smoothness',
    SMOOTHNESS_HINT: 'Frames a second. Higher is smoother in slow motion and uses more disk.',
    SMOOTHNESS_OBS_HINT: 'Frames a second OBS records, in real time. Set it above what this PC keeps up with and OBS drops frames: test a short capture first.',
    FOV: 'Field of view',
    FOV_HINT: "mirv_fov, for every demo, first-person and HLTV. Sent after the Clip settings lines and any cfg they exec, so it wins over every other mirv_fov: a batch leaves those lines out, and a cfg that sets a different one is warned about in Configuration › Commands. The game's default is 90.",
    FOV_UNIT: '°',
    BAD_FOV: 'Enter a field of view from 1 to 179.',
    AGR_RATE: 'AGR positions a second',
    AGR_RATE_HINT: 'How many positions a second each .agr file records. For slow motion use at least the output frame rate times the slow-down (300 for 5x at 60 fps).',
    fps: (fps) => `${fps} fps`,
    FPS_UNIT: 'fps',
    FINISHED: 'Finished clips',
    FINISHED_HINT: 'What happens to the takes when a batch ends.',
    FINISH_LEAVE: 'Leave for the Render tab',
    FINISH_AUTO: 'Finish automatically',
    SAVED_AS: 'Saved as',
    SAVED_AS_TITLE: 'What each finished clip is saved as, for this project\'s capture mode.',
    RENDER_TAB_CODEC: "Same as the Render tab's codec",
    OBS_KEEP_NOTE: 'OBS clips can stay as recorded: Keep as captured copies them in seconds.',
    AGR_NOTHING_TO_FINISH: 'AGR takes are not videos, so there is nothing to finish. Open them on the Blender tab.',

    CLIP_LENGTH: 'Clip length',
    CLIP_LENGTH_SUB: 'Extra time around the kills',
    BEFORE: 'Before the first kill',
    BEFORE_HINT: 'Tip: 3 to 5 s shows the lead-up without dead air.',
    AFTER: 'After the last kill',
    AFTER_HINT: 'How long the clip runs on after the last kill.',
    SECONDS_UNIT: 's',
    lengthLine: (before, after) => `${before} s, first kill … last kill, ${after} s`,
    GAPS: 'Quiet gaps between kills',
    GAPS_HINT: 'Fast-forward records long gaps between kills sped up, so a clip spends less time waiting.',
    GAPS_KEEP: 'Keep',
    GAPS_FAST: 'Fast-forward',
    GAPS_OVER: 'Gaps over',
    GAPS_AT: 'at',
    SPEED_UNIT: 'x',

    CLIP_NAMES: 'Clip names',
    NAME_LIKE: 'Name clips like',
    NAME_HINT: 'Every finished clip is named from this, unless its row has a name typed in. Keep {time} in it: without it, two clips of one player with one weapon on one map get the same name.',
    DEFAULT: 'Default',
    defaultTitle: (template) => `Back to ${template}`,
    PLACEHOLDERS: 'Placeholders',
    PLACEHOLDERS_HINT: 'Click a chip to add it where the cursor is.',

    FOLDERS: 'Folders',
    FOLDERS_SUB: 'This PC only',
    CAPTURE_FOLDERS: 'Capture folders',
    CAPTURE_FOLDERS_HINT: "Takes go here, spread across the folders. On another PC the project uses that PC's own.",
    EXPORT_FOLDERS: 'Export folders',
    EXPORT_FOLDERS_HINT: 'Finished clips go here.',
    ADD_FOLDER: '+ Add folder…',
    REMOVE_MARK: '×',
    removeFolderTitle: (path) => `Take ${path} out of this project. Nothing on disk is deleted.`,
    NO_FOLDERS: 'None yet',
    freeSpace: (gb) => `${gb} GB free`,
    FREE_UNKNOWN: 'Free space not known',

    INSTALL: 'Install',
    INSTALL_HINT: 'The Half-Life install this project is made with. Pick another in Configuration › Paths: opening the project while another one is picked asks whether to switch.',
    INSTALL_PICKED: 'Picked now',
    INSTALL_OTHER: 'Another install is picked',
    installOtherTitle: (picked) => `Picked now: ${picked}. Opening this project again asks whether to switch.`,
    INSTALL_NONE: 'Not recorded yet',

    OK: 'OK',
    OK_TITLE: 'Apply any changes and go back.',
    CANCEL: 'Cancel',
    CANCEL_TITLE: 'Drop the changes since the last Apply and go back.',
    APPLY: 'Apply',
    APPLY_NOTHING_TITLE: 'Nothing has changed yet.',
    fixFirstTitle: (problem) => `Fix this first: ${problem}`,

    LEAVE_TITLE: 'Apply the changes?',
    LEAVE_MESSAGE: 'Project settings has changes that are not applied yet.',
    LEAVE_APPLY: 'Apply',
    LEAVE_DISCARD: 'Discard',
    LEAVE_STAY: 'Stay',

    BAD_SIZE: 'Width and height must be whole numbers above 0.',
    BAD_RATE: 'Enter a whole number of frames a second above 0.',
    AGR_RATE_REQUIRED: 'Set AGR positions a second: an AGR capture will not start without it.',
    AGR_RATE_PLACEHOLDER: 'e.g. 30',
    BAD_SECONDS: 'Enter 0 or more seconds.',
    badKillGaps: (max) => `Gaps must be over 0 s, sped up more than 1x and at most ${max}x.`,
  },
  // ── Project settings › Clip settings (clip_settings_tab.js, #1085 step 8) ──
  CLIP_SETTINGS: {
    START_FROM_SUB: 'Pick a preset, then change anything below',
    PRESET: 'Preset',
    PRESET_HINT: 'Picking one copies its lines into this project. Nothing links back: changing or deleting the preset later leaves this project as it is.',
    NO_PRESET: 'No preset',
    NO_PRESET_TITLE: 'Empty every row and command: the game keeps its own settings.',
    BUILTIN_TITLE: 'Built in. Picking it copies its lines; the preset itself never changes.',
    presetEdited: (name) => `${name} (edited)`,
    SAVE_AS_NEW: '+ Save as new…',
    SAVE_AS_NEW_TITLE: 'Save these rows and commands as a preset on this PC, to pick in other projects.',
    SAVE_AS_NAME_PLACEHOLDER: 'Preset name',
    SAVE: 'Save',
    CANCEL_SAVE: 'Cancel',
    presetNameTaken: (name) => `${name} is a built-in preset. Pick another name.`,
    presetSavedToast: (name) => `Saved the preset ${name} on this PC.`,
    MANAGE_NOTE: 'Rename, change or delete a preset in Configuration › Commands.',

    NOT_SET: 'Not set',
    NOT_SET_TITLE: "Studio sends nothing for this, so the game keeps what its own Settings tab or your configs set.",
    OFF: 'Off',
    ON: 'On',
    typedValue: (value) => `${value} (typed)`,
    AT_LOAD: 'At map load',
    AT_LOAD_TITLE: 'The game reads this as a map loads, and these lines run once it has loaded: a change reaches the second demo of a batch, not the first.',
    pictureValue: (fov) => `${fov}°`,
    PICTURE_LINK: 'Set on the Project tab',
    PICTURE_LINK_TITLE: 'Field of view belongs to the whole project: Project › Picture.',
    SPRAYS_STATE: 'Follows Configuration › Players',
    SPRAYS_TITLE: "Sprays and crosshairs come from the Players table, which Studio sends itself. It moves to this page's Players and sprays tab in a later update.",
    badNumber: (min) => (min === undefined ? 'Enter a number.' : `Enter a number of ${min} or more.`),
    BAD_OFFSET: 'Each command needs 0 or more seconds.',

    OWN_COMMANDS_SUB: 'For anything not listed above',
    WHICH_WINS: 'Which wins',
    WHICH_WINS_HINT: 'Each runs after the one before, so later wins.',
    WHICH_WINS_ORDER: ['Game configs', 'Your own commands', 'Clip settings rows'],
    COMMANDS: 'Commands',
    COMMANDS_HINT: 'Run when each demo loads, one per line. A line that sets a row above moves into that row.',
    ADD_COMMAND: '+ Add a command',
    REMOVE_COMMAND_TITLE: 'Remove this command',
    NO_COMMANDS: 'None',
    AT_EACH_CLIP: 'At each clip',
    AT_EACH_CLIP_HINT: 'Runs around every clip in this project: Before that many seconds before the first kill, After that many after the last.',
    BEFORE: 'Before',
    AFTER: 'After',
    SECONDS: 's',

    SECTIONS: {
      'start-from': 'Start from',
      hud: 'HUD',
      view: 'View',
      spectating: 'Spectating',
      players: 'Players',
      map: 'Map',
      sound: 'Sound',
      'own-commands': 'Your own commands',
    },
    SECTION_SUBS: {
      hud: 'What is drawn over the picture',
      view: 'First-person demos only',
      spectating: 'HLTV demos only',
    },
    // [label, hint] by clip_settings_rows.js's row id.
    ROWS: {
      hud_draw: ['Show the HUD', "The game's own HUD"],
      draw_gun: ['Show the gun', 'The weapon in view'],
      hide_crosshair: ['Hide the crosshair', ''],
      hide_scoreboard: ['Hide the scoreboard', 'A recorded TAB press no longer covers the shot'],
      hide_teammate_info: ['Hide teammate name under the crosshair', 'First-person demos'],
      hide_voice_menus: ['Hide voice-command menus', ''],
      hide_hints: ['Hide the hint box', "And the machine gun's reload prompt"],
      show_scope: ['Show the sniper scope', ''],
      hide_map_text: ['Hide map text', 'Mortar warnings, round results'],
      hide_clan_text: ['Hide clan-match text', 'Warm-up rules, countdowns'],
      hide_center_text: ['Hide centre-screen text', ''],
      hide_plugin_text: ['Hide server plugin text', 'Coloured plugin messages, menus and votes'],
      fov: ['Field of view', ''],
      hide_red_screen: ['Hide the headshot red flash', ''],
      hide_screen_shake: ['Hide screen shake', 'Grenade blasts'],
      own_blood: ['Your own blood', 'Blood the game skips this close to the camera. All is blocky up close'],
      use_cl_bob: ['Bob the view like live play', 'Bobs by your own bob settings'],
      match_pov: ["Match the player's own view", "A spectated first-person view looks like the player's own recording"],
      player_info: ['Over each player', ''],
      deathcam: ['Death cam when the watched player dies', ''],
      hide_bars: ['Hide the spectator bars', ''],
      hide_camera_text: ['Hide the camera-mode text', ''],
      hide_hltv_messages: ['Hide HLTV messages', ''],
      sprays: ['Use my sprays and crosshairs', "Each player's spray and crosshair in your clips"],
      xray: ['X-ray', 'In team colours. First-person demos show only the players the recording carries'],
      hide_hand_signals: ['Hide hand signals', 'The nod, the point, the wave'],
      overview_players: ['Overview map icons', 'HLTV demos only'],
      overview_marker: ['Overview icon', 'The marker on the watched player'],
      overview_speaker: ['Hide the speaker icon on the overview', ''],
      hd_textures: ['HD textures', 'The style comes from the HD textures page'],
      weather: ['Weather', 'Needs cl_particlefx 2'],
      map_lighting: ['Use my map lighting', 'Brighter or darker spots you set per map'],
      map_shaders: ['Map shaders', '25th Anniversary build only'],
      map_cfgs: ['Run my map .cfg files', 'The .cfg files you keep for all maps and for each map'],
      decals: ['Decals kept on walls', 'More keeps more bullet holes and blood on walls. Clean clips keeps 256'],
      mute_voice: ['Mute voice commands', '"Fire in the hole!" and the rest'],
      mute_victory: ['Mute victory music', ''],
      mute_flags: ['Mute flag announcements', ''],
      restore_gunshots: ['Restore missing gunshots', 'Shots a demo left silent'],
      recording_volume: ['Recording volume', "HLAE's sound level in each take, 0 to 1. About 0.25 keeps gunfire from clipping"],
    },
    // Each select row's choices, in its values' order.
    CHOICES: {
      own_blood: ["The game's own", 'A little', 'Most', 'Nearly all', 'All'],
      player_info: ['Off', 'Name', 'Health', 'Name and health'],
      xray: ['Off', 'Through walls', 'Through walls and outlines'],
      overview_players: ["The game's own", 'Everyone', 'The other team', 'The watched player', 'Nobody'],
      overview_marker: ["The game's own", 'Their team icon in HLTV too', 'None'],
      weather: ["The map's own", 'Rain', 'Snow', 'None'],
    },
  },

  // After a .json project loads (#528, project_convert.js).
  PROJECT_CONVERT: {
    TITLE: 'Switch to .dodstudio?',
    MESSAGE: 'Switch this project to the .dodstudio file type? It will open in DoD Studio when double-clicked. The file is renamed in its folder; nothing inside it changes.',
    SWITCH: 'Switch to .dodstudio',
    KEEP: 'Keep as .json',
    switchedToast: (name) => `Project switched to ${name}.`,
    failedToast: (err) => `The project stays as .json: ${err}`,
  },
  MAIN: {
    SELECT_CAPTURE_OUTPUT_DIR_TITLE: 'Select Capture Output Directory',
    SELECT_RENDER_EXPORT_DIR_TITLE: 'Select Render Export Directory',
    SAVE_PROJECT_SESSION_TITLE: 'Save Studio Project Session',
    SELECT_HLAE_EXE_TITLE: 'Select HLAE Executable (hlae.exe)',
    SELECT_HL_EXE_TITLE: 'Select Half-Life Executable (hl.exe)',
    SELECT_FFMPEG_EXE_TITLE: 'Select FFmpeg Executable (ffmpeg.exe)',
    SELECT_GOLDSRC_HOOKS_DLL_TITLE: 'Select GoldSrc Hooks DLL (dodstudio_goldsrc_hooks.dll)',
    SELECT_OBS_EXE_TITLE: 'Select OBS Executable (obs64.exe)',
    SELECT_DEMO_FILES_TITLE: 'Select Demo Files (.dem)',
    SELECT_DEMO_FOLDER_TITLE: 'Select Demo Folder',

    PROJECT_FILTER_NAME: 'DoD Studio Project',
    JSON_PROJECT_FILTER_NAME: 'JSON Project File',
    EXECUTABLE_FILTER_NAME: 'Executable',
    DLL_FILTER_NAME: 'DLL',
    CONFIG_FILTER_NAME: 'Config File',
    SELECT_MOVIE_CFG_TITLE: 'Select Movie Config',
    DEMO_FILES_FILTER_NAME: 'Demo Files',

    NOTHING_TO_SAVE: 'Nothing to save yet — add demo files or load a session first.',
    ALREADY_SAVED: 'Already saved — no changes since the last save.',
    projectSavedToast: (path) => `Project session saved successfully to ${path}`,
    NEW_SESSION_TOAST: 'Started a new session.',
    SAVE_PROJECT_ERROR: 'Error saving project session.',
    loadedDemosToast: (count) => `Loaded ${count} demos from project file`,
    // #21: demos a loaded project names that are no longer where it says.
    RELOCATE_DEMOS_TITLE: 'Demos have moved',
    RELOCATE_DEMOS_MESSAGE: 'These demos aren\'t at their saved location any more, but a matching file (same size, same start) was found for each. Hover one for its full path.',
    RELOCATE_DEMOS_QUESTION: 'Use the new locations?',
    relocateRenamed: (oldName, newName) => `${oldName} → ${newName}`,
    relocateFolder: (folder) => `now in ${folder}`,
    RELOCATE_CONFIRM: 'Use new locations',
    RELOCATE_CANCEL: 'Leave as missing',
    relocatedDemosToast: (count) => `Updated the location of ${count} moved demo(s).`,
    LOCATE_DEMO_DIALOG_TITLE: 'Where is this demo now?',
    LOCATE_MISMATCH_TITLE: 'Different file',
    locateMismatchMessage: (name, picked) =>
      `${picked} isn't the ${name} that was scanned (its size or start is different), so this row's highlights won't line up with it.\n\nReplace the row with ${picked}? It's scanned fresh with its own highlights, and this row's statuses and notes are dropped.`,
    LOCATE_MISMATCH_CONFIRM: 'Replace with this demo',
    locateAlreadyQueued: (name) => `That file is already in the queue as ${name}. Pick this demo's own file, or remove one of the two rows first.`,
    missingDemosToast: (names) => {
      const shown = names.slice(0, 3).join(', ');
      const more = names.length > 3 ? ` and ${names.length - 3} more` : '';
      return `${names.length} demo(s) in this project could not be found: ${shown}${more}. Their highlights can't be captured until they're back.`;
    },
    leftMissingToast: (names) => {
      const shown = names.slice(0, 3).join(', ');
      const more = names.length > 3 ? ` and ${names.length - 3} more` : '';
      return `Left ${names.length} moved demo(s) as missing: ${shown}${more}. Each row has a Use found copy button.`;
    },
    USE_ALL_FOUND_COPIES: 'Use all found copies',
    foundCopiesGoneToast: (names) =>
      `No longer where it was found: ${names.join(', ')}. Use Locate… to pick it.`,
    // #21: a scanned demo that is an identical copy of one already queued.
    identicalCopiesToast: (pairs) => {
      const shown = pairs.slice(0, 3).map(([copy, original]) => `${copy} (same as ${original})`).join(', ');
      const more = pairs.length > 3 ? ` and ${pairs.length - 3} more` : '';
      return `Skipped ${pairs.length} identical cop${pairs.length === 1 ? 'y' : 'ies'} of a demo already in the queue: ${shown}${more}.`;
    },
    IDENTICAL_COPIES_TITLE: 'Already in the queue',
    IDENTICAL_COPIES_MESSAGE: "These files are identical copies of demos already in the queue, under another name. They weren't added as new rows, which would capture every highlight twice.",
    IDENTICAL_COPIES_QUESTION: 'Point those rows at these files instead? They keep their highlights, statuses and notes.',
    IDENTICAL_COPIES_SWITCH: 'Use these files',
    IDENTICAL_COPIES_KEEP: 'Keep the queued files',
    identicalCopyQueuedMissing: (folder) => `in ${folder} (the queued file is missing)`,
    IDENTICAL_COPY_SAME_FOLDER: 'in the same folder as the queued file',
    identicalCopyOthers: (names) => ` · other copies, not added: ${names.join(', ')}`,
    PICKED_COPIES_TITLE: 'Identical copies picked',
    PICKED_COPIES_MESSAGE: "Some of the files you picked are identical copies of each other under different names. One of each was added, the one with the shortest name, so no highlight is captured twice.",
    PICKED_COPIES_OK: 'OK',
    pickedCopiesSkipped: (names) => `not added: ${names.join(', ')}`,
    identicalCopyFolder: (folder) => `in ${folder}`,
    // #21: demos whose file changed on disk after they were scanned.
    CHANGED_DEMOS_TITLE: 'Demos have changed',
    CHANGED_DEMOS_MESSAGE: "These demos aren't the files they were scanned from (their size or start is different), so their highlights won't line up. Capture didn't start.",
    CHANGED_DEMOS_QUESTION: "Rescan them now? Their highlights are replaced by the new scan; marks, notes, status and kill ranges go with any highlight found again.",
    CHANGED_DEMOS_RESCAN: 'Rescan',
    // Opening a project with demos an older DoD Studio scanned (scan_version.js).
    OUTDATED_DEMOS_TITLE: 'Demos scanned by an older DoD Studio',
    // `n` of the project's `total` demos; "All 158" when it is every one.
    // `time`: how long it takes (rescan_estimate.js), or nothing when that
    // couldn't be worked out.
    outdatedDemosMessage: (n, total = n, time = '') => {
      const which = n < total ? `${n}/${total} demos` : (n === 1 ? '1 demo' : `All ${n} demos`);
      return `${which} in this project ${n === 1 ? 'was' : 'were'} scanned with an older DoD Studio. Rescan ${n === 1 ? 'it' : 'them'} now? Your marks, notes, status and kill ranges are carried over. `
        + (time ? `${time} ` : '')
        + `Press “Not now” to leave ${n === 1 ? 'it' : 'them'} as ${n === 1 ? 'it is' : 'they are'}; you'll be asked again the next time you open the project.`;
    },
    // How long the rescan takes (#937): `duration` is "3 s", "4 min", "an hour".
    rescanTimeLine: (duration, fullReads) => (fullReads > 0
      ? `About ${duration}: ${fullReads} need${fullReads === 1 ? 's' : ''} a full read.`
      : `About ${duration}.`),
    RESCAN_TIME_PENDING: 'Working out how long…',
    rescanEstimateFailedLog: (err) => `[rescan] could not work out how long the rescan takes: ${err}`,
    scanReadRateLog: (measured, rate) => `[scan] full reads went at ${measured} MB/s; this PC's read rate for rescan estimates is now ${rate} MB/s`,
    OUTDATED_DEMOS_RESCAN: 'Rescan',
    OUTDATED_DEMOS_NOT_NOW: 'Not now',
    // What a rescan did with the highlights' work (rescan_carry.js).
    rescanCarryToast: (kept, total) => kept === total
      ? `Rescan kept the marks, notes, status and kill ranges of all ${total} highlight${total === 1 ? '' : 's'} with work on them.`
      : `Rescan kept the work on ${kept} of ${total} highlights. The other ${total - kept} couldn't be found in the new scan; their marks and notes are listed in the activity log (View Logs).`,
    rescanCarryLog: (kept, total, demos) => `[rescan] kept the work on ${kept} of ${total} highlight(s) across ${demos} demo(s)`,
    rescanDemoLog: (name) => `[rescan] ${name}:`,
    rescanNarrowedLog: (time, player) => `[rescan]   ${time} ${player}: the new scan splits its Kill Range over two highlights; kept on the one holding most of its kills`,
    rescanUncarriedLog: (time, player, timeline, reason, work) => `[rescan]   NOT KEPT ${time} ${player} (${timeline}): ${reason}. ${work}`,
    RESCAN_UNCARRIED_REASON: {
      'no-piece': 'the demo did not scan',
      'not-found': 'no highlight in the new scan has the same player and kills',
      ambiguous: 'the same player and kills fit more than one highlight',
      taken: 'another highlight\'s work went to the same highlight',
    },
    // #217: splitting a queued demo that recorded more than one map.
    queueSplitDone: (name, names) => `Split ${name} into ${names.length} demo${names.length === 1 ? '' : 's'}: ${names.join(', ')}`,
    // What happened to the original's highlight work (split_carry.js).
    splitCarryToast: (name, kept, total) => kept === total
      ? `${name}: kept the marks, notes, status and kill ranges of all ${total} highlight${total === 1 ? '' : 's'} with work on them.`
      : `${name}: kept the work on ${kept} of ${total} highlights. The other ${total - kept} couldn't be found in the split demos; their marks and notes are listed in the activity log (View Logs).`,
    splitCarryLog: (name, kept, total, pieces) => `[split] ${name} -> ${pieces.join(', ')}: kept the work on ${kept} of ${total} highlight(s)`,
    splitNarrowedLog: (time, player, piece) => `[split]   ${time} ${player}: its Kill Range spans two highlights in ${piece}; kept on the one holding most of its kills`,
    splitUncarriedLog: (time, player, timeline, reason, work) => `[split]   NOT KEPT ${time} ${player} (${timeline}): ${reason}. ${work}`,
    SPLIT_UNCARRIED_REASON: {
      'no-piece': 'its map was not written as a demo of its own (shorter than a minute), or the piece did not scan',
      'not-found': 'no highlight in the split demo has the same player and kills',
      ambiguous: 'the same player and kills fit more than one highlight',
      taken: 'another highlight\'s work went to the same highlight',
    },
    splitWorkText: ({ curation, status, picked, notes, clipName, range }) => [
      curation && `Review: ${curation}`,
      status && `Status: ${status}`,
      picked && 'Picked',
      range && `Kill Range: ${range}`,
      clipName && `Clip name: "${clipName}"`,
      notes && `Note: "${notes}"`,
    ].filter(Boolean).join('; '),
    queueSplitFailed: (name, err) => `Could not split ${name}: ${err}`,
    // The queue's line while a split runs (#951).
    queueSplitHead: (name) => `Splitting ${name}`,
    QUEUE_SPLIT_ADDING: 'adding its maps to the queue',
    queueSplitDoneLine: (elapsed) => `Done in ${elapsed}: split 1 demo`,
    // #217: the activity log's lines when picks are in demos with several
    // maps: one per demo naming what is captured and what is left out, or
    // one count when there are more than queue_split.js's FIRST_MAP_LINES_MAX.
    captureFirstMapOnlyDemo: (name, maps) => {
      const rest = maps.slice(1);
      return `[capture] ${name} recorded ${maps.length} maps: capturing ${maps[0]} only; `
        + `${rest.join(', ')} ${rest.length === 1 ? 'is' : 'are'} left out (split the demo to capture ${rest.length === 1 ? 'it' : 'them'})`;
    },
    captureFirstMapOnly: (n) => `[capture] Capturing only the first map of ${n} demos that recorded more than one map; `
      + 'split them to capture the maps after it',
    RELOCATE_CANCEL_PLAIN: 'Cancel',
    LOAD_PROJECT_ERROR: 'Error loading project session.',

    // The queue's progress line through a scan (#951): queue_progress.js.
    SCAN_STARTING: 'Looking for demos…',
    // Opening a project, on the queue's line (#958).
    OPENING_PROJECT: 'Opening the project…',
    CHECKING_PROJECT_DEMOS: "Checking the project's demos are where they were…",
    // The line once it has opened, so the last scan's "Done in…" doesn't come back.
    projectOpenedLine: (n) => `Opened the project: ${n} demo${n === 1 ? '' : 's'}`,
    lookingForMissingDemos: (n) => `Looking for ${n} missing demo${n === 1 ? '' : 's'}…`,
    lookingLine: (folders, demos, last) => {
      const n = (x) => Number(x || 0).toLocaleString('en-US');
      const was = last > 0 ? ` (about ${n(last)} last time)` : '';
      return `Looking for demos: ${n(folders)} folder${folders === 1 ? '' : 's'}${was}, ${n(demos)} demo${demos === 1 ? '' : 's'} found`;
    },
    // Then 64 KB of each, to skip demos already queued and identical copies.
    keyingLine: (done, total) => {
      const n = (x) => Number(x || 0).toLocaleString('en-US');
      return `Checking which demos are new: ${n(done)} of ${n(total)}`;
    },
    scanDoneLine: (elapsed, { added = 0, rescanned = 0, unchanged = 0 } = {}) => {
      const parts = [
        added > 0 && `${added} demo${added === 1 ? '' : 's'} added`,
        rescanned > 0 && `${rescanned} rescanned`,
        unchanged > 0 && `${unchanged} already in the queue`,
      ].filter(Boolean);
      return `Done in ${elapsed}: ${parts.length ? parts.join(', ') : 'no demos found'}`;
    },
    scanCancelledLine: (elapsed, added) => `Cancelled after ${elapsed}: ${added > 0 ? `${added} demo${added === 1 ? '' : 's'} added` : 'nothing added'}`,
    // Appended to the two lines above when the scan skipped demos (#23).
    skippedStatusSuffix: (n) => (n > 0 ? `, ${n} could not be read` : ''),
    // `skipped` is [{name, reason}]; names the first three, counts the rest.
    skippedDemosToast: (skipped) => {
      const shown = skipped.slice(0, 3).map((s) => `${s.name} (${s.reason})`).join('; ');
      const more = skipped.length > 3 ? `; and ${skipped.length - 3} more` : '';
      return `${skipped.length} demo(s) could not be read and were skipped: ${shown}${more}`;
    },
    statusGeneric: (status) => `Status: ${status}`,
    SCAN_CANCELLING_STATUS: 'Cancelling…',
    SCANNING_PLEASE_WAIT_ROW: 'Scanning... please wait.',
    scanCompleteToast: (count, unchanged = 0) =>
      unchanged > 0
        ? `Scan complete (${count} new or changed demo(s) found, ${unchanged} already in the queue and unchanged)`
        : `Scan complete (${count} demo(s) found)`,
    // #531: the folder walk's bounds stopped it short; `cap` is the scan's.
    scanCapToast: (cap) => {
      const parts = [];
      if (cap.depth) parts.push(`folders more than ${cap.max_depth} levels down were not read`);
      if (cap.entries) parts.push(`the walk stopped after ${Number(cap.max_entries).toLocaleString('en-US')} files and folders`);
      return `Some demos may be missing: ${parts.join(', and ')}. Add a folder further down to scan the rest.`;
    },
    scanCancelledToast: (count) =>
      count > 0
        ? `Scan cancelled (${count} demo(s) added before cancel)`
        : 'Scan cancelled, nothing added',
    scanErrorToast: (err) => `Error: ${err}`,
    scanErrorStatus: (err) => `Could not scan: ${err}`,

    EXPORT_POOL_FREE_DEFAULT: 'Capture Output Free: 0.0 GB',
    exportPoolFree: (gb) => `Capture Output Free: ${gb} GB`,
    EXPORT_POOL_ERROR: 'Capture Output Free: Error calculating space',

    QUEUE_ALREADY_EMPTY: 'Queue is already empty.',
    NO_DEMOS_MATCH_SEARCH: 'No demos match the current search.',
    filterScopeNote: (visible, total) => ` (search filter active — only considered ${visible} of ${total} demo(s) in the queue)`,
    NOTHING_TRACKED_TO_CLEAR: 'Nothing to clear — every visible demo has tracked work on it.',
    removedUntrackedToast: (count, keptNote, scopeNote) => `Removed ${count} untracked demo(s)${keptNote}.${scopeNote}`,
    keptWithTrackedWork: (count) => `, kept ${count} with tracked work`,
    clearUntrackedLog: (count, keptNote, scopeNote, names) => `[queue] Clear Untracked: removed ${count} demo(s)${keptNote}.${scopeNote} — ${names}`,

    CLEAR_SELECTED_TITLE: 'Clear Selected Demos',
    CLEAR_ALL_TITLE: 'Clear All Demos',
    REMOVE_TRACKED_DEMO_TITLE: 'Remove Tracked Demo',
    CLEAR_SELECTED_ANYWAY: 'Clear Selected Anyway',
    CLEAR_ALL_ANYWAY: 'Clear All Anyway',
    REMOVE_ANYWAY: 'Remove Anyway',
    CLEAR_ANYWAY_DEFAULT: 'Clear Anyway',
    DEMO_SINGULAR: 'demo',
    DEMO_PLURAL: 'demos',
    VERB_REMOVES: 'removes',
    clearSummaryTracked: (verb, count, plural, trackedCount) => `This ${verb} ${count} ${plural} — ${trackedCount} of them have tracked work (a Pending/Captured/Rendered status, a note, or an edited kill range) that will be lost. This cannot be undone.`,
    clearSummaryUntracked: (verb, count, plural) => `This ${verb} ${count} ${plural}. None currently have tracked work on them. This cannot be undone.`,

    NO_DEMOS_SELECTED: 'No demos selected — check rows in the queue first.',
    allSelectedHiddenToast: (count) => `All ${count} selected demo(s) are hidden by the current search — nothing visible to remove.`,
    hiddenCheckedNote: (count) => ` (${count} other selected demo(s) hidden by the search filter were left untouched)`,
    removeSelectedConfirm: (count, hiddenNote) => `Remove ${count} selected demo(s) from the queue?${hiddenNote}`,
    removedSelectedToast: (savedFirst, count, hiddenNote) => `${savedFirst ? 'Saved, then removed' : 'Removed'} ${count} demo(s) from the queue.${hiddenNote}`,
    clearSelectedLog: (count, savedNote, hiddenNote, names) => `[queue] Clear Selected: removed ${count} demo(s)${savedNote}.${hiddenNote} — ${names}`,
    removeAllConfirm: (count, note) => `Remove ${count} demo(s) from the queue? None have tracked work on them.${note}`,
    clearedAllToast: (savedFirst, count, note) => `${savedFirst ? 'Saved, then cleared' : 'Cleared'} ${count} demo(s) from the queue.${note}`,
    clearAllLog: (count, savedNote, note, names) => `[queue] Clear All: removed ${count} demo(s)${savedNote}.${note} — ${names}`,
    SAVED_SESSION_FIRST_NOTE: ' (saved session first)',
  },
  // ── rescan_recent.js: every recent session's outdated demos in one go (#938)
  RESCAN_RECENT: {
    TITLE: 'Rescan recent sessions',
    MESSAGE: 'These sessions have demos an older DoD Studio scanned. Rescan them all now? Each session is backed up first (a .before-rescan copy beside it), then rescanned and saved. Marks, notes, status and kill ranges are carried over.',
    SKIP_HINT: 'This takes a while: “Skip for now” leaves a session until you next open it.',
    RESCAN_ALL: 'Rescan all',
    NOT_NOW: 'Not now',
    CANCEL: 'Cancel',
    SKIP: 'Skip for now',
    SKIPPED: 'Skipped',
    TIME_PENDING: 'working out how long…',
    ALL_CURRENT_TOAST: 'Every recent session is up to date.',
    NONE_RECENT_TOAST: 'No recent sessions to rescan.',
    BUSY_TOAST: 'Recent sessions are already being rescanned.',
    READING: 'Reading recent sessions…',
    demos: (n) => `${n} demo${n === 1 ? '' : 's'}`,
    MISSING: 'file not found, skipped',
    UNREADABLE: "couldn't be read, skipped",
    total: (duration) => `Total: about ${duration}.`,
    // The progress line: which session, which step.
    step: (i, n, name, what) => `${name} (${i} of ${n}): ${what}`,
    STEP_BACKUP: 'backing up…',
    STEP_OPEN: 'opening…',
    stepScan: (n) => `rescanning ${n} demo${n === 1 ? '' : 's'}…`,
    STEP_SAVE: 'saving…',
    STEP_REOPEN: 'Reopening the session you had open…',
    STOPPING: 'stopping after this session…',
    doneToast: (done, total, failed) => `Rescanned ${done} of ${total} session${total === 1 ? '' : 's'}`
      + (failed > 0 ? `; ${failed} failed (see View Logs).` : '.'),
    // Activity log, one line per session, like #926's [scan] lines.
    doneLog: (name, demos, elapsed, backup) => `[rescan] ${name}: ${demos} demo(s) rescanned in ${elapsed} and saved (backup: ${backup})`,
    failedLog: (name, elapsed, err) => `[rescan] ${name}: failed after ${elapsed}, not saved: ${err}`,
    skippedLog: (name) => `[rescan] ${name}: skipped for now`,
    missingLog: (path) => `[rescan] ${path}: file not found, skipped`,
    noDemosLog: (name) => `[rescan] ${name}: none of its outdated demos are on disk, nothing to rescan`,
    stoppedLog: (left) => `[rescan] stopped by Cancel; ${left} session(s) left as they were`,
    OPEN_FAILED: "the session didn't open",
    SCAN_FAILED: 'the scan failed',
    SAVE_FAILED: "the session wasn't saved",
    SESSION_CHANGED: 'another session was opened while it scanned',
    SCAN_RUNNING_TOAST: 'A scan is running: rescan recent sessions once it ends.',
  },
};
