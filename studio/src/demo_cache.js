// demo_cache.js — the Demo Analyzer's "Cache all" button (#569): analyses
// every demo of the current folder into the analyzer cache in the background
// (`cache_demos`), so opening one later, the in-game Highlights tab and the
// player filters are instant. Progress arrives as `demo_cache_progress`
// events; while a run goes, the button is Stop. The demos already cached go
// first, then the parses, with a time left from their pace (#687). Before
// either, analyses an older cache format left are upgraded, a step of its own
// with its own count.
import { listen } from '@tauri-apps/api/event';
import { cacheDemos, cancelDemoCache } from './ipc_bridge.js';
import { cacheRunView, parseClock } from './scan_progress.js';
import { STRINGS } from './strings.js';

/**
 * @param {{ getDemoPaths: () => string[] }} deps  the current folder's demos
 * @returns {{ isCaching: (path: string) => boolean }}  whether a run has
 *   `path` queued, so the player read leaves it to the run (#992)
 */
export function initDemoCache({ getDemoPaths }) {
  const button = document.querySelector('#analyzer-cache-all-btn');
  const status = document.querySelector('#analyzer-cache-status');
  const bar = document.querySelector('#analyzer-cache-bar');
  // The running run's demos, cleared when it ends.
  let queued = new Set();
  const api = { isCaching: (path) => queued.has(path) };
  if (!button || !status) return api;
  let running = false;
  const parses = parseClock();

  const setRunning = (on) => {
    running = on;
    button.textContent = on ? STRINGS.ANALYZER.CACHE_STOP_BUTTON : STRINGS.ANALYZER.CACHE_ALL_BUTTON;
    if (bar) bar.hidden = !on;
  };
  // No value: a bar that moves without a count, while the backend checks
  // which demos the cache has.
  const showBar = (value, max) => {
    if (!bar) return;
    if (value === undefined) {
      bar.removeAttribute('value');
      return;
    }
    bar.max = max;
    bar.value = value;
  };

  button.addEventListener('click', async () => {
    if (running) {
      await cancelDemoCache();
      status.textContent = STRINGS.ANALYZER.CACHE_STOPPING;
      return;
    }
    const paths = getDemoPaths();
    if (paths.length === 0) {
      status.textContent = STRINGS.ANALYZER.CACHE_NOTHING;
      return;
    }
    setRunning(true);
    queued = new Set(paths);
    showBar();
    parses.reset();
    // Until the first event the backend is checking which demos the cache has.
    status.textContent = STRINGS.SCAN_PROGRESS.CHECKING_CACHE;
    try {
      await cacheDemos(paths);
    } catch (err) {
      setRunning(false);
      queued = new Set();
      status.textContent = String(err);
    }
  });

  listen('demo_cache_progress', ({ payload }) => {
    if (payload.finished) {
      setRunning(false);
      queued = new Set();
      status.textContent = STRINGS.ANALYZER.cacheDone(payload);
    } else {
      setRunning(true);
      const view = cacheRunView(payload, parses);
      status.textContent = view.text;
      showBar(view.value, view.max);
    }
  });
}
