// rescan_estimate.js
// How long the "Demos scanned by an older DoD Studio" rescan takes (#937).
// The backend sorts the demos by what the analyzer cache holds
// (`estimate_rescan`: cached, upgradable, to parse and their bytes); the
// demos it has or upgrades take milliseconds each, the full reads go at this
// PC's own read rate, learned from its scans and kept in the settings file.

import { estimateRescan, logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** A full read until a scan has measured one: cautious, since a PC slower
 *  than the developer's (0.5 s per demo, #937) is the one that needs warning. */
export const DEFAULT_SECS_PER_FULL_READ = 3;
/** A cached or upgraded demo: 158 WSOD25 demos rescanned in 2.1 s (#937),
 *  about 13 ms each, rounded up. */
export const MS_PER_CACHED_DEMO = 15;
/** How far one scan moves the learned rate: its bytes over this, at most
 *  half way, so one odd scan (a single small demo, a cold disk) can't swing
 *  it and a big one counts for more. */
const FULL_WEIGHT_BYTES = 1e9;
const MAX_WEIGHT = 0.5;

/** This PC's full-read rate in bytes per second; 0 until a scan measures one. */
let readRate = 0;

export function getScanReadRate() {
  return readRate;
}

/** The rate as a settings field, for settings_sync.js's save. */
export function scanRateSettings() {
  return { scan_read_bytes_per_sec: readRate };
}

/** Takes the rate from a loaded settings file; a missing or bad one is 0. */
export function applyScanRateSettings(settings) {
  const rate = Number(settings?.scan_read_bytes_per_sec);
  readRate = Number.isFinite(rate) && rate > 0 ? rate : 0;
}

/**
 * The rate after a scan: `outcome` is scan_directory's answer, whose
 * `full_reads`/`full_read_bytes` are the demos it read in full and
 * `not_cached_ms` their wall time (with the scan's workers, the same ones a
 * rescan uses). The first measurement replaces the default; later ones move
 * it by their weight. A cancelled scan, or one that read nothing in full,
 * leaves it.
 */
export function learnRate(rate, outcome) {
  const bytes = Number(outcome?.full_read_bytes) || 0;
  const ms = Number(outcome?.not_cached_ms) || 0;
  if (outcome?.cancelled || !(bytes > 0) || !(ms > 0)) return rate;
  const measured = bytes / (ms / 1000);
  if (!(rate > 0)) return measured;
  const weight = Math.min(MAX_WEIGHT, bytes / FULL_WEIGHT_BYTES);
  return rate + weight * (measured - rate);
}

/** Learns from a finished scan. Returns the activity log's line when the
 *  rate changed (the caller saves the settings), else null. */
export function learnFromScan(outcome) {
  const before = readRate;
  readRate = learnRate(readRate, outcome);
  if (readRate === before) return null;
  const mbps = (bytes) => (bytes / 1e6).toFixed(0);
  const measured = outcome.full_read_bytes / (outcome.not_cached_ms / 1000);
  return STRINGS.MAIN.scanReadRateLog(mbps(measured), mbps(readRate));
}

/** Seconds the rescan takes, from `estimate_rescan`'s answer. */
export function estimateSeconds({
  cached = 0, upgradable = 0, to_parse: toParse = 0, bytes_to_parse: bytesToParse = 0,
} = {}, rate = 0) {
  const quick = ((cached + upgradable) * MS_PER_CACHED_DEMO) / 1000;
  if (!(toParse > 0)) return quick;
  const full = rate > 0 && bytesToParse > 0 ? bytesToParse / rate : toParse * DEFAULT_SECS_PER_FULL_READ;
  return quick + full;
}

/** "3 s" under a minute (1 s at least), "4 min" under an hour, then "an
 *  hour" up to 90 min and "2 h" after. */
export function formatDuration(seconds) {
  const s = Math.max(1, Math.ceil(Number(seconds) || 0));
  if (s < 60) return `${s} s`;
  const min = Math.round(s / 60);
  if (min < 60) return `${min} min`;
  if (min < 90) return 'an hour';
  return `${Math.round(min / 60)} h`;
}

/** The prompt's line: "About 3 s." or "About 4 min: 120 need a full read." */
export function rescanTimeLine(estimate, rate = 0) {
  const duration = formatDuration(estimateSeconds(estimate, rate));
  return STRINGS.MAIN.rescanTimeLine(duration, Number(estimate?.to_parse) || 0);
}

/**
 * @param {object} [deps]  estimate(paths), the backend's estimate_rescan;
 *   log(line), the activity log.
 */
export function createRescanEstimate({ estimate = estimateRescan, log = logFrontendEvent } = {}) {
  /** The prompt's time line for `paths`, or null when it can't be worked out
   *  (logged): the prompt then says nothing about time. */
  async function timeLine(paths) {
    try {
      return rescanTimeLine(await estimate(paths), readRate);
    } catch (err) {
      log(STRINGS.MAIN.rescanEstimateFailedLog(err));
      return null;
    }
  }
  return { timeLine };
}
