// repair_pane.js
// The Demo Auditor's Damaged Demos tab (#15): every demo in the Target Folder
// with a gap in it (the frames stop making sense partway), and a Repair for
// each one and for the ticked ones. A repair is saved next to the demo as
// <name>_repaired.dem (demo_repair_manager.rs); the original is never
// changed. A demo that recorded more than one map also gets Repair & Split
// (repair_split.js): a split can't read past the damage, so it is repaired
// first. Find shows a bar while it reads the demos; each repair shows its
// own bar and step line in its row, then what it came to.

import { listen } from '@tauri-apps/api/event';
import {
  findDamagedDemos, cancelDamagedScan, repairDemo, cancelDemoRepair, revealInExplorer, logFrontendEvent,
} from './ipc_bridge.js';
import { escapeHtml } from './html.js';
import { STRINGS } from './strings.js';
import { setAuditorFooter } from './auditor_footer.js';
import { fileNameOf } from './path_display.js';
import { clockFloor, durationTextMs } from './time_format.js';
import { sizeText } from './analyzer_damage.js';
import { repairProgressBar, repairResultText, repairGapsTitle } from './repair_view.js';
import { repairThenSplit } from './repair_split.js';

/** Wires the tab. Needs nothing from main.js: it reads the page's Target
 *  Folder and talks to the backend itself. */
