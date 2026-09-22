// master_pane.js
// Renders the Master Demo Queue table with live per-demo status columns,
// selected-streak yield counts, and a functional delete action (M1-M4 parity).

import { isDemoTracked, isRangeModified } from './take_index.js';
import { logFrontendEvent } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { TRASH_ICON_SVG } from './list_editor.js';
import { STRINGS } from './strings.js';

// Feather "bookmark" icon, same stroke="currentColor" pattern as
// list_editor.js's trash icon — WebView2 renders emoji as a flat monochrome
// glyph that ignores CSS color, so an inline SVG is what actually themes.
const TRACKED_ICON_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"></path></svg>`;

/** Which of the three "tracked" reasons (take_index.js's isHighlightTracked)
 *  apply anywhere in this demo, for the tracked badge's tooltip. Must stay
 *  in sync with isHighlightTracked's own checks. */
function describeTrackedReasons(demo) {
  const streaks = demo?.streaks || [];
  let hasStatus = false, hasNotes = false, hasRange = false;
  streaks.forEach(s => {
    if (s.status === 'Pending' || s.status === 'Captured' || s.status === 'Rendered') hasStatus = true;
    if (s.notes && s.notes.trim()) hasNotes = true;
    if (isRangeModified(s)) hasRange = true;
  });
  const reasons = [];
  if (hasStatus) reasons.push(STRINGS.WORKSPACE.REASON_STATUS);
  if (hasNotes) reasons.push(STRINGS.WORKSPACE.REASON_NOTE);
  if (hasRange) reasons.push(STRINGS.WORKSPACE.REASON_RANGE);
  return reasons;
}

let currentDemos = [];
let currentOnSelectDemo = null;
let currentOnDeleteDemo = null;
// Tracks whether the queue was already empty on the last render, so the
// "queue just became empty" notification to currentOnSelectDemo only fires
// once per transition — see renderMasterList's empty-queue branch.
let wasEmptyQueue = false;
// main.js-owned async confirmation for deleting a *tracked* demo via the
// row's 🗑 button — opens the same Clear Selected/All modal (with its
// Save-First option) rather than a lesser plain confirm() just because it's
// one row. Resolves true/false; undefined here only if main.js never wires
// it, in which case the delete handler falls back to a plain confirm().
let currentOnRequestTrackedDeleteConfirm = null;
let currentSearchTerm = '';
// Row checkboxes for Clear Selected (Phase 4) — keyed by demo.path rather
// than array index, since delete-from-queue splices currentDemos and would
// otherwise leave an index-based selection pointing at the wrong rows.
const checkedPaths = new Set();

/** Single source of truth for what the search box currently matches — used
 *  by rendering, the select-all header checkbox, and (via getVisibleDemos,
 *  exported for main.js) every bulk Clear action, so they can't drift apart
 *  on what "visible" means. */
function matchesSearch(demo, term) {
  if (!term) return true;
  return (demo.name && demo.name.toLowerCase().includes(term)) ||
    (demo.path && demo.path.toLowerCase().includes(term)) ||
    (demo.map_name && demo.map_name.toLowerCase().includes(term));
}

/** The demos currently visible under the active search filter (or every
 *  scanned demo, with no filter). Bulk Clear actions (main.js) scope to
 *  this rather than the full queue, matching the select-all checkbox's
 *  existing scoping — a search filter should narrow what a bulk action
 *  touches, not just what's on screen. */
export function getVisibleDemos() {
  return currentDemos.filter((d) => matchesSearch(d, currentSearchTerm));
}

