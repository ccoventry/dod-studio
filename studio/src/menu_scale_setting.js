// menu_scale_setting.js
// Configuration › Paths: "Size the Game's Menus Like the Pre-Anniversary
// Engine" (#985). On a 25th Anniversary install, ticking it puts DoD Studio's
// copy of the menu scheme in <game folder>_addon\resource (menu_scale.rs);
// unticking removes it. Its state is the install's, read back whenever the
// Half-Life Executable or Game Folder changes, not a saved setting, so
// nothing can get out of step with what's on disk. Hidden for a
// pre-Anniversary install, whose engine already draws the menus this size.

import { menuScaleStatus, setMenuScale } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const S = () => STRINGS.CAPTURE_CONFIG;

/** What the row shows for a status from menu_scale_status (null: no install
 *  or unreadable). Shown on an Anniversary install, and wherever our copy
 *  is still on so it can be taken back out. */
export function menuScaleView(status) {
  if (!status || !(status.applies || status.on)) {
    return { visible: false, checked: false, disabled: true, title: '' };
  }
  if (status.foreign) {
    return { visible: true, checked: false, disabled: true, title: S().menuScaleForeign(status.path) };
  }
  return { visible: true, checked: !!status.on, disabled: false, title: S().MENU_SCALE_TITLE };
}

/**
 * @param {object} deps
 * @param {() => string} deps.getHlPath  the Half-Life Executable field
 * @param {() => string} deps.getGameFolder  the Game Folder setting
 * @returns {{ refresh: () => Promise<void> }}
 */
export function createMenuScaleSetting({ getHlPath, getGameFolder }) {
  const row = document.querySelector('#config-menu-scale-row');
  const box = document.querySelector('#config-menu-scale');
  const label = document.querySelector('label[for="config-menu-scale"]');
  // Only the newest refresh draws: a path change can start one while the
  // last is still reading.
  let generation = 0;

  function show(status) {
    const view = menuScaleView(status);
    if (row) row.style.display = view.visible ? '' : 'none';
    if (box) {
      box.checked = view.checked;
      box.disabled = view.disabled;
      box.title = view.title;
    }
    if (label) label.title = view.title;
  }

  async function refresh() {
    const mine = ++generation;
    const hlPath = getHlPath();
    const status = hlPath ? await menuScaleStatus(hlPath, getGameFolder()) : null;
    if (mine === generation) show(status);
  }

  box?.addEventListener('change', async () => {
    const wanted = box.checked;
    const mine = ++generation;
    box.disabled = true;
    try {
      const status = await setMenuScale(getHlPath(), getGameFolder(), wanted);
      if (mine === generation) show(status);
    } catch (err) {
      showToast(S().menuScaleFailed(err), 'error');
      await refresh();
    }
  });

  return { refresh };
}
