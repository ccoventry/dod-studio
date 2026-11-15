// field_error.js
// Marks one input as wrong and puts the reason on a line right under it (#25),
// so a problem with a field shows at the field instead of only in a toast.
// The mark clears itself the moment the field is edited.

const LINE_CLASS = 'field-error-line';
const INVALID_CLASS = 'field-invalid';

/** Where the line goes: under the whole row when the input sits in a row with
 *  buttons (so it is not squeezed in beside them), else right after the input. */
function anchorOf(input) {
  return input.closest('.input-row') || input;
}

/** The line this input owns, made on first use. */
function lineFor(input) {
  const anchor = anchorOf(input);
  let line = anchor.nextElementSibling;
  if (line?.classList.contains(LINE_CLASS) && line.dataset.forField === (input.id || '')) return line;
  line = document.createElement('div');
  line.className = LINE_CLASS;
  line.dataset.forField = input.id || '';
  line.hidden = true;
  line.setAttribute('role', 'alert');
  anchor.insertAdjacentElement('afterend', line);
  return line;
}

/** Red or not, for a field that already has its own message line. */
export function markFieldInvalid(input, invalid) {
  if (!input) return;
  input.classList.toggle(INVALID_CLASS, invalid);
  if (invalid) input.setAttribute('aria-invalid', 'true');
  else input.removeAttribute('aria-invalid');
}

/** Removes the mark and the line's text. */
export function clearFieldError(input) {
  if (!input) return;
  markFieldInvalid(input, false);
  const line = anchorOf(input).nextElementSibling;
  if (line?.classList.contains(LINE_CLASS)) {
    line.textContent = '';
    line.hidden = true;
  }
}

/**
 * Turns the input red and shows `message` under it; an empty message clears
 * instead. Editing the input (typing, or a `change`) clears it, and so does a
 * change to the children of `opts.list`, for a field that adds to a list.
 * A field whose own handler re-judges on every edit passes `autoClear: false`
 * and calls this each time, so the handlers never race.
 * @param {HTMLElement | null} input
 * @param {string} message
 * @param {{ list?: Element | null, autoClear?: boolean }} [opts]
 */
export function setFieldError(input, message, opts = {}) {
  if (!input) return;
  if (!message) {
    clearFieldError(input);
    return;
  }
  markFieldInvalid(input, true);
  const line = lineFor(input);
  line.textContent = message;
  line.hidden = false;
  if (opts.autoClear === false) return;
  if (!input.dataset.fieldErrorWired) {
    input.dataset.fieldErrorWired = '1';
    for (const type of ['input', 'change']) input.addEventListener(type, () => clearFieldError(input));
  }
  if (opts.list && !opts.list.dataset.fieldErrorWatch) {
    opts.list.dataset.fieldErrorWatch = '1';
    new MutationObserver(() => clearFieldError(input)).observe(opts.list, { childList: true });
  }
}
