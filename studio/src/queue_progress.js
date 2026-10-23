// queue_progress.js
// The Master Demo Queue's progress line and bar (#951): the line beside the
// scan spinner and the bar under it, for everything that reads demos into
// the queue. A scan looks for demos, reads them and groups them by match on
// one bar that never goes back; a split from Start Capture runs each demo's
// split and the scan of its maps on that same bar. They used to overwrite
// each other's line, and the bar stayed empty while folders were listed.
//
// A job owns the line from `begin` to `end`. Work inside a job (a scan's
// grouping, a split's scan) is handed a `place`: the job and the share of
// its bar the work fills, and optionally the line it shows instead of its
// own. The pure parts come first, so they can be tested on their own.

import { STRINGS } from './strings.js';
import { againstLastRun, createClock, hideBar, showBar } from './progress_view.js';

/// Each step's share of a scan's bar. Looking for demos is quick unless the
/// folder is a whole drive; reading the demos is the scan; grouping reads
/// the analyzer cache the scan just filled.
export const SCAN_STEPS = { looking: [0, 10], reading: [10, 90], grouping: [90, 100] };

/// A split's share of its demo's part of the bar; the scan of the maps it
/// wrote fills the rest.
export const SPLIT_SHARE = 70;

/// `pct` of a step (0-100) as a point on the whole bar, inside `[from, to]`.
export function inRange([from, to], pct) {
  const p = Math.max(0, Math.min(100, Number(pct) || 0));
  return from + ((to - from) * p) / 100;
}

/// `place`'s share `[from, to]`, itself a share of `place`'s range. `text`
/// is the line the inner work shows; without one it keeps `place`'s, if any.
export function nest(place, [from, to], text = place?.text) {
  const range = place?.range ?? [0, 100];
  return { job: place?.job ?? null, range: [inRange(range, from), inRange(range, to)], text };
}

/// A step's own `{ text, pct, indeterminate }` placed on the whole bar.
export function placed(place, view) {
  return {
    ...view,
    pct: inRange(place?.range ?? [0, 100], view.pct),
    text: place?.text ?? view.text,
  };
}

/// One `scan_progress` event as the step's own view, `null` for the last
/// event (complete or cancelled), which says nothing the scan's end doesn't.
/// While folders are listed (`listing`) the bar measures against the last
/// scan of the same folders, `expectedFolders`, or slides; then it counts
/// demos read of the total, with the time left `timeLeft` (scan_progress.js).
export function scanStepView(p, { expectedFolders = 0, timeLeft = '' } = {}) {
  if (!p || p.cancelled || p.status === 'Complete') return null;
  if (p.listing) {
    const bar = againstLastRun(p.folders ?? 0, expectedFolders);
    return {
      step: 'looking',
      text: STRINGS.MAIN.lookingLine(p.folders ?? 0, p.demos ?? 0, expectedFolders),
      pct: bar.pct,
      indeterminate: bar.indeterminate,
    };
  }
  const total = p.total ?? 0;
  const done = Math.min(p.scanned ?? 0, total);
  return {
    step: 'reading',
    text: (p.status || '') + timeLeft,
    pct: total > 0 ? (100 * done) / total : 0,
    indeterminate: false,
  };
}

/// One `match_group_progress` event as the step's own view.
export function groupingStepView(p) {
  const total = p?.total ?? 0;
  const done = Math.min(p?.done ?? 0, total);
  return {
    step: 'grouping',
    text: STRINGS.WORKSPACE.groupingStatus(done, total),
    pct: total > 0 ? (100 * done) / total : 0,
    indeterminate: false,
  };
}

/// The line and bar. `begin()` takes them over and starts the clock; with
/// `ifIdle`, it gives `null` while another job runs (a regroup started by
/// the queue's Group by never takes the line from a scan). `show` ignores a
/// job that no longer owns the line; `end` puts up the job's last line, or
/// the line from before it when given none.
export function createQueueProgress({ doc = globalThis.document } = {}) {
  const els = () => ({
    text: doc?.querySelector('#scan-status'),
    spinner: doc?.querySelector('#scan-spinner'),
    bar: doc?.querySelector('#scan-progress'),
  });
  let current = null;
  let view = null;
  const draw = () => {
    if (!current || !view) return;
    const { text, bar } = els();
    if (text) text.textContent = view.text + clock.suffix();
    showBar(bar, view);
  };
  const clock = createClock({ redraw: draw });

  return {
    begin({ ifIdle = false, text = STRINGS.MAIN.SCAN_STARTING } = {}) {
      if (current && ifIdle) return null;
      const { text: textEl, spinner } = els();
      const job = { before: current ? current.before : (textEl?.textContent ?? '') };
      current = job;
      view = { text, pct: 0, indeterminate: true };
      clock.start();
      if (spinner) spinner.style.display = 'inline-block';
      draw();
      return job;
    },
    show(job, next) {
      if (!job || job !== current) return;
      view = { ...view, ...next };
      draw();
    },
    /** Milliseconds since `job` began, while it runs. */
    ms(job) {
      return job && job === current ? clock.ms() : 0;
    },
    end(job, text = null) {
      if (!job || job !== current) return;
      current = null;
      view = null;
      clock.stop();
      const { text: textEl, spinner, bar } = els();
      if (textEl) textEl.textContent = text ?? job.before;
      if (spinner) spinner.style.display = 'none';
      hideBar(bar);
    },
  };
}

// The queue has one line, so the scan, the grouping and the splits share one.
let shared = null;

/** The queue's progress line, made on first use. */
export function queueProgress() {
  if (!shared) shared = createQueueProgress();
  return shared;
}
