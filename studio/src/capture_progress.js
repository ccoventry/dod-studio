// capture_progress.js
// The capture batch's footer line and bar (#952). One bar for the whole
// batch: getting the demos ready fills its first tenth and the clips the
// rest, so it never drops back to 0 between the two, never goes backwards,
// and never shows a made-up 50%. A step it can't measure (starting the
// game, loading a demo, the clip being recorded) keeps the fill where it is
// and slides a block over that step's share. A clock runs on the line from
// the first report to the end, which says "Done in 6 min 12 s: 12 clips
// captured".
//
// The backend (capture_manager.rs, hook_batch.rs) sends each `capture_status`
// with a `stage`, the line in plain words, and `index` of `total`: demos for
// the steps that get them ready, clips for the rest, takes for the check at
// the end. The pure parts come first, so they can be tested on their own.

import { STRINGS } from './strings.js';
import { createClock } from './progress_view.js';
import { durationTextMs } from './time_format.js';
import { clearStepBar, showFillAndSlide } from './step_bar.js';
import { timeLeftSuffix } from './capture_time_left.js';
import { setStatusLine } from './status_line.js';

/// Each step's share of the bar, in percent. Reading a demo again happens
/// only for a project saved before demo-player times existed, and patching
/// only for a batch that doesn't seek between clips, so most batches start
/// at the clips' share; the check of the takes at the end is quick.
export const CAPTURE_SHARES = {
  readingAgain: [0, 5],
  preparing: [5, 10],
  clips: [10, 98],
  checking: [98, 100],
};

/// `pct` (0-100) of a share `[from, to]` as a point on the whole bar.
function inShare([from, to], pct) {
  const p = Math.max(0, Math.min(100, Number(pct) || 0));
  return from + ((to - from) * p) / 100;
}

/// One running `capture_status` payload as `{ fill, slideTo }` on the whole
/// bar, before the bar's never-go-back rule: the fill in percent, and where
/// the sliding block over an unmeasured step ends (`null`: no block).
/// A payload without a stage measures nothing.
export function captureStepView(p) {
  const total = Math.max(0, Number(p?.total) || 0);
  const index = Math.max(0, Math.min(total, Number(p?.index) || 0));
  const of = (n) => (total > 0 ? (100 * Math.min(n, total)) / total : 0);
  switch (p?.stage) {
    case 'reading_again': {
      // One demo read at a time, with nothing to measure inside it.
      const share = CAPTURE_SHARES.readingAgain;
      return { fill: inShare(share, of(index)), slideTo: inShare(share, of(index + 1)) };
    }
    case 'preparing': {
      // The first line has no count yet: the block slides over all of the
      // getting-ready shares.
      if (total === 0) return { fill: 0, slideTo: CAPTURE_SHARES.preparing[1] };
      const share = CAPTURE_SHARES.preparing;
      const fill = inShare(share, of(index));
      return { fill, slideTo: p.measured === false ? inShare(share, of(Math.floor(index) + 1)) : null };
    }
    case 'trimming':
      // A trimmed copy being made (#207): its line has the percent, and a
      // patched batch fills the same share afterwards, so it only slides.
      return { fill: 0, slideTo: CAPTURE_SHARES.preparing[1] };
    case 'starting':
    case 'loading':
    case 'capturing': {
      // `index` clips are done; the next one's share slides until it is.
      const share = CAPTURE_SHARES.clips;
      if (total === 0) return { fill: share[0], slideTo: share[1] };
      return { fill: inShare(share, of(index)), slideTo: inShare(share, of(index + 1)) };
    }
    case 'checking':
      return { fill: inShare(CAPTURE_SHARES.checking, of(index)), slideTo: null };
    default:
      return { fill: 0, slideTo: null };
  }
}

/// `view` held to the bar's rule: the fill never goes below `shown`, and
/// the block, when there is one, starts at the fill.
export function heldView(view, shown) {
  const fill = Math.max(shown, view.fill);
  if (view.slideTo == null || view.slideTo <= fill) return { fill, slideFrom: null, slideWidth: 0 };
  return { fill, slideFrom: fill, slideWidth: view.slideTo - fill };
}

/// The line a batch ends on, from its last `capture_status` (`running:
/// false`) and how long it ran: "Done in …" with the clips that landed, the
/// error, or "Cancelled", as the toasts say.
export function endLine(p, ms) {
  if (p?.error) return STRINGS.CAPTURE.captureErrorStatusText(p.status || STRINGS.CAPTURE.CAPTURE_ERROR_TEXT_DEFAULT);
  if (p?.status === 'Cancelled') return STRINGS.CAPTURE.CANCELLED;
  return STRINGS.CAPTURE_PROGRESS.done(durationTextMs(ms), p?.clips_captured ?? null, p?.clips_total ?? null);
}

/// How a batch ended, for the activity log and Last Batch.
export function endKind(p) {
  if (p?.error) return 'error';
  return p?.status === 'Cancelled' ? 'cancelled' : 'completed';
}

/// The footer's line and bar. `update(payload)` for every running report
/// (the first one starts the clock), `end(payload)` for the last; `log` is
/// where the `[capture]` start and end lines go (the activity log).
export function createCaptureProgress({ statusEl, container, now = () => Date.now(), log = () => {} } = {}) {
  let running = false;
  let shown = 0;
  let line = '';
  let timeLeft = null;
  // Render While Capturing's hold (#861): `{ text, replaces }`. A batch held
  // between clips records nothing, so its line replaces the capture's; one
  // that will hold after this clip adds to it.
  let room = null;

  const draw = () => {
    if (!running) return;
    const text = room?.replaces
      ? room.text
      : line + timeLeftSuffix(timeLeft, now()) + (room ? ` · ${room.text}` : '');
    setStatusLine(statusEl, text + clock.suffix());
  };
  const clock = createClock({ redraw: draw, now });

  return {
    isRunning: () => running,

    update(p) {
      if (!running) {
        running = true;
        shown = 0;
        timeLeft = null;
        room = null;
        clock.start();
        log(STRINGS.CAPTURE_PROGRESS.startLog());
      }
      // The checks before the batch slid the whole bar; from here it measures.
      container?.classList.remove('indeterminate');
      const held = heldView(captureStepView(p), shown);
      shown = held.fill;
      showFillAndSlide(container, held.fill, held.slideFrom, held.slideWidth);
      // Only clip boundaries carry an estimate; it counts down in between,
      // and goes once the clips are done.
      if (p?.seconds_left != null) timeLeft = { seconds: p.seconds_left, at: now() };
      if (p?.stage === 'checking') timeLeft = null;
      line = p?.status || STRINGS.CAPTURE.CAPTURING_ELLIPSIS_DEFAULT;
      draw();
    },

    /// Render While Capturing's hold line, or `null` once it's over.
    setRoom(r) {
      room = r || null;
      draw();
    },

    /// The batch ended. Returns how long it ran, in milliseconds.
    end(p) {
      const ms = running ? clock.stop() : 0;
      const wasRunning = running;
      running = false;
      timeLeft = null;
      room = null;
      clearStepBar(container);
      const fill = container?.querySelector('.progress-bar-fill');
      const kind = endKind(p);
      if (fill && kind === 'completed') fill.style.width = '100%';
      if (fill && kind === 'cancelled') fill.style.width = '0%';
      setStatusLine(statusEl, endLine(p, ms));
      if (wasRunning) {
        log(STRINGS.CAPTURE_PROGRESS.endLog(kind, durationTextMs(ms), p?.clips_captured, p?.clips_total ?? null));
      }
      return ms;
    },
  };
}
