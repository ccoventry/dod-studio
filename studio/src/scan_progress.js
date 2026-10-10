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
