// Progress from the analyzer, cut down to what the page can show: the parser
// reports a few hundred to several thousand times per demo, and each report
// crosses from the worker to the page as a message. No DOM, so it is unit
// tested (studio/src/web_analyzer_worker.test.js).

/** At most one progress message per frame at ~30 fps (CLAUDE.md). */
export const PROGRESS_INTERVAL_MS = 33;

/**
 * Returns `(processed, total) => void` that calls `send(pct)` with a whole
 * percentage, only when it changed and at most once per `intervalMs`.
 * 100 always goes through.
 */
export function createProgressThrottle(send, { intervalMs = PROGRESS_INTERVAL_MS, now = () => performance.now() } = {}) {
  let lastAt = -Infinity;
  let lastPct = -1;
  return (processed, total) => {
    if (!total) return;
    const pct = Math.min(100, Math.round((processed / total) * 100));
    if (pct === lastPct) return;
    const t = now();
    if (pct < 100 && t - lastAt < intervalMs) return;
    lastAt = t;
    lastPct = pct;
    send(pct);
  };
}