export function initMasterPane(onDeleteDemo, onRequestTrackedDeleteConfirm) {
  if (onDeleteDemo) {
    currentOnDeleteDemo = onDeleteDemo;
  }
  if (onRequestTrackedDeleteConfirm) {
    currentOnRequestTrackedDeleteConfirm = onRequestTrackedDeleteConfirm;
  }

  const searchInput =
    document.querySelector('#demo-search-input') ||
    document.querySelector('#demo-search-filter');

  if (searchInput) {
    searchInput.addEventListener('input', (e) => {
      currentSearchTerm = (e.target.value || '').toLowerCase().trim();
      renderMasterList(currentDemos, null, currentOnSelectDemo);
    });
  }

  const selectAllCb = document.querySelector('#master-select-all-cb');
  if (selectAllCb) {
    selectAllCb.addEventListener('change', (e) => {
      // Header checkbox only ever acts on the currently visible (filtered)
      // rows — checking it with an active search shouldn't silently select
      // demos that are hidden right now.
      const visiblePaths = getVisibleDemos().map(d => d.path);
      if (e.target.checked) {
        visiblePaths.forEach(p => checkedPaths.add(p));
      } else {
        visiblePaths.forEach(p => checkedPaths.delete(p));
      }
      renderMasterList(currentDemos, null, currentOnSelectDemo);
    });
  }

  window.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const activeTab = document.querySelector('.nav-tab-btn.active')?.dataset.nav;
    if (activeTab !== 'workspace') return;

    const rows = Array.from(document.querySelectorAll('#master-demo-table-body tr'));
    if (!rows.length || rows[0].querySelector('.table-empty')) return;

    // Instant select, no separate cursor: selecting a row here just wires up
    // currentOnSelectDemo from an already-in-memory `demo` object (no IPC),
    // so there's no cost to gate behind a commit step. #99.
    const currentIndex = rows.findIndex(r => r.classList.contains('table-row-selected'));
    const dir = e.key === 'ArrowDown' ? 1 : -1;
    const newIndex = currentIndex === -1 ? 0 : Math.min(rows.length - 1, Math.max(0, currentIndex + dir));
    if (newIndex === currentIndex) return;

    e.preventDefault();
    rows[newIndex].click();
    rows[newIndex].scrollIntoView({ block: 'nearest' });
  });
}

// ── Status helpers ────────────────────────────────────────────────────────────

/**
 * Streaks belong to whichever player got the kills, not just the demo's
 * recording player — `demo.streaks` covers every player in the match. The
 * Highlight Details table (detail_pane.js) filters down to the recording
 * player's own streaks before displaying rows; mirror that same filter here
 * so the queue's counts agree with what the table actually shows instead of
 * summing every player in the match.
 *
 * Gate on whether local_player_index actually resolved, not on demo.is_pov
 * — is_pov reflects any SvcHltv/SvcDirector message anywhere in the file,
 * which also fires on an ordinary player-recorded demo whenever an HLTV
 * caster was merely spectating the live match (server-broadcast messages
 * every connected client picks up), so it's not a reliable "no single
 * owner" signal. True HLTV proxy files are already rejected earlier in the
 * pipeline (scan_demo_for_highlights), so None here means "no resolvable
 * owner", not "is_pov".
 */
export function recordingPlayerStreaks(demo) {
  const streaks = demo.streaks || [];
  const recPlayer = demo.local_player_index;
  if (recPlayer === null || recPlayer === undefined) return streaks;

  return streaks.filter((s) => s.player_index === recPlayer);
}

/** Count streaks matching a given status string. An unset status (still
 *  `undefined` — see take_index.js's isHighlightTracked doc comment) counts
 *  as 'None', not 'Pending': Pending is now a deliberate flag the user sets
 *  to mark a highlight for later capture, not the implicit default. */
function countByStatus(streaks, status) {
  return (streaks || []).filter((s) => (s.status || STRINGS.HIGHLIGHTS.STATUS_UNSET_DEFAULT) === status).length;
}

// ── Row selection (Clear Selected) ──────────────────────────────────────────

/** Demo paths currently checked in the Master Queue, for Clear Selected. */
export function getCheckedDemoPaths() {
  return Array.from(checkedPaths);
}

/** Clears all row checkboxes — call after Clear Untracked/All, or any other
 *  action that changes the queue wholesale and makes the old selection
 *  meaningless. Clear Selected does NOT use this for rows it never touched —
 *  see setCheckedDemoPaths below. */
export function clearCheckedPaths() {
  checkedPaths.clear();
  const selectAllCb = document.querySelector('#master-select-all-cb');
  if (selectAllCb) { selectAllCb.checked = false; selectAllCb.indeterminate = false; }
  syncClearSelectedButtonState();
}

