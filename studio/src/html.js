// html.js
// Escaping text for innerHTML and attribute values, in one place.

/** `s` with `& < > "` escaped, so it can go inside element text or a
 *  double-quoted attribute. `null`/`undefined` become an empty string. */
export function escapeHtml(s) {
  return String(s ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
