// number_field.js
// Reads a numeric <input>, falling back only when the field is empty or not a
// number.
//
// `parseFloat(value) || fallback` also threw away a typed 0, so Pre-roll,
// Post-roll and Initial Delay could never be 0: each came back as its default
// on save, in the capture payload and in the disk estimate.

/**
 * The number in the input matching `selector`, or `fallback`.
 *
 * `integer` parses with parseInt. `positive` also falls back for 0 and below,
 * for fields where 0 is not a real value (FPS, resolution, FF Speed).
 */
export function numberField(selector, fallback, { integer = false, positive = false } = {}) {
  return parseNumber(document.querySelector(selector)?.value, fallback, { integer, positive });
}

/** `numberField` without the DOM lookup, so it can be tested on its own. */
export function parseNumber(raw, fallback, { integer = false, positive = false } = {}) {
  const n = integer ? parseInt(raw, 10) : parseFloat(raw);
  if (!Number.isFinite(n)) return fallback;
  if (positive && n <= 0) return fallback;
  return n;
}
