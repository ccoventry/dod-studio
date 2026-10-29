// review_library.js
// The review library (#848): every highlight's Review mark, note and Kill
// Range, saved outside the project file (review_library.rs keeps it in the
// app data folder), so a review with no project open isn't lost, and a
// project that adds the demo later can bring the marks in.
//
// Saving: every change that marks the project dirty also runs sync(), which
// compares each highlight's work with what it last saw and saves the ones
// that changed, a demo at a time, a moment after the last change. Marks made
// inside a project are saved too, so the library holds everything ever
// marked; the latest mark of a highlight wins.
//
// Bringing in: a demo sync() hasn't seen before (one a scan just added) is
// looked up by its contents (`source_key`, size + head hash), so a moved,
// renamed or copied demo still matches. Its saved highlights are found again
// by their kills, as a rescan carries work (rescan_carry.js), so a change to
// highlight detection doesn't lose them. Only highlights with no work of
// their own get them. With no project file open they come in at once (they
// are the user's own marks for this queue); with one open the user is
// asked first. A loaded project's demos are only noted (noteLoaded()): the
// project already holds what it wants.

import { reviewLibraryGet, reviewLibraryUpdate } from './ipc_bridge.js';
import { matchRescanHighlights } from './rescan_carry.js';
import { applyCarriedWork, hasWork } from './split_carry.js';
import { isRangeModified } from './take_index.js';
import { fileNameOf, folderOf, shortFolder } from './path_display.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** How long after the last change the library is written. */
export const SAVE_DELAY_MS = 400;

/** The demo's contents key, the same on every highlight a scan made. */
export function demoKey(demo) {
  return (demo?.streaks || []).find((s) => s.source_key)?.source_key || null;
}

/** A highlight's id within its demo: player slot and first/last kill tick
 *  (take_index.js's uid without the path, which differs between copies). */
export function highlightId(streak) {
  const kills = streak.kills || [];
  const first = kills.length ? kills[0][0] : streak.start_tick;
  const last = kills.length ? kills[kills.length - 1][0] : streak.end_tick;
  return `${streak.player_index}#${first}#${last}`;
}

/** True when the highlight has work the library keeps: a mark, a note or a
 *  narrowed Kill Range. Status, ticks and clip names stay in the project. */
export function hasLibraryWork(streak) {
  return !!streak.curation || !!(streak.notes && streak.notes.trim()) || isRangeModified(streak);
}

/** The saved form of a highlight: its work, and what finds it again by its
 *  kills (split_carry.js's matcher), or null when it has no work. */
export function libraryEntry(streak) {
  if (!hasLibraryWork(streak)) return null;
  const last = Math.max((streak.kills || []).length - 1, 0);
  const entry = {
    player_index: streak.player_index,
    kills: streak.kills || [],
    start_index: streak.start_index ?? 0,
    end_index: streak.end_index ?? last,
  };
  if (streak.steam_id) entry.steam_id = streak.steam_id;
  if (streak.target_player) entry.target_player = streak.target_player;
  if (streak.victims) entry.victims = streak.victims;
  if (streak.curation) entry.curation = streak.curation;
  if (streak.notes && streak.notes.trim()) entry.notes = streak.notes;
  return entry;
}

/** What sync() compares: the library's part of a highlight's work. */
export function workSignature(streak) {
  const entry = libraryEntry(streak);
  return entry ? JSON.stringify([entry.curation, entry.notes, entry.start_index, entry.end_index]) : '';
}

/**
 * Finds `saved` (a library demo, `{ highlights: { id: entry } }`) on
 * `demo`'s highlights. Pure. Returns the carries (rescan_carry.js's shape)
 * onto highlights that have no work yet: the demo's own work always wins.
 */
export function matchSavedMarks(saved, demo) {
  const previous = { path: demo.path, streaks: Object.values(saved?.highlights || {}).map((e) => ({ ...e })) };
  const result = matchRescanHighlights(previous, demo);
  return { previous, carried: result.carried.filter((c) => !hasWork(c.to)) };
}

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, hasProjectFile() whether a project file is open,
 *   and onBroughtIn(streaks) to redraw those highlights and the queue.
 *   `bridge` and `ask` are for tests.
 */
