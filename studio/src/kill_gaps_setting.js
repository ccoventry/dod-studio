// kill_gaps_setting.js
// Configuration > Timing's "Gaps between kills over [N] s: Keep /
// Fast-forward [4]x" (#831): the controls, the `kill_gaps` field the
// settings file and the capture payload carry (`native::patch::types::
// KillGaps`), and what the disk estimate needs from it.

// Review's "Fast-forward gaps over 8 s" (#665) and its 4x, so the two
// settings share one idea of a long gap. Mirrors KillGaps::default().
export const KILL_GAPS_DEFAULTS = Object.freeze({ mode: 'keep', over_seconds: 8, speed: 4 });
// Review's own clamp (`MAX_KILL_GAP_SPEED`).
export const MAX_KILL_GAP_SPEED = 16;
const MODES = ['keep', 'fast_forward'];

const SECONDS = '#config-kill-gap-seconds';
const MODE = '#config-kill-gap-mode';
const SPEED = '#config-kill-gap-speed';

const el = (selector) => document.querySelector(selector);

/** A `kill_gaps` value with each missing or unusable field at its default:
 *  a gap length that isn't a positive number, a speed of 1x or less, an
 *  unknown mode. A speed over the clamp is brought down to it. */
export function killGapsFrom(raw) {
  const over = Number(raw?.over_seconds);
  const speed = Number(raw?.speed);
  return {
    mode: MODES.includes(raw?.mode) ? raw.mode : KILL_GAPS_DEFAULTS.mode,
    over_seconds: Number.isFinite(over) && over > 0 ? over : KILL_GAPS_DEFAULTS.over_seconds,
    speed: Number.isFinite(speed) && speed > 1 ? Math.min(speed, MAX_KILL_GAP_SPEED) : KILL_GAPS_DEFAULTS.speed,
  };
}

/** What the disk estimate takes (`computeRequiredCaptureBytes`' killGaps):
 *  `{ overSeconds, speed }` when gaps fast-forward on a Seek Between Clips
 *  batch (`seeks`), else null. A patched batch keeps its gaps. */
export function estimateKillGaps(killGaps, seeks) {
  const k = killGapsFrom(killGaps);
  return k.mode === 'fast_forward' && seeks ? { overSeconds: k.over_seconds, speed: k.speed } : null;
}

/** The controls as a `kill_gaps` value. */
export function readKillGaps() {
  return killGapsFrom({
    mode: el(MODE)?.value,
    over_seconds: el(SECONDS)?.value,
    speed: el(SPEED)?.value,
  });
}

/** The speed only matters with Fast-forward: dimmed with Keep. */
function syncSpeedEnabled() {
  const speed = el(SPEED);
  if (speed) speed.disabled = el(MODE)?.value !== 'fast_forward';
}

/** Fills the controls from a loaded settings file's `kill_gaps`. */
export function applyKillGaps(raw) {
  const k = killGapsFrom(raw);
  if (el(MODE)) el(MODE).value = k.mode;
  if (el(SECONDS)) el(SECONDS).value = String(k.over_seconds);
  if (el(SPEED)) el(SPEED).value = String(k.speed);
  syncSpeedEnabled();
}

/** Calls `onChange` whenever one of the controls changes (save the
 *  settings, re-run the disk estimate). */
export function wireKillGaps(onChange) {
  for (const selector of [SECONDS, MODE, SPEED]) {
    el(selector)?.addEventListener('change', () => {
      syncSpeedEnabled();
      onChange?.();
    });
  }
  syncSpeedEnabled();
}
