// hook_dll_row.js
// Configuration > Paths > GoldSrc Hooks DLL is for testing a hook build
// instead of the one DoD Studio ships. Only a debug build or one made on this
// PC uses it (updater_manager.rs `hook_dll_override_allowed`); an installed
// release always runs its bundled DLL, whatever is saved, so the row is
// hidden there.
import { hookDllOverrideAllowed } from './ipc_bridge.js';

/** Shows the row only where the override is used. */
export async function refreshHookDllRow() {
  const row = document.querySelector('#goldsrc-hooks-dll-row');
  if (!row) return;
  let allowed = false;
  try {
    allowed = await hookDllOverrideAllowed();
  } catch {
    // Logged by the bridge. Unknown counts as a release build.
  }
  row.style.display = allowed ? '' : 'none';
  if (!allowed) {
    const warning = document.querySelector('#goldsrc-hooks-path-warning');
    if (warning) warning.style.display = 'none';
  }
}
