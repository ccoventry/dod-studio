// hd_pane.js — the HD Textures page (#372): what is built under
// <game>/dod/dodstudio_hd, the movie.cfg lines to use it, downloading the
// upscaler (and a Python when the PC has none), running the build, the
// style comparison sheet, the user's own styles (my_styles.txt), and the
// textures the game last reported as kept original (the misses view).
// The build is goldsrc-hooks/tools/hd's own scripts, so this page and the
// command line always make the same files.

import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import {
  hdStatus, hdSetupTools, hdBuild, hdCancel, hdSetPython, hdSetUpscaler, hdMisses, hdSaveStyle, hdRemoveStyle,
  hdPreview,
} from './ipc_bridge.js';
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
  const previewMap = document.querySelector('#hd-preview-map');
  const previewStyles = document.querySelector('#hd-preview-styles');
  const previewBtn = document.querySelector('#hd-preview-btn');
  const previewText = document.querySelector('#hd-preview-text');
  const previewWrap = document.querySelector('#hd-preview-wrap');
  const previewImg = document.querySelector('#hd-preview-img');
  const myStylesText = document.querySelector('#hd-my-styles-text');
  const myStylesList = document.querySelector('#hd-my-styles-list');
  const styleName = document.querySelector('#hd-style-name');
  const styleKind = document.querySelector('#hd-style-kind');
  const styleSharpening = document.querySelector('#hd-style-sharpening');
  const styleModel = document.querySelector('#hd-style-model');
  const styleModelHint = document.querySelector('#hd-style-model-hint');
  const stylePercent = document.querySelector('#hd-style-percent');
  const styleA = document.querySelector('#hd-style-a');
  const styleB = document.querySelector('#hd-style-b');
  const styleLine = document.querySelector('#hd-style-line');
  const styleSaveBtn = document.querySelector('#hd-style-save-btn');
  const styleMessage = document.querySelector('#hd-style-message');
  const missesBtn = document.querySelector('#hd-misses-btn');
  const missesCommand = document.querySelector('#hd-misses-command');
  const missesCopyBtn = document.querySelector('#hd-misses-copy-btn');
  const missesText = document.querySelector('#hd-misses-text');
  const missesOnPurpose = document.querySelector('#hd-misses-on-purpose');
  const missesMaps = document.querySelector('#hd-misses-maps');
  if (!statusBody || !statusHead) return;

  // The cvar names come from the backend (native::hd), not from here.
  let cvars = null;
  // A download or a build is running: the backend allows one at a time.
  let busy = false;
  // Whether the build can run at all: scripts shipped, and a Python found.
  let canBuild = false;
  // Whether Download has anything to fetch: no complete upscaler folder, or
  // no Python the build can use.
  let nothingMissing = false;

  function gamePath() {
    return document.querySelector('#hl-path-input')?.value?.trim() || '';
  }

  function setBusy(value) {
    busy = value;
    setupBtn.disabled = busy || nothingMissing;
    setupBtn.textContent = nothingMissing ? STRINGS.HD.NOTHING_MISSING_BUTTON : STRINGS.HD.SETUP_BUTTON;
    buildBtn.disabled = busy || !canBuild || !ticked(buildStyles).length || !ticked(buildTypes).length;
    pythonPickBtn.disabled = busy;
    pythonResetBtn.disabled = busy;
    upscalerPickBtn.disabled = busy;
    upscalerResetBtn.disabled = busy;
  }

  const ticked = (container) => [...container.querySelectorAll('input:checked')].map((i) => i.value);

  function renderCfgLines() {
    if (!cvars || !cfgLines) return;
    cfgLines.textContent = `${cvars.enabled} 1\n${cvars.style} ${styleSelect.value}`;
  }

  const myStyles = (status) => status.my_styles?.styles || [];

  function allStyles(status) {
    // Built-in styles first, then the user's own (my_styles.txt's, then any
    // other built folder).
    const names = [...status.known_styles, ...myStyles(status).map((s) => s.name), ...status.built_styles];
    return [...new Set(names)];
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

  // The probe reports modules as imported (`PIL`); people know the packages
  // by the names pip installs them under.
  const PACKAGE_NAMES = { numpy: 'NumPy', PIL: 'Pillow', scipy: 'SciPy' };
  const packages = (missing) => missing.map((m) => PACKAGE_NAMES[m] || m);

  function renderPython(python) {
    const lines = [];
    if (python?.chosen_problem) {
      const p = python.chosen_problem;
      lines.push(STRINGS.HD.pythonChosenProblem(p.exe, p.version, packages(p.missing)));
    }
    if (python?.using) {
      const u = python.using;
      lines.push(STRINGS.HD.pythonUsing(u.source, u.exe, u.version));
    } else if (python?.found_unusable) {
      const p = python.found_unusable;
      lines.push(STRINGS.HD.pythonFoundUnusable(p.exe, p.version, packages(p.missing)));
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
    // The user's own AI styles need the upscaler and whatever model they name.
    const models = new Set(status.tools.available_models);
    const custom = new Map(myStyles(status).map((s) => [s.name, s]));
    renderChoices(buildStyles, allStyles(status).map((name) => {
      const mine = custom.get(name);
      let needs = '';
      if (aiStyles.has(name) && !ready.has(name)) needs = STRINGS.HD.STYLE_NEEDS_UPSCALER;
      else if (mine?.kind === 'ai' && !status.tools.upscaler_present) needs = STRINGS.HD.STYLE_NEEDS_UPSCALER;
      else if (mine?.kind === 'ai' && !models.has(mine.model)) needs = STRINGS.HD.STYLE_NEEDS_MODEL;
      return { value: name, label: name + needs, disabled: !!needs };
    }), (name) => name === status.default_style);
    renderChoices(buildTypes, BUILD_TYPES.map((t) => ({
      value: t, label: STRINGS.HD.TYPE_NAMES[t] || t,
    })), () => true);

    canBuild = !!status.scripts && !!status.python?.using && !status.my_styles?.error;
    nothingMissing = status.tools.upscaler_present
      && status.tools.models.every((m) => m.present)
      && !!status.python?.using;
    if (!status.scripts) buildProgress.textContent = STRINGS.HD.NO_SCRIPTS;
    setBusy(busy);
  }

  // A refresh walks the whole HD folder and asks each Python it finds for
  // its version, which can take a few seconds: the button says so while it
  // runs, and the status line ends with when it last finished.
  let refreshing = null;
  function refresh() {
    refreshing ??= (async () => {
      refreshBtn.disabled = true;
      refreshBtn.textContent = STRINGS.HD.REFRESHING;
      try {
        await refreshNow();
      } finally {
        refreshBtn.disabled = false;
        refreshBtn.textContent = STRINGS.HD.REFRESH_BUTTON;
        refreshing = null;
      }
    })();
    return refreshing;
  }

  async function refreshNow() {
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
      STRINGS.HD.checkedAt(new Date().toLocaleTimeString()),
    ].join(' ');
    renderTable(status);
    renderStyles(status);
    renderCfgLines();
    renderTools(status.tools);
    renderPython(status.python);
    renderBuild(status);
    renderMyStyles(status);
    renderPreviewChoices(status);

    const total = status.types.flatMap((t) => t.folders).reduce((sum, f) => sum + f.bytes, 0);
    if (footerSummary) {
      footerSummary.textContent = STRINGS.HD.footerSummary(status.built_styles.join(', '), formatSize(total));
    }
  }

  // The style comparison: which maps and built styles to put in the sheet.
  let canPreview = false;
  let previewing = false;

  function renderPreviewChoices(status) {
    if (!previewMap) return;
    const previous = previewMap.value;
    previewMap.innerHTML = '';
    for (const [value, label] of [['', STRINGS.HD.PREVIEW_AUTO_MAP], ...status.maps.map((m) => [m, m])]) {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      previewMap.appendChild(option);
    }
    if (status.maps.includes(previous)) previewMap.value = previous;
    renderChoices(previewStyles, status.built_styles.map((name) => ({ value: name, label: name })), () => true);
    canPreview = !!status.scripts && !!status.python?.using && status.built_styles.length > 0;
    if (!status.built_styles.length) previewText.textContent = STRINGS.HD.PREVIEW_NOTHING_BUILT;
    renderPreviewButton();
  }

  function renderPreviewButton() {
    if (previewBtn) previewBtn.disabled = previewing || !canPreview || !ticked(previewStyles).length;
  }

  async function showPreview() {
    const request = { maps: previewMap.value ? [previewMap.value] : [], styles: ticked(previewStyles) };
    previewing = true;
    renderPreviewButton();
    previewBtn.textContent = STRINGS.HD.PREVIEW_WORKING;
    previewText.textContent = '';
    try {
      const preview = await hdPreview(gamePath(), request);
      previewImg.src = preview.image;
      previewWrap.hidden = false;
      previewText.textContent = [
        STRINGS.HD.previewDone(preview.samples, preview.maps),
        preview.skipped.length ? STRINGS.HD.previewSkipped(preview.skipped) : '',
      ].filter(Boolean).join(' ');
    } catch (err) {
      previewText.textContent = STRINGS.IPC.hdPreviewFailed(err);
    } finally {
      previewing = false;
      previewBtn.textContent = STRINGS.HD.PREVIEW_BUTTON;
      renderPreviewButton();
    }
  }

  previewBtn?.addEventListener('click', showPreview);
  previewStyles?.addEventListener('change', renderPreviewButton);
  // 1:1 by default (the point of the sheet); a click fits it to the page.
  previewImg?.addEventListener('click', () => previewWrap.classList.toggle('hd-preview-fit'));

  // The custom-style form. `lastStatus` is the newest status report: the
  // form's model and blend lists come from it.
  let lastStatus = null;
  const STYLE_NAME = /^[a-z0-9_-]{1,32}$/;

  function renderMyStyles(status) {
    lastStatus = status;
    const mine = status.my_styles;
    if (!myStylesList || !mine) return;
    const lines = [mine.old_place ? STRINGS.HD.myStylesOldPlace(mine.old_place, mine.path)
      : STRINGS.HD.myStylesFile(mine.path, mine.exists)];
    if (mine.error) lines.push(STRINGS.HD.myStylesError(mine.error));
    myStylesText.textContent = lines.join(' ');

    myStylesList.innerHTML = '';
    if (!mine.styles.length && !mine.error) {
      const none = document.createElement('li');
      none.textContent = STRINGS.HD.MY_STYLES_NONE;
      myStylesList.appendChild(none);
    }
    for (const style of mine.styles) {
      const item = document.createElement('li');
      const name = document.createElement('span');
      name.className = 'hd-my-style-name';
      name.textContent = style.name;
      const what = document.createElement('span');
      what.className = 'hd-miss-detail';
      what.textContent = STRINGS.HD.styleDescription(style);
      const edit = document.createElement('button');
      edit.textContent = STRINGS.HD.STYLE_EDIT_BUTTON;
      edit.addEventListener('click', () => fillForm(style));
      const remove = document.createElement('button');
      remove.textContent = STRINGS.HD.STYLE_REMOVE_BUTTON;
      remove.addEventListener('click', () => removeStyle(style.name));
      item.append(name, what, edit, remove);
      myStylesList.appendChild(item);
    }

    // The form's lists: the models the upscaler folder has, and every style
    // a blend can mix.
    const fill = (select, values, empty) => {
      const previous = select.value;
      select.innerHTML = '';
      for (const value of values) {
        const option = document.createElement('option');
        option.value = value;
        option.textContent = value;
        select.appendChild(option);
      }
      if (!values.length && empty) {
        const option = document.createElement('option');
        option.value = '';
        option.textContent = empty;
        select.appendChild(option);
      }
      if (values.includes(previous)) select.value = previous;
      return values.includes(previous);
    };
    fill(styleModel, status.tools.available_models, STRINGS.HD.STYLE_NO_MODELS);
    styleModelHint.textContent = STRINGS.HD.styleModelHint(status.tools.dir);
    const styles = allStyles(status);
    if (!fill(styleA, styles)) styleA.value = status.default_style;
    if (!fill(styleB, styles) && styles.includes('plain')) styleB.value = 'plain';
    renderForm();
  }

  // What the form would save, and why it can't (' ' when the reason is plain
  // to see, like an empty name).
  function formStyle() {
    const name = styleName.value.trim().toLowerCase();
    const kind = styleKind.value;
    const def = kind === 'ai' ? { kind, model: styleModel.value }
      : kind === 'plain' ? { kind, sharpening: Math.round(Number(styleSharpening.value)) }
        : { kind, a: styleA.value, b: styleB.value, percent: Math.round(Number(stylePercent.value)) };
    let problem = '';
    if (!STYLE_NAME.test(name)) problem = name ? STRINGS.HD.STYLE_BAD_NAME : ' ';
    else if (lastStatus?.known_styles.includes(name)) problem = STRINGS.HD.styleBuiltIn(name);
    else if (kind === 'ai' && !def.model) problem = ' ';
    else if (kind === 'plain' && !(def.sharpening >= 0 && def.sharpening <= 500)) problem = ' ';
    else if (kind === 'blend' && !(def.percent >= 0 && def.percent <= 100)) problem = ' ';
    else if (kind === 'blend' && (def.a === name || def.b === name)) problem = STRINGS.HD.STYLE_BLENDS_ITSELF;
    return { name, def, problem };
  }

  // my_styles.txt's line for a style: what native::hd::my_styles writes.
  function styleValue(def) {
    if (def.kind === 'ai') return def.model;
    if (def.kind === 'plain') return `plain ${def.sharpening}`;
    return `blend ${def.a} ${def.b} ${def.percent}`;
  }

  // Whether the message line is showing a form problem (cleared once fixed),
  // rather than the outcome of a save.
  let messageIsProblem = false;

  function renderForm() {
    if (!styleKind) return;
    document.querySelectorAll('[data-style-kind]').forEach((el) => {
      el.hidden = el.dataset.styleKind !== styleKind.value;
    });
    const { name, def, problem } = formStyle();
    styleLine.textContent = `${name || '<name>'} = ${styleValue(def)}`;
    styleSaveBtn.disabled = !!problem || !lastStatus?.my_styles;
    if (problem.trim()) {
      styleMessage.textContent = problem;
      messageIsProblem = true;
    } else if (messageIsProblem) {
      styleMessage.textContent = '';
      messageIsProblem = false;
    }
  }

  function fillForm(style) {
    styleName.value = style.name;
    styleKind.value = style.kind;
    if (style.kind === 'ai') styleModel.value = style.model;
    if (style.kind === 'plain') styleSharpening.value = style.sharpening;
    if (style.kind === 'blend') {
      styleA.value = style.a;
      styleB.value = style.b;
      stylePercent.value = style.percent;
    }
    styleMessage.textContent = '';
    renderForm();
    styleName.focus();
  }

  async function saveStyle() {
    const { name, def, problem } = formStyle();
    if (problem) return;
    styleSaveBtn.disabled = true;
    try {
      await hdSaveStyle(gamePath(), name, def);
      styleMessage.textContent = STRINGS.HD.styleSaved(name);
    } catch (err) {
      styleMessage.textContent = String(err);
      renderForm();
      return;
    }
    messageIsProblem = false;
    await refresh();
  }

  async function removeStyle(name) {
    try {
      await hdRemoveStyle(gamePath(), name);
      styleMessage.textContent = STRINGS.HD.styleRemoved(name);
    } catch (err) {
      styleMessage.textContent = String(err);
      return;
    }
    messageIsProblem = false;
    await refresh();
  }

  for (const input of [styleName, styleKind, styleSharpening, styleModel, stylePercent, styleA, styleB]) {
    input?.addEventListener('input', renderForm);
    input?.addEventListener('change', renderForm);
  }
  styleSaveBtn?.addEventListener('click', saveStyle);
  renderForm();

  // The misses view: the newest list the game wrote to the hook log.
  let missReport = null;

  function renderMisses() {
    missesMaps.innerHTML = '';
    if (!missReport) {
      missesText.textContent = STRINGS.HD.MISSES_NONE;
      return;
    }
    const r = missReport;
    missesText.textContent = STRINGS.HD.missesFrom(r.date, r.time, r.style, r.summary);
    const showOnPurpose = missesOnPurpose.checked;
    const el = (tag, className, text) => {
      const e = document.createElement(tag);
      if (className) e.className = className;
      if (text !== undefined) e.textContent = text;
      return e;
    };
    for (const map of r.maps) {
      const details = el('details', 'hd-miss-map');
      // Open when the list is short enough to take in at once.
      details.open = r.maps.length <= 3;
      details.append(el('summary', null, STRINGS.HD.missesMap(map.map, map.total, map.on_purpose)));
      const groups = map.groups.filter((g) => showOnPurpose || g.reason !== 'on_purpose');
      if (!groups.length) details.append(el('p', 'hd-hint', STRINGS.HD.MISSES_ONLY_ON_PURPOSE));
      for (const group of groups) {
        details.append(el('div', 'hd-miss-heading', `${group.heading} (${group.entries.length})`));
        const advice = STRINGS.HD.MISSES_ADVICE[group.reason];
        if (advice) details.append(el('p', 'hd-hint', advice));
        const list = el('ul', 'hd-miss-list');
        for (const entry of group.entries) {
          const item = el('li');
          const notes = [entry.detail];
          if (entry.loads > 1) notes.push(STRINGS.HD.missesLoads(entry.loads, entry.asset_type));
          item.append(
            el('span', 'hd-miss-type', STRINGS.HD.MISS_TYPE_NAMES[entry.asset_type] || entry.asset_type),
            el('span', 'hd-mono', entry.name),
            el('span', 'hd-miss-detail', notes.join(', ')),
          );
          if (entry.also_on.length) {
            const also = el('span', 'hd-miss-detail', STRINGS.HD.missesAlsoOn(entry.also_on.length));
            also.title = entry.also_on.join(', ');
            item.append(also);
          }
          list.append(item);
        }
        details.append(list);
      }
      missesMaps.append(details);
    }
  }

  let readingMisses = null;
  function readMisses() {
    if (!missesBtn || !missesMaps) return null;
    readingMisses ??= (async () => {
      missesBtn.disabled = true;
      missesBtn.textContent = STRINGS.HD.MISSES_READING;
      try {
        const view = await hdMisses();
        missesCommand.textContent = view.command;
        missReport = view.report;
        renderMisses();
      } catch (err) {
        missesText.textContent = String(err);
      } finally {
        missesBtn.disabled = false;
        missesBtn.textContent = STRINGS.HD.MISSES_BUTTON;
        readingMisses = null;
      }
    })();
    return readingMisses;
  }

  refreshBtn?.addEventListener('click', refresh);
  styleSelect?.addEventListener('change', renderCfgLines);
  // Refresh whenever the page is opened: builds can happen outside the app,
  // and the game writes new misses to its log.
  document.querySelector('.nav-tab-btn[data-nav="hd-textures"]')?.addEventListener('click', () => {
    refresh();
    readMisses();
  });
  missesBtn?.addEventListener('click', readMisses);
  missesOnPurpose?.addEventListener('change', renderMisses);

  async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
      showToast(STRINGS.HD.COPIED, 'success');
    } catch (err) {
      console.error('Clipboard write failed:', err);
    }
  }
  copyBtn?.addEventListener('click', () => copyText(cfgLines.textContent));
  missesCopyBtn?.addEventListener('click', () => copyText(missesCommand.textContent));

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
    const request = { styles: ticked(buildStyles), types: ticked(buildTypes) };
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

  // Build needs at least one style and one kind of file ticked.
  buildStyles?.addEventListener('change', () => setBusy(busy));
  buildTypes?.addEventListener('change', () => setBusy(busy));

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
