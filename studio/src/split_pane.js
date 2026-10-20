// split_pane.js
// The Demo Auditor's Split Maps tab (#624): every demo in the folder that
// recorded more than one map, each map's length, and a button that writes
// the ticked maps out as demos of their own (native::demo_split). Find shows
// a bar through both its steps (finding the demos, then reading each one's
// map lengths), and a demo's card appears once its lengths are in, ready to
// use: nothing on it changes after it shows (#844). While a split runs, each
// ticked map's row shows its Status, bar, size and Show in folder, like a
// Render tab job (#876, split_status.js).

import { listen } from '@tauri-apps/api/event';
import {
  findMultiMapDemos, cancelMultiMapScan, demoMapSegments, splitDemoMaps, cancelDemoSplit, revealInExplorer,
} from './ipc_bridge.js';
import { escapeHtml } from './html.js';
import { splitProgressBar } from './split_progress.js';
import {
  startRows, applyProgress, applyOutcome, applyError, statusText, summaryText,
} from './split_status.js';
import { STRINGS } from './strings.js';
import { clockLong } from './time_format.js';
import { setAuditorFooter } from './auditor_footer.js';

/** A map shorter than this is ticked off by default: almost always the next
 *  map loading as the recording stopped. */
export const SHORT_MAP_SECONDS = 60;

export const fileName = (path) => path.split(/[\\/]/).pop();
const folderOf = (path) => path.slice(0, path.length - fileName(path).length).replace(/[\\/]$/, '');
/** `name` in the folder of `path`, with `path`'s own separator. */
const besidePath = (path, name) => path.slice(0, path.length - fileName(path).length) + name;
const mb = (bytes) => (bytes / 1e6).toFixed(1);