/** Replaces the checked set outright — used by Clear Selected (main.js) to
 *  restore the checkboxes on rows it left untouched because a search filter
 *  hid them. Those demos were never acted on, so their checked state
 *  shouldn't be wiped just because clearCheckedPaths() runs as part of the
 *  same click (it resets everything, including rows the action never saw). */
export function setCheckedDemoPaths(paths) {
  checkedPaths.clear();
  (paths || []).forEach((p) => checkedPaths.add(p));
  syncClearSelectedButtonState();
}

/** Clear Selected only makes sense with something checked. */
function syncClearSelectedButtonState() {
  const btn = document.querySelector('#clear-selected-btn');
  if (btn) btn.disabled = checkedPaths.size === 0;
}

function syncSelectAllCheckboxState(visibleDemos) {
  const selectAllCb = document.querySelector('#master-select-all-cb');
  if (!selectAllCb) return;
  const visiblePaths = visibleDemos.map(d => d.path);
  const checkedVisible = visiblePaths.filter(p => checkedPaths.has(p));
  selectAllCb.checked = visiblePaths.length > 0 && checkedVisible.length === visiblePaths.length;
  selectAllCb.indeterminate = checkedVisible.length > 0 && checkedVisible.length < visiblePaths.length;
}

// ── Render ───────────────────────────────────────────────────────────────────

