// hd_pane.js — the HD Textures page (#372): what is built under
// <game>/dod/dodstudio_hd, the movie.cfg lines to use it, and downloading the
// upscaler. Building is still the goldsrc-hooks/tools/hd scripts' job; this
// page points at them until the Rust port lands.

import { listen } from '@tauri-apps/api/event';
import { hdStatus, hdSetupTools, hdSetupCancel } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

function formatSize(bytes) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function initHdPane() {
  const statusText = document.querySelector('#hd-status-text');
  const statusBody = document.querySelector('#hd-status-body');
  const refreshBtn = document.querySelector('#hd-refresh-btn');
  const styleSelect = document.querySelector('#hd-style-select');
  const cfgLines = document.querySelector('#hd-cfg-lines');
  const copyBtn = document.querySelector('#hd-copy-cfg-btn');
  const toolsText = document.querySelector('#hd-tools-text');
  const setupBtn = document.querySelector('#hd-setup-btn');
  const cancelBtn = document.querySelector('#hd-setup-cancel-btn');
  const progressText = document.querySelector('#hd-setup-progress');
  const realesrganLine = document.querySelector('#hd-realesrgan-line');
  const footerSummary = document.querySelector('#footer-hd-summary');
  if (!statusBody) return;

  // The cvar names come from the backend (native::hd), not from here.
  let cvars = null;

  function renderCfgLines() {
    if (!cvars || !cfgLines) return;
    cfgLines.textContent = `${cvars.enabled} 1\n${cvars.style} ${styleSelect.value}`;
  }

  function renderStyles(status) {
    const previous = styleSelect.value;
    const built = new Set(status.built_styles);
    // Built-in styles first, then any of the user's own that are built.
    const names = [...status.known_styles, ...status.built_styles.filter((s) => !status.known_styles.includes(s))];
    styleSelect.innerHTML = '';
    for (const name of names) {
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

  function renderTable(status) {
    statusBody.innerHTML = '';
    for (const type of status.types) {
      const row = document.createElement('tr');
      const name = document.createElement('td');
      name.textContent = STRINGS.HD.TYPE_NAMES[type.asset_type] || type.asset_type;
      const folders = document.createElement('td');
      const filled = type.folders.filter((f) => f.files > 0);
      folders.textContent = filled.length
        ? filled.map((f) => STRINGS.HD.folderSummary(f.name, f.files, formatSize(f.bytes))).join('; ')
        : STRINGS.HD.NOTHING_BUILT;
      row.append(name, folders);
      statusBody.appendChild(row);
    }
  }

  function renderTools(tools) {
    const missing = tools.models.filter((m) => !m.present).map((m) => m.style);
    const lines = [
      tools.upscaler_present ? STRINGS.HD.upscalerPresent(tools.upscaler) : STRINGS.HD.UPSCALER_MISSING,
      missing.length ? STRINGS.HD.modelsMissing(missing) : STRINGS.HD.ALL_MODELS_PRESENT,
    ];
    toolsText.textContent = lines.join(' ');
    realesrganLine.textContent = `set REALESRGAN=${tools.upscaler}`;
  }

  async function refresh() {
    const gamePath = document.querySelector('#hl-path-input')?.value?.trim() || '';
    let status;
    try {
      status = await hdStatus(gamePath);
    } catch (err) {
      statusText.textContent = String(err);
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

    const total = status.types.flatMap((t) => t.folders).reduce((sum, f) => sum + f.bytes, 0);
    if (footerSummary) {
      footerSummary.textContent = STRINGS.HD.footerSummary(status.built_styles.join(', '), formatSize(total));
    }
  }

  refreshBtn?.addEventListener('click', refresh);
  styleSelect?.addEventListener('change', renderCfgLines);
  // Refresh whenever the page is opened: builds happen outside the app.
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

  setupBtn?.addEventListener('click', async () => {
    setupBtn.disabled = true;
    cancelBtn.disabled = false;
    progressText.textContent = '';
    try {
      const outcome = await hdSetupTools();
      progressText.textContent = STRINGS.HD.setupDone(outcome.fetched.length);
    } catch (err) {
      progressText.textContent = err === 'cancelled' ? STRINGS.HD.SETUP_CANCELLED : STRINGS.IPC.hdSetupFailed(err);
    } finally {
      setupBtn.disabled = false;
      cancelBtn.disabled = true;
      refresh();
    }
  });

  cancelBtn?.addEventListener('click', () => {
    cancelBtn.disabled = true;
    hdSetupCancel().catch(() => {});
  });
}
