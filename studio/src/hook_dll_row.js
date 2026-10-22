// hook_dll_row.js
// Configuration > Paths > GoldSrc Hooks DLL is for testing a hook build
// instead of the one DoD Studio ships, so it shows in local and debug builds.
// A release build shows it only while a path is set, so an override saved
// earlier can still be seen and cleared rather than applying out of sight.
import { isLocalOrDebugBuild } from './updater_pane.js';

/** Whether the row shows: always for a local or debug build, else only with
 *  a path in the field. */
export function hookDllRowShown(isLocalBuild, path) {
  return !!isLocalBuild || !!(path || '').trim();
}

/** Shows or hides the row from the field's current value. */
export async function refreshHookDllRow() {
  const row = document.querySelector('#goldsrc-hooks-dll-row');
  if (!row) return;
  const path = document.querySelector('#goldsrc-hooks-dll-path-input')?.value || '';
  let local = false;
  try {
    local = await isLocalOrDebugBuild();
  } catch {
    // Unknown counts as a release build: the row still shows with a path set.
  }
  const shown = hookDllRowShown(local, path);
  row.style.display = shown ? '' : 'none';
  if (!shown) {
    const warning = document.querySelector('#goldsrc-hooks-path-warning');
    if (warning) warning.style.display = 'none';
  }
}