export function renderMasterList(demos, selectedDemoIdx, onSelectDemo) {
  if (demos !== currentDemos) {
    currentDemos = demos || [];
  }
  if (onSelectDemo) {
    currentOnSelectDemo = onSelectDemo;
  }

  const tableBody = document.querySelector('#master-demo-table-body');
  if (!tableBody) return;
  tableBody.innerHTML = '';

  if (!currentDemos || currentDemos.length === 0) {
    tableBody.innerHTML =
      `<tr><td colspan="7" class="table-empty">${STRINGS.WORKSPACE.TABLE_EMPTY_NO_DEMOS_IN_DIRS}</td></tr>`;
    // Notify only on the transition INTO empty, not on every re-render of an
    // already-empty queue. Without this guard, a currentOnSelectDemo(null,
    // null) implementation that calls renderDetailView (Load Session's does)
    // can cascade back into detail_pane.js's onSelectionChange callback,
    // which calls renderMasterList again — still empty, notifies again,
    // forever. Real crash: RangeError, Maximum call stack size exceeded,
    // hit via Clear All emptying the queue after a session had been loaded.
    if (!wasEmptyQueue) {
      wasEmptyQueue = true;
      if (currentOnSelectDemo) currentOnSelectDemo(null, null);
    }
    clearCheckedPaths();
    return;
  }
  wasEmptyQueue = false;

  const searchInput = document.querySelector('#demo-search-input');
  if (searchInput && searchInput.value !== undefined) {
    currentSearchTerm = searchInput.value.toLowerCase().trim();
  }

  const filteredDemos = getVisibleDemos();

  if (filteredDemos.length === 0) {
    tableBody.innerHTML =
      `<tr><td colspan="8" class="table-empty">${STRINGS.WORKSPACE.TABLE_EMPTY_NO_MATCH_SEARCH}</td></tr>`;
    syncClearSelectedButtonState();
    return;
  }

  // Drop checkboxes for demos no longer in the list at all (deleted, or a
  // fresh scan/load replaced the array) so the Set can't accumulate stale
  // paths that no row will ever un-check.
  const allPaths = new Set(currentDemos.map(d => d.path));
  Array.from(checkedPaths).forEach(p => { if (!allPaths.has(p)) checkedPaths.delete(p); });
  syncClearSelectedButtonState();

  filteredDemos.forEach((demo) => {
    const originalIdx = currentDemos.indexOf(demo);
    // Highlights/Pending/Captured/Rendered all count only the recording
    // player's own streaks, matching the Highlight Details table's rows.
    const ownStreaks = recordingPlayerStreaks(demo);

    // ── Derive live column values ─────────────────────────────────────────
    const selected    = ownStreaks.filter((s) => s.selected === true).length;
    const pending     = countByStatus(ownStreaks, 'Pending');    // M4
    const captured    = countByStatus(ownStreaks, 'Captured');   // M4
    const rendered    = countByStatus(ownStreaks, 'Rendered');   // M4

    const tr = document.createElement('tr');
    tr.style.borderBottom = '1px solid #333';
    tr.style.cursor = 'pointer';

    if (selectedDemoIdx === originalIdx) {
      tr.classList.add('table-row-selected');
    }

    // ── Build cells ───────────────────────────────────────────────────────
    // Col 1: Row checkbox (Clear Selected)
    const tdCheck = document.createElement('td');
    tdCheck.style.padding = '6px 8px';
    tdCheck.style.textAlign = 'center';
    const rowCb = document.createElement('input');
    rowCb.type = 'checkbox';
    rowCb.checked = checkedPaths.has(demo.path);
    rowCb.addEventListener('click', (e) => e.stopPropagation()); // don't select the row
    rowCb.addEventListener('change', (e) => {
      if (e.target.checked) checkedPaths.add(demo.path);
      else checkedPaths.delete(demo.path);
      syncSelectAllCheckboxState(filteredDemos);
      syncClearSelectedButtonState();
    });
    tdCheck.appendChild(rowCb);

    // Col 2: Demo name — plus a bookmark badge when the demo has tracked
    // work on it (isDemoTracked, take_index.js), the same predicate Clear
    // Untracked uses, so the row that survives it is visibly explained
    // rather than just a mystery afterward.
    const tdName = document.createElement('td');
    tdName.style.padding = '6px 8px';
    tdName.style.fontWeight = 'bold';
    tdName.style.maxWidth = '200px';
    tdName.style.display = 'flex';
    tdName.style.alignItems = 'center';
    tdName.style.gap = '5px';

    // Name comes first (not the badge) so every row's text starts at the
    // same spot — a badge only some rows have would otherwise push the
    // name over inconsistently and leave them visually unaligned.
    const nameSpan = document.createElement('span');
    // min-width: 0 overrides the flex-item default of min-width: auto,
    // which otherwise refuses to shrink below the untruncated text's
    // intrinsic width and silently defeats the ellipsis below.
    nameSpan.style.minWidth = '0';
    nameSpan.style.flex = '1 1 auto';
    nameSpan.style.overflow = 'hidden';
    nameSpan.style.textOverflow = 'ellipsis';
    nameSpan.style.whiteSpace = 'nowrap';
    nameSpan.title = demo.name || '';
    nameSpan.textContent = demo.name || STRINGS.WORKSPACE.EMPTY_DASH;
    tdName.appendChild(nameSpan);

    const demoIsTracked = isDemoTracked(demo);
    if (demoIsTracked) {
      const badge = document.createElement('span');
      badge.innerHTML = TRACKED_ICON_SVG;
      badge.style.color = '#ffa726';
      badge.style.flexShrink = '0';
      badge.style.display = 'inline-flex';
      const reasons = describeTrackedReasons(demo);
      badge.title = STRINGS.WORKSPACE.trackedBadgeTooltip(reasons);
      tdName.appendChild(badge);
    }

    // Col 3: Highlights (total streak count, recording player only)  [M2]
    const tdYield = document.createElement('td');
    tdYield.style.padding = '6px 8px';
    tdYield.style.textAlign = 'center';
    tdYield.textContent = `${ownStreaks.length}`;

    // Col 4: Selected count — how many of this demo's highlights are checked
    // for the next capture batch, same predicate refreshLaunchGuard uses.
    const tdSelected = document.createElement('td');
    tdSelected.style.padding = '6px 8px';
    tdSelected.style.textAlign = 'center';
    tdSelected.style.color = selected > 0 ? '#64b5f6' : '#555';
    tdSelected.textContent = selected;

    // Col 5: Pending count   [M4]
    const tdPending = document.createElement('td');
    tdPending.style.padding = '6px 8px';
    tdPending.style.textAlign = 'center';
    tdPending.style.color = pending > 0 ? '#ffa726' : '#555';
    tdPending.textContent = pending;

    // Col 6: Captured count  [M4]
    const tdCaptured = document.createElement('td');
    tdCaptured.style.padding = '6px 8px';
    tdCaptured.style.textAlign = 'center';
    tdCaptured.style.color = captured > 0 ? '#4caf50' : '#555';
    tdCaptured.textContent = captured;

    // Col 7: Rendered count  [M4]
    const tdRendered = document.createElement('td');
    tdRendered.style.padding = '6px 8px';
    tdRendered.style.textAlign = 'center';
    tdRendered.style.color = rendered > 0 ? '#2196f3' : '#555';
    tdRendered.textContent = rendered;

    // Col 8: Actions — remove-from-queue only, no status badge  [M3]
    const tdActions = document.createElement('td');
    tdActions.style.padding = '6px 8px';
    tdActions.style.textAlign = 'center';
    tdActions.style.whiteSpace = 'nowrap';

    // Same inline SVG list_editor.js uses for Capture Output/Custom Commands
    // rows, not the 🗑 emoji — WebView2 renders that emoji as a flat
    // monochrome glyph that ignores CSS `color` and reads as a pause icon.
    const deleteBtn = document.createElement('button');
    deleteBtn.type = 'button';
    deleteBtn.className = 'list-editor-remove-btn';
    deleteBtn.innerHTML = TRASH_ICON_SVG;
    deleteBtn.title = STRINGS.WORKSPACE.REMOVE_DEMO_TITLE;
    deleteBtn.setAttribute('aria-label', STRINGS.WORKSPACE.REMOVE_DEMO_TITLE);
    deleteBtn.addEventListener('click', async (e) => {
      e.stopPropagation(); // do not select the row when deleting
      // Untracked deletes stay frictionless (matches Clear Untracked/Clear
      // Selected — nothing worth protecting). Tracked ones get the same
      // Clear Selected/All modal (Save-First included) via main.js's
      // callback; falls back to a plain confirm() only if that was never
      // wired up.
      if (demoIsTracked) {
        const proceed = currentOnRequestTrackedDeleteConfirm
          ? await currentOnRequestTrackedDeleteConfirm(demo)
          : await themedConfirm(STRINGS.WORKSPACE.removeDemoConfirm(demo.name || demo.path));
        if (!proceed) return;
      }
      // Re-resolve the index by identity rather than trusting the closured
      // originalIdx — the modal path awaits user input, during which
      // another action (a different delete, a Clear button) could have
      // already spliced this array and shifted every index after it.
      const currentIdx = currentDemos.indexOf(demo);
      if (currentIdx === -1) return; // already removed some other way
      currentDemos.splice(currentIdx, 1);
      // main.js's onDeleteDemo returns the surviving selectedDemoIdx (the
      // same demo, shifted down; or a fresh default if the deleted row was
      // the one selected) — pass it straight to the re-render below so the
      // row highlight matches Highlight Details instead of always dropping
      // it, even when the deleted row wasn't the one selected.
      const newSelectedIdx = currentOnDeleteDemo ? currentOnDeleteDemo(currentIdx, currentDemos) : null;
      logFrontendEvent(STRINGS.WORKSPACE.rowDeleteLog(demo.name || demo.path, demoIsTracked ? STRINGS.WORKSPACE.TRACKED_NOTE_SUFFIX : ''));
      renderMasterList(currentDemos, newSelectedIdx, currentOnSelectDemo);
    });
    tdActions.appendChild(deleteBtn);

    tr.appendChild(tdCheck);
    tr.appendChild(tdName);
    tr.appendChild(tdYield);
    tr.appendChild(tdSelected);
    tr.appendChild(tdPending);
    tr.appendChild(tdCaptured);
    tr.appendChild(tdRendered);
    tr.appendChild(tdActions);

    // Row click selects the demo (but not when clicking the delete btn)
    tr.addEventListener('click', () => {
      const allRows = tableBody.querySelectorAll('tr');
      allRows.forEach((r) => r.classList.remove('table-row-selected'));
      tr.classList.add('table-row-selected');
      if (currentOnSelectDemo) currentOnSelectDemo(demo, originalIdx);
    });

    tableBody.appendChild(tr);
  });

  syncSelectAllCheckboxState(filteredDemos);
}
