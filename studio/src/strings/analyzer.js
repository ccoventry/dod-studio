// strings/analyzer.js: the app's text for the Demo Analyzer pane.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

/**
 * "(2 upgraded, 3 already cached, 1 failed)", or nothing when all are 0
 * (#569). `already` counts the upgraded ones too: they are in the cache by
 * the time they are counted.
 */
function cacheCounts(already, failed, upgraded = 0) {
  const parts = [];
  if (upgraded) parts.push(`${upgraded} upgraded`);
  if (already > upgraded) parts.push(`${already - upgraded} already cached`);
  if (failed) parts.push(`${failed} failed`);
  return parts.length ? ` (${parts.join(', ')})` : '';
}

/** The network protocol of older game builds' demos (#239): the game plays
 *  one with an outdated-protocol warning. */
const OLDER_BUILD_NETWORK_PROTOCOL = 47;

export const ANALYZER_STRINGS = {
  // ── Demo Analyzer pane (explorer, filters, 7 report tabs) ────────────────
  ANALYZER: {
    EXPLORER_TITLE: 'Explorer',
    REFRESH_TREE_TITLE: 'Refresh folder tree',
    REFRESH_TREE_BUTTON: '⟳ Refresh',
    EXPLORER_SETTINGS_SUMMARY: '⚙ Explorer Settings',
    SHOW_FOLDER_COUNTS_LABEL: 'Show folder demo counts in the tree',
    ADD_PIN_BUTTON: '➕ Add Pin…',
    DEMOS_TITLE: 'Demos',
    CACHE_ALL_BUTTON: 'Cache all',
    CACHE_STOP_BUTTON: 'Stop',
    CACHE_ALL_TITLE: 'Analyse every demo in this folder now, in the background, so opening one later is instant. The game’s Highlights tab and the player filters use the same cache. Demos already cached are skipped, and ones an older version of DoD Studio cached are upgraded instead of analysed again where possible.',
    CACHE_NOTHING: 'No demos in this folder to cache.',
    CACHE_STOPPING: 'Stopping after the demos in progress…',
    // A cache-format change leaves older analyses behind; they are upgraded
    // first, which is fast, and only the rest are parsed.
    cacheUpgrading: ({ upgrade: { done = 0, total = 0 } = {} }) =>
      `Upgrading cached analyses: ${done.toLocaleString('en-US')} of ${total.toLocaleString('en-US')}`,
    cacheProgress: ({ done, total, already, failed, upgrade }) =>
      `Caching ${done} / ${total}` + cacheCounts(already, failed, upgrade?.upgraded),
    cacheDone: ({ done, total, already, failed, cancelled, upgrade }) =>
      (cancelled ? `Stopped at ${done} / ${total}` : `Cached ${total} demo${total === 1 ? '' : 's'}`) +
      cacheCounts(already, failed, upgrade?.upgraded),
    SEARCH_NAME_MAP_PLACEHOLDER: 'Search name/map...',
    TYPE_ALL: 'All',
    TYPE_POV: 'POV',
    TYPE_HLTV: 'HLTV',
    MAP_PLACEHOLDER: 'Map',
    // #437: every demo a player appears in.
    PLAYER_PLACEHOLDER: 'Player',
    PLAYER_FILTER_TITLE: 'Show only demos this player is in, recorded or played. Pick a name from the list (it matches every name that player used), or type part of a name or a SteamID.',
    playersReading: (done, total) => `Reading players: ${done} / ${total} not read yet`,
    // #992: the one quick lookup of the players already read.
    PLAYERS_LOOKING: 'Looking up players…',
    playersInCloud: (n) => `Players not read for ${n} demo${n === 1 ? '' : 's'} on a cloud drive (reading one downloads it). Cache all reads them.`,
    ROLE_RECORDED: 'recorded it',
    ROLE_PLAYED: 'played in it',
    MIN_DATE_PLACEHOLDER: 'Min Date (YYYY-MM-DD)',
    MAX_DATE_PLACEHOLDER: 'Max Date (YYYY-MM-DD)',
    RESET_BUTTON: 'Reset',
    DEMO_TABLE_HEADERS: { name: 'Name', type: 'Type', map: 'Map', date: 'Date' },
    COL_TYPE: 'Type',
    COL_MAP: 'Map',
    COL_DATE: 'Date',
    ANALYZER_TITLE: 'Demo Analyzer',
    BROWSE_DEMO_BUTTON: 'Browse Demo...',
    SUBTAB_SUMMARY: 'Summary',
    SUBTAB_SCOREBOARD: 'Scoreboard',
    SUBTAB_PLAYER_DETAILS: 'Player Details',
    SUBTAB_TEAM_DETAILS: 'Team Details',
    SUBTAB_TIMELINE: 'Timeline',
    SUBTAB_ROUNDS: 'Rounds',
    SUBTAB_FLAGS: 'Flags',
    SUBTAB_CHAT: 'Chat Log',
    SUBTAB_KILL_MAP: 'Kill Map',
    EMPTY_PICK_DEMO: 'Pick a folder and demo on the left, browse for a file, or select one from the Workspace and click "View Match Telemetry".',
    EMPTY_PICK_DEMO_JS_FALLBACK: 'Browse for a demo file, or select one from the Workspace and click "View Match Telemetry".',

    SCANNING_WORKSPACE: 'Scanning workspace…',
    NO_DEMO_FOLDERS_FOUND: 'No demo folders found.',
    TIER_PINNED: '📌 Pinned',
    TIER_RECENT: '🕒 Recent',
    TIER_LOCAL: '📂 Local',
    PIN_FOLDER_TITLE: 'Pin folder',
    UNPIN_FOLDER_TITLE: 'Unpin folder',
    ADD_PINNED_FOLDER_DIALOG_TITLE: 'Add Pinned Folder',
    SELECT_DEMO_DIALOG_TITLE: 'Select Demo to Analyze',
    LOADING_LABEL: 'Loading…',
    THIS_PC_LABEL: '💻 This PC',
    PICK_FOLDER_FROM_SIDEBAR: 'Pick a folder from the Explorer sidebar.',
    NO_DEMOS_IN_FOLDER: 'No demos found in this folder.',
    // A folder's file names are on their way; a cloud drive can take a while.
    READING_FOLDER: 'Reading this folder…',
    // The Map cell until the start of the demo is read.
    MAP_READING: '…',
    NO_DEMOS_MATCH_FILTERS: 'No demos match the current filters.',
    ANALYZING_ELLIPSIS: 'Analyzing…',
    ANALYZING_DEMO_ELLIPSIS: 'Analyzing demo…',
    analyzingPct: (pct) => `Analyzing… ${pct}%`,
    analyzingDemoPct: (pct) => `Analyzing demo… ${pct}%`,
    // A demo the analyzer cache has is read, not parsed (#958).
    READING_CACHE: 'Reading from the analyzer cache…',
    READING_CACHE_SHORT: 'Reading from the cache…',
    analyzeFailed: (err) => `Failed to analyze demo: ${err}`,
    NO_DEMO_LOADED: 'No demo loaded',
    // #217: a demo that recorded more than one map.
    multiMapNotice: (maps) => `This demo recorded ${maps.length} maps (${maps.join(', ')}). `
      + 'Only the first is analysed, and highlights and captures use it too. Split it to use the rest.',
    multiMapAlreadySplit: (maps) => `This demo recorded ${maps.length} maps (${maps.join(', ')}) and was already split. `
      + 'This report covers the first.',
    multiMapAlreadySplitTitle: (files) => `Split next to it into: ${files.join(', ')}`,
    MULTI_MAP_SPLIT_BUTTON: 'Split now',
    MULTI_MAP_SPLIT_TITLE: 'Writes each map out as a demo of its own, next to this one, and opens the first. '
      + 'This demo is kept as it is. A map under a minute (the next map loading as the recording stopped) is left out; '
      + 'the Demo Auditor\'s Split Maps tab lets you pick.',
    multiMapSplitDone: (names) => `Split into ${names.length} demo${names.length === 1 ? '' : 's'}: ${names.join(', ')}`,
    multiMapSplitFailed: (err) => `Could not split it: ${err}`,
    // #811: a demo damaged partway, read up to the break.
    damagedDemoNotice: (clock, rest, reason) => `Damaged demo: read up to ${clock}. `
      + `The rest of the file (${rest}) doesn't parse (${reason}), so everything here stops at ${clock}.`,
    DAMAGED_DEMO_REASON_UNKNOWN: 'no reason given',

    NO_PLAYERS_FOUND: 'No players found in this demo.',
    NO_WEAPON_DATA: 'No weapon data.',
    NO_KILL_STREAKS: 'No kill streaks recorded.',
    NO_TEAM_SCORE_EVENTS: 'No team score events recorded',
    NO_COMPLETED_ROUNDS: 'No completed rounds recorded.',
    NO_MESSAGES_MATCH_FILTERS: 'No messages match the current filters.',
    HIDE_ALL_TITLE: 'Hide all',
    SHOW_ALL_TITLE: 'Show all',
    SEARCH_SENDER_TEXT_PLACEHOLDER: 'Search sender or text...',
    CHAT_HEADING: 'Chat &amp; System Log',
    SELECT_ALL_BUTTON: 'Select All',
    CLEAR_ALL_BUTTON: 'Clear All',
    ALL_CHAT_LABEL: 'All Chat',
    TEAM_CHAT_LABEL: 'Team Chat',
    STATUS_ALL: 'All',
    STATUS_ALIVE: 'Alive',
    STATUS_DEAD: 'Dead',
    TEAM_LABEL: 'Team:',
    SYSTEM_LOGS_LABEL: 'System Logs:',
    JOINS_LEAVES_LABEL: 'Joins/Leaves',
    TEAM_CHANGES_LABEL: 'Team Changes',
    GAMEPLAY_LABEL: 'Gameplay',
    OTHER_SYSTEM_LABEL: 'Other System',

    scoreboardHeading: (alliesLabel, alliesScore, cmp, axisScore) => `Scoreboard: ${alliesLabel} (${alliesScore}) ${cmp} Axis (${axisScore})`,
    compareGlyph: (a, b) => (a > b ? '>' : (a === b ? '=' : '<')),
    megabytesLabel: (mb) => `${mb} MB`,
    KD_BADGE_LABEL: 'K/D',
    partialRecordingBoth: (fmt) => `Partial recording — demo started with ${fmt} remaining and ended before the match concluded.`,
    partialRecordingStartedLate: (fmt) => `Partial recording — demo started with ${fmt} remaining on the clock.`,
    partialRecordingEndedEarly: (fmt) => `Partial recording — demo ended before the match concluded (${fmt} remaining at cutoff).`,
    groupLabelWithCount: (label, count) => `${label} — ${count} player(s)`,
    COL_NAME: 'Name',
    COL_CLASS: 'Class',
    COL_SCORE: 'Score',
    COL_KILLS: 'Kills',
    COL_DEATHS: 'Deaths',
    AXIS_LABEL: 'Axis',
    SPECTATORS_LABEL: 'Spectators',
    UNASSIGNED_LABEL: 'Unassigned',
    RECONNECTED_TITLE: 'Player reconnected mid-demo',
    PRE_DEMO_ACTIVITY_TITLE: 'Player had pre-existing stats when recording started',
    UNKNOWN_CLASS: 'Unknown',

    PLAYER_LABEL: 'Player:',
    LEGIT_PROOF_LINK_TITLE: 'Search this player on Legit-Proof',
    LEGIT_PROOF_TEXT: 'Legit-Proof',
    STEAM_PROFILE_TEXT: 'Steam Profile',
    NO_STEAM_ID: 'No Steam ID',
    STEAM_ID_LABEL: 'Steam ID: ',
    // #536: a real player's SteamID in all three forms, each copyable.
    STEAM_ID64_LABEL: 'SteamID64',
    STEAM_ID_CLASSIC_LABEL: 'Classic',
    STEAM_ID3_LABEL: 'SteamID3',
    COPY_BUTTON: 'Copy',
    COPIED_BUTTON: 'Copied',
    COPY_FAILED_BUTTON: "Couldn't copy",
    copyValueTitle: (value) => `Copy ${value}`,
    COPY_SHOW_ONLY_BUTTON: 'Copy kill-feed command',
    copyShowOnlyTitle: (line) => `Copies "${line}". Paste it into the game console to hide every kill-feed line that doesn't involve this player.`,
    CLOCK_UNKNOWN: '??:??',
    TIMELINE_START_LABEL: '0:00',
    connectedSlot: (id) => `Connected (Slot ${id})`,
    DISCONNECTED: 'Disconnected',
    RECONNECTED_MID_DEMO: '🔄 Reconnected mid-demo',
    PRE_EXISTING_STATS: '* Pre-existing stats',
    MATCH_SCORE_TITLE: 'Match Score',
    KILLS_TITLE: 'Kills',
    DEATHS_TITLE: 'Deaths',
    AVG_LIFESPAN_TITLE: 'Avg. Lifespan',
    minMaxBadge: (min, max) => `Min: ${min} / Max: ${max}`,
    WEAPON_BREAKDOWN_TITLE: 'Weapon Breakdown',
    COL_WEAPON: 'Weapon',
    COL_PCT_TOTAL: '% of Total',
    COL_TEAM_KILLS: 'Team Kills',
    KILL_STREAKS_TITLE: 'Kill Streaks',
    COL_WAVE: 'Wave',
    COL_TIME: 'Time',
    COL_DURATION: 'Duration',
    // #448: engagement distance in the kill list and the player's tables.
    COL_DISTANCE: 'Distance',
    COL_AVG_DISTANCE: 'Avg. distance',
    AVG_DISTANCE_TITLE: 'Avg. Distance',
    DISTANCE_UNKNOWN_TITLE: 'Position unknown',
    DISTANCE_AVERAGE_NOTE: 'Averaged over the kills where the position of both players is known; team kills are left out.',
    distanceMeasured: (measured, total) => `${measured} of ${total} kills`,
    metres: (m) => `${m.toFixed(1)} m`,
    WEAPON_CATEGORY_GRENADES: 'Grenades',
    WEAPON_CATEGORY_MELEE: 'Melee',
    WEAPON_CATEGORY_ALLIED: 'Allied',
    WEAPON_CATEGORY_OTHER: 'Other',

    TEAM_DETAILS_HEADING: 'Team Details',
    MATCH_OVERVIEW_TITLE: 'Match Overview',
    ROUND_SCORE_LABEL: 'Round Score',
    TOTAL_KILLS_LABEL: 'Total Kills',
    TOTAL_DEATHS_LABEL: 'Total Deaths',
    TEAM_KD_LABEL: 'Team K/D',
    ACTIVE_PLAYERS_LABEL: 'Active Players',
    TEAM_WEAPON_PERFORMANCE_TITLE: 'Team Weapon Performance',
    ALLIES_US_LABEL: 'Allies (US)',
    ALLIES_LABEL: 'Allies',
    BRITISH_LABEL: 'British',

    TEAM_SCORE_TIMELINE_TITLE: 'Team Score Timeline',
    TIMELINE_TOOLTIP_ELAPSED_LABEL: 'Time Elapsed:',
    TIMELINE_TOOLTIP_TIMESTAMP_LABEL: 'Demo Timestamp:',

    // Kill Map tab (#448)
    KILL_MAP_TITLE: 'Where Players Died',
    KILL_MAP_LOADING: 'Loading the map…',
    killMapNoOverview: (map) => `No map picture for ${map}, so deaths can't be placed on it. The app looks in the game's dod\\overviews folder.`,
    KILL_MAP_NO_KILLS: 'No deaths recorded.',
    killMapCoverage: (placed, total) => `${placed} of ${total} deaths shown.`,
    KILL_MAP_POV_NOTE: 'This demo was recorded by a player, and only has positions for enemies that player could see — so some deaths are missing, and some kills have no distance.',
    killMapDiedLegend: (team) => `${team} player died`,
    killMapTooltip: (victim, killer, weapon) => killer ? `${victim} killed by ${killer} (${weapon})` : `${victim} died (${weapon})`,
    KILL_MAP_TOOLTIP_DISTANCE_LABEL: 'Distance:',
    KILL_MAP_TOOLTIP_TIME_LABEL: 'Demo time:',
    KILL_MAP_TEAMKILL_TAG: 'team kill',
    KILL_MAP_UNKNOWN_PLAYER: 'Unknown player',
    ENGAGEMENT_TITLE: 'Engagement Distance',
    ENGAGEMENT_EXPLAINER: 'How far apart the killer and the victim were at each kill. Team kills and kills where either player\'s position is unknown are left out.',
    NO_ENGAGEMENT_DATA: 'No kills with both players\' positions known.',
    COL_KILLS_MEASURED: 'Kills',
    COL_AVERAGE_DISTANCE: 'Average',
    COL_LONGEST_DISTANCE: 'Longest',
    ALL_WEAPONS_LABEL: 'All weapons',

    ROUNDS_TITLE: 'Rounds',
    // #192: the Flags tab.
    FLAGS_NONE: 'This demo has no flag messages: not a flag map, or recorded without them.',
    flagsTeamBadge: (captures, fromEnemy, blocks, attempts) => `capture${captures === 1 ? '' : 's'} (${fromEnemy} from the other team) · ${blocks} block${blocks === 1 ? '' : 's'} · ${attempts} timed attempt${attempts === 1 ? '' : 's'}`,
    FLAGS_TITLE: 'Flags',
    flagArea: (area) => `Area ${area}`,
    COL_FLAG: 'Flag',
    COL_OWNER_AT_END: 'Held at the end by',
    COL_CAPTURES: 'Captures',
    COL_BLOCKED: 'Blocked',
    FLAGS_NO_LAYOUT: "The demo started after the flags were set up, so their layout isn't known.",
    CAPTURES_TITLE: 'Captures',
    COL_TEAM: 'Team',
    COL_CAPPERS: 'Cappers',
    FLAGS_FROM_ENEMY: 'from the other team',
    FLAGS_NO_CAPTURES: 'No flag was captured after the match went live.',
    CAPPERS_TITLE: 'Cappers',
    COL_PLAYER: 'Player',
    COL_CAP_CREDITS: 'Caps',
    COL_CAP_CREDITS_TITLE: 'Captures the player took part in: the one the game names, and everyone whose objective score rose in the same moment',
    COL_CAP_BREAKS: 'Breaks',
    COL_CAP_BREAKS_TITLE: 'Enemy captures the player stopped: a timed capture cancelled within 1.1 seconds of the player killing one of its cappers',
    COL_OBJ_POINTS: 'Objective points',
    COL_OBJ_POINTS_TITLE: "Every rise in the player's objective score since the match went live",
    COL_ROUND_NUM: '#',
    COL_START_TIME: 'Start Time',
    COL_WINNER: 'Winner',
    COL_KILLS_BY_WINNER: 'Kills by Winner',

    // Summary tab
    FILE_INFO_SECTION: 'File Information',
    FILE_NAME_LABEL: 'File name',
    FILE_PATH_LABEL: 'File path',
    FILE_SIZE_LABEL: 'File size',
    FILE_CREATED_LABEL: 'File created',
    GAME_DETAILS_SECTION: 'Game Details',
    GAME_MOD_LABEL: 'Game mod',
    MAP_NAME_LABEL: 'Map name',
    MAP_CHECKSUM_LABEL: 'Map checksum',
    // #1539: the notice above the tabs for a demo recorded on another build of its map.
    wrongMapBuildNotice: (map, olderRelease) =>
      `Different map version: this demo was recorded on another version of ${map} than the one you have, so some walls, water or doors may look wrong.`
      + (olderRelease ? ' It comes from an older release of Day of Defeat.' : ''),
    // #1547: the notice for a demo whose build Studio's map builds hold.
    fromLibraryMapBuildNotice: (map, build) =>
      `Old map version: this demo was recorded on another version of ${map} than the one installed. DoD Studio plays it on that version (build ${build}) from its map builds when DoD Studio starts the game for this demo. The Kill Map and the kill distances are still measured on the installed map.`,
    // The troubleshooting detail under that notice (a tooltip).
    wrongMapBuildDetail: (recorded, installed) => `Map builds: demo ${recorded}, installed ${installed}`,
    // #1539: an HLTV demo's header has no map checksum; its server info does.
    mapChecksumValue: (info) => {
      if (info.map_checksum === 0 && Number.isInteger(info.server_info_map_checksum)) {
        const hex = info.server_info_map_checksum.toString(16).padStart(8, '0');
        return `${info.server_info_map_checksum} (${hex}), from the server info`;
      }
      return String(info.map_checksum);
    },
    SERVER_INFO_SECTION: 'Server Information',
    SERVER_NAME_LABEL: 'Server name',
    SERVER_ADDRESS_LABEL: 'Server address',
    DEMO_MATCH_DETAILS_SECTION: 'Demo & Match Details',
    RECORDED_BY_LABEL: 'Recorded by',
    DEMO_TYPE_LABEL: 'Demo type',
    MATCH_TYPE_LABEL: 'Match type',
    DEMO_DURATION_LABEL: 'Demo duration',
    MATCH_DURATION_LABEL: 'Match duration',
    TECH_SPECS_SECTION: 'Technical Specifications',
    DEMO_PROTOCOL_LABEL: 'Demo protocol',
    NETWORK_PROTOCOL_LABEL: 'Network protocol',
    // #239: protocol 47 is an older build's; the game warns and plays it.
    networkProtocolValue: (protocol) => (protocol === OLDER_BUILD_NETWORK_PROTOCOL
      ? `${protocol} (older build): plays with an outdated-protocol warning`
      : String(protocol)),
    // #207: what decides whether the pre-Anniversary engine can play it.
    PEAK_ENTITIES_LABEL: 'Most entities in one snapshot',
    peakEntitiesValue: (peak) => (peak > 256
      ? `${peak}: over the pre-Anniversary engine's 256, so it closes the game there. The 25th Anniversary engine plays it.`
      : peak >= 240 ? `${peak}: close to the pre-Anniversary engine's limit of 256` : String(peak)),
    GAME_MOD_DOD: 'Day of Defeat',
    GAME_MOD_CS: 'Counter-Strike',
    GAME_MOD_HL: 'Half-Life',
    MATCH_TYPE_PUBLIC: 'Public / Pickup',
    MATCH_TYPE_PREGAME: 'Clan Match (Pre-game)',
    MATCH_TYPE_INCOMPLETE: 'Clan Match (Incomplete Recording)',
    MATCH_TYPE_FULL: 'Clan Match (Fully Recorded)',
    RECORDED_BY_HLTV_DEFAULT: 'HLTV',
    RECORDED_BY_UNKNOWN: 'Unknown',
    WEAPON_UNKNOWN: 'Unknown',
    EMPTY_DASH: '—',
    CHAT_DEAD_BADGE: '*DEAD*',
    CHAT_SYSTEM_TAG: '[system]',
    CHAT_TEAM_BADGE: '(Team)',
    CHAT_SENDER_UNKNOWN: 'Unknown',
  },
};
