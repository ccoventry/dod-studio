// queue_actions.js
// The Master Demo Queue's Clear Untracked / Selected / All, and the shared
// "tracked work at risk" modal they and the row delete button ask through.
// Moved out of main.js (#683), which owns the queue: it's reassigned there,
// so it's reached through getDemos() rather than held.

import { logFrontendEvent } from './ipc_bridge.js';
import { isDemoTracked } from './take_index.js';
import { getCheckedDemoPaths, setCheckedDemoPaths, getVisibleDemos } from './master_pane.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, replaceDemos(demos) main.js's replaceScannedDemos,
 *   and saveProjectSession() for the modal's Save Session First.
 */
export function createQueueActions({ getDemos, replaceDemos, saveProjectSession }) {
  // One-line callout appended to Clear actions' toasts/summaries whenever an
  // active search filter narrowed what got acted on, so "Clear All" (etc.)
  // doesn't silently do less than its name implies without the user noticing.
  function filterScopeNote(visibleCount, totalCount) {
    return visibleCount < totalCount
      ? STRINGS.MAIN.filterScopeNote(visibleCount, totalCount)
      : '';
  }

  // Clear Untracked — removes only demos with no tracked work (isDemoTracked).
  // Scoped to the currently search-filtered demos, matching the select-all
  // checkbox — a demo hidden by the search box is left untouched no matter
  // its status.
  const clearUntrackedBtn = document.querySelector('#clear-untracked-btn');
  if (clearUntrackedBtn) {
    clearUntrackedBtn.addEventListener('click', () => {
      if (getDemos().length === 0) {
        showToast(STRINGS.MAIN.QUEUE_ALREADY_EMPTY, 'info');
        return;
      }
      const visible = getVisibleDemos();
      if (visible.length === 0) {
        showToast(STRINGS.MAIN.NO_DEMOS_MATCH_SEARCH, 'info');
        return;
      }
      const trackedVisibleCount = visible.filter(isDemoTracked).length;
      const untrackedVisible = new Set(visible.filter((d) => !isDemoTracked(d)).map((d) => d.path));
      if (untrackedVisible.size === 0) {
        showToast(STRINGS.MAIN.NOTHING_TRACKED_TO_CLEAR, 'info');
        return;
      }
      const totalCount = getDemos().length;
      const removedNames = getDemos().filter((d) => untrackedVisible.has(d.path)).map((d) => d.name || d.path);
      replaceDemos(getDemos().filter((d) => !untrackedVisible.has(d.path)));
      // "Kept N with tracked work" only ever refers to visible demos that
      // were actually evaluated and found tracked — never demos hidden by
      // the search filter, which weren't touched for a completely different
      // reason and would otherwise get mislabeled as "kept ... tracked".
      const keptNote = trackedVisibleCount > 0 ? STRINGS.MAIN.keptWithTrackedWork(trackedVisibleCount) : '';
      const scopeNote = filterScopeNote(visible.length, totalCount);
      showToast(
        STRINGS.MAIN.removedUntrackedToast(untrackedVisible.size, keptNote, scopeNote),
        'success'
      );
      logFrontendEvent(STRINGS.MAIN.clearUntrackedLog(untrackedVisible.size, keptNote, scopeNote, removedNames.join(', ')));
    });
  }

  // Shared "tracked work at risk" confirmation modal — a pure yes/no
  // primitive (with an optional Save-First detour), awaited by every caller
  // that needs to warn before removing tracked demos: Clear Selected, Clear
  // All, and the row-level delete button (master_pane.js, via
  // requestTrackedDeleteConfirm below). It does NOT perform the removal
  // itself — Clear All/Selected replace the whole scanned-demo list, while
  // the row delete button needs to preserve its own selection-shift logic
  // instead, so "how to remove" stays with each caller; the modal only
  // answers "should we." Resolves `false` on Cancel, `'confirm'` on Confirm,
  // `'save-first'` once a save actually succeeded — the last two are both
  // truthy but let callers word their success toast accordingly.
  let pendingConfirmResolve = null;
  const clearAllModal = document.querySelector('#clear-all-modal');

  function requestTrackedClearConfirmation(targets, { title, verb, filterNote, confirmLabel }) {
    const trackedCount = targets.filter(isDemoTracked).length;
    const plural = targets.length === 1 ? STRINGS.MAIN.DEMO_SINGULAR : STRINGS.MAIN.DEMO_PLURAL;
    const titleEl = document.querySelector('#clear-all-title');
    if (titleEl) titleEl.textContent = title;
    const confirmBtnEl = document.querySelector('#clear-all-confirm-btn');
    if (confirmBtnEl) confirmBtnEl.textContent = confirmLabel || STRINGS.MAIN.CLEAR_ANYWAY_DEFAULT;
    const summaryEl = document.querySelector('#clear-all-summary');
    if (summaryEl) {
      summaryEl.textContent = (trackedCount > 0
        ? STRINGS.MAIN.clearSummaryTracked(verb, targets.length, plural, trackedCount)
        : STRINGS.MAIN.clearSummaryUntracked(verb, targets.length, plural)
      ) + (filterNote || '');
    }
    if (clearAllModal) clearAllModal.style.display = 'flex';
    return new Promise(resolve => { pendingConfirmResolve = resolve; });
  }

  if (clearAllModal) {
    document.querySelector('#clear-all-cancel-btn')?.addEventListener('click', () => {
      clearAllModal.style.display = 'none';
      pendingConfirmResolve?.(false);
      pendingConfirmResolve = null;
    });
    document.querySelector('#clear-all-confirm-btn')?.addEventListener('click', () => {
      clearAllModal.style.display = 'none';
      pendingConfirmResolve?.('confirm');
      pendingConfirmResolve = null;
    });
    document.querySelector('#clear-all-save-first-btn')?.addEventListener('click', async () => {
      // Saves the whole current queue (not just whatever's about to be
      // removed) — the point is that everything at risk is still
      // recoverable from the saved file afterward, whether this is a
      // single tracked delete, Clear Selected, or Clear All.
      const saved = await saveProjectSession();
      if (!saved) return; // leave the modal open — nothing was lost yet
      clearAllModal.style.display = 'none';
      pendingConfirmResolve?.('save-first');
      pendingConfirmResolve = null;
    });
  }

  // Clear Selected — removes checked rows regardless of status, same in
  // both modes (the user explicitly checked them). Whenever any checked
  // demo is tracked, escalate from a plain confirm() to the shared modal.
  // Scoped to visible rows too: a row checked, then hidden by a later
  // search, is left in the queue AND stays checked — the action never saw
  // it, so it shouldn't lose that selection just because clearCheckedPaths()
  // (inside replaceScannedDemos) resets everything by default.
  const clearSelectedBtn = document.querySelector('#clear-selected-btn');
  if (clearSelectedBtn) {
    clearSelectedBtn.addEventListener('click', async () => {
      const checkedPaths = new Set(getCheckedDemoPaths());
      if (checkedPaths.size === 0) {
        showToast(STRINGS.MAIN.NO_DEMOS_SELECTED, 'info');
        return;
      }
      const visiblePaths = new Set(getVisibleDemos().map((d) => d.path));
      const targets = getDemos().filter(d => checkedPaths.has(d.path) && visiblePaths.has(d.path));
      if (targets.length === 0) {
        showToast(STRINGS.MAIN.allSelectedHiddenToast(checkedPaths.size), 'info');
        return;
      }
      const hiddenCheckedCount = checkedPaths.size - targets.length;
      const hiddenNote = hiddenCheckedCount > 0
        ? STRINGS.MAIN.hiddenCheckedNote(hiddenCheckedCount)
        : '';
      let savedFirst = false;
      if (targets.some(isDemoTracked)) {
        const outcome = await requestTrackedClearConfirmation(targets, { title: STRINGS.MAIN.CLEAR_SELECTED_TITLE, verb: STRINGS.MAIN.VERB_REMOVES, filterNote: hiddenNote, confirmLabel: STRINGS.MAIN.CLEAR_SELECTED_ANYWAY });
        if (!outcome) return;
        savedFirst = outcome === 'save-first';
      } else if (!(await themedConfirm(STRINGS.MAIN.removeSelectedConfirm(targets.length, hiddenNote), { title: STRINGS.MAIN.CLEAR_SELECTED_TITLE }))) {
        return;
      }
      const removePaths = new Set(targets.map((d) => d.path));
      const removedNames = targets.map((d) => d.name || d.path);
      // Preserve checkboxes on rows the action never touched (checked, but
      // hidden by the search filter) — captured before replaceScannedDemos
      // wipes the whole checked set via clearCheckedPaths().
      const survivingHiddenChecked = Array.from(checkedPaths).filter((p) => !removePaths.has(p));
      replaceDemos(getDemos().filter(d => !removePaths.has(d.path)));
      if (survivingHiddenChecked.length > 0) setCheckedDemoPaths(survivingHiddenChecked);
      showToast(STRINGS.MAIN.removedSelectedToast(savedFirst, targets.length, hiddenNote), 'success');
      logFrontendEvent(STRINGS.MAIN.clearSelectedLog(targets.length, savedFirst ? STRINGS.MAIN.SAVED_SESSION_FIRST_NOTE : '', hiddenNote, removedNames.join(', ')));
    });
  }

  // Clear All — escalates to the shared modal (enumerating what would be
  // lost, offering to save first) whenever something tracked is actually at
  // risk, same threshold as Clear Selected/row delete. Also scoped to the
  // search filter, same as the other two Clear actions — "All" means "all
  // visible," with an explicit callout whenever that's fewer than the full
  // queue, so it never silently does less than its name implies.
  const clearAllBtn = document.querySelector('#clear-all-btn');
  if (clearAllBtn) {
    clearAllBtn.addEventListener('click', async () => {
      if (getDemos().length === 0) {
        showToast(STRINGS.MAIN.QUEUE_ALREADY_EMPTY, 'info');
        return;
      }
      const targets = getVisibleDemos();
      if (targets.length === 0) {
        showToast(STRINGS.MAIN.NO_DEMOS_MATCH_SEARCH, 'info');
        return;
      }
      const note = filterScopeNote(targets.length, getDemos().length);
      let savedFirst = false;
      if (targets.some(isDemoTracked)) {
        const outcome = await requestTrackedClearConfirmation(targets, { title: STRINGS.MAIN.CLEAR_ALL_TITLE, verb: STRINGS.MAIN.VERB_REMOVES, filterNote: note, confirmLabel: STRINGS.MAIN.CLEAR_ALL_ANYWAY });
        if (!outcome) return;
        savedFirst = outcome === 'save-first';
      } else if (!(await themedConfirm(STRINGS.MAIN.removeAllConfirm(targets.length, note), { title: STRINGS.MAIN.CLEAR_ALL_TITLE }))) {
        return;
      }
      const removePaths = new Set(targets.map((d) => d.path));
      const removedNames = targets.map((d) => d.name || d.path);
      replaceDemos(getDemos().filter((d) => !removePaths.has(d.path)));
      showToast(STRINGS.MAIN.clearedAllToast(savedFirst, targets.length, note), 'success');
      logFrontendEvent(STRINGS.MAIN.clearAllLog(targets.length, savedFirst ? STRINGS.MAIN.SAVED_SESSION_FIRST_NOTE : '', note, removedNames.join(', ')));
    });
  }

  // Single-row tracked delete (master_pane.js's 🗑 button) reuses the same
  // modal via this thin wrapper, so a tracked demo gets the exact same
  // Save-First affordance as Clear Selected/All instead of a lesser plain
  // confirm() just because it's one row. Returns whether the caller should
  // proceed — master_pane.js still owns the actual splice + selection-shift
  // logic, since that's specific to a single-row delete.
  async function requestTrackedDeleteConfirm(demo) {
    const outcome = await requestTrackedClearConfirmation([demo], { title: STRINGS.MAIN.REMOVE_TRACKED_DEMO_TITLE, verb: STRINGS.MAIN.VERB_REMOVES, confirmLabel: STRINGS.MAIN.REMOVE_ANYWAY });
    return !!outcome;
  }

  return { requestTrackedClearConfirmation, requestTrackedDeleteConfirm };
}
