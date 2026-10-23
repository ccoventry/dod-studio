// hd_progress.js
// What the HD Textures page's progress lines say and what their bars show
// (#956), from the backend's events: Set up tools (one equal share of the
// bar per file, filled by its bytes, the block sliding over an unpack),
// Build (one share per step, the block sliding over the running step, since
// build_all.py can go a minute without printing) and the map coverage check.
// Each view is `{ text, bar }`, `bar` as `step_bar.js`'s `drawBar` takes it.
import { STRINGS } from './strings.js';
import { elapsedWords } from './time_format.js';

export function formatSize(bytes) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/// The moment Set up is clicked: the backend first checks what's there.
export function setupStartView() {
  return { text: STRINGS.HD.SETUP_CHECKING, bar: { slide: true } };
}

/// One `hd_setup_progress` event (`native::hd::setup::SetupProgress`).
export function setupView(p) {
  const steps = Math.max(1, p.steps || 1);
  const done = Math.max(0, (p.step || 1) - 1);
  if (p.unpacking) {
    return { text: STRINGS.HD.unpackingLine(p.item, p.step, steps), bar: { done, total: steps } };
  }
  if (p.bytes_total) {
    return {
      text: STRINGS.HD.progressLine(p.item, p.step, steps,
        STRINGS.HD.bytesOf(formatSize(p.bytes_done), formatSize(p.bytes_total))),
      bar: { done, total: steps, fraction: Math.min(1, p.bytes_done / p.bytes_total) },
    };
  }
  // The server didn't say how big the file is: the block slides over it.
  return {
    text: STRINGS.HD.progressLine(p.item, p.step, steps, formatSize(p.bytes_done)),
    bar: { done, total: steps },
  };
}

/// The moment Build is clicked, before build_all.py's first step line.
export function buildStartView() {
  return { text: STRINGS.HD.BUILD_STARTING, bar: { slide: true } };
}

/// The running step (`native::hd::build::BuildProgress`), `secs` into the
/// build. Step 2 of 6 fills the bar to 17%, and the block slides from there
/// to 33% until step 3 starts.
export function buildView(p, secs, typeName) {
  const steps = Math.max(1, p.steps || 1);
  return {
    text: STRINGS.HD.buildStep(p.step, steps, p.style, typeName, elapsedWords(secs)),
    bar: { done: Math.max(0, (p.step || 1) - 1), total: steps },
  };
}

/// The map coverage check (`hd_coverage_progress`), shown from its start:
/// the block slides until the first count.
export function coverageView(progress) {
  if (!progress?.total) return { text: STRINGS.HD.COVERAGE_CHECKING, bar: { slide: true } };
  return {
    text: STRINGS.HD.coverageChecking(progress.done, progress.total),
    bar: { pct: Math.round((progress.done / progress.total) * 100) },
  };
}
