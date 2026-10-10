// demo_rename_ui.js
// The Demo Auditor's Rename Demos panel (#469): the two templates, the
// preview of every demo in the Target Folder, Rename, and Undo Last Rename.
// The names are built in demo_rename.js; the renames happen in
// native::demo_rename, which refuses anything but a plain rename in place.

import { listen } from '@tauri-apps/api/event';
import { MODIFIERS } from './clip_name.js';
import {
  DEFAULT_POV_TEMPLATE, DEFAULT_HLTV_TEMPLATE, DEMO_PLACEHOLDER_GROUPS, placeholdersFor,
  parseDemoTemplate, demoValues, planRenames, renamePairs,
} from './demo_rename.js';
import { demoRenameList, demoRenameCancel, demoRenameApply, demoRenameUndo, demoRenameUndoable } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const $ = (selector) => document.querySelector(selector);

/** The two templates in effect: each field's, or its default while blank. */
export function getDemoRenameTemplates() {
  const value = (selector, fallback) => {
    const v = $(selector)?.value;
    return v && v.trim() ? v : fallback;
  };
  return {
    pov: value('#rename-pov-template', DEFAULT_POV_TEMPLATE),
    hltv: value('#rename-hltv-template', DEFAULT_HLTV_TEMPLATE),
    lowercase: !!$('#rename-lowercase')?.checked,
  };
}

/** Sets both fields and the lowercase box from saved settings. */
export function setDemoRenameTemplates(pov, hltv, lowercase) {
  const povInput = $('#rename-pov-template');
  const hltvInput = $('#rename-hltv-template');
  const lowerBox = $('#rename-lowercase');
  if (povInput) povInput.value = pov || DEFAULT_POV_TEMPLATE;
  if (hltvInput) hltvInput.value = hltv || DEFAULT_HLTV_TEMPLATE;
  if (lowerBox) lowerBox.checked = !!lowercase;
  refresh();
}

let facts = [];
let selected = new Set();
let rows = [];
let listing = false;
// Whether List Demos has run, so an empty table says "no demos" rather than
// how to start.
let listedOnce = false;
let cancelRequested = false;
let busy = false;
let getProjectTeams = () => null;
// The template field the chips insert into: the last one focused.
let chipTarget = null;

function setStatus(text) {
  const el = $('#rename-status');
  if (el) el.textContent = text;
}

function showErrors(selector, errors) {
  const el = $(selector);
  if (!el) return;
  el.textContent = errors.join(' ');
  el.style.display = errors.length ? '' : 'none';
}

function insertAtCursor(text) {
  const input = chipTarget || $('#rename-pov-template');
  if (!input) return;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? start;
  // A modifier goes inside the placeholder the cursor sits just after.
  const into = text.startsWith(':') && start === end && input.value[end - 1] === '}';
  const from = into ? end - 1 : start;
  const to = into ? end - 1 : end;
  input.value = input.value.slice(0, from) + text + input.value.slice(to);
  const caret = from + text.length + (into ? 1 : 0);
  input.setSelectionRange(caret, caret);
  input.focus();
  input.dispatchEvent(new Event('input'));
  input.dispatchEvent(new Event('change'));
}

/** The demo type of the template the chips insert into. */
function chipType() {
  return chipTarget?.id === 'rename-hltv-template' ? 'hltv' : 'pov';
}

let chipsKey = null;
function renderChips() {
  const box = $('#rename-chips');
  if (!box) return;
  // Every placeholder, always in the same place, one row per group
  // (DEMO_PLACEHOLDER_GROUPS) and a Format row for the modifiers; the ones
  // the focused template's demo type doesn't have are greyed out and say
  // why. Each shows its value for the first demo of that type.
  const type = chipType();
  const first = facts.find((f) => !f.error && (f.demo_type === 'hltv' ? 'hltv' : 'pov') === type);
  const values = first ? demoValues(first, getProjectTeams()) : null;
  const key = JSON.stringify([type, values]);
  if (key === chipsKey && box.childElementCount) return;
  chipsKey = key;
  const D = STRINGS.DEMO_RENAME;
  const chip = (text, title, usable = true) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'rename-chip';
    b.textContent = text;
    b.title = title;
    b.disabled = !usable;
    // Keeps the cursor in the field (see clip_name_ui.js).
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => insertAtCursor(text));
    return b;
  };
  const usable = new Set(placeholdersFor(type));
  const row = (key, chips) => {
    const label = document.createElement('span');
    label.className = 'rename-chip-group-label';
    label.textContent = D.GROUPS[key];
    const list = document.createElement('div');
    list.className = 'rename-chip-group';
    list.dataset.group = key;
    list.append(...chips);
    return [label, list];
  };
  box.replaceChildren(
    ...DEMO_PLACEHOLDER_GROUPS.flatMap((g) => row(g.key, g.names.map((name) => (usable.has(name)
      ? chip(`{${name}}`, D.chipTitle(D.DESCRIPTIONS[name], values?.[name]))
      : chip(`{${name}}`, D.povOnly(`{${name}}`), false))))),
    ...row('format', MODIFIERS.map((m) => chip(`:${m}`, D.DESCRIPTIONS[m]))),
  );
}

