// steam_wait.js
// The polling half of steam_guard.js, kept free of Tauri and the DOM so it
// can be unit-tested.

/**
 * Asks `getState` every `pollMs` until it says "ready", `isCancelled()` turns
 * true, or `timeoutMs` passes. Resolves "ready", "cancelled" or "timeout".
 */
export async function waitForSteam(getState, { timeoutMs, pollMs = 1000, isCancelled = () => false, sleep = defaultSleep }) {
  for (let waited = 0; waited < timeoutMs; waited += pollMs) {
    await sleep(pollMs);
    if (isCancelled()) return 'cancelled';
    if ((await getState()) === 'ready') return 'ready';
  }
  return 'timeout';
}

function defaultSleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
