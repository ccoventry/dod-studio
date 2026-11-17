// strings/repair.js: the app's text for splitting and repairing demo files.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const REPAIR_STRINGS = {
  // ── Demo Auditor pane + auditor_pane.js ──────────────────────────────────
  // Demo Auditor's Split Maps tab (#624).
  SPLIT: {
    COL_NUM: '#',
    COL_MAP: 'Map',
    COL_WHEN: 'Length',
    COL_DONE: 'Already split',
    ALREADY_SPLIT_CHIP: 'Already split',
    ALREADY_SPLIT_TITLE: 'A split already wrote this map next to the demo, so it starts unticked. Tick it to write it again (as a new file beside the old one).',
    footer: (demos, split) => `Multi-map demos: ${demos} | Already split: ${split}`,
    // split_progress.js: a split's progress line (#217).
    progressReading: (pct) => `Reading the demo… ${pct}%`,
    progressWriting: (map, pct) => `Writing ${map}… ${pct}%`,
    progressChecking: (map, pct) => `Checking ${map}… ${pct}%`,
    // Several maps are written and checked side by side (#876).
    progressMaps: (done, parts, pct) => `Writing and checking ${parts} maps: ${done} done… ${pct}%`,
    // split_status.js: each map's row while it's split (#876).
    COL_STATUS: 'Status',
    COL_PROGRESS: 'Progress',
    COL_SIZE: 'Size',
    STATUS_WAITING: 'Waiting',
    statusReading: (pct) => `Reading the demo… ${pct}%`,
    STATUS_WRITING: 'Writing…',
    statusChecking: (pct) => `Checking… ${pct}%`,
    STATUS_FINISHED: 'Finished',
    statusFailed: (e) => `Failed: ${e}`,
    STATUS_CANCELLED: 'Cancelled',
    NOT_WRITTEN: 'not written',
    sizeMb: (mb) => `${mb} MB`,
    splitSummary: (wrote, failed, cancelled) => {
      const parts = [`Wrote ${wrote} demo${wrote === 1 ? '' : 's'}`];
      if (failed) parts.push(`${failed} failed`);
      if (cancelled) parts.push(`${cancelled} cancelled`);
      return `${parts.join(', ')}.`;
    },
    TITLE: 'Demos With More Than One Map',
    HINT: "A demo that kept recording through a map change holds every map, but viewdemo only shows the first. Tick the maps to keep: each becomes its own demo next to the original, which is never changed.",
    RECURSIVE: 'Include subfolders',
    FIND_BUTTON: 'Find Multi-Map Demos',
    CANCEL_BUTTON: 'Cancel',
    CHOOSE_FOLDER_FIRST: 'Choose a folder first.',
    scanning: (done, total, demo) => `Finding demos: ${done} of ${total} checked${demo ? ` (${demo})` : ''}`,
    readingLengths: (n, total, demo) => `Reading map lengths: ${n} of ${total} demos (${demo})`,
    cancelledListed: (listed, total) => `Cancelled: ${listed} of ${total} demos with more than one map listed.`,
    CANCELLING: 'Cancelling...',
    found: (n, total) => n === 0
      ? 'No demo here has more than one map.'
      : `${n} demo${n === 1 ? '' : 's'} with more than one map${total ? ` (of the ones checked)` : ''}.`,
    scanFailed: (e) => `Couldn't check the folder: ${e}`,
    mapsCount: (n) => `${n} maps`,
    detailsFailed: (e) => `Couldn't read this demo's maps: ${e}`,
    SHORT_MAP_TITLE: 'Under a minute: probably the next map loading as the recording stopped. Unticked.',
    SPLIT_BUTTON: 'Split Checked Maps',
    CANCEL_SPLIT_BUTTON: 'Cancel',
    CANCELLING_SPLIT: 'Cancelling after this step…',
    SHOW_IN_FOLDER: 'Show in folder',
    NOTHING_TICKED: 'Tick at least one map to split.',
    splitFailed: (e) => `Split failed: ${e}`,
    SOURCE_TITLE: { cache: 'Remembered from an earlier check', analyzer: 'From the analyzer cache', scan: 'Read from the demo' },
  },
  // #15: repairing a demo damaged partway (repair_view.js, repair_pane.js,
  // queue_repair.js, analyzer_damage.js).
  REPAIR: {
    TAB: 'Damaged Demos',
    TITLE: 'Damaged Demos',
    HINT: 'A demo damaged on disk stops playing (or crashes the game) at its first gap. Repair cuts the gaps out and joins the rest so it plays to the end. The repair is saved next to the original as <name>_repaired.dem; the original is never changed.',
    RECURSIVE: 'Include subfolders',
    FIND_BUTTON: 'Find Damaged Demos',
    CANCEL_BUTTON: 'Cancel',
    REPAIR_CHECKED_BUTTON: 'Repair Checked',
    REPAIR_BUTTON: 'Repair',
    CHOOSE_FOLDER_FIRST: 'Choose a folder first.',
    CANCELLING: 'Cancelling…',
    COL_DEMO: 'Demo',
    COL_BREAKS: 'Breaks at',
    COL_SIZE: 'Size',
    COL_RESULT: 'Repair',
    BREAKS_TITLE: 'Where the first gap starts, in time since the recording started. Everything after it is unreadable until repaired.',
    breaksAt: (clock, rest) => `${clock} (${rest} unread)`,
    scanning: (done, total, demo) => `Checking demos: ${done} of ${total}${demo ? ` (${demo})` : ''}`,
    found: (n) => (n === 0 ? 'No damaged demo here.' : `${n} damaged demo${n === 1 ? '' : 's'}.`),
    cancelledFound: (n) => `Cancelled: ${n} damaged demo${n === 1 ? '' : 's'} found before it stopped.`,
    scanFailed: (e) => `Couldn't check the folder: ${e}`,
    footer: (damaged, repaired) => `Damaged demos: ${damaged} | Repaired: ${repaired}`,
    ALREADY_REPAIRED_CHIP: 'Repaired before',
    alreadyRepairedTitle: (name) => `A repair is already saved next to it: ${name}. Repairing again saves a new copy beside it.`,
    NOTHING_TICKED: 'Tick at least one demo to repair.',
    repairingN: (i, n, name) => `Repairing ${i} of ${n}: ${name}`,
    repairedN: (done, n) => `Repaired ${done} of ${n} demo${n === 1 ? '' : 's'}.`,
    // A repair's progress line, one per step (analysis::repair::RepairStep).
    STEP: {
      finding_holes: 'Finding the gaps',
      reading: 'Reading the recording',
      rebuilding_joins: 'Joining the recording across the gaps',
      writing: 'Writing the repaired demo',
      checking: 'Checking the repaired demo',
    },
    progressLine: (step, pct) => `${step}… ${pct}%`,
    // What a repair came to.
    gap: (from, to, lost) => `${from}–${to} (${lost} s)`,
    repaired: (gaps, lost, kept, name) => `Repaired: ${gaps} gap${gaps === 1 ? '' : 's'} cut out (${gaps === 1 ? '' : 'in all '}${lost} s of recording lost, ${kept}% kept). Saved as ${name}.`,
    gapsTitle: (list) => `Gaps, in time since the recording started: ${list}`,
    ENDS_AT_LAST_GAP: 'After that the recording breaks once more and never resumes, so the repair ends at that break.',
    repairedEndCutOff: (kept, name) => `Repaired: the recording breaks and never resumes, so the repair ends at the break (${kept}% kept). Saved as ${name}.`,
    NOT_DAMAGED: 'Not damaged: nothing to repair, nothing saved.',
    refused: (reasons) => `Not saved: the repaired demo failed a check (${reasons.join('; ')}).`,
    CANCELLED: 'Cancelled. Nothing was saved.',
    failed: (e) => `Repair failed: ${e}`,
    SHOW_IN_FOLDER: 'Show in folder',
    // The Master Queue's row button (queue_repair.js).
    QUEUE_BUTTON: 'Damaged · Repair',
    queueButtonTitle: (clock) => `This demo is damaged at ${clock} on the playback clock: it plays, and its highlights cover it, up to there. Repair cuts the damage out and swaps in the repaired copy (saved next to it as <name>_repaired.dem), keeping the highlights' marks, notes and status.`,
    queueHead: (name) => `Repairing ${name}`,
    QUEUE_ADDING: 'adding the repaired demo to the queue',
    queueDoneLine: (elapsed) => `Done in ${elapsed}: repaired 1 demo`,
    queueFailed: (name, err) => `Could not repair ${name}: ${err}`,
    queueNotSaved: (name, why) => `${name} was not repaired: ${why}`,
    carryToast: (name, kept, total) => kept === total
      ? `${name}: kept the marks, notes, status and kill ranges of all ${total} highlight${total === 1 ? '' : 's'} with work on them.`
      : `${name}: kept the work on ${kept} of ${total} highlights. The other ${total - kept} couldn't be found in the repaired demo; their marks and notes are listed in the activity log (View Logs).`,
    carryLog: (name, kept, total, repaired) => `[repair] ${name} -> ${repaired}: kept the work on ${kept} of ${total} highlight(s)`,
    narrowedLog: (time, player, piece) => `[repair]   ${time} ${player}: its Kill Range spans two highlights in ${piece}; kept on the one holding most of its kills`,
    uncarriedLog: (time, player, timeline, reason, work) => `[repair]   NOT KEPT ${time} ${player} (${timeline}): ${reason}. ${work}`,
    UNCARRIED_REASON: {
      'no-piece': 'the repaired demo did not scan',
      'not-found': 'no highlight in the repaired demo has the same player and kills',
      ambiguous: 'the same player and kills fit more than one highlight',
      taken: "another highlight's work went to the same highlight",
    },
    // The game's damaged-demo notice (#1373, damaged_demo_toast.js).
    gameDamaged: (name, clock) => (clock
      ? `${name} is damaged at ${clock}: the game stops playing it there.`
      : `${name} is damaged: the game stops playing it partway.`),
    gameDamagedDuringBatch: (name, clock) => `${name} is damaged${clock ? ` at ${clock}` : ''}: the game stops playing it there. Repair it once the capture batch has ended (its row's Repair in the Master Demo Queue, or Damaged Demos).`,
    GAME_DAMAGED_ACTION: 'Repair',
    gameDamagedWorkingCopyLog: (name) => `[repair] no toast for ${name}: it is one of DoD Studio's own working copies`,
    gameDamagedLog: (path, clock, reason) => `[repair] the game says ${path} is damaged${clock ? ` at ${clock}` : ''} (${reason})`,
    gameRepairStartLog: (name) => `[repair] from the game's notice: repairing ${name}`,
    gameRepairEndLog: (name, verdict, elapsed) => `[repair] from the game's notice: ${name}: ${verdict} in ${elapsed}`,
    queueStartLog: (name) => `[repair] queue: repairing ${name}`,
    queueEndLog: (name, elapsed) => `[repair] queue: ${name} swapped for its repair in ${elapsed}`,
    // The Demo Analyzer's damaged-demo notice (analyzer_damage.js).
    ANALYZER_BUTTON: 'Repair now',
    ANALYZER_BUTTON_TITLE: 'Cut the damage out and save the repaired demo next to this one (<name>_repaired.dem), then open it. This demo is not changed.',
    analyzerDone: (name) => `Repaired, saved as ${name}`,
    // A demo both damaged and multi-map: repair first, then split (#15).
    SPLIT_NEEDS_REPAIR: 'Repair it first: the damage stops a split. Repair & Split does both.',
    // A damaged demo with no break for the repair to cut out (repair_status.js):
    // the damage is inside a frame, so Repair is not offered.
    CANNOT_REPAIR: "Repair can't fix this one: the damage is inside the recording rather than a break in it, so there is no gap to cut out.",
    // Its Split stays greyed out too: the split parses the whole demo and
    // stops at the same damage.
    SPLIT_DAMAGE_CANNOT_REPAIR: "The damage stops a split, and Repair can't cut it out.",
    // The Demo Analyzer's damage notice once the repair's maps were split
    // next to the demo (analyzer_damage.js): nothing is left to repair.
    analyzerAlreadySplit: (maps) => `Already repaired and split into its ${maps.length} maps (${maps.join(', ')}) next to it.`,
    cannotRepairLog: (path) => `Repair: ${path} is damaged inside a frame, with no gap to cut out; Repair is not offered for it, and Split stays greyed out (the damage stops a split too)`,
    // The analyzer's map list stops at the damage; the repair check's byte
    // scan found the maps after it (repair_status.js).
    mapsPastDamageLog: (path, maps) => `Repair: ${path} recorded ${maps.length} maps (${maps.join(', ')}), some after its damage; Repair & Split is offered for it`,
    REPAIR_SPLIT_BUTTON: 'Repair & Split',
    REPAIR_SPLIT_TITLE: 'Repair the damage, then split the repaired demo into one demo per map. Both are saved next to this one, which is not changed.',
    repairSplitRepairing: (text) => `Repairing: ${text}`,
    repairSplitSplitting: (text) => `Splitting the repaired demo: ${text}`,
    repairSplitDone: (gaps, names) => `Repaired (${gaps} gap${gaps === 1 ? '' : 's'} cut out) and split into ${names.length} demo${names.length === 1 ? '' : 's'}: ${names.join(', ')}`,
    queueRepairSplitHead: (name) => `Repairing and splitting ${name}`,
    QUEUE_REPAIR_SPLIT_ADDING: 'adding its maps to the queue',
    queueRepairSplitDoneLine: (elapsed, pieces) => `Done in ${elapsed}: repaired 1 demo and split it into ${pieces}`,
    repairSplitFailed: (e) => `Repair & Split failed: ${e}`,
    mapsCount: (n) => `${n} maps`,
  },
};
