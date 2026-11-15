// review_settings.js
// The Highlights page's Review controls (Covers, Order, Tick Yes for capture,
// Fast-forward gaps over N s) kept in the settings file, so a choice survives
// a restart. Defaults match settings_manager.rs: All, Chronological, Tick Yes
// on, fast-forward on over 8 s.

export const REVIEW_DEFAULTS = Object.freeze({
  review_covers: 'all',
  review_order: 'time',
  review_tick_yes: true,
  review_fast_forward: true,
  review_fast_forward_gap: 8,
});

const COVERS = '#review-covers-select';
const ORDER = '#review-order-select';
const TICK_YES = '#review-tick-yes-cb';
const FAST_FORWARD = '#review-fast-forward-cb';
const GAP = '#review-fast-forward-gap';

const el = (selector) => document.querySelector(selector);

/** Sets a select to `value` when it offers it; else leaves it as it is. */
function setSelect(select, value) {
  if (select && value && [...select.options].some((o) => o.value === value)) select.value = value;
}

/** The controls as settings fields, with the defaults for a missing control. */
export function readReviewSettings() {
  const gap = parseInt(el(GAP)?.value, 10);
  return {
    review_covers: el(COVERS)?.value || REVIEW_DEFAULTS.review_covers,
    review_order: el(ORDER)?.value || REVIEW_DEFAULTS.review_order,
    review_tick_yes: el(TICK_YES)?.checked ?? REVIEW_DEFAULTS.review_tick_yes,
    review_fast_forward: el(FAST_FORWARD)?.checked ?? REVIEW_DEFAULTS.review_fast_forward,
    review_fast_forward_gap: Number.isFinite(gap) ? gap : REVIEW_DEFAULTS.review_fast_forward_gap,
  };
}

/** The five review fields of a loaded settings file, a missing or mistyped
 *  one replaced by its default. */
export function reviewSettingsFrom(settings) {
  const s = { ...REVIEW_DEFAULTS };
  for (const [key, fallback] of Object.entries(REVIEW_DEFAULTS)) {
    const value = settings?.[key];
    if (value != null && typeof value === typeof fallback) s[key] = value;
  }
  return s;
}

/** Fills the controls from a loaded settings file; a missing field gets its
 *  default. */
export function applyReviewSettings(settings) {
  const s = reviewSettingsFrom(settings);
  setSelect(el(COVERS), s.review_covers);
  setSelect(el(ORDER), s.review_order);
  if (el(TICK_YES)) el(TICK_YES).checked = s.review_tick_yes !== false;
  if (el(FAST_FORWARD)) el(FAST_FORWARD).checked = s.review_fast_forward !== false;
  if (el(GAP) && Number.isFinite(Number(s.review_fast_forward_gap))) el(GAP).value = String(s.review_fast_forward_gap);
}

/** Saves the settings whenever one of the controls changes. */
export function wireReviewSettings(persist) {
  for (const selector of [COVERS, ORDER, TICK_YES, FAST_FORWARD, GAP]) {
    el(selector)?.addEventListener('change', () => { persist?.(); });
  }
}
