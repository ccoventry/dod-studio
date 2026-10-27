// obs_capture_test_logic.js
// The OBS capture test's decisions (#646), pure so they are unit-tested:
// which rates the "highest steady FPS" run tries, when it stops, what it
// reports, and the one setting to change. The verdict for each clip comes
// from the backend (native::obs::bench::judge).

import { STRINGS } from './strings.js';

/** native::obs::bench::FPS_LADDER. */
export const FPS_LADDER = [60, 120, 144, 165, 180, 200, 240];

/** Clip lengths, in seconds: the usual test, and the one for warm-up. */
export const CLIP_SECONDS = { short: 20, long: 60 };

export const LEVEL = { STEADY: 'steady', BORDERLINE: 'borderline', NOT_STEADY: 'not_steady' };

/** OBS Quality choices from highest to lowest, with what lowering one gives. */
const QUALITY_DOWN = { max: 'high', high: 'standard', standard: 'small' };

/** The display name of an OBS Quality id, from its dropdown option. */
export function qualityName(id) {
  const names = {
    small: 'Small', standard: 'Standard', high: 'High', max: 'Max', obs_simple: "OBS's own",
  };
  return names[id] || names.standard;
}

export function levelName(level) {
  if (level === LEVEL.STEADY) return STRINGS.OBS_TEST.LEVEL_STEADY;
  if (level === LEVEL.BORDERLINE) return STRINGS.OBS_TEST.LEVEL_BORDERLINE;
  return STRINGS.OBS_TEST.LEVEL_NOT_STEADY;
}

export function causeName(cause) {
  if (cause === 'encoder') return STRINGS.OBS_TEST.CAUSE_ENCODER;
  if (cause === 'rendering') return STRINGS.OBS_TEST.CAUSE_RENDERING;
  if (cause === 'game') return STRINGS.OBS_TEST.CAUSE_GAME;
  return '';
}

/** A share (0..1) as a percentage people read: "0%", "0.07%", "4.2%". */
export function pct(share) {
  const p = (Number(share) || 0) * 100;
  if (p === 0) return '0%';
  if (p < 1) return `${p.toFixed(2)}%`;
  return `${p.toFixed(1)}%`;
}

/** The next rate below `rate` worth trying: the ladder's step below, or half. */
export function lowerRate(rate) {
  const below = FPS_LADDER.filter((r) => r < rate);
  if (below.length) return below[below.length - 1];
  return Math.max(30, Math.round(rate / 2));
}

/**
 * The one setting to change after a clip at `rate` and OBS Quality `quality`:
 * `{ kind: 'none' }`, `{ kind: 'fps', value }` or `{ kind: 'quality', value }`.
 * An encoder that lagged at High or Max first gives up quality (#1060: High
 * peaked at 94% of the encoder where Standard peaked at 75%); everything else
 * lowers the rate.
 */
export function adviceFor(verdict, rate, quality) {
  if (!verdict || verdict.level === LEVEL.STEADY) return { kind: 'none' };
  if (verdict.cause === 'encoder' && QUALITY_DOWN[quality] && quality !== 'standard') {
    return { kind: 'quality', value: QUALITY_DOWN[quality] };
  }
  return { kind: 'fps', value: lowerRate(rate) };
}

/** The advice as the line the panel shows. */
export function adviceText(advice) {
  if (advice.kind === 'fps') return STRINGS.OBS_TEST.adviceFps(advice.value);
  if (advice.kind === 'quality') return STRINGS.OBS_TEST.adviceQuality(qualityName(advice.value));
  return STRINGS.OBS_TEST.ADVICE_NONE;
}

/** The sound check as the line the panel shows. */
export function soundText(verdict, file) {
  if (verdict?.sound === 'missing') return STRINGS.OBS_TEST.SOUND_MISSING;
  if (verdict?.sound === 'silent') return STRINGS.OBS_TEST.SOUND_SILENT;
  const peak = file?.audio_peak_db;
  return STRINGS.OBS_TEST.soundOk(peak == null ? '?' : Number(peak).toFixed(1));
}

/**
 * Whether the ladder stops after `results` (`[{ rate, verdict }]`, in the
 * order run): after a clear failure, after two rates in a row that weren't
 * steady, or after a step that produced no verdict (an error or a stop).
 */
export function ladderStops(results) {
  const last = results[results.length - 1];
  if (!last) return false;
  if (!last.verdict) return true;
  if (last.verdict.level === LEVEL.NOT_STEADY) return true;
  const prev = results[results.length - 2];
  return last.verdict.level !== LEVEL.STEADY && !!prev?.verdict && prev.verdict.level !== LEVEL.STEADY;
}

/** The highest rate that held steady, or null. */
export function highestSteady(results) {
  const steady = results.filter((r) => r.verdict?.level === LEVEL.STEADY).map((r) => r.rate);
  return steady.length ? Math.max(...steady) : null;
}

/** The first result above the highest steady rate that didn't hold, or null. */
export function firstFailure(results) {
  const best = highestSteady(results) ?? 0;
  return results.find((r) => r.rate > best && r.verdict && r.verdict.level !== LEVEL.STEADY) || null;
}

/** The ladder's summary lines. */
export function ladderSummary(results, quality) {
  const best = highestSteady(results);
  const lines = [best == null
    ? STRINGS.OBS_TEST.LADDER_NONE
    : STRINGS.OBS_TEST.ladderBest(best, qualityName(quality))];
  const fail = firstFailure(results);
  if (fail) {
    lines.push(STRINGS.OBS_TEST.ladderFirstFail(fail.rate, levelName(fail.verdict.level), causeName(fail.verdict.cause)));
  } else if (best != null && best === FPS_LADDER[FPS_LADDER.length - 1]) {
    lines.push(STRINGS.OBS_TEST.ladderAllPassed(best));
  }
  return lines;
}

/** One clip's result as the panel's lines. */
export function stepLines({ rate, verdict, file }, quality) {
  const lines = [STRINGS.OBS_TEST.verdictLine(rate, levelName(verdict.level), qualityName(quality))];
  if (verdict.level !== LEVEL.STEADY) lines.push(STRINGS.OBS_TEST.causeLine(causeName(verdict.cause)));
  lines.push(STRINGS.OBS_TEST.numbersLine(pct(verdict.skipped_share), pct(verdict.missed_share), pct(verdict.repeated_share)));
  if (!verdict.counters_read) lines.push(STRINGS.OBS_TEST.COUNTERS_UNREAD);
  lines.push(soundText(verdict, file));
  lines.push(adviceText(adviceFor(verdict, rate, quality)));
  return lines;
}

/** A session id for one test clip: starts with the backend's prefix. */
export function testSessionId(rate, now = new Date()) {
  const p = (n) => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}_${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`;
  return `obs_test_${stamp}_${rate}fps`;
}
