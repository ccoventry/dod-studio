// strings/shared.js: the app's text for dialogs, widgets and progress lines used by several areas.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const SHARED_STRINGS = {
  // ── Clear Previews modal ─────────────────────────────────────────────────
  CLEAR_PREVIEWS_MODAL: {
    TITLE: 'Clear Previews',
    SELECT_ALL_BUTTON: 'Select All',
    SCANNING_STATUS: 'Scanning for orphaned preview demos...',
    TABLE_HEADER_FILE: 'File',
    TABLE_HEADER_SIZE: 'Size',
    TABLE_HEADER_MODIFIED: 'Modified',
    SCANNING_ROW: 'Scanning...',
    FOOTER_DEFAULT: 'Found: 0 | Reclaimable: 0.00 GB',
    foundReclaimable: (count, gb) => `Found: ${count} | Reclaimable: ${gb} GB`,
    DELETE_SELECTED_BUTTON: 'Delete Selected',
    CLOSE_BUTTON: 'Close',
  },
  // ── Clear All / Clear Selected / Remove Tracked Demo modal ───────────────
  CLEAR_ALL_MODAL: {
    TITLE_DEFAULT: 'Clear All Demos',
    SAVE_SESSION_FIRST_BUTTON: 'Save Session First',
    CLEAR_ANYWAY_DEFAULT: 'Clear Anyway',
    CANCEL_BUTTON: 'Cancel',
  },
  // ── Generic themed Confirm/Cancel dialog (themed_confirm.js) ─────────────
  THEMED_CONFIRM_MODAL: {
    TITLE_DEFAULT: 'Confirm',
    CONFIRM_BUTTON: 'Confirm',
    CANCEL_BUTTON: 'Cancel',
  },
  // ── Unsaved-changes prompt on window close ─────────────
  UNSAVED_CHANGES_MODAL: {
    TITLE: 'Unsaved Changes',
    MESSAGE: 'DoD Studio has unsaved changes. Save your session before closing?',
    SAVE_BUTTON: 'Save & Close',
    DISCARD_BUTTON: 'Close Without Saving',
    CANCEL_BUTTON: 'Cancel',
  },
  // ── list_editor.js: shared row-list widget (Browse/Move/Remove) ─────────
  LIST_EDITOR: {
    BROWSE_TITLE: 'Browse…',
    MOVE_UP_TITLE: 'Move up',
    MOVE_DOWN_TITLE: 'Move down',
    REMOVE_TITLE: 'Remove',
    REMOVE_ARIA_LABEL: 'Remove',
  },
  // ── progress_view.js: what every long job's progress line shares (#951)
  PROGRESS: {
    elapsedSuffix: (clock) => ` · ${clock}`,
  },
  // ── scan_progress.js: what every scan reading demos one by one adds to
  //    its progress line (List Demos, Cache all, the Master Queue scan; #687)
  SCAN_PROGRESS: {
    fromCache: (cached) => ` (${cached} from the analyzer cache)`,
    CHECKING_CACHE: 'Checking the analyzer cache…',
    timeLeft: (seconds) => (seconds >= 90 ? ` · about ${Math.round(seconds / 60)} min left` : ` · about ${Math.max(1, seconds)} s left`),
    // The Master Queue scan's start and end in the activity log, with its
    // times, so how long a rescan takes can be measured (#926).
    startLog: (paths) => `[scan] started: ${paths} path(s) to read`,
    endLog: (outcome, elapsed, read, unchanged) => `[scan] ${outcome} after ${elapsed}: ${read} demo(s) read, ${unchanged} unchanged`,
    endLogSplit: (cached, notCached, elapsed) => ` (${cached} from the analyzer cache, ${notCached} not${notCached > 0 ? `: those took ${elapsed}` : ''})`,
    failedLog: (elapsed, err) => `[scan] failed after ${elapsed}: ${err}`,
  },
  // The name placeholders both templates share (#751): their chip rows, and
  // what each means where the clip name and Rename Demos agree. Each
  // template's own meanings are in CLIP_NAME.DESCRIPTIONS and
  // DEMO_RENAME.DESCRIPTIONS.
  PLACEHOLDERS: {
    GROUPS: {
      player: 'Player',
      sides: 'Sides',
      teams: 'Teams',
      demo: 'Demo',
      highlight: 'Highlight',
      format: 'Format',
    },
    DESCRIPTIONS: {
      map: 'The map, without dod_',
      date: "The demo file's date, YYYY-MM-DD",
      demo_type: 'pov or hltv',
      allies: 'The team on the Allied side',
      axis: 'The team on the Axis side',
      team1: 'The two teams in alphabetical order, so both halves agree: the first',
      team2: 'The second of the two teams in alphabetical order',
      lower: 'Add after a placeholder name to lower-case it: {map:lower}',
      upper: 'Add after a placeholder name to upper-case it: {map:upper}',
      first: 'Add after a placeholder name to keep only its first word: {name:first} gives m00cat for "m00cat :D"',
    },
    fallbackNotAllowed: (raw) => `${raw}: only the name, faction, team, half and match placeholders take a word after |.`,
    fallbackTitle: (placeholders) => 'Click just after a placeholder, then this, then type a word: the word is used when the placeholder has no value, '
      + `like the team names in a pug. {team_name|mix} gives mix there; without it you get unknown. Works on ${placeholders}.`,
    noValueNote: (name) => `With no value (a pug has no team names) it gives unknown, or your word after |, as in {${name}|mix}.`,
  },
};