function statusNote(row) {
  const D = STRINGS.DEMO_RENAME;
  switch (row.status) {
    case 'same': return D.STATUS_SAME;
    case 'skipped': return D.STATUS_SKIPPED;
    case 'unreadable': return D.statusUnreadable(row.error);
    case 'template': return D.STATUS_TEMPLATE;
    default: return '';
  }
}

function renderTable() {
  const body = $('#rename-body');
  if (!body) return;
  const cell = (text, className) => {
    const td = document.createElement('td');
    td.textContent = text;
    if (className) td.className = className;
    return td;
  };
  if (!facts.length) {
    const td = cell(listing ? '' : (listedOnce ? STRINGS.DEMO_RENAME.NO_DEMOS : STRINGS.DEMO_RENAME.TABLE_EMPTY), 'table-empty');
    td.colSpan = 4;
    const tr = document.createElement('tr');
    tr.append(td);
    body.replaceChildren(tr);
    return;
  }
  body.replaceChildren(...rows.map((row) => {
    const tr = document.createElement('tr');
    // An unticked row stays listed, dimmed: it's left as it is.
    tr.classList.toggle('rename-unticked', !selected.has(row.path));
    const tdCb = document.createElement('td');
    tdCb.className = 'rename-center';
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = selected.has(row.path);
    cb.disabled = busy;
    cb.addEventListener('change', () => {
      if (cb.checked) selected.add(row.path); else selected.delete(row.path);
      refresh();
    });
    tdCb.append(cb);
    const current = cell(row.from);
    current.title = row.path;
    const renamed = row.status === 'rename';
    const next = cell(renamed ? row.to : statusNote(row), renamed ? 'rename-new' : 'rename-note');
    next.title = next.textContent;
    if (row.numbered) {
      next.classList.add('rename-numbered');
      next.title = STRINGS.DEMO_RENAME.NUMBERED_TITLE;
    }
    const type = cell('', 'rename-center');
    const chip = document.createElement('span');
    chip.className = `rename-type rename-type-${row.demoType.toLowerCase()}`;
    chip.textContent = row.demoType.toUpperCase();
    type.append(chip);
    tr.append(tdCb, current, next, type);
    return tr;
  }));
}

function updateButtons() {
  const count = rows.filter((r) => r.status === 'rename').length;
  const apply = $('#rename-apply-btn');
  if (apply) {
    apply.disabled = busy || listing || count === 0;
    apply.textContent = count ? STRINGS.DEMO_RENAME.applyButton(count) : STRINGS.DEMO_RENAME.APPLY_BUTTON;
  }
  const list = $('#rename-list-btn');
  if (list) list.disabled = busy || listing;
  const cancel = $('#rename-cancel-btn');
  if (cancel) cancel.disabled = !listing;
  const all = $('#rename-select-all');
  if (all) {
    all.checked = facts.length > 0 && facts.every((f) => selected.has(f.path));
    all.disabled = busy || listing;
  }
}

/** Re-checks both templates and rebuilds the preview. */
function refresh() {
  const templates = getDemoRenameTemplates();
  showErrors('#rename-pov-errors', parseDemoTemplate(templates.pov, 'pov').errors);
  showErrors('#rename-hltv-errors', parseDemoTemplate(templates.hltv, 'hltv').errors);
  rows = planRenames(facts, {
    povTemplate: templates.pov,
    hltvTemplate: templates.hltv,
    lowercase: templates.lowercase,
    projectTeams: getProjectTeams(),
    selected,
  });
  renderChips();
  renderTable();
  updateButtons();
}

async function refreshUndo() {
  const btn = $('#rename-undo-btn');
  if (!btn) return;
  const batch = await demoRenameUndoable();
  btn.disabled = busy || !batch;
  btn.title = batch
    ? STRINGS.DEMO_RENAME.undoTitle(batch.count, new Date(batch.created_unix_secs * 1000).toLocaleString())
    : STRINGS.DEMO_RENAME.NOTHING_TO_UNDO;
}

