// close_guard.js
// What stands between the user and losing work when the window closes or the
// page reloads: a running batch (#545), an unsaved project, and a settings
// edit still in a focused field. Kept out of main.js, which only wires (#683).

import { getCurrentWindow } from '@tauri-apps/api/window';
import { confirmCloseDuringBatch } from './batch_close_prompt.js';
import { isCaptureRunning } from './capture_pane.js';
import { isLocalOrDebugBuild } from './updater_pane.js';

/**
 * @param {object} app  needsSavePrompt() and requestUnsavedChangesConfirmation()
 *   from project_session.js, persistAppSettings() saves the settings file.
 */
export function startCloseGuard({ needsSavePrompt, requestUnsavedChangesConfirmation, persistAppSettings }) {
  // Settings are flushed before the window closes: list_editor.js (Init/Custom
  // Commands, numeric fields) only writes on 'change' (blur/Enter), so closing
  // while a field still has focus would otherwise drop an edit that's already
  // in the in-memory state persistAppSettings() reads (a reproducible
  // data-loss case).
  const appWindow = getCurrentWindow();
  appWindow.onCloseRequested(async (event) => {
    event.preventDefault();
    // A running batch first: closing leaves it unwatched (#545).
    if (!(await confirmCloseDuringBatch({ isRunning: isCaptureRunning, isLocalBuild: isLocalOrDebugBuild }))) return;
    // Project state changed since the last save: offer save, discard or cancel.
    if (needsSavePrompt()) {
      const outcome = await requestUnsavedChangesConfirmation();
      if (!outcome) return; // Cancel — leave the window open
      // 'save' already wrote the file inside the modal's Save button
      // handler; 'discard' falls through to close as-is either way.
    }
    await persistAppSettings();
    await appWindow.destroy();
  });

  // A page reload (F5, Ctrl+R, any in-place navigation) is a WebView2
  // navigation, not a window close, so onCloseRequested never sees it.
  // beforeunload's dialog is browser-native and can't be styled or given
  // custom text (a web platform rule against sites faking dialogs); setting
  // returnValue is what shows it.
  window.addEventListener('beforeunload', (event) => {
    if (needsSavePrompt()) {
      event.preventDefault();
      event.returnValue = '';
    }
  });
}
