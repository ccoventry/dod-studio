import init, { analyzeDemo } from './pkg/web_analyzer.js';
import { STRINGS } from './strings.js';
import { setReport, initSubtabs, renderActiveTab } from './render.js';
import { selectDemoFiles, buildExportEntry, toJson, jsonFileNameFor, batchFileName } from './export.js';

const dropzone = document.querySelector('#dropzone');
const fileInput = document.querySelector('#file-input');
const fileIndicator = document.querySelector('#current-file');
const tabContent = document.querySelector('#analyzer-tab-content');
const batchPanel = document.querySelector('#batch-panel');
const batchSummary = document.querySelector('#batch-summary');
const batchList = document.querySelector('#batch-list');
const batchDownloadAll = document.querySelector('#batch-download-all');

let wasmReady = init({ module_or_path: new URL('./pkg/web_analyzer_bg.wasm', import.meta.url) });

// Bumped on every new pick/drop, so a batch still running from an earlier
// pick stops at its next file instead of writing into the new one's list.
let generation = 0;
// The current batch: one { file, analysis, error, status, pct, row } per demo.
let batch = [];
let viewedIndex = -1;

function setFileIndicator(text) {
  if (fileIndicator) fileIndicator.textContent = text || STRINGS.ANALYZER.NO_DEMO_LOADED;
}

function showReport(file, analysis) {
  setReport({
    file_name: file.name,
    file_size_mb: file.size / 1_048_576,
    file_created_unix_secs: file.lastModified ? Math.floor(file.lastModified / 1000) : 0,
    demo_info: analysis.demo_info,
    state: analysis.state,
  });
  setFileIndicator(file.name);
  renderActiveTab();
}

function showAnalyzing(pct) {
  if (!tabContent) return;
  tabContent.innerHTML = pct == null
    ? `<p class="analyzer-empty">${STRINGS.ANALYZER.ANALYZING_DEMO_ELLIPSIS}</p>`
    : `<p class="analyzer-empty">${STRINGS.ANALYZER.analyzingDemoPct(pct)}</p>`;
}

function showFailed(err) {
  if (tabContent) {
    tabContent.innerHTML = `<p class="analyzer-empty" style="color:#f44336;">${STRINGS.ANALYZER.analyzeFailed(String(err))}</p>`;
  }
}

async function runAnalysis(file, onPct) {
  await wasmReady;
  const bytes = new Uint8Array(await file.arrayBuffer());
  return analyzeDemo(bytes, (processed, total) => {
    if (!total) return;
    onPct(Math.min(100, Math.round((processed / total) * 100)));
  });
}

function downloadJson(fileName, value) {
  const blob = new Blob([toJson(value)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

// ── Single demo (unchanged from before batch support) ───────────────────────

async function loadDemoFile(file) {
  if (!file) return;
  const gen = ++generation;
  batch = [];
  if (batchPanel) batchPanel.hidden = true;
  setFileIndicator(STRINGS.ANALYZER.ANALYZING_ELLIPSIS);
  showAnalyzing(null);

  try {
    const analysis = await runAnalysis(file, (pct) => {
      setFileIndicator(STRINGS.ANALYZER.analyzingPct(pct));
      showAnalyzing(pct);
    });
    if (gen !== generation) return;
    showReport(file, analysis);
  } catch (err) {
    if (gen !== generation) return;
    console.error('Failed to analyze demo:', err);
    showFailed(err);
    setFileIndicator('');
  }
}

// ── Several demos (#102) ────────────────────────────────────────────────────

function statusText(item) {
  switch (item.status) {
    case 'analyzing': return item.pct == null ? STRINGS.ANALYZER.ANALYZING_ELLIPSIS : STRINGS.BATCH.statusAnalyzingPct(item.pct);
    case 'done': return STRINGS.BATCH.STATUS_DONE;
    case 'failed': return STRINGS.BATCH.statusFailed(item.error);
    default: return STRINGS.BATCH.STATUS_WAITING;
  }
}

function updateRow(item, index) {
  const { row } = item;
  row.classList.toggle('viewing', index === viewedIndex);
  row.classList.toggle('failed', item.status === 'failed');
  const status = row.querySelector('.batch-status');
  status.textContent = statusText(item);
  // A parse error can be long; the row truncates it, the tooltip doesn't.
  status.title = item.status === 'failed' ? status.textContent : '';
  row.querySelector('.batch-view').disabled = item.status !== 'done';
  row.querySelector('.batch-json').disabled = item.status !== 'done' && item.status !== 'failed';
}

function updateSummary() {
  const done = batch.filter((b) => b.status === 'done').length;
  const failed = batch.filter((b) => b.status === 'failed').length;
  if (batchSummary) batchSummary.textContent = `— ${STRINGS.BATCH.batchSummary(batch.length, done, failed)}`;
  if (batchDownloadAll) batchDownloadAll.disabled = done + failed < batch.length;
}

function viewBatchItem(index) {
  const item = batch[index];
  if (!item || item.status !== 'done') return;
  viewedIndex = index;
  batch.forEach(updateRow);
  showReport(item.file, item.analysis);
}

function exportEntry(item) {
  return buildExportEntry(item.file, item.analysis, item.error);
}

function buildRow(item, index) {
  const li = document.createElement('li');
  li.className = 'batch-row';

  const name = document.createElement('span');
  name.className = 'batch-name';
  name.textContent = item.file.name;
  name.title = item.file.name;

  const status = document.createElement('span');
  status.className = 'batch-status';

  const view = document.createElement('button');
  view.type = 'button';
  view.className = 'batch-view';
  view.textContent = STRINGS.BATCH.VIEW_BUTTON;
  view.title = STRINGS.BATCH.VIEW_BUTTON_TITLE;
  view.addEventListener('click', () => viewBatchItem(index));

  const json = document.createElement('button');
  json.type = 'button';
  json.className = 'batch-json';
  json.textContent = STRINGS.BATCH.DOWNLOAD_ONE_BUTTON;
  json.title = STRINGS.BATCH.DOWNLOAD_ONE_TITLE;
  json.addEventListener('click', () => downloadJson(jsonFileNameFor(item.file.name), exportEntry(item)));

  li.append(name, status, view, json);
  return li;
}

// The analysis itself runs synchronously on the main thread, so give the
// browser a frame to paint the list's status between demos.
function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)));
}

