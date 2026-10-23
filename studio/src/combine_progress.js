// combine_progress.js — what Combine Clips' bar and step line say (#955),
// with no DOM: the clips being re-read, then FFmpeg joining them, then the
// end. combine_clips.js puts it on the window.
import { STRINGS } from './strings.js';
import { clockRound, elapsedWords } from './time_format.js';

// No time left until the join has run this long and got this far: FFmpeg's
// first seconds (opening files, the encoder warming up) say little about
// its pace.
const LEFT_AFTER_MS = 2000;
const LEFT_AFTER_FRACTION = 0.02;

/// The line the moment Combine is clicked, before any event.
export function startView() {
  return { pct: 0, text: STRINGS.COMBINE.CHECKING_STEP, indeterminate: true };
}

/// A `combine_progress` event → the bar and the line. `totalSecs` is the
/// plan's length; `joinMs` how long the join has run (0 before it starts).
/// Re-reading the clips can't be measured inside a clip, so it slides; an
/// event with no `step` is a join (the shape before #955).
export function combineProgressView(p, totalSecs, joinMs = 0) {
  if (p?.step === 'checking') {
    const total = p.total ?? 0;
    const done = Math.min(p.done ?? 0, total);
    return {
      pct: 0,
      text: total ? STRINGS.COMBINE.checkingLine(Math.min(done + 1, total), total) : STRINGS.COMBINE.CHECKING_STEP,
      indeterminate: true,
    };
  }
  const fraction = Math.max(0, Math.min(1, Number(p?.fraction) || 0));
  let text = STRINGS.COMBINE.joiningLine(clockRound(fraction * totalSecs), clockRound(totalSecs));
  if (fraction >= LEFT_AFTER_FRACTION && fraction < 1 && joinMs >= LEFT_AFTER_MS) {
    const left = Math.round(((joinMs / fraction) * (1 - fraction)) / 1000);
    text += STRINGS.SCAN_PROGRESS.timeLeft(left);
  }
  return { pct: Math.round(fraction * 100), text, indeterminate: false };
}

/// The line once it ends: `outcome` is 'saved', 'cancelled' or 'failed'.
export function endText(outcome, ms) {
  if (outcome === 'saved') return STRINGS.COMBINE.savedIn(elapsedWords(Math.max(0, Math.round(ms / 1000))));
  if (outcome === 'cancelled') return STRINGS.COMBINE.STOPPED;
  return STRINGS.COMBINE.FAILED;
}
