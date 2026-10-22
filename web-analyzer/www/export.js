// export.js — pure helpers for the JSON export (issue #102). No DOM access,
// so web-analyzer/tests/export.test.mjs can run them under plain `node --test`.

/// True for a `.dem` file name (case-insensitive).
export function isDemoFileName(name) {
  return /\.dem$/i.test(String(name || ''));
}

/// Which of the picked/dropped files to analyse. A single file is passed
/// through untouched, exactly as before batch support; with several, anything
/// that isn't a `.dem` (a dropped folder's .txt or .cfg, say) is left out.
export function selectDemoFiles(files) {
  const list = Array.from(files || []);
  if (list.length <= 1) return list;
  return list.filter((f) => isDemoFileName(f.name));
}

/// One demo's export entry. `analysis` is `analyzeDemo`'s result, or null
/// when the demo failed to parse, in which case `error` says why.
export function buildExportEntry(file, analysis, error = null) {
  const entry = {
    file_name: file.name,
    file_size_bytes: file.size,
    file_modified_unix_secs: file.lastModified ? Math.floor(file.lastModified / 1000) : 0,
  };
  if (analysis) {
    entry.demo_info = analysis.demo_info;
    entry.state = analysis.state;
  } else {
    entry.error = String(error ?? 'unknown error');
  }
  return entry;
}

/// Serialises an entry, or an array of them, as pretty-printed JSON. A u64
/// the wasm side ever hands over as a BigInt is written as a string rather
/// than making JSON.stringify throw.
export function toJson(value) {
  return JSON.stringify(value, (_key, v) => (typeof v === 'bigint' ? v.toString() : v), 2);
}

/// Download name for one demo's JSON: the demo's name with `.dem` swapped
/// for `.json` (`dod_anzio_0001.dem` -> `dod_anzio_0001.json`).
export function jsonFileNameFor(demoName) {
  const base = String(demoName || 'demo').replace(/\.dem$/i, '') || 'demo';
  return `${base}.json`;
}

/// Download name for the whole batch, e.g. `demo-analysis-3-demos-2026-09-29.json`.
export function batchFileName(count, date = new Date()) {
  const pad = (n) => String(n).padStart(2, '0');
  const day = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  return `demo-analysis-${count}-demo${count === 1 ? '' : 's'}-${day}.json`;
}
