// path_display.js
// Showing a long Windows path in a small space: the drive, then as many of
// the last folders as fit. Pure, so it can be tested on its own.

/** The file name at the end of a path. */
export function fileNameOf(path) {
  return String(path || '').split(/[\\/]/).pop();
}

/** The folder a file is in: everything before its name. */
export function folderOf(path) {
  return String(path || '').replace(/[\\/][^\\/]*$/, '');
}

/**
 * `folder` cut to about `maxLength` characters, keeping the drive and the
 * last folders, which are what tell two locations apart:
 * `C:\…\dod\test stuff\subfolder`. The last folder is always kept, even
 * when it alone is longer. Uses the path's own separator.
 */
export function shortFolder(folder, maxLength = 48) {
  const text = String(folder || '');
  if (text.length <= maxLength) return text;
  const sep = text.includes('\\') ? '\\' : '/';
  const parts = text.split(/[\\/]/).filter((p, i) => p || i === 0);
  const drive = parts[0];
  const tail = [];
  // Drive, separator, ellipsis, separator: the fixed cost of the short form.
  let length = drive.length + 3;
  for (let i = parts.length - 1; i > 0; i--) {
    const add = parts[i].length + (tail.length ? 1 : 0);
    if (tail.length && length + add > maxLength) break;
    tail.unshift(parts[i]);
    length += add;
  }
  if (tail.length === parts.length - 1) return text;
  return [drive, '…', ...tail].join(sep);
}