export function createReviewLibrary({
  getDemos,
  hasProjectFile = () => false,
  onBroughtIn = () => {},
  bridge = { get: reviewLibraryGet, update: reviewLibraryUpdate },
  ask = askToBringIn,
  saveDelayMs = SAVE_DELAY_MS,
}) {
  const seenDemos = new WeakSet();
  const signatures = new WeakMap();
  // source_key -> { name, changes: Map(id -> streak) }, not yet written. The
  // streak, not its entry: the entry is taken as it is written, so a write
  // tried again after a failure never puts back an older mark.
  const pending = new Map();
  let timer = null;
  // The writes under way, one after another: a batch that started later must
  // not land before, and then under, an older one.
  let writing = Promise.resolve();

  function note(demo) {
    seenDemos.add(demo);
    (demo.streaks || []).forEach((s) => signatures.set(s, workSignature(s)));
  }

  /** A loaded project's demos: noted as they are, nothing saved or offered. */
  function noteLoaded() {
    getDemos().forEach(note);
  }

  function queueChange(key, name, id, streak) {
    if (!pending.has(key)) pending.set(key, { name, changes: new Map() });
    pending.get(key).changes.set(id, streak);
  }

  /** Writes what changed now, after any write already under way. Resolves
   *  once it is on disk (or failed). */
  function flush() {
    if (timer) clearTimeout(timer);
    timer = null;
    const batch = [...pending];
    pending.clear();
    writing = writing.then(() => write(batch));
    return writing;
  }

  async function write(batch) {
    for (const [key, { name, changes }] of batch) {
      const entries = {};
      changes.forEach((streak, id) => { entries[id] = libraryEntry(streak); });
      try {
        await bridge.update(key, name, entries);
      } catch {
        // ipc_bridge has shown the error. Queued again, so the next change
        // or the close tries once more instead of dropping the marks.
        changes.forEach((streak, id) => queueChange(key, name, id, streak));
      }
    }
  }

  /** Saves the highlights whose work changed, and looks up demos not seen
   *  before. Returns the lookup's promise (for tests). */
  function sync() {
    const arrived = [];
    for (const demo of getDemos()) {
      if (!seenDemos.has(demo)) {
        note(demo);
        arrived.push(demo);
        continue;
      }
      const key = demoKey(demo);
      if (!key) continue;
      for (const streak of demo.streaks || []) {
        const signature = workSignature(streak);
        if (!signatures.has(streak)) {
          signatures.set(streak, signature);
          continue;
        }
        if (signatures.get(streak) === signature) continue;
        signatures.set(streak, signature);
        queueChange(key, fileNameOf(demo.path), highlightId(streak), streak);
      }
    }
    if (pending.size > 0) {
      if (timer) clearTimeout(timer);
      timer = setTimeout(flush, saveDelayMs);
    }
    return arrived.length > 0 ? bringIn(arrived) : Promise.resolve(null);
  }

  async function bringIn(demos) {
    const keyed = demos.filter(demoKey);
    if (keyed.length === 0) return null;
    let saved;
    try {
      saved = await bridge.get([...new Set(keyed.map(demoKey))]);
    } catch {
      return null;
    }
    // Demos taken out of the queue while the library was read are left out.
    const queued = new Set(getDemos());
    const plans = keyed
      .filter((demo) => queued.has(demo) && saved?.[demoKey(demo)])
      .map((demo) => ({ demo, ...matchSavedMarks(saved[demoKey(demo)], demo) }))
      .filter((p) => p.carried.length > 0);
    if (plans.length === 0) return null;
    const highlights = plans.reduce((n, p) => n + p.carried.length, 0);
    if (hasProjectFile() && !(await ask(plans, highlights))) return { brought: 0, plans };
    const changed = [];
    plans.forEach(({ previous, carried }) => {
      // Skips any highlight given work while the question was open.
      const still = carried.filter((c) => !hasWork(c.to));
      applyCarriedWork(previous, still);
      still.forEach((c) => {
        signatures.set(c.to, workSignature(c.to));
        changed.push(c.to);
      });
    });
    if (changed.length === 0) return { brought: 0, plans };
    showToast(STRINGS.REVIEW.libraryBroughtInToast(plans.length, changed.length), 'success', 8000);
    onBroughtIn(changed);
    return { brought: changed.length, plans };
  }

  return { sync, noteLoaded, flush };
}

/** Asks before bringing saved marks into an open project. */
function askToBringIn(plans, highlights) {
  return themedConfirm(STRINGS.REVIEW.libraryAskMessage(plans.length, highlights), {
    title: STRINGS.REVIEW.LIBRARY_TITLE,
    confirmLabel: STRINGS.REVIEW.LIBRARY_BRING_IN,
    cancelLabel: STRINGS.REVIEW.LIBRARY_NOT_NOW,
    details: plans.map(({ demo, carried }) => ({
      primary: fileNameOf(demo.path),
      secondary: `${STRINGS.REVIEW.libraryDemoLine(carried.length)} · ${shortFolder(folderOf(demo.path))}`,
      title: demo.path,
    })),
    footer: STRINGS.REVIEW.LIBRARY_ASK_FOOTER,
    focusConfirm: true,
  });
}
