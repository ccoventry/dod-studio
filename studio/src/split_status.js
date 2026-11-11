// split_status.js
// Whether a demo that recorded more than one map was already split next to
// it (native::demo_split_status), for the queue row's hint and the Demo
// Analyzer's notice: once every map a split would write has its file there,
// they stop suggesting a split.
//
// Asked when shown, never stored with the scan: a split can happen after the
// scan and its pieces can be deleted again, so an answer is kept only a few
// seconds, enough to spare the backend while the queue redraws.

import { demoSplitStatus } from './ipc_bridge.js';
import { SHORT_MAP_SECONDS } from './split_pane.js';

/** How long an answer is reused before the folder is looked at again. */
export const RECHECK_AFTER_MS = 3000;

/**
 * @param {object} deps
 *   ask(path, maps) resolves native's `{ split, files }`;
 *   now() the clock in ms (tests pass their own).
 * @returns {{ last(path): object|null, check(path, maps): Promise<object|null> }}
 *   `last` is the answer already known for `path`, or null; `check` resolves
 *   a fresh one (or the last, while it is under RECHECK_AFTER_MS old), and
 *   null when the backend could not tell: the caller then keeps suggesting
 *   a split, which is harmless. Two checks of one demo at once share a call.
 */
export function createSplitStatusCheck({ ask, now = () => Date.now() }) {
  const known = new Map(); // path -> { status, at }
  const pending = new Map(); // path -> Promise

  function last(path) {
    return known.get(path)?.status ?? null;
  }

  function check(path, maps) {
    if (pending.has(path)) return pending.get(path);
    const entry = known.get(path);
    if (entry && now() - entry.at < RECHECK_AFTER_MS) return Promise.resolve(entry.status);
    const call = Promise.resolve()
      .then(() => ask(path, maps))
      .then((status) => {
        known.set(path, { status, at: now() });
        return status;
      })
      // A demo that can't be read now keeps its hint; nothing to remember.
      .catch(() => null)
      .finally(() => pending.delete(path));
    pending.set(path, call);
    return call;
  }

  return { last, check };
}

/** Studio's one checker, over IPC, with the auto split's shortest map. */
export const splitStatus = createSplitStatusCheck({
  ask: (path, maps) => demoSplitStatus(path, maps, SHORT_MAP_SECONDS),
});

/** Whether `status` (native's answer, or null when unknown) says split. */
export function isAlreadySplit(status) {
  return status?.split === true;
}
