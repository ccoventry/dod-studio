// The page's side of analyzer_worker.js. No DOM: the worker comes from
// `makeWorker`, so it is unit tested with a fake (studio/src/web_analyzer_worker.test.js).

/** What a cancelled analysis rejects with. */
export const CANCELLED = 'cancelled';

/**
 * `analyze(file, onPct)` resolves with the analysis or rejects with the
 * error text. Send one demo at a time (main.js waits for each): the worker
 * reads files asynchronously, so overlapping calls may finish out of order.
 * `cancel()` stops the demo
 * being parsed (a sync wasm call can't be interrupted, so the worker is
 * replaced) and rejects everything pending with CANCELLED.
 */
export function createAnalyzerClient(makeWorker) {
  let worker = null;
  let nextId = 0;
  const pending = new Map();

  function drop() {
    if (worker) worker.terminate();
    worker = null;
  }

  function failAll(error) {
    const all = [...pending.values()];
    pending.clear();
    drop();
    all.forEach((p) => p.reject(error));
  }

  function ensureWorker() {
    if (worker) return worker;
    const w = makeWorker();
    worker = w;
    // Events still queued from a worker already replaced are ignored.
    w.onmessage = ({ data }) => {
      if (worker !== w) return;
      const p = pending.get(data.id);
      if (!p) return;
      if (data.type === 'progress') {
        p.onPct?.(data.pct);
        return;
      }
      pending.delete(data.id);
      if (data.type === 'done') {
        p.resolve(data.analysis);
        return;
      }
      p.reject(data.error);
      // A wasm trap (out of memory, a panic) or a failed load can leave the
      // instance broken, so the next demo gets a fresh one: failAll drops
      // this worker, and with it anything already queued behind this demo.
      // A parse error is an ordinary result and keeps the instance.
      if (data.fatal) failAll(data.error);
    };
    // The worker script or the wasm failed to load, or the worker died.
    w.onerror = (e) => {
      e?.preventDefault?.();
      if (worker !== w) return;
      failAll(e?.message || 'The analyzer worker stopped');
    };
    return w;
  }

  return {
    analyze(file, onPct) {
      return new Promise((resolve, reject) => {
        const id = ++nextId;
        pending.set(id, { resolve, reject, onPct });
        ensureWorker().postMessage({ id, file });
      });
    },
    cancel() {
      if (pending.size > 0) failAll(CANCELLED);
    },
  };
}
