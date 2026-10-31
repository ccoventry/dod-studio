// vegas_script_install.js
// The "Add Nameplate Script to VEGAS" button on the Render tab (#429): copies
// the bundled VEGAS script into VEGAS's script menu folder. The copy, the
// activity-log lines and the folder lookup are the backend's
// (`vegas_manager.rs`).

import { vegasInstallScript } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** Wires the button. `install` is injectable for tests. A failure is already
 *  toasted by the IPC wrapper. */
export function startVegasScriptInstall({ install = vegasInstallScript } = {}) {
  const button = document.querySelector('#vegas-script-install-btn');
  button?.addEventListener('click', async () => {
    button.disabled = true;
    try {
      const path = await install();
      showToast(STRINGS.RENDER.vegasScriptDone(path), 'success');
    } catch {
      // Toasted and logged where it was caught (ipc_bridge.js).
    } finally {
      button.disabled = false;
    }
  });
}
