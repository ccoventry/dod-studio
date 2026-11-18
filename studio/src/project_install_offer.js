// project_install_offer.js
// "This project was made with POST. Switch?" (#1085, build plan step 4): a
// project remembers the install it was made with (`settings.install.madeWith`),
// and opening it while another install is picked asks, never switching by
// itself. Switch picks that install as Configuration's dropdown would; Keep
// makes the picked one the project's, so the next open doesn't ask again.

import { findInstalls, logFrontendEvent } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { installText, switchInstall } from './install_picker.js';
import { installToOffer, installFor, withMadeWith } from './project_settings_logic.js';
import { projectInstallOfferLine } from './project_log.js';
import { fileNameOf, folderOf } from './path_display.js';
import { STRINGS } from './strings.js';

/** The install picked in Configuration now. */
const pickedHlExe = () => document.querySelector('#hl-path-input')?.value?.trim() || '';

/**
 * Asks to switch to the install the project at `path` was made with, when
 * another is picked and this PC has that one. `settings` is the session's
 * createProjectSettings; `markDirty()` marks the project unsaved when Keep
 * changes its install. Resolves 'switched', 'kept' or null (nothing asked,
 * or the switch was called off at the running-game prompt).
 */
export async function offerProjectInstall({ path, settings, markDirty }) {
  const madeWith = settings.block()?.install?.madeWith;
  if (!madeWith) return null;
  const current = pickedHlExe();
  const installs = await findInstalls(current);
  const install = installToOffer(madeWith, current, installs);
  if (!install) return null;
  const S = STRINGS.PROJECT_SETTINGS;
  const currentRow = installFor(installs, current);
  const currentName = currentRow ? installText(currentRow) : (fileNameOf(folderOf(current)) || S.NO_INSTALL);
  const wanted = installText(install);
  const yes = await themedConfirm(S.installOffer(wanted, currentName), {
    title: S.INSTALL_OFFER_TITLE,
    confirmLabel: S.SWITCH,
    cancelLabel: S.KEEP,
    focusConfirm: true,
  });
  if (yes) {
    const switched = await switchInstall(install.hl_exe);
    logFrontendEvent(projectInstallOfferLine(path, wanted, switched ? 'switched' : 'switch called off'));
    return switched ? 'switched' : null;
  }
  // With no install picked there is nothing to keep: the next open asks again.
  if (!current) return null;
  settings.setBlock(withMadeWith(settings.block(), current));
  markDirty();
  logFrontendEvent(projectInstallOfferLine(path, wanted, `kept ${currentName}`));
  return 'kept';
}
