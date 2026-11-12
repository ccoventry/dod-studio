// cached_check.js
// A question asked of the backend about one demo, whose answer is kept for a
// short while: the queue redraws its rows on every search, filter, sort and
// scan, and each redraw asks again. Two checks of one demo at once share one
// call, and a failed call answers null and is not remembered, so the caller
// keeps what it shows by default. Used by split_status.js (already split?)
// and repair_status.js (anything for Repair to cut out?).

/**
 * @param {object} deps
 *   ask(key, ...args) resolves the backend's answer for `key` (a demo path);
 *   keepMs how long an answer is reused before asking again;
 *   now() the clock in ms (tests pass their own).
 * @returns {{ last(key): any, check(key, ...args): Promise<any>, remember(key, answer): void }}
 *   `last` is the answer already known for `key`, or null; `check` resolves
 *   a fresh one (or the last, while it is under `keepMs` old), and null when
 *   the backend could not tell; `remember` stores an answer learned another
 *   way (a repair's own verdict), as if a check had just returned it.
 */
export function createCachedCheck({ ask, keepMs, now = () => Date.now() }) {
  const known = new Map(); // key -> { answer, at }
  const pending = new Map(); // key -> Promise

  function last(key) {
    return known.get(key)?.answer ?? null;
  }

  function remember(key, answer) {
    known.set(key, { answer, at: now() });
  }

  function check(key, ...args) {
    if (pending.has(key)) return pending.get(key);
    const entry = known.get(key);
    if (entry && now() - entry.at < keepMs) return Promise.resolve(entry.answer);
    const call = Promise.resolve()
      .then(() => ask(key, ...args))
      .then((answer) => {
        remember(key, answer);
        return answer;
      })
      // A demo that can't be read now keeps its default; nothing to remember.
      .catch(() => null)
      .finally(() => pending.delete(key));
    pending.set(key, call);
    return call;
  }

  return { last, check, remember };
}