async function loadDemoFiles(files) {
  const gen = ++generation;
  batch = files.map((file) => ({ file, analysis: null, error: null, status: 'waiting', pct: null, row: null }));
  viewedIndex = -1;

  if (batchList) {
    batchList.replaceChildren();
    batch.forEach((item, i) => {
      item.row = buildRow(item, i);
      batchList.appendChild(item.row);
      updateRow(item, i);
    });
  }
  if (batchPanel) batchPanel.hidden = false;
  updateSummary();
  setFileIndicator(STRINGS.ANALYZER.ANALYZING_ELLIPSIS);
  showAnalyzing(null);

  for (let i = 0; i < batch.length; i++) {
    const item = batch[i];
    item.status = 'analyzing';
    updateRow(item, i);
    await nextFrame();
    if (gen !== generation) return;

    try {
      item.analysis = await runAnalysis(item.file, (pct) => {
        item.pct = pct;
        updateRow(item, i);
        if (viewedIndex < 0) showAnalyzing(pct);
      });
      if (gen !== generation) return;
      item.status = 'done';
    } catch (err) {
      if (gen !== generation) return;
      console.error(`Failed to analyze ${item.file.name}:`, err);
      item.status = 'failed';
      item.error = String(err);
    }
    updateRow(item, i);
    updateSummary();
    // Show the first demo that works as soon as it's ready; after that the
    // report stays on whichever demo the user picked with View.
    if (item.status === 'done' && viewedIndex < 0) viewBatchItem(i);
  }

  if (viewedIndex < 0) {
    // Every demo failed.
    showFailed(batch[batch.length - 1]?.error ?? '');
    setFileIndicator('');
  }
}

function handleFiles(fileList) {
  const all = Array.from(fileList || []);
  if (all.length === 0) return;
  const files = selectDemoFiles(all);
  if (files.length === 0) {
    generation++;
    batch = [];
    if (batchPanel) batchPanel.hidden = true;
    if (tabContent) tabContent.innerHTML = `<p class="analyzer-empty">${STRINGS.BATCH.NO_DEMO_FILES}</p>`;
    setFileIndicator('');
    return;
  }
  if (files.length === 1) loadDemoFile(files[0]);
  else loadDemoFiles(files);
}

batchDownloadAll?.addEventListener('click', () => {
  if (batch.length === 0) return;
  downloadJson(batchFileName(batch.length), batch.map(exportEntry));
});
if (batchDownloadAll) {
  batchDownloadAll.textContent = STRINGS.BATCH.DOWNLOAD_ALL_BUTTON;
  batchDownloadAll.title = STRINGS.BATCH.DOWNLOAD_ALL_TITLE;
}

dropzone?.addEventListener('click', () => fileInput?.click());
fileInput?.addEventListener('change', () => {
  if (fileInput.files) handleFiles(fileInput.files);
  // Clear it so picking the same file(s) again still fires `change`.
  fileInput.value = '';
});

['dragenter', 'dragover'].forEach((evt) => {
  dropzone?.addEventListener(evt, (e) => {
    e.preventDefault();
    dropzone.classList.add('dragover');
  });
});
['dragleave', 'drop'].forEach((evt) => {
  dropzone?.addEventListener(evt, (e) => {
    e.preventDefault();
    dropzone.classList.remove('dragover');
  });
});
dropzone?.addEventListener('drop', (e) => {
  handleFiles(e.dataTransfer?.files);
});

initSubtabs();
setFileIndicator('');
renderActiveTab();
