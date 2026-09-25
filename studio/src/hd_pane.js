// hd_pane.js — the HD Textures page (#372): what is built under
// <game>/dod/dodstudio_hd, the movie.cfg lines to use it, downloading the
// upscaler (and a Python when the PC has none), and running the build.
// The build is goldsrc-hooks/tools/hd's own scripts, so this page and the
// command line always make the same files.

import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { hdStatus, hdSetupTools, hdBuild, hdCancel, hdSetPython, hdSetUpscaler } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

function formatSize(bytes) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function formatElapsed(secs) {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return m ? `${m}m ${String(s).padStart(2, '0')}s` : `${s}s`;
}

// The order the build runs types in (quickest first), as build_all.py's TYPES.
const BUILD_TYPES = ['sky', 'sprites', 'models', 'detail', 'world'];

export function initHdPane() {
  const statusText = document.querySelector('#hd-status-text');
  const statusHead = document.querySelector('#hd-status-head');
  const statusBody = document.querySelector('#hd-status-body');
  const refreshBtn = document.querySelector('#hd-refresh-btn');
  const styleSelect = document.querySelector('#hd-style-select');
  const cfgLines = document.querySelector('#hd-cfg-lines');
  const copyBtn = document.querySelector('#hd-copy-cfg-btn');
  const toolsText = document.querySelector('#hd-tools-text');
  const upscalerPickBtn = document.querySelector('#hd-upscaler-pick-btn');
  const upscalerResetBtn = document.querySelector('#hd-upscaler-reset-btn');
  const pythonText = document.querySelector('#hd-python-text');
  const pythonPickBtn = document.querySelector('#hd-python-pick-btn');
  const pythonResetBtn = document.querySelector('#hd-python-reset-btn');
  const setupBtn = document.querySelector('#hd-setup-btn');
  const cancelBtn = document.querySelector('#hd-setup-cancel-btn');
  const progressText = document.querySelector('#hd-setup-progress');
  const buildStyles = document.querySelector('#hd-build-styles');
  const buildTypes = document.querySelector('#hd-build-types');
  const buildBtn = document.querySelector('#hd-build-btn');
  const buildCancelBtn = document.querySelector('#hd-build-cancel-btn');
  const buildProgress = document.querySelector('#hd-build-progress');
  const buildLine = document.querySelector('#hd-build-line');
  const realesrganLine = document.querySelector('#hd-realesrgan-line');
  const footerSummary = document.querySelector('#footer-hd-summary');
  if (!statusBody || !statusHead) return;

  // The cvar names come from the backend (native::hd), not from here.
  let cvars = null;
  // A download or a build is running: the backend allows one at a time.
  let busy = false;
  // Whether the build can run at all: scripts shipped, and a Python found.
  let canBuild = false;

  function gamePath() {
    return document.querySelector('#hl-path-input')?.value?.trim() || '';
  }

  function setBusy(value) {
    busy = value;
    setupBtn.disabled = busy;
    buildBtn.disabled = busy || !canBuild;
    pythonPickBtn.disabled = busy;
    pythonResetBtn.disabled = busy;
    upscalerPickBtn.disabled = busy;
    upscalerResetBtn.disabled = busy;
  }

  function renderCfgLines() {
    if (!cvars || !cfgLines) return;
    cfgLines.textContent = `${cvars.enabled} 1\n${cvars.style} ${styleSelect.value}`;
  }

  function allStyles(status) {
    // Built-in styles first, then any of the user's own that are built.
    return [...status.known_styles, ...status.built_styles.filter((s) => !status.known_styles.includes(s))];
  }

  function renderStyles(status) {
    const previous = styleSelect.value;
    const built = new Set(status.built_styles);
    styleSelect.innerHTML = '';
    for (const name of allStyles(status)) {
      const option = document.createElement('option');
      option.value = name;
      option.textContent = name
        + (name === status.default_style ? STRINGS.HD.DEFAULT_SUFFIX : '')
        + (built.has(name) ? '' : STRINGS.HD.NOT_BUILT_SUFFIX);
      styleSelect.appendChild(option);
    }
    const pick = [previous, status.default_style, status.built_styles[0]]
      .find((s) => s && built.has(s)) || status.default_style;
    styleSelect.value = pick;
  }

  // One row per style (and `overrides`, last), one column per asset type.
  function renderTable(status) {
    const cell = (tag, text) => {
      const el = document.createElement(tag);
      el.textContent = text;
      return el;
    };
    const head = document.createElement('tr');
    head.append(cell('th', STRINGS.HD.TABLE_STYLE),
      ...status.types.map((t) => cell('th', STRINGS.HD.TYPE_NAMES[t.asset_type] || t.asset_type)));
    statusHead.replaceChildren(head);

    const names = status.types.flatMap((t) => t.folders.filter((f) => f.files > 0).map((f) => f.name));
    const known = allStyles(status);
    const rows = [...new Set(names)].sort((a, b) => {
      const rank = (n) => (n === 'overrides' ? Infinity : known.includes(n) ? known.indexOf(n) : known.length);
      return rank(a) - rank(b) || a.localeCompare(b);
    });

    statusBody.innerHTML = '';
    if (!rows.length) {
      const empty = cell('td', STRINGS.HD.NOTHING_BUILT);
      empty.colSpan = status.types.length + 1;
      const row = document.createElement('tr');
      row.append(empty);
      statusBody.appendChild(row);
      return;
    }
    for (const name of rows) {
      const row = document.createElement('tr');
      row.append(cell('td', name), ...status.types.map((t) => {
        const folder = t.folders.find((f) => f.name === name && f.files > 0);
        return cell('td', folder ? STRINGS.HD.cellSummary(folder.files, formatSize(folder.bytes)) : '–');
      }));
      statusBody.appendChild(row);
    }
  }

  function renderPython(python) {
    const lines = [];
    if (python?.chosen_problem) {
      const p = python.chosen_problem;
      lines.push(STRINGS.HD.pythonChosenProblem(p.exe, p.version, p.missing));
    }
    if (python?.using) {
      const u = python.using;
      lines.push(STRINGS.HD.pythonUsing(u.source, u.exe, u.version));
    } else if (python?.found_unusable) {
      const p = python.found_unusable;
      lines.push(STRINGS.HD.pythonFoundUnusable(p.exe, p.version, p.missing));
    } else {
      lines.push(STRINGS.HD.PYTHON_NONE);
    }
    pythonText.textContent = lines.join(' ');
    pythonResetBtn.hidden = !python?.chosen;
  }

  function renderTools(tools) {
    const missing = tools.models.filter((m) => !m.present).map((m) => m.style);
    const lines = [];
    if (tools.chosen && tools.source !== 'chosen') lines.push(STRINGS.HD.upscalerChosenUnused(tools.chosen));
    if (tools.upscaler_present) {
      lines.push(STRINGS.HD.upscalerUsing(tools.source, tools.dir),
        missing.length ? STRINGS.HD.modelsMissing(missing) : STRINGS.HD.ALL_MODELS_PRESENT);
    } else {
      lines.push(STRINGS.HD.UPSCALER_MISSING);
    }
    toolsText.textContent = lines.join(' ');
    upscalerResetBtn.hidden = !tools.chosen;
    realesrganLine.textContent = `set REALESRGAN=${tools.upscaler}`;
  }

  // A checkbox per choice, keeping what the user had ticked across refreshes.
  function renderChoices(container, choices, fallbackChecked) {
    const ticked = new Set([...container.querySelectorAll('input:checked')].map((i) => i.value));
    const firstTime = !container.children.length;
    container.innerHTML = '';
    for (const { value, label, disabled } of choices) {
      const wrap = document.createElement('label');
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.value = value;
      box.disabled = !!disabled;
      box.checked = !disabled && (firstTime ? fallbackChecked(value) : ticked.has(value));
      wrap.append(box, document.createTextNode(label));
      container.appendChild(wrap);
    }
  }

  function renderBuild(status) {
    // An AI style can't build until the upscaler and its own model are there.
    const ready = new Set(status.tools.upscaler_present
      ? status.tools.models.filter((m) => m.present).map((m) => m.style)
      : []);
    const aiStyles = new Set(status.tools.models.map((m) => m.style));
    renderChoices(buildStyles, allStyles(status).map((name) => {
      const needs = aiStyles.has(name) && !ready.has(name);
      return { value: name, label: name + (needs ? STRINGS.HD.STYLE_NEEDS_UPSCALER : ''), disabled: needs };
    }), (name) => name === status.default_style);
    renderChoices(buildTypes, BUILD_TYPES.map((t) => ({
      value: t, label: STRINGS.HD.TYPE_NAMES[t] || t,
    })), () => true);

    canBuild = !!status.scripts && !!status.python?.using;
    if (!status.scripts) buildProgress.textContent = STRINGS.HD.NO_SCRIPTS;
    setBusy(busy);
  }

  async function refresh() {
    let status;
    try {
      status = await hdStatus(gamePath());
    } catch (err) {
      statusText.textContent = String(err);
      statusHead.replaceChildren();
      statusBody.innerHTML = '';
      return;
    }
    cvars = { enabled: status.enabled_cvar, style: status.style_cvar };
    statusText.textContent = [
      status.hd_root_exists ? STRINGS.HD.hdRootFound(status.hd_root) : STRINGS.HD.hdRootMissing(status.hd_root),
      status.built_styles.length ? STRINGS.HD.stylesBuilt(status.built_styles) : STRINGS.HD.NO_STYLES_BUILT,
    ].join(' ');
    renderTable(status);
    renderStyles(status);
    renderCfgLines();
    renderTools(status.tools);
    renderPython(status.python);
    renderBuild(status);

    const total = status.types.flatMap((t) => t.folders).reduce((sum, f) => sum + f.bytes, 0);
    if (footerSummary) {
      footerSummary.textContent = STRINGS.HD.footerSummary(status.built_styles.join(', '), formatSize(total));
    }
  }

  refreshBtn?.addEventListener('click', refresh);
  styleSelect?.addEventListener('change', renderCfgLines);
  // Refresh whenever the page is opened: builds can happen outside the app.
  document.querySelector('.nav-tab-btn[data-nav="hd-textures"]')?.addEventListener('click', refresh);

  copyBtn?.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(cfgLines.textContent);
      showToast(STRINGS.HD.COPIED, 'success');
    } catch (err) {
      console.error('Clipboard write failed:', err);
    }
  });

  listen('hd_setup_progress', (event) => {
    const p = event.payload;
    if (p.unpacking) {
      progressText.textContent = STRINGS.HD.unpackingLine(p.item, p.step, p.steps);
      return;
    }
    const done = p.bytes_total
      ? STRINGS.HD.bytesOf(formatSize(p.bytes_done), formatSize(p.bytes_total))
      : formatSize(p.bytes_done);
    progressText.textContent = STRINGS.HD.progressLine(p.item, p.step, p.steps, done);
  }).catch((err) => console.error('hd_setup_progress listener failed:', err));

  listen('hd_build_progress', (event) => {
    const p = event.payload;
    const type = STRINGS.HD.TYPE_NAMES[p.asset_type] || p.asset_type;
    buildProgress.textContent = STRINGS.HD.buildStep(p.step, p.steps, p.style, type, formatElapsed(p.elapsed_secs));
    if (p.line) buildLine.textContent = p.line;
  }).catch((err) => console.error('hd_build_progress listener failed:', err));

  setupBtn?.addEventListener('click', async () => {
    setBusy(true);
    cancelBtn.disabled = false;
    progressText.textContent = '';
    try {
      const outcome = await hdSetupTools();
      progressText.textContent = STRINGS.HD.setupDone(outcome.fetched.length);
    } catch (err) {
      progressText.textContent = err === 'cancelled' ? STRINGS.HD.SETUP_CANCELLED : STRINGS.IPC.hdSetupFailed(err);
    } finally {
      setBusy(false);
      cancelBtn.disabled = true;
      refresh();
    }
  });

  cancelBtn?.addEventListener('click', () => {
    cancelBtn.disabled = true;
    hdCancel().catch(() => {});
  });

  buildBtn?.addEventListener('click', async () => {
    const ticked = (container) => [...container.querySelectorAll('input:checked')].map((i) => i.value);
    const request = { styles: ticked(buildStyles), types: ticked(buildTypes) };
    if (!request.styles.length || !request.types.length) {
      buildProgress.textContent = STRINGS.HD.BUILD_NEEDS_CHOICE;
      return;
    }
    setBusy(true);
    buildCancelBtn.disabled = false;
    buildProgress.textContent = '';
    buildLine.textContent = '';
    try {
      const outcome = await hdBuild(gamePath(), request);
      buildProgress.textContent = STRINGS.HD.buildDone(outcome.steps, formatElapsed(outcome.elapsed_secs), outcome.log_path);
    } catch (err) {
      buildProgress.textContent = err === 'cancelled' ? STRINGS.HD.BUILD_CANCELLED : STRINGS.IPC.hdBuildFailed(err);
    } finally {
      setBusy(false);
      buildCancelBtn.disabled = true;
      refresh();
    }
  });

  buildCancelBtn?.addEventListener('click', () => {
    buildCancelBtn.disabled = true;
    hdCancel().catch(() => {});
  });

  pythonPickBtn?.addEventListener('click', async () => {
    let picked;
    try {
      picked = await open({
        title: STRINGS.HD.PYTHON_PICK_TITLE,
        multiple: false,
        directory: false,
        filters: [{ name: 'python.exe', extensions: ['exe'] }],
      });
    } catch (err) {
      console.error('Python picker failed:', err);
      return;
    }
    if (!picked) return;
    try {
      await hdSetPython(picked);
    } catch {
      return; // hdSetPython already showed why
    }
    refresh();
  });

  upscalerPickBtn?.addEventListener('click', async () => {
    let picked;
    try {
      picked = await open({ title: STRINGS.HD.UPSCALER_PICK_TITLE, directory: true, multiple: false });
    } catch (err) {
      console.error('Upscaler folder picker failed:', err);
      return;
    }
    if (!picked) return;
    try {
      await hdSetUpscaler(picked);
    } catch {
      return; // hdSetUpscaler already showed why
    }
    refresh();
  });

  upscalerResetBtn?.addEventListener('click', async () => {
    try {
      await hdSetUpscaler(null);
    } catch {
      return;
    }
    refresh();
  });

  pythonResetBtn?.addEventListener('click', async () => {
    try {
      await hdSetPython(null);
    } catch {
      return;
    }
    refresh();
  });
}
