// clip_name_ui.js
// Configuration > Render Output's Clip Name Template (#441): the field, its
// placeholder chips, the checks as you type, and a live preview built from
// the selected highlight.

import {
  DEFAULT_TEMPLATE, PLACEHOLDERS, MODIFIERS, WITH_FALLBACK, NAME_WARN_LENGTH,
  parseTemplate, buildName, highlightValues, clipNameFor, maxNameLength,
} from './clip_name.js';
import { STRINGS } from './strings.js';

let getPreviewHighlight = () => null;
let getExportDirs = () => [];

function templateInput() {
  return document.querySelector('#config-clip-name-template');
}

/** The template in effect: the field's, or the default while it's blank. */
export function getClipNameTemplate() {
  const value = templateInput()?.value;
  return value && value.trim() ? value : DEFAULT_TEMPLATE;
}

/** Sets the field from saved settings. */
export function setClipNameTemplate(template) {
  const input = templateInput();
  if (input) input.value = template || DEFAULT_TEMPLATE;
  refreshClipNamePreview();
}

/** The template's problems, or [] when it can be used. */
export function clipNameTemplateErrors() {
  return parseTemplate(getClipNameTemplate()).errors;
}

function showLines(selector, lines) {
  const el = document.querySelector(selector);
  if (!el) return;
  el.textContent = lines.join(' ');
  el.style.display = lines.length ? '' : 'none';
}

function insertAtCursor(text) {
  const input = templateInput();
  if (!input) return;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? start;
  // A modifier or a | goes inside the placeholder the cursor sits just
  // after; after a |, the cursor stays inside for the word to be typed.
  const fallback = text === '|';
  const intoPlaceholder = (text.startsWith(':') || fallback) && start === end && input.value[end - 1] === '}';
  const from = intoPlaceholder ? end - 1 : start;
  const to = intoPlaceholder ? end - 1 : end;
  input.value = input.value.slice(0, from) + text + input.value.slice(to);
  const caret = from + text.length + (intoPlaceholder && !fallback ? 1 : 0);
  input.setSelectionRange(caret, caret);
  input.focus();
  input.dispatchEvent(new Event('input'));
  input.dispatchEvent(new Event('change'));
}

let chipsKey = null;

/** The | chip reads "|fallback" but inserts a bare |, for the word to follow. */
function fallbackChip(button) {
  button.textContent = '|fallback';
  return button;
}

function renderChips(values) {
  const box = document.querySelector('#config-clip-name-chips');
  if (!box) return;
  // Rebuilt only when a tooltip would change.
  const key = JSON.stringify(values);
  if (key === chipsKey && box.childElementCount) return;
  chipsKey = key;
  const label = document.createElement('span');
  label.textContent = STRINGS.CLIP_NAME.INSERT_LABEL;
  const chip = (text, title) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'clip-name-chip';
    b.textContent = text;
    b.title = title;
    b.style.cssText = 'padding: 1px 6px; font-family: monospace; font-size: 1em;';
    // Keeps focus (and the cursor) in the field: a blur would fire its
    // change event and rebuild these chips under the click.
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => insertAtCursor(text));
    return b;
  };
  box.replaceChildren(
    label,
    ...PLACEHOLDERS.map((name) => chip(`{${name}}`,
      STRINGS.CLIP_NAME.chipTitle(STRINGS.CLIP_NAME.DESCRIPTIONS[name], values?.[name]))),
    ...MODIFIERS.map((m) => chip(`:${m}`, STRINGS.CLIP_NAME.DESCRIPTIONS[m])),
    fallbackChip(chip('|', STRINGS.CLIP_NAME.fallbackTitle(PLACEHOLDERS.filter((n) => WITH_FALLBACK.has(n)).map((n) => `{${n}}`).join(' ')))),
  );
}

/** Re-checks the template and rebuilds the preview (the selection moved,
 *  or an export folder changed). */
export function refreshClipNamePreview() {
  const parsed = parseTemplate(getClipNameTemplate());
  showLines('#config-clip-name-errors', parsed.errors);

  const highlight = getPreviewHighlight();
  const values = highlight ? highlightValues(highlight.demo, highlight.streak) : null;
  renderChips(values);

  const preview = document.querySelector('#config-clip-name-preview');
  const warnings = [...parsed.warnings];
  if (!highlight) {
    if (preview) preview.textContent = STRINGS.CLIP_NAME.PREVIEW_NONE;
    showLines('#config-clip-name-warnings', warnings);
    return;
  }
  const dirs = (getExportDirs() || []).filter(Boolean);
  const maxLength = maxNameLength(dirs);
  const typed = clipNameFor(highlight.demo, highlight.streak, getClipNameTemplate(), { maxLength });
  const built = typed.typed ? typed : buildName(parsed, values, { maxLength });
  const folder = String(dirs[0] || '').replace(/[\\/]+$/, '');
  const path = folder ? `${folder}\\${built.name}` : built.name;
  if (built.name.length > NAME_WARN_LENGTH) warnings.push(STRINGS.CLIP_NAME.longName(built.name.length));
  if (built.trimmed) warnings.push(STRINGS.CLIP_NAME.TRIMMED);
  if (preview) {
    const lines = [];
    if (typed.typed) lines.push(STRINGS.CLIP_NAME.PREVIEW_TYPED);
    lines.push(STRINGS.CLIP_NAME.previewName(built.name));
    if (folder) lines.push(STRINGS.CLIP_NAME.previewPath(path, path.length));
    preview.replaceChildren(...lines.map((line) => {
      const div = document.createElement('div');
      div.textContent = line;
      return div;
    }));
  }
  showLines('#config-clip-name-warnings', warnings);
}

/**
 * Wires the field. `getHighlight()` gives the highlight the preview uses
 * (`{ demo, streak }` or null); `onChange()` runs when the template settles
 * (blur, Enter, a chip, Default), to save it and refresh the names shown.
 */
export function initClipNameSettings({ getHighlight, getExportDirList, onChange }) {
  if (getHighlight) getPreviewHighlight = getHighlight;
  if (getExportDirList) getExportDirs = getExportDirList;
  const input = templateInput();
  if (!input) return;
  if (!input.value) input.value = DEFAULT_TEMPLATE;
  input.addEventListener('input', refreshClipNamePreview);
  input.addEventListener('change', () => {
    refreshClipNamePreview();
    if (onChange) onChange();
  });
  document.querySelector('#config-clip-name-reset')?.addEventListener('click', () => {
    input.value = DEFAULT_TEMPLATE;
    refreshClipNamePreview();
    if (onChange) onChange();
  });
  refreshClipNamePreview();
}
