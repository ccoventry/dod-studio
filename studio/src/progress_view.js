// progress_view.js
// What every long job's progress line and bar share (#951, lifted out of
// #901's Duplicates bar): the clock that runs on the line and the "Done in
// X s" seconds, the measure against the last run while the total isn't known
// yet (a folder walk), and the bar that switches between real progress and
// the sliding block. CLAUDE.md's "Visible progress" is the spec.
import { STRINGS } from './strings.js';

/// `m:ss` for a job's running time.
export function elapsedText(ms) {
  const secs = Math.max(0, Math.floor((Number(ms) || 0) / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`;
}

/// Milliseconds as seconds to one decimal, for "Done in 6.8 s".
export function secondsText(ms) {
  return (Math.max(0, Number(ms) || 0) / 1000).toFixed(1);
}

/// The key a last run's count is remembered under: `area` and the same
/// paths, in any order and any case. `extra` tells apart runs over the same
/// paths that count differently (Duplicates' All drives).
export function lastRunKey(area, paths, extra = '') {
  const sorted = [...(paths || [])].map((p) => String(p).toLowerCase()).sort();
  return `${area}:${extra ? `${extra}|` : ''}${sorted.join('|')}`;
}

/// The last run's count under `key`, 0 when there is none or the storage
/// can't be read (a private window, blocked site data).
export function readLastRun(key, storage = globalThis.localStorage) {
  try {
    return Number(storage?.getItem(key)) || 0;
  } catch {
    return 0;
  }
}

/// Remembers this run's count for the next one. A convenience only: when it
/// can't be kept, the next run's bar just slides.
export function rememberLastRun(key, count, storage = globalThis.localStorage) {
  try {
    if (count > 0) storage?.setItem(key, String(count));
  } catch {
    // Nothing to do: see above.
  }
}

/// How far a walk is against the last run's count, as `{ pct, indeterminate }`.
/// It stops short of full, since this run may count more; with no last run
/// there's nothing to measure against, and the bar slides.
export function againstLastRun(seen, expected) {
  if (expected > 0) {
    return { pct: Math.min(95, Math.round(((Number(seen) || 0) / expected) * 100)), indeterminate: false };
  }
  return { pct: 0, indeterminate: true };
}

/// Draws `{ pct, indeterminate }` on a `.progress-bar-container`: shows it,
/// then the sliding block, or the fill at `pct`.
export function showBar(bar, { pct = 0, indeterminate = false } = {}) {
  if (!bar) return;
  bar.hidden = false;
  bar.classList.toggle('indeterminate', !!indeterminate);
  const fill = bar.querySelector('.progress-bar-fill');
  if (fill) fill.style.width = `${Math.max(0, Math.min(100, pct))}%`;
}

/// Hides a bar `showBar` drew, and empties it for the next job.
export function hideBar(bar) {
  if (!bar) return;
  bar.hidden = true;
  bar.classList.remove('indeterminate');
  const fill = bar.querySelector('.progress-bar-fill');
  if (fill) fill.style.width = '0%';
}

/// A running job's clock. `redraw` is called every second while it runs, so
/// the line's `· m:ss` moves even when no progress event comes; `suffix()`
/// is that text, or nothing while stopped.
export function createClock({
  redraw = () => {}, now = () => Date.now(), every = 1000,
} = {}) {
  let startedAt = 0;
  let timer = null;
  const ms = () => (startedAt ? now() - startedAt : 0);
  return {
    start() {
      if (timer !== null) clearInterval(timer);
      startedAt = now();
      timer = setInterval(redraw, every);
    },
    /** Stops the clock; returns how long it ran, in milliseconds. */
    stop() {
      if (timer !== null) clearInterval(timer);
      timer = null;
      return ms();
    },
    running: () => timer !== null,
    ms,
    suffix() {
      return timer !== null ? STRINGS.PROGRESS.elapsedSuffix(elapsedText(ms())) : '';
    },
  };
}
