// render_batch_progress.js
// The Render tab's batch line and bar (#954). They count only this batch's
// rows: rows finished in an earlier batch stay in the table (#772), and
// counting them started a new batch at "Batch 60%" and "(6/10 done)". The
// line says which render it's on and how long is left at the pace so far,
// with a clock, and the end says "Done in 7 min 40 s: 10 rendered". The
// pure parts come first, so they can be tested on their own.

import { STRINGS } from './strings.js';
import { JOB_STATUS } from './render_jobs.js';
import { createClock, durationWords, secondsText } from './progress_view.js';

/// `jobs` that are in the batch `ids`.
export function rowsOf(jobs, ids) {
  return (jobs || []).filter((j) => ids.has(j.id));
}

/// The batch's frames done and left. A finished or failed row has no work
/// left, a rendering one is at its own progress, a queued one has it all
/// to do; a cancelled one drops out.
export function batchFrames(rows) {
  let done = 0;
  let left = 0;
  for (const j of rows || []) {
    const frames = Math.max(0, Number(j.frames) || 0);
    if (j.status === JOB_STATUS.CANCELLED) continue;
    if (j.status === JOB_STATUS.FINISHED || j.status === JOB_STATUS.ERROR) {
      done += frames;
    } else if (j.status === JOB_STATUS.RENDERING) {
      const part = (frames * Math.min(100, Math.max(0, Number(j.progress) || 0))) / 100;
      done += part;
      left += frames - part;
    } else {
      left += frames;
    }
  }
  return { done, left };
}

/// How long the frames left take at the pace so far, in seconds; `null`
/// until the batch has run for a few seconds and rendered some frames (the
/// first seconds are FFmpeg starting up, not its pace).
export function secondsLeft({ done, left }, elapsedMs, minMs = 5000) {
  if (!(done > 0) || !(elapsedMs >= minMs)) return null;
  return left / (done / (elapsedMs / 1000));
}

/// The running line: "Rendering 3 of 10 · about 4 min left". The number is
/// the next render to finish: those finished or failed, plus one.
export function renderingLine(rows, seconds) {
  const counted = (rows || []).filter((j) => j.status !== JOB_STATUS.CANCELLED);
  const ended = counted.filter((j) => j.status === JOB_STATUS.FINISHED || j.status === JOB_STATUS.ERROR).length;
  const total = counted.length;
  const text = STRINGS.RENDER.renderingLine(Math.min(total, ended + 1), total);
  return seconds == null ? text : text + STRINGS.SCAN_PROGRESS.timeLeft(Math.round(seconds));
}

/// "Found 14 takes (3 new) in 2.1 s" from the backend's merge summary
/// (render_scan_summary.js says the same in the toast).
export function scanEndLine(summary, ms) {
  const R = STRINGS.RENDER;
  const { total = 0, added = 0, refreshed = 0, gone = 0, gone_queued: goneQueued = 0 } = summary || {};
  const seconds = secondsText(ms);
  if (total === 0 && gone === 0) return R.noTakesFoundIn(seconds);
  const parts = [];
  if (added > 0) parts.push(R.scanNewShort(added));
  if (refreshed > 0) parts.push(R.scanRefreshed(refreshed));
  if (gone > 0) parts.push(R.scanGone(gone, goneQueued));
  return R.foundTakesIn(total, parts.length ? parts.join(', ') : R.SCAN_NONE_NEW, seconds);
}

/// The batch: which rows it has, the clock over it, and the `[render]` log
/// lines. `add(ids)` takes rows into the batch, and the first one starts it;
/// `finish()` ends it and keeps its rows, so the bar still shows how it
/// ended until the next batch starts. `redraw` is called every second while
/// it runs, so the clock and the time left move between snapshots.
export function createRenderBatch({ redraw = () => {}, log = () => {}, now = () => Date.now() } = {}) {
  const current = new Set();
  let last = new Set();
  const clock = createClock({ redraw, now });
  return {
    add(ids) {
      for (const id of ids) {
        if (current.size === 0) {
          last = new Set();
          clock.start();
          log(STRINGS.RENDER.batchStartLog());
        }
        current.add(id);
      }
    },
    running: () => current.size > 0,
    ids: () => current,
    /// The rows the bar shows: this batch's, or the last one's once it ended.
    shownIds: () => (current.size > 0 ? current : last),
    ms: () => clock.ms(),
    suffix: () => clock.suffix(),
    /// Forgets a batch that never started (Start was refused).
    discard() {
      if (current.size > 0) clock.stop();
      current.clear();
    },
    /// Ends the batch: its rows in `jobs`, how many finished, failed and
    /// were cancelled, and how long it ran, in milliseconds.
    finish(jobs) {
      const wasRunning = current.size > 0;
      const ms = wasRunning ? clock.stop() : 0;
      last = new Set(current);
      current.clear();
      const rows = rowsOf(jobs, last);
      const count = (status) => rows.filter((j) => j.status === status).length;
      const result = {
        rows,
        ms,
        finished: count(JOB_STATUS.FINISHED),
        failed: count(JOB_STATUS.ERROR),
        cancelled: count(JOB_STATUS.CANCELLED),
      };
      if (wasRunning) {
        log(STRINGS.RENDER.batchEndLog(secondsText(ms), result.finished, result.failed, result.cancelled));
      }
      return result;
    },
  };
}
