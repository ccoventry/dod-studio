// crosshair_file.js
// Players tab, "Add a crosshair file…" (#842): the pure part. The name a
// picked file is saved under, customxhair-<name>.spr, mirrored from
// native/src/crosshair_import.rs (sanitize_name), which has the last word;
// this copy only prefills the name box and shows the file name as you type.

/** Vertex's crosshair creator, opened in the browser. */
export const CREATOR_URL = 'https://vertexs-custom-crosshair-creator.replit.app/';
export const PREFIX = 'customxhair-';
export const MAX_NAME_CHARS = 40;
/** The name box's value when the picked file's name gives nothing. */
export const FALLBACK_NAME = 'my-crosshair';

/** Lower-case letters, digits, `-` and `_`; other ASCII becomes one `-`,
 *  anything else is dropped; a trailing `.spr` and a leading `customxhair` go
 *  (the file name adds both back); at most MAX_NAME_CHARS. Same cases as the
 *  Rust tests. */
export function sanitizeCrosshairName(raw) {
  let out = '';
  for (const ch of String(raw ?? '').trim().replace(/\.spr$/i, '')) {
    const c = ch.toLowerCase();
    if (/^[a-z0-9_]$/.test(c) && ch.charCodeAt(0) < 128) {
      out += c;
    } else if (ch.charCodeAt(0) < 128 && out && !out.endsWith('-')) {
      out += '-';
    }
  }
  let name = out;
  for (;;) {
    const trimmed = name.replace(/^[-_]+|[-_]+$/g, '');
    if (trimmed.startsWith('customxhair')) {
      name = trimmed.slice('customxhair'.length);
    } else {
      name = trimmed;
      break;
    }
  }
  return name.slice(0, MAX_NAME_CHARS).replace(/[-_]+$/, '');
}

/** `customxhair-<name>.spr`, or '' when nothing of the name is usable. */
export function crosshairFileName(name) {
  const clean = sanitizeCrosshairName(name);
  return clean ? `${PREFIX}${clean}.spr` : '';
}

/** The name box's first value: the picked file's name without its folder
 *  and extension, or FALLBACK_NAME when that leaves nothing (the creator's
 *  own customxhair.spr). */
export function defaultCrosshairName(path) {
  const base = String(path ?? '').split(/[\\/]/).pop() || '';
  const stem = base.replace(/\.(spr|zip)$/i, '');
  return sanitizeCrosshairName(stem) || FALLBACK_NAME;
}

/** Whether `fileName` is already one of `sprites`, ignoring case (Windows
 *  file names do). */
export function crosshairTaken(fileName, sprites) {
  const want = String(fileName || '').toLowerCase();
  return !!want && (sprites || []).some((s) => String(s).toLowerCase() === want);
}