export function initSplitPane() {
  const folderInput = document.querySelector('#audit-target-folder-input');
  const recursive = document.querySelector('#split-recursive');
  const findBtn = document.querySelector('#split-find-btn');
  const cancelBtn = document.querySelector('#split-cancel-btn');
  const status = document.querySelector('#split-status');
  const list = document.querySelector('#split-list');
  const progress = document.querySelector('#split-progress');
  if (!findBtn || !list) return;

  const showProgress = (pct, text) => {
    if (progress) {
      progress.hidden = false;
      const fill = progress.querySelector('.progress-bar-fill');
      if (fill) fill.style.width = `${Math.round(pct)}%`;
    }
    status.textContent = text;
  };
  const hideProgress = () => { if (progress) progress.hidden = true; };
  // Set by Cancel while the lengths are read: the demo being read finishes,
  // and no other starts.
  let stopReading = false;
  let reading = false;

  // Find redraws the list, so it waits while a split runs: a new list
  // would drop that split's progress bar and the files it wrote.
  const refresh = () => { findBtn.disabled = !folderInput?.value?.trim() || scanning || splitting > 0; };
  let scanning = false;
  let splitting = 0;
  // The listed demos, for the status bar's line (#844).
  let shownDemos = [];
  const updateFooter = () => setAuditorFooter('split', STRINGS.SPLIT.footer(
    shownDemos.length, shownDemos.filter((d) => (d.split_files || []).some(Boolean)).length));
  folderInput?.addEventListener('input', refresh);
  refresh();

  listen('split_scan_progress', (event) => {
    if (!scanning || reading) return;
    const { done, total, demo } = event.payload || {};
    showProgress(total ? (done / total) * 50 : 0, STRINGS.SPLIT.scanning(done, total, demo));
  }).catch(() => {});

  findBtn.addEventListener('click', async () => {
    const folder = folderInput?.value?.trim();
    if (!folder) {
      status.textContent = STRINGS.SPLIT.CHOOSE_FOLDER_FIRST;
      return;
    }
    scanning = true;
    reading = false;
    stopReading = false;
    refresh();
    cancelBtn.disabled = false;
    list.innerHTML = '';
    shownDemos = [];
    updateFooter();
    showProgress(0, STRINGS.SPLIT.scanning(0, 0, ''));
    try {
      const demos = await findMultiMapDemos(folder, !!recursive?.checked);
      reading = true;
      const listed = await readAndList(demos);
      status.textContent = listed < demos.length
        ? STRINGS.SPLIT.cancelledListed(listed, demos.length)
        : STRINGS.SPLIT.found(demos.length);
    } catch (err) {
      status.textContent = STRINGS.SPLIT.scanFailed(err);
    } finally {
      scanning = false;
      reading = false;
      hideProgress();
      cancelBtn.disabled = true;
      refresh();
    }
  });

  cancelBtn.addEventListener('click', async () => {
    status.textContent = STRINGS.SPLIT.CANCELLING;
    cancelBtn.disabled = true;
    if (reading) {
      stopReading = true;
      return;
    }
    try { await cancelMultiMapScan(); } catch { /* logged by ipc_bridge */ }
  });

  /// Reads each demo's map lengths, one at a time (each is a full parse), and
  /// lists its card once they're in. Returns how many were listed.
  async function readAndList(demos) {
    for (let i = 0; i < demos.length; i++) {
      if (stopReading) return i;
      showProgress(50 + (i / demos.length) * 50,
        STRINGS.SPLIT.readingLengths(i + 1, demos.length, fileName(demos[i].path)));
      let segs = null;
      let failed = null;
      try {
        segs = await demoMapSegments(demos[i].path);
      } catch (err) {
        failed = err;
      }
      addCard(demos[i], i, segs, failed);
    }
    return demos.length;
  }

  /// Adds `d`'s card with its maps' lengths from `segs` (ticks final: a map
  /// under a minute or already split starts unticked), or `failed`'s reason.
  function addCard(d, i, segs, failed) {
    const length = (k) => {
      const seg = segs?.find((s) => s.index === k);
      return seg ? seg.end_seconds - seg.start_seconds : null;
    };
    const box = document.createElement('div');
    box.className = 'split-demo';
    box.dataset.demo = String(i);
    box.innerHTML = `
        <div class="split-demo-head">
          <strong class="split-demo-name" title="${escapeHtml(d.path)}">${escapeHtml(fileName(d.path))}</strong>
          <span class="split-demo-meta" title="${escapeHtml(STRINGS.SPLIT.SOURCE_TITLE[d.source] || '')}">${mb(d.size_bytes)} MB · ${escapeHtml(STRINGS.SPLIT.mapsCount(d.maps.length))} · ${escapeHtml(folderOf(d.path))}</span>
          ${(d.split_files || []).some(Boolean) ? `<span class="split-done-chip" title="${escapeHtml(STRINGS.SPLIT.ALREADY_SPLIT_TITLE)}">${escapeHtml(STRINGS.SPLIT.ALREADY_SPLIT_CHIP)}</span>` : ''}
        </div>
        <table class="split-maps analyzer-table">
          <thead>
            <tr>
              <th></th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_NUM)}</th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_MAP)}</th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_WHEN)}</th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_DONE)}</th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_STATUS)}</th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_PROGRESS)}</th>
              <th>${escapeHtml(STRINGS.SPLIT.COL_SIZE)}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            ${d.maps.map((m, k) => {
              const len = length(k);
              const done = d.split_files?.[k];
              const short = len !== null && len < SHORT_MAP_SECONDS;
              const cls = [done ? 'split-done' : '', short ? 'split-short' : ''].filter(Boolean).join(' ');
              const title = done ? STRINGS.SPLIT.ALREADY_SPLIT_TITLE : short ? STRINGS.SPLIT.SHORT_MAP_TITLE : '';
              return `
              <tr data-map="${k}"${cls ? ` class="${cls}"` : ''}${title ? ` title="${escapeHtml(title)}"` : ''}>
                <td><input type="checkbox" class="split-keep" data-map="${k}"${done || short ? '' : ' checked'} /></td>
                <td class="split-map-num">${k + 1}</td>
                <td class="split-map-name">${escapeHtml(m)}</td>
                <td class="split-map-when">${len === null ? '' : escapeHtml(clockLong(len))}</td>
                <td class="split-map-done">${escapeHtml(done || '')}</td>
                <td class="split-map-status"></td>
                <td class="split-map-bar"><div class="progress-bar-container" hidden><div class="progress-bar-fill" style="width: 0%;"></div></div></td>
                <td class="split-map-size"></td>
                <td class="split-map-actions"></td>
              </tr>`;
            }).join('')}
          </tbody>
        </table>
        <div class="split-actions">
          <div class="row-inputs">
            <button type="button" class="split-go primary-btn">${escapeHtml(STRINGS.SPLIT.SPLIT_BUTTON)}</button>
            <button type="button" class="split-cancel" hidden>${escapeHtml(STRINGS.SPLIT.CANCEL_SPLIT_BUTTON)}</button>
          </div>
          <span class="split-result">${failed ? escapeHtml(STRINGS.SPLIT.detailsFailed(failed)) : ''}</span>
        </div>`;
    box.querySelector('.split-go').addEventListener('click', () => split(d, box));
    box.querySelector('.split-cancel').addEventListener('click', (e) => {
      e.currentTarget.disabled = true;
      const note = box.querySelector('.split-cancel-note');
      if (note) note.textContent = STRINGS.SPLIT.CANCELLING_SPLIT;
      cancelDemoSplit(d.path).catch(() => {});
    });
    box.querySelectorAll('.split-keep').forEach((c) => c.addEventListener('change', () => updateGo(box)));
    (d.split_files || []).forEach((name, k) => {
      if (name) setReveal(box, k, besidePath(d.path, name));
    });
    updateGo(box);
    list.appendChild(box);
    shownDemos = [...shownDemos, d];
    updateFooter();
  }

  /// Split Checked Maps works only with a map ticked, and not while this
  /// demo's split runs.
  function updateGo(box) {
    const go = box.querySelector('.split-go');
    const none = !box.querySelector('.split-keep:checked');
    go.disabled = none || box.dataset.splitting === '1';
    go.title = none ? STRINGS.SPLIT.NOTHING_TICKED : '';
  }

  /// Map `k`'s Show in folder, for the file at `path`.
  function setReveal(box, k, path) {
    const cell = box.querySelector(`tr[data-map="${k}"] .split-map-actions`);
    if (!cell) return;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'split-reveal';
    btn.textContent = STRINGS.SPLIT.SHOW_IN_FOLDER;
    btn.title = path;
    btn.addEventListener('click', () => revealInExplorer(path).catch(() => {}));
    cell.replaceChildren(btn);
  }

  /// Each ticked map's Status, bar, size and Show in folder (#876).
  function drawRows(box, rows) {
    for (const [k, row] of Object.entries(rows)) {
      const tr = box.querySelector(`tr[data-map="${k}"]`);
      if (!tr) continue;
      tr.dataset.stage = row.stage;
      const status = tr.querySelector('.split-map-status');
      status.textContent = statusText(row);
      status.title = row.error || '';
      const bar = tr.querySelector('.split-map-bar .progress-bar-container');
      bar.hidden = false;
      bar.querySelector('.progress-bar-fill').style.width = `${row.bar}%`;
      if (row.file) {
        tr.querySelector('.split-map-size').textContent = STRINGS.SPLIT.sizeMb(mb(row.file.size_bytes));
        tr.querySelector('.split-map-done').textContent = fileName(row.file.path);
        if (tr.querySelector('.split-reveal')?.title !== row.file.path) setReveal(box, k, row.file.path);
      }
    }
  }

  async function split(demo, box) {
    const keep = [...box.querySelectorAll('.split-keep:checked')].map((c) => Number(c.dataset.map));
    const result = box.querySelector('.split-result');
    const go = box.querySelector('.split-go');
    if (!keep.length) return; // the button is greyed out then
    box.dataset.splitting = '1';
    go.disabled = true;
    // The ticks were read above; they stay as they are while it runs.
    box.querySelectorAll('.split-keep').forEach((c) => { c.disabled = true; });
    const cancel = box.querySelector('.split-cancel');
    cancel.hidden = false;
    cancel.disabled = false;
    splitting += 1;
    refresh();
    // Each ticked map's row says what it's doing; the bar under the button
    // is the whole split's (#876).
    let rows = startRows(keep);
    drawRows(box, rows);
    const bar = splitProgressBar();
    const note = document.createElement('div');
    note.className = 'split-cancel-note';
    result.replaceChildren(bar.el, note);
    const unlisten = await listen('split_progress', (event) => {
      if (event.payload?.path !== demo.path) return;
      bar.update(event.payload.progress);
      rows = applyProgress(rows, event.payload.progress);
      drawRows(box, rows);
    }).catch(() => () => {});
    try {
      const outcome = await splitDemoMaps(demo.path, keep);
      rows = applyOutcome(rows, outcome);
      result.textContent = summaryText(rows);
      demo.split_files = demo.split_files || demo.maps.map(() => null);
      for (const w of (outcome?.written ?? []).filter((x) => Number.isInteger(x.index))) {
        demo.split_files[w.index] = fileName(w.path);
      }
      updateFooter();
    } catch (err) {
      rows = applyError(rows, err);
      result.textContent = STRINGS.SPLIT.splitFailed(err);
    } finally {
      drawRows(box, rows);
      unlisten();
      box.dataset.splitting = '';
      cancel.hidden = true;
      box.querySelectorAll('.split-keep').forEach((c) => { c.disabled = false; });
      updateGo(box);
      splitting -= 1;
      refresh();
    }
  }
}