async function listDemos() {
  const folder = $('#audit-target-folder-input')?.value?.trim();
  if (!folder) {
    showToast(STRINGS.DEMO_RENAME.CHOOSE_FOLDER_FIRST, 'error');
    return;
  }
  listing = true;
  facts = [];
  refresh();
  setStatus(STRINGS.DEMO_RENAME.reading(0, 0));
  let cancelled = false;
  try {
    facts = await demoRenameList(folder);
    cancelled = cancelRequested;
  } catch {
    // ipc_bridge already said why.
    facts = [];
  } finally {
    listing = false;
    cancelRequested = false;
  }
  selected = new Set(facts.map((f) => f.path));
  listedOnce = true;
  refresh();
  if (cancelled) setStatus(STRINGS.DEMO_RENAME.CANCELLED);
  else showListed();
}

function showListed() {
  setStatus(STRINGS.DEMO_RENAME.listed(facts.length, rows.filter((r) => r.status === 'rename').length));
}

/** Points each listed demo at its new path, so the preview stays right
 *  without reading the folder again. */
function followRenames(renamed) {
  const byOld = new Map(renamed.map((r) => [r.from, r.to]));
  facts = facts.map((f) => {
    const to = byOld.get(f.path);
    if (!to) return f;
    const fileName = to.split(/[\\/]/).pop();
    if (selected.delete(f.path)) selected.add(to);
    return { ...f, path: to, file_name: fileName };
  });
}

function reportFailures(failed) {
  if (!failed.length) return;
  console.error('Demo renames that failed:', failed);
  const first = failed[0];
  showToast(STRINGS.DEMO_RENAME.failedToast(failed.length, `${first.from}: ${first.error}`), 'error');
}

async function applyRenames() {
  const pairs = renamePairs(rows);
  if (!pairs.length) return;
  const ok = await themedConfirm(STRINGS.DEMO_RENAME.CONFIRM_MESSAGE, {
    title: STRINGS.DEMO_RENAME.confirmTitle(pairs.length),
    confirmLabel: STRINGS.DEMO_RENAME.CONFIRM_OK,
  });
  if (!ok) return;
  busy = true;
  updateButtons();
  try {
    const outcome = await demoRenameApply(pairs);
    followRenames(outcome.renamed);
    if (outcome.renamed.length) showToast(STRINGS.DEMO_RENAME.renamedToast(outcome.renamed.length), 'success');
    reportFailures(outcome.failed);
  } catch {
    // ipc_bridge already said why.
  } finally {
    busy = false;
    refresh();
    showListed();
    refreshUndo();
  }
}

async function undoLast() {
  busy = true;
  updateButtons();
  try {
    const outcome = await demoRenameUndo();
    followRenames(outcome.renamed);
    if (outcome.renamed.length) showToast(STRINGS.DEMO_RENAME.undoneToast(outcome.renamed.length), 'success');
    reportFailures(outcome.failed);
  } catch {
    // ipc_bridge already said why.
  } finally {
    busy = false;
    refresh();
    if (listedOnce) showListed();
    refreshUndo();
  }
}

/**
 * Wires the panel. `projectTeams()` gives the loaded project's Teams list
 * (#445) for the team placeholders; `onChange()` runs when a template
 * settles (blur, Enter, a chip, Default), to save it.
 */
export function initDemoRenamePane({ projectTeams, onChange } = {}) {
  if (projectTeams) getProjectTeams = projectTeams;
  const inputs = [
    ['#rename-pov-template', '#rename-pov-reset', DEFAULT_POV_TEMPLATE],
    ['#rename-hltv-template', '#rename-hltv-reset', DEFAULT_HLTV_TEMPLATE],
  ];
  for (const [inputSel, resetSel, fallback] of inputs) {
    const input = $(inputSel);
    if (!input) return;
    if (!input.value) input.value = fallback;
    input.addEventListener('focus', () => {
      chipTarget = input;
      renderChips();
    });
    input.addEventListener('input', refresh);
    input.addEventListener('change', () => {
      refresh();
      if (onChange) onChange();
    });
    $(resetSel)?.addEventListener('click', () => {
      input.value = fallback;
      refresh();
      if (onChange) onChange();
    });
  }
  $('#rename-lowercase')?.addEventListener('change', () => {
    refresh();
    if (onChange) onChange();
  });
  $('#rename-select-all')?.addEventListener('change', (e) => {
    selected = e.target.checked ? new Set(facts.map((f) => f.path)) : new Set();
    refresh();
  });
  $('#rename-list-btn')?.addEventListener('click', listDemos);
  $('#rename-cancel-btn')?.addEventListener('click', () => {
    cancelRequested = true;
    demoRenameCancel().catch(() => {});
  });
  $('#rename-apply-btn')?.addEventListener('click', applyRenames);
  $('#rename-undo-btn')?.addEventListener('click', undoLast);
  listen('demo_rename_progress', (event) => {
    if (listing) setStatus(STRINGS.DEMO_RENAME.reading(event.payload.done, event.payload.total));
  });
  refresh();
  refreshUndo();
}
