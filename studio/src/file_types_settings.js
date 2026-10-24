// file_types_settings.js
// Settings → Pipeline: .dem files in Explorer (#528).
// - "Show DoD Studio in Open With for .dem Files" adds or removes DoD Studio
//   in Open With, per user (dem_open_with.rs). Its state is the registry's,
//   read back on every start, not a saved setting: nothing else can get out
//   of step with it.
// - "Open .dem Files In" is saved in the settings file (`dem_open_target`)
//   and read by launch_files.js when a demo arrives.

import { demOpenWithStatus, setDemOpenWith } from './ipc_bridge.js';
import { normalizeDemTarget } from './launch_files.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const S = () => STRINGS.CAPTURE_CONFIG;

/** The note under the setting: empty unless Open With opens another copy
 *  of DoD Studio (a dev build, an old install folder). */
export function demOpenWithNote(status) {
  if (!status?.registered || status.thisCopy || !status.otherExe) return '';
  return S().demOpenWithOtherCopy(status.otherExe);
}

export function getDemOpenTarget() {
  return normalizeDemTarget(document.querySelector('#config-dem-open-target')?.value);
}

export function applyDemOpenTarget(raw) {
  const el = document.querySelector('#config-dem-open-target');
  if (el) el.value = normalizeDemTarget(raw);
}

/** @param {object} app  onSettingsChange() saves the settings file. */
export function initFileTypeSettings({ onSettingsChange }) {
  const box = document.querySelector('#config-dem-open-with');
  const note = document.querySelector('#config-dem-open-with-note');

  function show(status) {
    if (box) box.checked = !!status?.registered;
    const text = demOpenWithNote(status);
    if (note) {
      note.textContent = text;
      note.style.display = text ? '' : 'none';
    }
  }

  document.querySelector('#config-dem-open-target')
    ?.addEventListener('change', () => onSettingsChange());

  if (!box) return;
  box.disabled = true;
  demOpenWithStatus().then((status) => {
    show(status);
    // Null: the registry couldn't be read, so the box can't say anything true.
    box.disabled = status === null;
  });

  box.addEventListener('change', async () => {
    const wanted = box.checked;
    box.disabled = true;
    try {
      show(await setDemOpenWith(wanted));
    } catch (err) {
      box.checked = !wanted;
      showToast(S().demOpenWithFailed(err), 'error');
    } finally {
      box.disabled = false;
    }
  });
}
