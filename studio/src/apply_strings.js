// apply_strings.js
//
// Walks the DOM for [data-str] / [data-str-title] / [data-str-placeholder] /
// [data-str-aria-label] elements and fills them in from strings.js's STRINGS
// object — the mechanism that lets index.html (static markup, can't import
// JS values directly into its own text) stay centralized on the same single
// source of strings.js uses everywhere else.
//
// Keys are dot-paths into STRINGS, e.g. data-str="NAV.SAVE_SESSION_BUTTON".

import { STRINGS } from './strings.js';

function resolveKey(key) {
  return key.split('.').reduce((obj, part) => (obj == null ? undefined : obj[part]), STRINGS);
}

export function applyStaticStrings(root = document) {
  root.querySelectorAll('[data-str]').forEach((el) => {
    const value = resolveKey(el.dataset.str);
    if (value !== undefined) el.textContent = value;
  });
  root.querySelectorAll('[data-str-title]').forEach((el) => {
    const value = resolveKey(el.dataset.strTitle);
    if (value === undefined) return;
    // .info-icon renders its own hover/focus tooltip (info_tooltip.js) instead
    // of the native `title` attribute — see #164. Everything else keeps the
    // native tooltip as before.
    if (el.classList.contains('info-icon')) {
      el.dataset.tooltipText = value;
      el.setAttribute('aria-label', value);
    } else {
      el.title = value;
    }
  });
  root.querySelectorAll('[data-str-placeholder]').forEach((el) => {
    const value = resolveKey(el.dataset.strPlaceholder);
    if (value !== undefined) el.placeholder = value;
  });
  root.querySelectorAll('[data-str-aria-label]').forEach((el) => {
    const value = resolveKey(el.dataset.strAriaLabel);
    if (value !== undefined) el.setAttribute('aria-label', value);
  });
}