export function createRepairPane() {
  const $ = (sel) => document.querySelector(sel);
  const folderInput = $('#audit-target-folder-input');
  const recursive = $('#repair-recursive');
  const findBtn = $('#repair-find-btn');
  const cancelBtn = $('#repair-cancel-btn');
  const checkedBtn = $('#repair-checked-btn');
  const selectAll = $('#repair-select-all');
  const status = $('#repair-status');
  const body = $('#repair-body');
  const progress = $('#repair-progress');
  if (!findBtn || !body) return null;

  const R = STRINGS.REPAIR;
  let scanning = false;
  let repairing = 0;
  // The listed demos and, per path, the row's widgets.
  let shown = [];
  const rows = new Map();
  // Each running repair's bar, by path, fed by `repair_progress`.
  const bars = new Map();
  // The demos a Repair & Split is running on (its bar is fed by repair_split.js).
  const splitting = new Set();
  let repairedCount = 0;

  const updateFooter = () => setAuditorFooter('repair', R.footer(shown.length, repairedCount));
  updateFooter();
  // Find redraws the list, so it waits while a repair runs.
  const refresh = () => {
    findBtn.disabled = !folderInput?.value?.trim() || scanning || repairing > 0;
    checkedBtn.disabled = scanning || repairing > 0 || shown.length === 0;
  };
  folderInput?.addEventListener('input', refresh);
  refresh();

  const showProgress = (pct, text) => {
    progress.hidden = false;
    const fill = progress.querySelector('.progress-bar-fill');
    if (fill) fill.style.width = `${Math.round(pct)}%`;
    status.textContent = text;
  };

  listen('repair_scan_progress', (event) => {
    if (!scanning) return;
    const { done, total, demo } = event.payload || {};
    showProgress(total ? (done / total) * 100 : 0, R.scanning(done, total, demo));
  }).catch(() => {});
  listen('repair_progress', (event) => {
    const bar = bars.get(event.payload?.path);
    bar?.update(event.payload.progress);
  }).catch(() => {});

  findBtn.addEventListener('click', async () => {
    const folder = folderInput?.value?.trim();
    if (!folder) {
      status.textContent = R.CHOOSE_FOLDER_FIRST;
      return;
    }
    scanning = true;
    cancelBtn.disabled = false;
    body.innerHTML = '';
    rows.clear();
    shown = [];
    repairedCount = 0;
    updateFooter();
    refresh();
    showProgress(0, R.scanning(0, 0, ''));
    let cancelled = false;
    try {
      const demos = await findDamagedDemos(folder, !!recursive?.checked);
      cancelled = cancelBtn.dataset.cancelled === '1';
      shown = demos;
      demos.forEach(addRow);
      status.textContent = cancelled ? R.cancelledFound(demos.length) : R.found(demos.length);
    } catch (err) {
      status.textContent = R.scanFailed(err);
    } finally {
      scanning = false;
      delete cancelBtn.dataset.cancelled;
      cancelBtn.disabled = true;
      progress.hidden = true;
      updateFooter();
      refresh();
    }
  });

  cancelBtn.addEventListener('click', async () => {
    status.textContent = R.CANCELLING;
    cancelBtn.disabled = true;
    if (scanning) {
      cancelBtn.dataset.cancelled = '1';
      try { await cancelDamagedScan(); } catch { /* logged by ipc_bridge */ }
      return;
    }
    // A repair run: stop each one that is going.
    for (const path of [...bars.keys(), ...splitting]) {
      try { await cancelDemoRepair(path); } catch { /* logged by ipc_bridge */ }
    }
  });

  selectAll?.addEventListener('change', () => {
    rows.forEach((r) => { r.tick.checked = selectAll.checked; });
  });

  checkedBtn.addEventListener('click', async () => {
    const picked = shown.filter((d) => rows.get(d.path)?.tick.checked);
    if (picked.length === 0) {
      status.textContent = R.NOTHING_TICKED;
      return;
    }
    const started = Date.now();
    logFrontendEvent(`[repair] Auditor: repairing ${picked.length} checked demo(s)`);
    let done = 0;
    cancelBtn.disabled = false;
    for (const [i, d] of picked.entries()) {
      status.textContent = R.repairingN(i + 1, picked.length, fileNameOf(d.path));
      if (await repairRow(d)) done += 1;
    }
    cancelBtn.disabled = true;
    status.textContent = R.repairedN(done, picked.length);
    logFrontendEvent(`[repair] Auditor: ${done} of ${picked.length} checked demo(s) repaired in ${durationTextMs(Date.now() - started)}`);
  });

  /// Adds `d`'s row: tick, name (with a chip when a repair was saved
  /// before), where it breaks, size, and its Repair button.
  function addRow(d) {
    const tr = document.createElement('tr');
    const name = fileNameOf(d.path);
    const maps = (d.maps || []).length > 1
      ? `<span class="split-demo-meta" title="${escapeHtml(d.maps.join(', '))}">${escapeHtml(R.mapsCount(d.maps.length))}</span> `
      : '';
    const before = d.repaired
      ? `<span class="split-done-chip" title="${escapeHtml(R.alreadyRepairedTitle(fileNameOf(d.repaired)))}">${escapeHtml(R.ALREADY_REPAIRED_CHIP)}</span>`
      : '';
    tr.innerHTML = `
      <td><input type="checkbox" class="repair-tick" checked /></td>
      <td><span class="repair-name" title="${escapeHtml(d.path)}">${escapeHtml(name)}</span> ${maps}${before}</td>
      <td title="${escapeHtml(R.BREAKS_TITLE)}">${escapeHtml(R.breaksAt(clockFloor(d.first_hole.after_secs), sizeText(d.first_hole.bytes_unread)))}</td>
      <td>${escapeHtml(sizeText(d.size_bytes))}</td>
      <td class="repair-result-cell"><button type="button" class="repair-row-btn">${escapeHtml(R.REPAIR_BUTTON)}</button>${maps ? ` <button type="button" class="repair-split-btn" title="${escapeHtml(R.REPAIR_SPLIT_TITLE)}">${escapeHtml(R.REPAIR_SPLIT_BUTTON)}</button>` : ''}<span class="repair-result"></span></td>`;
    const row = {
      tick: tr.querySelector('.repair-tick'),
      button: tr.querySelector('.repair-row-btn'),
      splitButton: tr.querySelector('.repair-split-btn'),
      cell: tr.querySelector('.repair-result-cell'),
      result: tr.querySelector('.repair-result'),
    };
    row.button.addEventListener('click', async () => {
      cancelBtn.disabled = false;
      await repairRow(d);
      cancelBtn.disabled = bars.size === 0;
    });
    row.splitButton?.addEventListener('click', async () => {
      cancelBtn.disabled = false;
      await repairSplitRow(d);
      cancelBtn.disabled = bars.size === 0 && splitting.size === 0;
    });
    rows.set(d.path, row);
    body.appendChild(tr);
  }

  /// Repairs one listed demo, its bar in its row while it runs and the
  /// outcome there after. Resolves whether a repair was saved.
  async function repairRow(d) {
    const row = rows.get(d.path);
    // One job per demo: Repair Checked reaching a row whose own Repair or
    // Repair & Split is still going would write a second repair of it.
    if (!row || busy(d.path)) return false;
    repairing += 1;
    refresh();
    row.button.disabled = true;
    if (row.splitButton) row.splitButton.disabled = true;
    row.result.textContent = '';
    const bar = repairProgressBar();
    bars.set(d.path, bar);
    row.cell.appendChild(bar.el);
    try {
      const result = await repairDemo(d.path);
      showResult(row, result);
      if (result.verdict === 'repaired') {
        repairedCount += 1;
        updateFooter();
        return true;
      }
      return false;
    } catch (err) {
      row.result.textContent = R.failed(err);
      return false;
    } finally {
      bar.el.remove();
      bars.delete(d.path);
      row.button.disabled = false;
      if (row.splitButton) row.splitButton.disabled = false;
      repairing -= 1;
      refresh();
    }
  }

  /// Repair & Split on one listed demo: one bar through both steps, then
  /// what it came to. Resolves whether it saved the repair.
  async function repairSplitRow(d) {
    const row = rows.get(d.path);
    if (!row || busy(d.path)) return false;
    repairing += 1;
    refresh();
    row.button.disabled = true;
    row.splitButton.disabled = true;
    row.result.textContent = '';
    const bar = repairProgressBar();
    row.cell.appendChild(bar.el);
    splitting.add(d.path);
    try {
      const { result, written } = await repairThenSplit(d.path, bar.update);
      showResult(row, result);
      if (written) {
        row.result.textContent = R.repairSplitDone(result.holes.length, written.map((w) => fileNameOf(w.path)));
        repairedCount += 1;
        updateFooter();
      }
      return result.verdict === 'repaired';
    } catch (err) {
      row.result.textContent = R.repairSplitFailed(err);
      return false;
    } finally {
      bar.el.remove();
      splitting.delete(d.path);
      row.button.disabled = false;
      row.splitButton.disabled = false;
      repairing -= 1;
      refresh();
    }
  }

  /** Whether a repair (alone or with its split) of the demo at `path` runs. */
  function busy(path) {
    return bars.has(path) || splitting.has(path);
  }

  function showResult(row, result) {
    row.result.textContent = repairResultText(result);
    row.result.title = repairGapsTitle(result);
    if (result.written) {
      const show = document.createElement('button');
      show.type = 'button';
      show.className = 'repair-show-btn';
      show.textContent = R.SHOW_IN_FOLDER;
      show.addEventListener('click', () => revealInExplorer(result.written).catch(() => {}));
      row.result.append(' ', show);
    }
  }

  return { refresh };
}
