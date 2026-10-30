// running_game_guard.js
// Before DoD Studio reuses or starts the game (#666): a running game started
// with other launch settings (another install, game folder, resolution or
// hook DLL, #1143) would be reused
// as it is, because the game only reads them when it starts. This asks to
// close it and start again, naming what differs. Called by Launch Preview,
// Launch Game, Start Capture Batch and Review highlights before their own
// "already running" handling, which then sees no game.

import { checkRunningGame, closeRunningGame } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** The confirm's list: both hl.exe paths when the install differs, and
 *  both hook DLLs (path and build) when the hook DLL does (#1143). */
export function runningGameDetails(check) {
  const S = STRINGS.RUNNING_GAME;
  const rows = [];
  if (check.differs.includes('install')) {
    rows.push(
      { primary: S.RUNNING_EXE, secondary: check.running.exe },
      { primary: S.STUDIO_EXE, secondary: check.wanted.exe },
    );
  }
  if (check.differs.includes('hook_dll') || check.differs.includes('hook_build')) {
    rows.push(
      { primary: S.RUNNING_HOOK, secondary: S.hookLine(check.running) },
      { primary: S.STUDIO_HOOK, secondary: S.hookLine(check.wanted) },
    );
  }
  if (check.differs.includes('hook_unreadable') && check.wanted.hook_dll) {
    rows.push({ primary: S.STUDIO_HOOK, secondary: S.hookLine(check.wanted) });
  }
  return rows;
}

/**
 * True when the caller can go on: no game is running, it was started with
 * these settings, the check itself failed (never block a launch on it), or
 * the user chose to close it and it has closed. False when the user kept
 * the game, or it didn't close; nothing is reused or started then.
 *
 * `request`: launch settings not saved yet (see checkRunningGame).
 * `button`: disabled while the game closes.
 * `switching`: asked by Configuration's install list (#931), which closes
 * the game to switch installs rather than to start it again.
 */
export async function closeGameWithOtherSettings(request = null, { button = null, switching = false } = {}) {
  const check = await checkRunningGame(request);
  if (!check || check.state !== 'mismatch') return true;
  const S = STRINGS.RUNNING_GAME;
  const ok = await themedConfirm(switching ? S.switchMessage(check) : S.message(check), {
    title: switching ? S.SWITCH_TITLE : S.TITLE,
    confirmLabel: switching ? S.CLOSE_AND_SWITCH : S.CLOSE_AND_RESTART,
    cancelLabel: S.CANCEL,
    details: runningGameDetails(check),
  });
  if (!ok) return false;

  const wasDisabled = button?.disabled;
  if (button) button.disabled = true;
  const toast = showToast(S.CLOSING, 'info', 15000);
  try {
    await closeRunningGame(check.pid);
    return true;
  } catch {
    return false; // already toasted by ipc_bridge.js
  } finally {
    toast?.remove?.();
    if (button) button.disabled = wasDisabled;
  }
}
