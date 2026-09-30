// status_line.js
// The capture footer's status line (#534). It sits beside the batch buttons
// and is clamped to two lines by CSS (#batch-status in styles.css), with the
// whole message in its tooltip. Kept free of anything else so it can be
// unit tested.

// Pointers at the log that the capture engine appends to its error text.
// They're right in the log (which gets its own copy of the message), and
// just noise beside the buttons, where the user isn't reading the log.
const LOG_ONLY_SUFFIXES = [
  / — see \[HLAE\] lines above in this log for timing\./g,
  / \(see View Logs for details\)/g,
];

/** `text` as the UI shows it: without the log-only suffixes. */
export function uiStatusText(text) {
  let out = String(text ?? '');
  for (const suffix of LOG_ONLY_SUFFIXES) out = out.replace(suffix, '');
  return out.trim();
}

/** Shows `text` in `el`, with the whole of it on hover in case it's clamped. */
export function setStatusLine(el, text) {
  if (!el) return;
  const shown = uiStatusText(text);
  el.textContent = shown;
  el.title = shown;
}
