// analyzer_browser_logic.js
// The Analyzer Explorer's path and date rules, with no DOM: how a path is
// shortened for the sidebar, a folder's parent and ancestors, and a demo's
// date as the date filter compares it. analyzer_browser.js draws with them.

// Guarantees the drive letter and final folder name stay visible, eliding
// the middle when the full path is too long to fit the sidebar — e.g.
// "C:\...\DoD Demos". Falls back to a forward-slash root-relative path when
// short enough (dev's own `display_path`, see tree quick-links in
// docs/archive/tauri_parity_audit.md Area 3). Full path is always available via the
// row's `title` attribute regardless of how the visible label is shortened.
export function shortenPath(fullPath, maxChars = 34) {
  if (!fullPath) return '';
  const forward = fullPath.replace(/\\/g, '/');
  if (forward.length <= maxChars) return forward;

  const driveMatch = fullPath.match(/^([A-Za-z]:\\|\\\\|\/)/);
  const drive = driveMatch ? driveMatch[0].replace(/[\\/]+$/, '') : '';
  const segments = fullPath.split(/[\\/]/).filter(Boolean);
  let finalComponent = segments[segments.length - 1] || fullPath;

  let collapsed = `${drive}\\...\\${finalComponent}`;
  if (collapsed.length > maxChars) {
    const keep = Math.max(6, maxChars - drive.length - 8);
    finalComponent = `…${finalComponent.slice(-keep)}`;
    collapsed = `${drive}\\...\\${finalComponent}`;
  }
  return collapsed;
}

// `pinned_folders`/`demo_folder_history` are shared with Capture Studio's
// own "+ Add Demo Files" flow, which legitimately pushes individual `.dem`
// FILE paths into the same list (main.js's scanPaths) for its own rescan
// purposes. Quick Links/the tree only ever navigate to folders, so filter
// those file entries out here rather than showing them as bogus "pinned
// folders" that error out with "Not a directory" when clicked.
export function isFolderPath(p) { return !/\.dem$/i.test(p); }

/** The separator `p` uses: a backslash when it has one, else a slash. */
export function pathSeparator(p) { return p.includes('\\') ? '\\' : '/'; }

/** The folder `path` is in, or null at the top of the tree. */
export function parentDirOf(path) {
  const sep = pathSeparator(path);
  const idx = path.lastIndexOf(sep);
  return idx > 0 ? path.slice(0, idx) : null;
}

// Full ancestor chain from the top of the tree down to (and including)
// `path` itself — used to auto-expand the Explorer Tree down to a selection
// and to detect ancestor/descendant relationships for the collapse-jumps-up
// mechanic (analyzer_browser.js).
export function ancestorChain(path) {
  const sep = pathSeparator(path);
  const trimmed = path.endsWith(sep) && path.length > sep.length ? path.slice(0, -1) : path;
  const segments = trimmed.split(sep).filter((s) => s.length > 0);
  const chain = [];
  if (sep === '\\' && /^[A-Za-z]:$/.test(segments[0] || '')) {
    let acc = `${segments[0]}\\`;
    chain.push(acc);
    for (let i = 1; i < segments.length; i++) {
      acc += segments[i];
      chain.push(acc);
      acc += '\\';
    }
  } else {
    let acc = '/';
    for (const seg of segments) {
      acc = acc === '/' ? `/${seg}` : `${acc}/${seg}`;
      chain.push(acc);
    }
  }
  return chain;
}

/** Whether `candidate` is a folder above `descendant` (not the same one). */
export function isAncestorOf(candidate, descendant) {
  if (!descendant || candidate === descendant) return false;
  return ancestorChain(descendant).includes(candidate);
}

/** A listed demo's modified date as `YYYY-MM-DD` (UTC), or '' when the
 *  listing has none: the form the date filter compares. */
export function demoDateISO(entry) {
  if (!entry.modified_unix_secs) return '';
  return new Date(entry.modified_unix_secs * 1000).toISOString().slice(0, 10);
}
