// scan_progress.js — what every scan that reads demos one by one adds to its
// progress line (#687): List Demos, Cache all and the Master Queue scan. The
// demos the analyzer cache has go first and take milliseconds; the parses
// come after, and the time left is measured from their pace.
import { STRINGS } from './strings.js';

/** " (N from the analyzer cache)", or nothing when the cache has none. */
export function fromCacheText(cached) {
  return cached > 0 ? STRINGS.SCAN_PROGRESS.fromCache(cached) : '';
}

/**
 * " · about N s left", once a parse has finished to measure the pace by;
 * otherwise nothing. `parseMs` is how long the parses have been running.
 * The pace goes by bytes (demo sizes vary too much, 5 MB tests to 90 MB
 * matches, for a time per demo), and by the demo count when the event has no
 * byte counts.
 */
export function timeLeftText({
  total = 0, cached = 0, parsed = 0, bytes_to_parse: bytesToParse = 0, bytes_parsed: bytesParsed = 0,
} = {}, parseMs = 0) {
  const left = total - cached - parsed;
  if (!(parsed > 0 && left > 0 && parseMs > 0)) return '';
  const msLeft = bytesToParse > 0 && bytesParsed > 0
    ? (parseMs / bytesParsed) * (bytesToParse - bytesParsed)
    : (parseMs / parsed) * left;
  return STRINGS.SCAN_PROGRESS.timeLeft(Math.round(msLeft / 1000));
}

/** Seconds to one decimal. */
const secs = (ms) => (Math.max(0, ms) / 1000).toFixed(1);

/**
 * The activity log's line for a finished or cancelled scan: how long it
 * took, how many demos it read and how many it skipped as unchanged, and,
 * when the last progress event said, how many came from the analyzer cache
 * and how long the parses took.
 */
export function scanEndLogLine({
  cancelled = false, ms = 0, read = 0, unchanged = 0, progress = null, parseMs = 0,
} = {}) {
  const S = STRINGS.SCAN_PROGRESS;
  const line = S.endLog(cancelled ? 'cancelled' : 'finished', secs(ms), read, unchanged);
  if (!progress || !(progress.total > 0)) return line;
  return line + S.endLogSplit(progress.cached || 0, progress.parsed || 0, secs(parseMs));
}

/** The activity log's line for a scan that failed. */
export function scanFailedLogLine(ms, err) {
  return STRINGS.SCAN_PROGRESS.failedLog(secs(ms), err);
}

/**
 * Times the parses, which start once the cached demos are done. `elapsed`
 * takes each event's done and cached counts and returns the milliseconds
 * since the parses started, 0 before; `reset` readies it for the next run.
 */
export function parseClock(now = () => Date.now()) {
  let startedAt = null;
  return {
    reset() { startedAt = null; },
    elapsed(done = 0, cached = 0) {
      if (startedAt === null && done >= cached) startedAt = now();
      return startedAt === null ? 0 : now() - startedAt;
    },
  };
}

/**
 * Cache all's progress line and bar for one `demo_cache_progress` event:
 * while older cached analyses are being upgraded, that step and its count;
 * then the caching count with its time left. `parses` is the run's
 * `parseClock`, which only starts once the upgrades are over, so their time
 * doesn't count as parsing.
 */
export function cacheRunView(payload, parses) {
  const upgrade = payload.upgrade ?? {};
  if (upgrade.running) {
    return {
      text: STRINGS.ANALYZER.cacheUpgrading(payload),
      value: upgrade.done ?? 0,
      max: Math.max(1, upgrade.total ?? 0),
    };
  }
  return {
    text: STRINGS.ANALYZER.cacheProgress(payload)
      + timeLeftText(payload, parses.elapsed(payload.done, payload.cached)),
    value: payload.done ?? 0,
    max: Math.max(1, payload.total ?? 0),
  };
}
