// demo_cache.js — the Demo Analyzer's "Cache all" button (#569): analyses
// every demo of the current folder into the analyzer cache in the background
// (`cache_demos`), so opening one later, the in-game Highlights tab and the
// player filters are instant. Progress arrives as `demo_cache_progress`
// events; while a run goes, the button is Stop. The demos already cached go
// first, then the parses, with a time left from their pace (#687).
import { listen } from '@tauri-apps/api/event';
import { cacheDemos, cancelDemoCache } from './ipc_bridge.js';
import { parseClock, timeLeftText } from './scan_progress.js';
import { STRINGS } from './strings.js';

/**
 * @param {{ getDemoPaths: () => string[] }} deps  the current folder's demos
 */
export function initDemoCache({ getDemoPaths }) {
  const button = document.querySelector('#analyzer-cache-all-btn');
  const status = document.querySelector('#analyzer-cache-status');
  if (!button || !status) return;
  let running = false;
  const parses = parseClock();

  const setRunning = (on) => {
    running = on;
    button.textContent = on ? STRINGS.ANALYZER.CACHE_STOP_BUTTON : STRINGS.ANALYZER.CACHE_ALL_BUTTON;
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
    parses.reset();
    // Until the first event the backend is checking which demos the cache has.
    status.textContent = STRINGS.SCAN_PROGRESS.CHECKING_CACHE;
    try {
      await cacheDemos(paths);
    } catch (err) {
      setRunning(false);
      status.textContent = String(err);
    }
  });

  listen('demo_cache_progress', ({ payload }) => {
    if (payload.finished) {
      setRunning(false);
      status.textContent = STRINGS.ANALYZER.cacheDone(payload);
    } else {
      setRunning(true);
      status.textContent = STRINGS.ANALYZER.cacheProgress(payload)
        + timeLeftText(payload, parses.elapsed(payload.done, payload.cached));
    }
  });
}
