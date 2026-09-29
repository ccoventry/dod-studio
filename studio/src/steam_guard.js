// steam_guard.js
// Checked before DoD Studio starts the game (capture batch, preview, the
// debug Launch Game). hl.exe started without Steam exits straight away with
// its own "Failed to initalize authentication interface" box, which doesn't
// say Steam. Whether the account owns the game can't be checked ahead;
// that failure is named in the batch's own error instead.

import { steamState, startSteam } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { waitForSteam } from './steam_wait.js';

const SIGN_IN_TIMEOUT_MS = 120000;

let waiting = false;

/**
 * True when the launch can go ahead: Steam was already signed in, or became
 * signed in while we waited. Offers to start Steam when it isn't running.
 */
export async function ensureSteamReady() {
  if (waiting) {
    showToast(STRINGS.STEAM.STILL_WAITING, 'info');
    return false;
  }
  const state = await steamState();
  if (state === 'ready') return true;

  if (state === 'not_running') {
    const ok = await themedConfirm(STRINGS.STEAM.NOT_RUNNING_MESSAGE, {
      title: STRINGS.STEAM.NOT_RUNNING_TITLE,
      confirmLabel: STRINGS.STEAM.START_STEAM,
      cancelLabel: STRINGS.STEAM.CANCEL,
    });
    if (!ok) return false;
    try {
      await startSteam();
    } catch (err) {
      return false; // already toasted by ipc_bridge.js
    }
  }

  // Not signed in yet (just started, or sitting at its sign-in window):
  // carry on by ourselves once it is, unless cancelled.
  waiting = true;
  let cancelled = false;
  const toast = showToast(STRINGS.STEAM.WAITING_FOR_SIGN_IN, 'info', SIGN_IN_TIMEOUT_MS, {
    action: { label: STRINGS.STEAM.CANCEL, onClick: () => { cancelled = true; } },
  });
  try {
    const result = await waitForSteam(steamState, {
      timeoutMs: SIGN_IN_TIMEOUT_MS,
      isCancelled: () => cancelled,
    });
    if (result === 'timeout') showToast(STRINGS.STEAM.NOT_SIGNED_IN, 'error', 10000);
    return result === 'ready';
  } finally {
    waiting = false;
    toast?.remove();
  }
}
