// strings/highlights.js: the app's text for highlights, clip names, players and review.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

import { WORKSPACE_STRINGS } from './workspace.js';

// One wording for "the search stopped early", defined with the queue's text (#1376).
const SEARCH_STOPPED_NOTE = WORKSPACE_STRINGS.MAIN.SEARCH_STOPPED_NOTE;

export const HIGHLIGHTS_STRINGS = {
  // ── Highlight Details: every capture of a highlight (#439) ───────────────
  CAPTURE_HISTORY: {
    TOGGLE_TITLE: 'Show or hide every time this highlight was captured, with its take and its renders',
    CHECKING: 'Checking the takes…',
    CHECK_FAILED: 'Could not look at the takes on disk.',
    NO_DATE: 'Captured earlier (date not recorded)',
    NO_SETTINGS: 'settings not recorded',
    take: (name) => `Take ${name}`,
    TAKE_GONE: 'take deleted',
    TAKE_GONE_TITLE: 'The take folder is not there any more. This line stays as the record of the capture.',
    TAKE_FOLDER_UNKNOWN: 'folder not recorded',
    TAKE_FOLDER_UNKNOWN_TITLE: 'This capture was recorded before Studio kept the take folder, so it cannot be checked.',
    OPEN_FOLDER: 'Open Folder',
    OPEN_FOLDER_TITLE: 'Show the take folder in Explorer',
    OPEN_VIDEO: 'Show video',
    OPEN_VIDEO_TITLE: 'Show the rendered video in Explorer',
    sharedWith: (labels) => `Shared with ${labels.join(', ')}`,
    SHARED_TITLE: 'Recorded together in one take',
    otherHighlight: (time, player) => (player ? `${player} at ${time}` : `highlight at ${time}`),
    UNKNOWN_HIGHLIGHT: 'a highlight no longer in the queue',
    NO_RENDERS: 'not rendered',
    REVEAL_FAILED: 'Could not show that in Explorer.',
    // The full settings a take was captured with (part of #1085).
    capturedWith: (text) => `Captured with: ${text}`,
    NOT_RECORDED: 'not recorded',
    NOT_RECORDED_TITLE: 'This take was captured before Studio kept the full settings of each capture. It is never marked as different from another take.',
    size: (width, height) => `${width}×${height}`,
    fov: (degrees) => `FOV ${degrees}`,
    hook: (name) => `hook ${name}`,
    NO_HOOK: 'no hook DLL',
    clipLength: (before, after) => `${before} s before, ${after} s after`,
    gapsFastForward: (speed, over) => `gaps over ${over} s at ${speed}x`,
    commandCounts: (initial, scheduled) =>
      `${initial} Initial ${initial === 1 ? 'Command' : 'Commands'}, ${scheduled} Scheduled`,
    INITIAL_HEADING: 'Initial Commands:',
    SCHEDULED_HEADING: 'Scheduled Commands:',
    NONE: '  (none)',
    initialLine: (line) => `  ${line}`,
    scheduledCommand: (after, seconds, command) => `  ${seconds} s ${after ? 'after the clip' : 'before the clip'}: ${command}`,
    hash: (hash, version) => `Settings hash ${hash}${version ? ` · DoD Studio ${version}` : ''}`,
  },
  // ── Highlight Details (detail_pane.js) + Advanced Diagnostics ───────────
  HIGHLIGHTS: {
    // #441: the name the finished file gets.
    COL_CLIP_NAME: 'Clip Name',
    CLIP_NAME_INPUT_TITLE: 'The finished file is named after this. The grey name is the automatic one, from the template in Configuration > Render Output. Type your own to use it instead; clear it to go back.',
    // The Studio sub-tabs (#117): pick the highlights, render the takes, set up.
    SUBTAB_HIGHLIGHTS: 'Highlights',
    SUBTAB_RENDER: 'Render',
    SUBTAB_CONFIGURATION: 'Configuration',
    DEFAULT_TITLE: 'Highlight Details',
    detailTitle: (name) => `Highlight Details: ${name}`,
    LAUNCH_PREVIEW_BUTTON: 'Launch Preview',
    LAUNCH_PREVIEW_TITLE: "Opens this demo in the game with viewdemo, to jump between its highlights. Your Initial and Scheduled Commands don't run in a preview: only a capture batch runs them.",
    LAUNCHING: 'Launching…',
    VIEW_TELEMETRY_BUTTON: 'View Match Telemetry',
    SELECT_ALL_CB_TITLE: 'Select/deselect all visible highlights',
    LAUNCH_STANDALONE_BUTTON: 'Launch Game',
    LAUNCH_STANDALONE_TITLE: "Starts the game with DoD Studio, no demo loaded. Previews, reviews and capture batches use this game while it's open. Only close it when Studio asks.",
    EMPTY_SELECT_DEMO: 'Select a demo in the Master List to view its highlights.',
    EMPTY_NO_STREAKS: 'No highlights detected in this demo.',
    COL_ROW_NUM: 'Row #',
    COL_SEL: 'Sel',
    COL_KILL_RANGE: 'Kill Range',
    COL_KILLS: 'Kills',
    // #1132: chips in the Kills cell for what was going on at the flags (flag_tags.js).
    FLAG_TAG_LABELS: {
      while_capping: 'Capping',
      flag_defence: 'Flag defence',
      cleared_flag: 'Cleared the flag',
    },
    FLAG_TAG_TITLES: {
      while_capping: 'The player was standing in a flag\'s capture area, capping it, at this kill. The demo only says this for the player who recorded it.',
      flag_defence: 'The victim was standing in a flag\'s capture area, capping it, at this kill. The demo only says this when the victim recorded it.',
      cleared_flag: 'This kill stopped a timed capture: one of the cappers died and the capture bar went away within about a second. Told from the timing, so once in a while a kill elsewhere gets it.',
    },
    flagTagCount: (label, count) => `${label} ×${count}`,
    COL_HEADSHOTS: 'HS',
    COL_HEADSHOTS_TITLE: 'Headshots among the kills in the Kill Range, from the helmet that flies off in the demo. A + ? means some kills could not be told: two kills on one frame, a British victim, whose helmet may not pop, or in an HLTV demo any kill with no helmet seen, since HLTV demos lose about half of them. A dash: scan the demo again.',
    // #247: an HLTV demo lists every player's highlights; a capture follows that player.
    COL_PLAYER: 'Player',
    COL_PLAYER_TITLE: 'Whose highlight. An HLTV demo lists every player\'s, and its capture keeps the camera on that player.',
    PLAYER_FILTER_LABEL: 'Player:',
    PLAYER_FILTER_TITLE: 'Show one player\'s highlights. An HLTV demo lists everyone\'s; the pick carries over to the next HLTV demo that has the same name.',
    playerFilterAll: (n) => `All players (${n})`,
    playerFilterOption: (name, n) => `${name} (${n})`,
    COL_TIME: 'Time',
    COL_DUR: 'Dur.',
    // The narrow layout stacks Time and Dur. (and Player, in an HLTV demo) in one column (#46).
    COL_STACK: 'Time · Dur.',
    COL_STACK_PLAYER: 'Player / Time · Dur.',
    // The stacked cell's second line, and its player line's tooltip (the name
    // is cut to fit, so the tooltip carries the name and the weapon chain).
    stackedWhen: (time, dur) => `${time} · ${dur}`,
    stackedPlayerTitle: (player, chain) => (chain ? `${player} — ${chain}` : player),
    // #44: an editorial mark, separate from Status.
    COL_REVIEW: 'Review',
    COL_REVIEW_TITLE: 'Your call on each highlight: Keep, Maybe (look again later), or Skip. Skip unticks the row and locks it out of every capture batch.',
    CURATION_UNREVIEWED: '–',
    CURATION_KEEP: 'Keep',
    CURATION_MAYBE: 'Maybe',
    CURATION_SKIP: 'Skip',
    SKIPPED_CB_TITLE: 'Marked Skip in the Review column, so it can\'t be captured. Set Review back to use it.',
    COL_STATUS: 'Status',
    COL_NOTES: 'Notes',
    COL_DETAILS: 'Details',
    NOTES_PLACEHOLDER: 'Add note...',
    KR_RESET_TITLE: 'Reset to full range',
    fallbackKillCount: (count) => `${count} kills`,
    STATUS_OPTIONS: ['None', 'Pending', 'Captured', 'Rendered'],
    // What an untouched highlight (streak.status still undefined) displays
    // and counts as. 'Pending' is now a deliberate, user-set flag ("capture
    // this one later") rather than the implicit default every unset row
    // used to show — see isHighlightTracked's doc comment (take_index.js).
    STATUS_UNSET_DEFAULT: 'None',
    // A status picked from the dropdown rather than set by a verified
    // capture or render (#105).
    STATUS_BY_HAND_MARK: '✎',
    STATUS_BY_HAND_TITLE: 'Set by hand. The next capture or render that DoD Studio checks on disk replaces this mark.',
    statusSetToast: (status) => `Status set to ${status}.`,
    UNDO: 'Undo',
    mergedTakeBadge: (takeName) => `merged → ${takeName}`,
    mergedBadgeTitle: (mergedCount) => `Merged with ${mergedCount - 1} other highlight(s) into one take — they were recorded together and share this take folder.`,
    HLAE_PATH_REQUIRED: 'Set the HLAE and Half-Life executable paths in Configuration → Paths before previewing.',
    PREVIEW_LAUNCHING_TOAST: 'Preview launching in HLAE...',
    copiedViewCommand: (cmd) => `Copied "${cmd}" to clipboard.`,
    sentToRunningGame: (cmd) => `Sent "${cmd}" to the running game.`,
    COPY_VIEW_COMMAND_FAILED: 'Failed to copy the view command to clipboard.',
    LAUNCHING_HLAE_TOAST: 'Launching HLAE...',
  },
  // ── camera_data.js: demos with no camera data (#920) ───────────────────
  CAMERA_DATA: {
    BADGE: 'no camera',
    BADGE_TITLE: 'This demo has no camera data: none of its frames holds the recorder\'s view, so it plays as a frozen camera with no gun. '
      + 'Kills and the scoreboard still read fine. It is usually a bad copy; if there is another copy of the demo, use that one.',
    batchTitle: (count) => `${count} demo${count === 1 ? ' has' : 's have'} no camera data`,
    batchMessage: (count) => `${count === 1 ? 'This demo has' : 'These demos have'} no camera data, so ${count === 1 ? 'its clips come' : 'their clips come'} out as a frozen camera with no gun. Start the batch anyway?`,
    PREVIEW_TITLE: 'This demo has no camera data',
    PREVIEW_MESSAGE: 'None of its frames holds the recorder\'s view, so it plays as a frozen camera with no gun. Launch it anyway?',
    DETAIL: 'No camera data: plays as a frozen camera',
    ANALYZER_NOTICE: 'No camera data: none of this demo\'s frames holds the recorder\'s view, so it plays and captures as a frozen camera with no gun. '
      + 'The kills and scoreboard here are fine. It is usually a bad copy; if there is another copy of the demo, use that one.',
    START_ANYWAY: 'Start anyway',
    PREVIEW_ANYWAY: 'Launch anyway',
  },
  // ── Review highlights (#623) ─────────────────────────────────────────────
  REVIEW: {
    BUTTON: 'Review highlights',
    TITLE: 'Play every highlight of the ticked demos in the game, one after another. With none ticked, every demo the list shows (after its search and filters) is reviewed. Answer each in the game with Y (Yes), M (Maybe) or N (No); R replays it, B goes back, Space goes on without answering, S stops the review, ESC opens the DoD Studio window\'s Review tab for the kill range and note. Yes marks the row Keep, Maybe marks it Maybe (look again later), No marks it Skip.',
    // #661: which Review marks a review plays.
    COVERS_TITLE: 'Which highlights the review plays, by their Review mark, always from the first. All plays every highlight, answered or not; Unmarked picks up where an earlier review left off; Keep, Maybe or Skip go back over those; Ticked plays only the highlights ticked for capture, a last look before a batch.',
    COVERS_ALL: 'All',
    COVERS_UNMARKED: 'Unmarked',
    COVERS_KEEP: 'Keep',
    COVERS_MAYBE: 'Maybe',
    COVERS_SKIP: 'Skip',
    COVERS_TICKED: 'Ticked',
    // #822: whose highlights an HLTV demo's review plays.
    HLTV_LABEL: 'HLTV:',
    HLTV_TITLE: 'An HLTV demo lists every player\'s highlights. Pick whose to review: by default, the players who recorded POV demos in the queue but not this half (the HLTV demo stands in for their missing recording); or everyone with a POV demo in the queue (your team, usually); everyone; any players you pick from the HLTV demos (a whole team at once from the Teams list), kept in the project; or one player. The game puts the camera on that player in first person for each one. An HLTV demo whose half isn\'t known yet reviews everyone with a POV demo in the queue.',
    HLTV_MISSING: 'Players missing their POV for this half',
    HLTV_RECORDERS: 'Players with a POV demo in the queue',
    HLTV_ALL: 'Everyone',
    // #828: any players from the HLTV demos, picked beside the box.
    HLTV_PICKED: 'Players I pick',
    PICK_ADD_PLAYER: 'Add a player…',
    PICK_ADD_PLAYER_TITLE: 'Everyone with a highlight in the queue\'s HLTV demos, one line per player with every name they used.',
    PICK_NO_PLAYERS: 'No HLTV highlights in the queue',
    PICK_ADD_TEAM: 'Add a team…',
    PICK_ADD_TEAM_TITLE: 'Adds every player of a team from the Teams list: whoever played on its side in an HLTV demo, or has its tag in their name.',
    // Short enough for the list's fixed 160 px; its tooltip says where teams come from.
    PICK_NO_TEAMS: 'No teams',
    pickTeamOption: (name, count) => `${name} (${count} player${count === 1 ? '' : 's'})`,
    PICK_NONE: 'Nobody picked yet',
    PICK_REMOVE: 'Take this player out',
    PICK_NOT_IN_QUEUE_TITLE: 'No highlights in the queue\'s HLTV demos. Kept in the pick for when their demo is back.',
    ORDER_TITLE: 'The order an HLTV demo\'s highlights play in when they belong to several players. Chronological: as the match went, the camera switching player between highlights (two starting at the same moment go A to Z). By player: every highlight of one player, then the next player\'s from the start, players A to Z. The game seeks back as easily as forward.',
    ORDER_TIME: 'Chronological',
    ORDER_PLAYER: 'By player (A–Z)',
    // The HLTV box left out every highlight there was (#822).
    hltvNoneChosen: (choice, count, pickedCount = 0) => {
      const n = `${count} HLTV highlight${count === 1 ? '' : 's'}`;
      if (choice === 'picked' && pickedCount === 0) {
        return `Nothing to review: HLTV is set to Players I pick and nobody is picked, so the ${n} were left out. Add players or a team beside the HLTV box, or set HLTV to Everyone.`;
      }
      if (choice === 'missing' || !choice) {
        return `Nothing to review: every player with a POV demo in the queue recorded this half themselves, so the ${n} were left out. To review a player from the HLTV demo, add one of their POV demos from another half, or set HLTV to Everyone.`;
      }
      return `Nothing to review: the ${n} belong to players the HLTV box didn't pick. Set HLTV to Everyone to see them all.`;
    },
    nothingCovered: (covers, ticked) => {
      const which = { unmarked: 'with no Review mark', keep: 'marked Keep', maybe: 'marked Maybe', skip: 'marked Skip', ticked: 'ticked for capture' }[covers] || 'to review';
      return `${ticked ? 'The ticked demos have' : 'The demos in the list have'} no highlights ${which}.`;
    },
    // #848: marks saved outside the project, brought onto demos a scan adds.
    LIBRARY_TITLE: 'Marks you saved before',
    libraryAskMessage: (demos, highlights) =>
      `${demos} of these demos ${demos === 1 ? 'has' : 'have'} Review marks you saved before (${highlights} highlight${highlights === 1 ? '' : 's'}). Bring them in?`,
    LIBRARY_ASK_FOOTER: 'Only highlights with no mark, note or Kill Range in this project get them; the project\'s own stay as they are.',
    LIBRARY_BRING_IN: 'Bring them in',
    LIBRARY_NOT_NOW: 'Not now',
    libraryDemoLine: (highlights) => `${highlights} highlight${highlights === 1 ? '' : 's'}`,
    libraryBroughtInToast: (demos, highlights) =>
      `Brought in your saved Review marks: ${highlights} highlight${highlights === 1 ? '' : 's'} in ${demos} demo${demos === 1 ? '' : 's'}.`,
    TICK_YES_LABEL: 'Tick Yes for capture',
    TICK_YES_TITLE: 'Also tick each highlight you answer Yes, ready for a capture batch. Off: Yes only marks it Keep. Maybe never ticks a row.',
    FAST_FORWARD_LABEL: 'Fast-forward gaps over',
    FAST_FORWARD_UNIT: 's',
    FAST_FORWARD_TITLE: 'When two kills in a highlight are further apart than this, the stretch between them plays at 4x: from 2 s after one kill to 4 s before the next. You still see everything, just quicker. Takes effect when a review starts.',
    NOTHING_TO_REVIEW: 'The Master Demo Queue shows no demos to review: add some, or clear its search and filters.',
    ONLY_OLD_HIGHLIGHTS: 'These highlights were found before DoD Studio kept their demo-player times. Rescan the demos, then review them.',
    STOP: 'Stop',
    NOT_STARTED: 'The review was not started.',
    // Review reached a demo whose file is missing (review_missing_demos.js).
    MISSING_TITLE: 'Demo not found',
    missingMessage: (name, searchStopped) =>
      `${name} was renamed, moved or deleted, so it can't be reviewed. What now?`
      + (searchStopped ? ` ${SEARCH_STOPPED_NOTE}` : ''),
    MISSING_LOCATE: 'Locate…',
    MISSING_USE_FOUND: 'Use found copy',
    MISSING_REMOVE: 'Remove',
    MISSING_SKIP: 'Skip and keep reviewing',
    MISSING_CANCEL: 'Cancel review',
    NOTHING_LEFT_AFTER_SKIP: 'Nothing is left to review without the skipped demo(s).',
    // #1142: the in-game Review tab asked for a review of the demo it has loaded.
    FROM_GAME_SCANNING: 'Scanning the demo in DoD Studio...',
    FROM_GAME_STARTING: 'Starting the review...',
    FROM_GAME_BUSY: 'DoD Studio is scanning other demos: try again when that scan is done.',
    fromGameAddingToast: (name) => `The game asked to review ${name}: adding it to the Master Demo Queue and scanning it.`,
    fromGameNotAdded: (name) => `DoD Studio scanned ${name} but it isn't in the Master Demo Queue (an identical copy of a demo already there, or unreadable), so there is nothing to review.`,
    fromGameNoHighlights: (name) => `${name} has no highlights to review.`,
    startedToast: (count, launched, skipped) =>
      `Reviewing ${count} highlight${count === 1 ? '' : 's'} in Day of Defeat${launched ? ' (starting the game)' : ''}.`
      + (skipped > 0 ? ` ${skipped} older highlight${skipped === 1 ? ' needs' : 's need'} a rescan first.` : ''),
    endedToast: (reason, answers) => {
      const answered = `${answers} answer${answers === 1 ? '' : 's'} saved`;
      if (reason === 'done') return `Review finished: ${answered}.`;
      if (reason === 'stopped') return `Review stopped: ${answered}.`;
      if (reason === 'closed') return `The game closed: ${answered}.`;
      return `Review ended (${reason}): ${answered}.`;
    },
    // #880: the game refused something DoD Studio sent while a review runs.
    refusedToast: (command) => {
      const name = String(command || '').toLowerCase();
      let what = 'do that';
      if (['viewdemo', 'playdemo', 'timedemo', 'ds_demo_reload'].includes(name)) what = 'load the preview';
      else if (name.startsWith('ds_demo_seek') || name === 'dem_jump') what = 'jump to that time';
      return `A review is running in the game, so it didn't ${what}. End the review first (Stop), then try again.`;
    },
  },
  CLIP_NAME: {
    TEMPLATE_LABEL: 'Clip Name Template:',
    TEMPLATE_TITLE: 'How each highlight\'s automatic clip name is built. The finished file is named after it. A name typed on a Highlight Details row wins over this.',
    RESET_BUTTON: 'Default',
    RESET_TITLE: 'Go back to the default template.',
    MISSING_VALUE: 'unknown',
    strayBrace: (pos) => `The "}" at character ${pos} has no "{" before it.`,
    unclosedBrace: (pos) => `The "{" at character ${pos} is never closed.`,
    EMPTY_PLACEHOLDER: '"{}" has no placeholder name in it.',
    unknownPlaceholder: (raw) => `${raw} isn't a placeholder. Pick one from the list below.`,
    unknownModifier: (raw) => `${raw}: only :lower, :upper and :first can follow a placeholder name.`,
    invalidCharacters: (text) => `Windows doesn't allow \\ / : * ? " < > | in a file name: ${text}`,
    EMPTY_TEMPLATE: 'This template gives an empty name.',
    NO_DISTINGUISHING: 'No {row}, {time} or {demo}, so two highlights can get the same name. Their files still stay separate (a _2 is added), but the names won\'t say which is which.',
    previewName: (name) => `Preview: ${name}`,
    previewPath: (path, length) => `${path} (${length} characters)`,
    longName: (length) => `This name is ${length} characters long. Some programs have trouble past 120.`,
    TRIMMED: 'Shortened so the full path stays under the Windows limit (victims, demo and weapons are cut first).',
    PREVIEW_TYPED: 'The selected highlight has a typed name, which wins over the template:',
    PREVIEW_NONE: 'Select a demo with highlights to see a preview.',
    // The clip name's own meaning of a placeholder, where it isn't the one
    // in PLACEHOLDERS.DESCRIPTIONS.
    DESCRIPTIONS: {
      name: "The highlight player's name without their team's tag, or their Use as name from the Players tab ({player} still works: the name as it is)",
      full_name: "The highlight player's name as it is, tag included, or their Use as name from the Players tab",
      faction: 'Their side: Allies, British, Axis',
      enemy_faction: 'The side of the players they killed',
      kills: "Kills in the chosen Kill Range (Rename Demos' {kills} is the player's kills in the whole demo instead)",
      headshots: 'Headshots in the chosen Kill Range, from the helmet that flies off in the demo. A kill the demo cannot tell (a British victim, whose helmet may not pop, or in an HLTV demo, which loses about half of them, a kill with no helmet seen) is not counted; with none known it gives unknown',
      weapons: 'Each weapon used, joined with -',
      first_weapon: 'The weapon of the first kill',
      victims: 'The players killed, joined with -',
      first_victim: 'The first player killed',
      row: 'The highlight\'s row number',
      time: 'Time into the demo, like 12m34s',
      demo: 'The demo file name, without .dem',
      team_name: "Their team's display name from the project's Teams list; unknown (or your word after |) when the side has no tag",
      opponent: "The first victim's team: its display name from the project's Teams list; unknown (or your word after |) when the side has no tag",
      half: 'Which half of its match the demo records, h1 or h2, as the Master Demo Queue\'s match grouping tells it from the demos\' contents. '
        + 'When nothing says, it gives unknown or your word after |, never a guess',
      match: 'The demo\'s match number in the Master Demo Queue\'s match grouping: every demo of one match shares it',
    },
    chipTitle: (description, value) => (value ? `${description}. Selected highlight: ${value}` : description),
    renderTemplateInvalid: (reason) => `Nothing was queued: the clip name template has a problem. ${reason} Fix it in Configuration > Render Output.`,
  },
  // ── First-run Setup window and the Highlights page's checklist (#599) ──────
  // Configuration > Players (#428): players_ui.js.
  PLAYERS: {
    USE_IN_GAME_LABEL: 'Use in the game',
    USE_IN_GAME_TITLE: 'On: sprays and crosshairs in your demos follow this table (ds_player_overrides 1, sent when Studio starts the game, a review or a batch, and right away to a game already running). Off: the game\'s own.',
    HELP: "Pick the spray and the crosshair each player shows in your demos: sprays are PNGs from the game's logos folder, crosshairs a 64x64 tile of a sprite in its sprites folder. Players are matched by SteamID, or by name when there is none.",
    PLAYER_PLACEHOLDER: 'A player from the queue, a SteamID or a name',
    ADD_BUTTON: 'Add Player',
    REFRESH_BUTTON: 'Refresh',
    REFRESH_TITLE: "Read the logos and sprites folders and the queue's players again",
    COLUMN_PLAYER: 'Player',
    COLUMN_USE_AS: 'Use as',
    USE_AS_TITLE: 'Used for {name} and {player} in clip and demo names instead of the in-game name. Leave empty to use the in-game name.',
    USE_AS_PLACEHOLDER: 'In-game name',
    USE_AS_NEEDS_STEAMID: 'A name is only swapped for a player with a SteamID: a player added by name has none.',
    useAsAria: (who) => `Use as name for ${who}`,
    // #429: a picture per player that the VEGAS nameplate script puts above their clips.
    COLUMN_NAMEPLATE: 'Nameplate',
    NAMEPLATE_TITLE: "A PNG put on its own track above each of this player's clips by the VEGAS nameplate script (Render > Export VEGAS EDL… saves the list it reads).",
    NAMEPLATE_PICK: 'Pick a PNG…',
    NAMEPLATE_PICK_TITLE: 'Pick a nameplate picture',
    NAMEPLATE_FILTER: 'PNG picture',
    NAMEPLATE_NEEDS_STEAMID: 'A nameplate follows a SteamID: a player added by name has none.',
    nameplateMissing: (name) => `${name} (missing)`,
    nameplateMissingTitle: (path) => `${path}\nThis file isn't there any more: pick it again, or put it back.`,
    nameplateAria: (who) => `Nameplate for ${who}`,
    nameplateClearAria: (who) => `Remove the nameplate for ${who}`,
    NAMEPLATE_CLEAR_TITLE: 'No nameplate for this player (the file itself stays)',
    COLUMN_SPRAY: 'Spray',
    COLUMN_CROSSHAIR: 'Crosshair',
    PICK_PNG: 'Pick a PNG…',
    FALLBACK_SPRAY_OPTION: 'Fallback spray',
    FALLBACK_CROSSHAIR_OPTION: 'Fallback crosshair',
    pngMissingOption: (name) => `${name} (not in logos)`,
    spriteMissingOption: (name) => `${name} (not in sprites)`,
    SPRITE_TITLE: "The crosshair's sprite, in the game's sprites folder (or dod_addon's, where Add a crosshair file… puts them)",
    TILE_TITLE: 'Pick the tile: the sprite is a 4x4 grid of 64x64 crosshairs, numbered as cl_xhair_style numbers them',
    PICK_TILE: 'Pick a tile',
    tileLabel: (tile) => `Tile ${tile}`,
    NO_PREVIEW: "This sprite couldn't be read for a preview: pick by number (1 top left, 16 bottom right).",
    notGridSize: (w, h) => `This sprite is ${w}x${h}, not 256x256: its tiles are still cut 64x64 from the top left.`,
    NO_PLAYERS: 'No players listed yet: everyone gets the fallbacks below.',
    MATCHED_BY_STEAMID: 'Matched by SteamID, whatever name they play under',
    MATCHED_BY_NAME: 'Matched by name, ignoring case',
    REMOVE_TITLE: 'Remove this player',
    removeAria: (who) => `Remove ${who}`,
    readingPlayers: (done, total) => `Reading the queue's players… ${done} of ${total} demos`,
    playersFound: (players, demos) => (demos
      ? `${players} player${players === 1 ? '' : 's'} in the queue's ${demos} demo${demos === 1 ? '' : 's'}`
      : 'No demos in the queue: type a SteamID or a name.'),
    NO_PNGS: "No PNG in the game's logos folder (in the game folder beside hl.exe). Put your spray pictures there, then Refresh.",
    pngsMissing: (names) => `Not in the logos folder: ${names.join(', ')}. Those sprays stay as the game draws them.`,
    spritesMissing: (names) => `Not in the sprites folder: ${names.join(', ')}. Those crosshairs stay the game's own.`,
    HLTV_FALLBACK_UNSET: 'HLTV demos: pick the fallback spray. Without one, players not listed show no spray (with nobody listed at all, the game\'s bullet hole).',
    POV_FALLBACK_UNSET: 'POV demos: pick the fallback spray. Without one, players not listed keep the spray the demo saved, and show none where it saved none.',
    NOT_IN_GAME: "Use in the game is off: the game draws its own sprays and crosshairs.",
    POV_FALLBACK_LABEL: 'Fallback spray, POV demos:',
    HLTV_FALLBACK_LABEL: 'Fallback spray, HLTV demos:',
    FALLBACK_HINT: 'Used for players not listed, and when From the demo finds none.',
    FALLBACK_TITLE: "Used for players not listed, and when From the demo finds none. A stock lambda spray names no player, so it gets this too, unless it's the POV recorder's own, which gets their row.",
    POV_OWN_SPRAY_RETIRED: 'Your POV fallback spray was "Their own spray", which is gone, so it is now "No spray". Change it in Configuration > Players.',
    FALLBACK_PNG: 'One fallback spray',
    FALLBACK_NONE: 'No spray',
    CROSSHAIR_FALLBACK_LABEL: 'Crosshair, players not listed:',
    CROSSHAIR_FALLBACK_TITLE: "The game's own is your cl_xhair_style tile of customXHair.spr, as without this table. The crosshair shown is the watched player's in a first-person spectator view (an HLTV demo, or a POV demo while the recorder is dead and watching a teammate), and otherwise the recorder's in a POV demo.",
    CROSSHAIR_GAME: "Game's own",
    CROSSHAIR_ONE: 'One fallback crosshair',
    HD_NOTE: "Every spray from a PNG is drawn at the spray size in every demo, whether or not the demo saved one, with or without HD textures (with them on, it's sharper). A spray change shows from the next demo you play. A crosshair changes on the next frame, and in a POV demo needs your cl_xhair_style at 1-16 (0 is DoD's classic crosshair, which has no tile to swap).",
    SPRAY_SIZE_LABEL: 'Spray size:',
    SPRAY_SIZE_TITLE: "How big a spray built from a PNG is on the wall (its longer side; the picture's shape is kept): every listed player's spray, and every fallback spray. A spray from the demo keeps its real size. A change shows from the next demo you play.",
    spraySizeOption: (n) => `${n}${n === 64 ? ' (like a saved spray)' : ''}`,
    // "Make your own crosshair…" / "Add a crosshair file…" (#842).
    MAKE_CROSSHAIR_BUTTON: 'Make your own crosshair…',
    MAKE_CROSSHAIR_TITLE: "Opens Vertex's crosshair maker in your web browser. Draw your crosshairs there and download them.",
    ADD_CROSSHAIR_BUTTON: 'Add a crosshair file…',
    ADD_CROSSHAIR_TITLE: 'Pick the file the crosshair maker gave you (customxhair.spr, or the ZIP it came in). Studio copies it into the game\'s dod_addon\\sprites folder under a name you choose, and never replaces a file already there.',
    MAKE_CROSSHAIR_HELP: 'The crosshair maker gives you a file called customxhair.spr, or a ZIP with it inside. Click Add a crosshair file… and pick it (no need to unzip): it then shows in the crosshair lists above.',
    CROSSHAIR_NAME_LABEL: 'Name:',
    CROSSHAIR_NAME_TITLE: 'A short name for this crosshair, so several can sit side by side',
    CROSSHAIR_SAVE_BUTTON: 'Add',
    CROSSHAIR_CANCEL_BUTTON: 'Cancel',
    CROSSHAIR_PICK_TITLE: 'Pick a crosshair file',
    CROSSHAIR_FILTER: 'Crosshair (.spr or .zip)',
    CROSSHAIR_NEEDS_HL: 'Set the Half-Life executable first (Setup): the crosshair goes into that game.',
    CROSSHAIR_NAME_EMPTY: 'Type a name (letters or numbers).',
    crosshairSavedAs: (file) => `Saved as ${file}`,
    crosshairNameTaken: (file) => `There's already a crosshair called ${file}: pick another name.`,
    CROSSHAIR_ADDING: 'Adding…',
    crosshairAdded: (file) => `Added ${file}. Pick it in a crosshair list above.`,
    crosshairAddFailed: (why) => `Couldn't add the crosshair: ${why}`,
    FROM_DEMO_OPTION: 'From the demo',
    FROM_DEMO_TITLE: "The spray the demo saved for this player, drawn as it is. When the demo saved none (they joined before the recording, or any HLTV demo), the fallback spray for that demo type.",
  },
  // The one-time look at the pinned folders (#533): pinned_review.js.
  PINNED_REVIEW: {
    TITLE: 'Pinned folders',
    MESSAGE: "These may have been pinned automatically by + Add Folder. It now puts the folders it scans under Recent instead, and pinning is up to you. Untick any folder you don't want pinned, then press Done.",
    KEEP_TITLE: 'Ticked: stays pinned. Unticked: unpinned (the folder and its demos stay where they are).',
    DONE_BUTTON: 'Done',
    DONE_TITLE: 'Unpins the unticked folders and keeps the ticked ones. This list doesn\'t come up again.',
    answeredLog: (kept, unpinned) => `[pinned folders] the one-time look: kept ${kept}, unpinned ${unpinned}`,
  },
};
