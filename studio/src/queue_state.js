// The demo queue's list and which demo is selected (#1106). main.js used to
// hold both and every rule that changes them; this owns them, and the pieces
// that draw (the master list, the detail view) are handed in so the rules can
// be tested on their own.

/**
 * @param {object} deps
 * @param {(demos: object[], selectedIdx: number|null, onSelect?: Function) => void} deps.renderList
 *   Draws the Master Queue; `onSelect` is the row-click handler.
 * @param {(demo: object|null, idx: number|null) => void} deps.renderDetail
 * @param {() => void} deps.clearChecked   Unticks every row's checkbox.
 * @param {() => void} deps.resetMapWarnings
 * @param {(demos: object[]) => void} deps.updateFooter
 * @param {() => void} deps.markDirty      The project has unsaved changes.
 * @param {() => void} [deps.afterLoad]    Runs once a project's demos are in.
 */
export function createQueueState({
  renderList, renderDetail, clearChecked, resetMapWarnings, updateFooter, markDirty, afterLoad,
}) {
  let demos = [];
  let selectedIdx = null;

  const getDemos = () => demos;
  const getSelectedIdx = () => selectedIdx;
  const setSelectedIdx = (idx) => { selectedIdx = idx; };
  const selectedDemo = () => (selectedIdx !== null ? demos[selectedIdx] : null) || null;

  // ── Shared "a demo row was selected" handler ────────────────────────────────
  // Extracted so every renderList call site behaves identically —
  // renderMasterList persists whichever callback it was last given
  // (master_pane.js's currentOnSelectDemo) as the fallback for re-renders
  // that don't pass one, so an inline callback that drifted from this one
  // would apply inconsistently depending on which code path rendered last.
  function select(demo, idx) {
    selectedIdx = idx;
    renderDetail(demo, selectedIdx);
  }

  // Redraws the selected demo's details, when one is selected.
  function redrawSelected() {
    if (selectedIdx !== null && demos[selectedIdx]) renderDetail(demos[selectedIdx], selectedIdx);
  }

  // Redraws the Master Queue with the current selection highlighted.
  function renderQueue() {
    renderList(demos, selectedIdx);
  }

  // A project's demos replace the queue; the first one is selected.
  function load(newDemos) {
    demos = newDemos;
    selectedIdx = demos.length > 0 ? 0 : null;
    renderList(demos, selectedIdx, select);
    if (demos.length > 0) {
      select(demos[0], selectedIdx);
    }
    updateFooter(demos);
    afterLoad?.();
  }

  // The queue drew again after demos were relocated or re-scanned in place;
  // when `changed`, the selected demo's details follow and the project is dirty.
  function refreshAfterRelocation(changed) {
    renderList(demos, selectedIdx, select);
    if (!changed) return;
    if (selectedIdx != null && demos[selectedIdx]) {
      select(demos[selectedIdx], selectedIdx);
    }
    markDirty();
  }

  // Delete callback: remove a demo from the active scan list and re-render.
  // Called by master_pane.js when the 🗑 button is clicked on a row. Returns
  // the new selectedDemoIdx so the caller's own renderMasterList call can
  // pass it through for the row-highlight — master_pane.js doesn't otherwise
  // know this file's selectedDemoIdx, and previously always re-rendered with
  // no selection at all (visually dropping the highlight) even when the
  // deleted row wasn't the selected one and the selection should have held.
  function remove(deletedOriginalIdx, updatedDemos) {
    demos = updatedDemos;
    markDirty();
    // If the deleted demo was the selected one, clear the detail view.
    if (selectedIdx === deletedOriginalIdx) {
      selectedIdx = demos.length > 0 ? 0 : null;
      if (selectedIdx !== null) {
        renderDetail(demos[0], selectedIdx);
      } else {
        renderDetail(null, null);
      }
    } else if (selectedIdx !== null && selectedIdx > deletedOriginalIdx) {
      // Shift selection index down if a demo above it was removed.
      selectedIdx -= 1;
    }
    updateFooter(demos);
    return selectedIdx;
  }

  // Shared by all three Clear actions: swaps the scanned-demo list,
  // resets checkboxes, and re-renders both the queue and the detail view.
  //
  // Selection is preserved by object identity, not just reset to row 0:
  // Clear Untracked/Selected/All can all leave the previously-selected demo
  // still in the queue (it just wasn't the one removed), and jumping the
  // selection to whatever's now first is jarring — you were looking at one
  // demo's highlights and suddenly a different one's are shown. Only falls
  // back to row 0 (or nothing) when the selected demo was actually the one
  // that got removed.
  function replace(newDemos) {
    const previouslySelectedDemo = selectedIdx !== null ? demos[selectedIdx] : null;
    demos = newDemos;
    markDirty();
    const preservedIdx = previouslySelectedDemo ? demos.indexOf(previouslySelectedDemo) : -1;
    selectedIdx = preservedIdx !== -1 ? preservedIdx : (demos.length > 0 ? 0 : null);
    clearChecked();
    updateFooter(demos);
    renderList(demos, selectedIdx);
    renderDetail(selectedIdx !== null ? demos[selectedIdx] : null, selectedIdx);
    // The banner is about demos in the queue. With the queue empty there is
    // nothing left for it to be about.
    if (demos.length === 0) resetMapWarnings();
  }

  return {
    getDemos, getSelectedIdx, setSelectedIdx, selectedDemo,
    select, redrawSelected, renderQueue, load, refreshAfterRelocation, remove, replace,
  };
}
