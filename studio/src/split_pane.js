// split_pane.js
// The Demo Auditor's Split Maps tab (#624): every demo in the folder that
// recorded more than one map, each map's start and length, and a button that
// writes the ticked maps out as demos of their own (native::demo_split).

import { listen } from '@tauri-apps/api/event';
import {
  findMultiMapDemos, cancelMultiMapScan, demoMapSegments, splitDemoMaps, revealInExplorer,
} from './ipc_bridge.js';
import { escapeHtml } from './html.js';
import { splitProgressBar } from './split_progress.js';
import { STRINGS } from './strings.js';
import { clockLong } from './time_format.js';
import { setAuditorFooter } from './auditor_footer.js';

/** A map shorter than this is ticked off by default: almost always the next
 *  map loading as the recording stopped. */
export const SHORT_MAP_SECONDS = 60;

export const fileName = (path) => path.split(/[\\/]/).pop();
const folderOf = (path) => path.slice(0, path.length - fileName(path).length).replace(/[\\/]$/, '');
const mb = (bytes) => (bytes / 1e6).toFixed(1);

export function initSplitPane() {
  const folderInput = document.querySelector('#audit-target-folder-input');
  const recursive = document.querySelector('#split-recursive');
  const findBtn = document.querySelector('#split-find-btn');
  const cancelBtn = document.querySelector('#split-cancel-btn');
  const status = document.querySelector('#split-status');
  const list = document.querySelector('#split-list');
  if (!findBtn || !list) return;

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
    if (!scanning) return;
    const { done, total, demo } = event.payload || {};
    status.textContent = STRINGS.SPLIT.scanning(done, total, demo);
  }).catch(() => {});

  findBtn.addEventListener('click', async () => {
    const folder = folderInput?.value?.trim();
    if (!folder) {
      status.textContent = STRINGS.SPLIT.CHOOSE_FOLDER_FIRST;
      return;
    }
    scanning = true;
    refresh();
    cancelBtn.disabled = false;
    list.innerHTML = '';
    status.textContent = STRINGS.SPLIT.scanning(0, 0, '');
    try {
      const demos = await findMultiMapDemos(folder, !!recursive?.checked);
      status.textContent = STRINGS.SPLIT.found(demos.length);
      renderDemos(demos);
    } catch (err) {
      status.textContent = STRINGS.SPLIT.scanFailed(err);
    } finally {
      scanning = false;
      cancelBtn.disabled = true;
      refresh();
    }
  });

  cancelBtn.addEventListener('click', async () => {
    status.textContent = STRINGS.SPLIT.CANCELLING;
    cancelBtn.disabled = true;
    try { await cancelMultiMapScan(); } catch { /* logged by ipc_bridge */ }
  });

  function renderDemos(demos) {
    list.innerHTML = demos.map((d, i) => `
      <div class="split-demo" data-demo="${i}">
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
            </tr>
          </thead>
          <tbody>
            ${d.maps.map((m, k) => `
              <tr data-map="${k}"${d.split_files?.[k] ? ` class="split-done" title="${escapeHtml(STRINGS.SPLIT.ALREADY_SPLIT_TITLE)}"` : ''}>
                <td><input type="checkbox" class="split-keep" data-map="${k}"${d.split_files?.[k] ? '' : ' checked'} disabled /></td>
                <td class="split-map-num">${k + 1}</td>
                <td class="split-map-name">${escapeHtml(m)}</td>
                <td class="split-map-when">${escapeHtml(STRINGS.SPLIT.LOADING_DETAILS)}</td>
                <td class="split-map-done">${escapeHtml(d.split_files?.[k] || '')}</td>
              </tr>`).join('')}
          </tbody>
        </table>
        <div class="split-actions">
          <button type="button" class="split-go primary-btn" disabled title="${escapeHtml(STRINGS.SPLIT.WAIT_FOR_LENGTHS_TITLE)}">${escapeHtml(STRINGS.SPLIT.SPLIT_BUTTON)}</button>
          <span class="split-result"></span>
        </div>
      </div>`).join('');
    demos.forEach((d, i) => {
      const box = list.querySelector(`.split-demo[data-demo="${i}"]`);
      box.querySelector('.split-go').addEventListener('click', () => split(d, box));
    });
    shownDemos = demos;
    updateFooter();
    loadDetails(demos);
  }

  // One demo at a time: each is a full parse.
  async function loadDetails(demos) {
    for (let i = 0; i < demos.length; i++) {
      const box = list.querySelector(`.split-demo[data-demo="${i}"]`);
      if (!box) return;
      try {
        const segs = await demoMapSegments(demos[i].path);
        for (const seg of segs) {
          const row = box.querySelector(`tr[data-map="${seg.index}"]`);
          if (!row) continue;
          const length = seg.end_seconds - seg.start_seconds;
          row.querySelector('.split-map-when').textContent =
            clockLong(length);
          if (length < SHORT_MAP_SECONDS) {
            row.querySelector('.split-keep').checked = false;
            row.title = STRINGS.SPLIT.SHORT_MAP_TITLE;
            row.classList.add('split-short');
          }
        }
      } catch (err) {
        box.querySelectorAll('.split-map-when').forEach((c) => { c.textContent = ''; });
        box.querySelector('.split-result').textContent = STRINGS.SPLIT.detailsFailed(err);
      }
      // Split waits for the lengths: they untick a map under a minute, and a
      // split started before them would write it anyway. After a failed read
      // the button works, so the split itself can say what's wrong.
      const go = box.querySelector('.split-go');
      go.disabled = false;
      go.removeAttribute('title');
      // The tick boxes too: their final state waits for the lengths.
      box.querySelectorAll('.split-keep').forEach((c) => { c.disabled = false; });
    }
  }

  async function split(demo, box) {
    const keep = [...box.querySelectorAll('.split-keep:checked')].map((c) => Number(c.dataset.map));
    const result = box.querySelector('.split-result');
    const go = box.querySelector('.split-go');
    if (!keep.length) {
      result.textContent = STRINGS.SPLIT.NOTHING_TICKED;
      return;
    }
    go.disabled = true;
    splitting += 1;
    refresh();
    const bar = splitProgressBar();
    result.replaceChildren(bar.el);
    const unlisten = await listen('split_progress', (event) => {
      if (event.payload?.path === demo.path) bar.update(event.payload.progress);
    }).catch(() => () => {});
    try {
      const written = await splitDemoMaps(demo.path, keep);
      result.innerHTML = `${escapeHtml(STRINGS.SPLIT.wrote(written.length))}<ul class="split-written">${
        written.map((w, k) => `<li>${escapeHtml(STRINGS.SPLIT.writtenLine(fileName(w.path), clockLong(w.seconds), mb(w.size_bytes)))}
          <button type="button" class="split-reveal" data-k="${k}">${escapeHtml(STRINGS.SPLIT.SHOW_IN_FOLDER)}</button></li>`).join('')
      }</ul>`;
      result.querySelectorAll('.split-reveal').forEach((b) => {
        b.addEventListener('click', () => revealInExplorer(written[Number(b.dataset.k)].path).catch(() => {}));
      });
      demo.split_files = demo.split_files || demo.maps.map(() => null);
      for (const w of written.filter((x) => Number.isInteger(x.index))) {
        demo.split_files[w.index] = fileName(w.path);
        const cell = box.querySelector(`tr[data-map="${w.index}"] .split-map-done`);
        if (cell) cell.textContent = fileName(w.path);
      }
      updateFooter();
    } catch (err) {
      result.textContent = STRINGS.SPLIT.splitFailed(err);
    } finally {
      unlisten();
      go.disabled = false;
      splitting -= 1;
      refresh();
    }
  }
}
