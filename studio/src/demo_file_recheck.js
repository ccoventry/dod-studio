// demo_file_recheck.js
// A queued demo renamed, moved or deleted while Studio is open. #21's check
// marks missing demos when a project opens and at Start Capture Batch; this
// looks at the queue's files again when the window gets its focus back
// (which is how the user comes back from Explorer) and right before an
// action that opens a demo's file (Launch Preview, Review, Open in Demo
// Analyzer, Open Folder). A file that went gets the queue's missing state
// through project_demos.js, so its row shows Locate and Use found copy and
// its right-click menu dims what needs the file; a file that came back loses
// it. A look is one stat per demo off the window's thread (files_exist); the
// search for a copy runs only for files that just went.

import { filesExist, logFrontendEvent } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { fileNameOf } from './path_display.js';
import { demoFilesRecheckedLine } from './project_log.js';
import { fileChanges, focusRecheckDue } from './demo_file_recheck_logic.js';

const nameOf = (demo) => demo.name || fileNameOf(demo.path);

/**
 * Starts the focus look, and returns `ensurePresent` for the actions.
 * @param {object} app
 *   getDemos() the queue; markDemosGone(demos) and markDemosBack(demos)
 *   project_demos.js's, which set and clear the missing state;
 *   refreshQueue(changed) queue_state.js's refreshAfterRelocation, which
 *   redraws the queue; useFoundCopies(demos) project_demos.js's,
 *   for the toast's button. `exists`, `now` and `target` (files_exist,
 *   Date.now, window) are for tests.
 */
export function createDemoFileRecheck({
  getDemos, markDemosGone, markDemosBack, refreshQueue, useFoundCopies,
  exists = filesExist, now = Date.now, target = window,
}) {
  let lastFocusLook = null;
  let focusLook = null;
  // One look at a time, so a focus look and an action's look never mark the
  // same row twice or log it twice.
  let chain = Promise.resolve();

  function look(demos, options) {
    const run = chain.then(() => lookNow(demos, options));
    chain = run.catch(() => {});
    return run;
  }

  // Resolves to the rows that just went missing, or null when files_exist
  // failed (it logged why).
  async function lookNow(demos, { toastGone }) {
    if (demos.length === 0) return [];
    const started = now();
    const checked = demos.map((demo) => ({ demo, path: demo.path }));
    const answer = await exists(checked.map((c) => c.path));
    if (!Array.isArray(answer)) return null;
    const { gone, back } = fileChanges(checked, answer);
    if (gone.length === 0 && back.length === 0) return [];
    markDemosBack(back);
    const marked = await markDemosGone(gone);
    // The missing state isn't saved, so the project isn't marked changed.
    refreshQueue(false);
    const found = marked.filter((d) => d.foundAt);
    logFrontendEvent(demoFilesRecheckedLine({
      checked: demos.length,
      gone: marked.map(nameOf),
      back: back.map(nameOf),
      found: found.length,
      elapsedMs: now() - started,
    }));
    if (toastGone && marked.length > 0) showGoneToast(marked, found);
    return marked;
  }

  function showGoneToast(marked, found) {
    const action = found.length === 0 ? undefined : {
      label: found.length === 1 ? STRINGS.WORKSPACE.USE_FOUND_COPY_BUTTON : STRINGS.MAIN.USE_ALL_FOUND_COPIES,
      onClick: () => useFoundCopies(found),
    };
    showToast(STRINGS.MAIN.demoFilesGoneToast(marked.map(nameOf), found.length), 'warning', 12000, { action });
  }

  target.addEventListener('focus', () => {
    if (focusLook || !focusRecheckDue(lastFocusLook, now())) return;
    lastFocusLook = now();
    focusLook = look(getDemos(), { toastGone: true })
      .catch((err) => console.error('Demo file look on focus failed:', err))
      .finally(() => { focusLook = null; });
  });

  /**
   * Looks at `demos`' files again before an action that opens them. Resolves
   * true when every one is there. Otherwise the missing ones get the missing
   * state, a toast says which and to use Locate, and it resolves false: the
   * action must not run. A look that itself failed resolves true, so the
   * action runs and reports its own error.
   */
  async function ensurePresent(demos) {
    if ((await look(demos, { toastGone: false })) === null) return true;
    const missing = demos.filter((d) => d.missing);
    if (missing.length === 0) return true;
    const found = missing.filter((d) => d.foundAt).length;
    showToast(STRINGS.MAIN.demoFilesMissing(missing.map(nameOf), found), 'error', 12000);
    return false;
  }

  return { ensurePresent };
}
