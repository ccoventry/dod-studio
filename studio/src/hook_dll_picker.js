// hook_dll_picker.js
// Configuration > Paths > GoldSrc Hooks DLL as a dropdown of the test builds
// in the checkout's local\ folder (hook_dll_list.rs), so trying another
// build is one pick instead of editing a path. The path itself stays in
// #goldsrc-hooks-dll-path-input (hidden), which every launch reads and
// settings_sync.js saves; this only sets it. Browse still picks any file.
// Only in builds that use the override (hook_dll_row.js).

import { listHookDlls } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** Same file, whatever the slashes or case. */
function samePath(a, b) {
  const norm = (p) => String(p || '').replace(/\//g, '\\').toLowerCase();
  return norm(a) === norm(b);
}

/** `Oct 22 17:54`, or '' without a time. */
export function shortDate(unixSecs) {
  if (!Number.isFinite(unixSecs)) return '';
  const d = new Date(unixSecs * 1000);
  const month = d.toLocaleString('en-US', { month: 'short' });
  const time = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  return `${month} ${d.getDate()} ${time}`;
}

/**
 * The dropdown's rows for `list` (from listHookDlls) with `current` the
 * saved path: the bundled DLL, this checkout's own build, each test DLL by
 * its label, and the saved path when it is none of those.
 * @returns {{ value: string, text: string, title: string }[]}
 */
export function pickerOptions(list, current) {
  const S = STRINGS.CAPTURE_CONFIG;
  const options = [{ value: '', text: S.HOOK_DLL_BUNDLED, title: S.HOOK_DLL_BUNDLED_TITLE }];
  if (list?.checkout_build) {
    options.push({ value: list.checkout_build, text: S.HOOK_DLL_CHECKOUT, title: list.checkout_build });
  }
  for (const dll of list?.dlls || []) {
    const date = shortDate(dll.modified_unix_secs);
    options.push({ value: dll.path, text: date ? `${dll.label} · ${date}` : dll.label, title: dll.path });
  }
  if (current && !options.some((o) => samePath(o.value, current))) {
    const name = String(current).split(/[\\/]/).slice(-2).join('\\');
    options.push({ value: current, text: S.hookDllOther(name), title: current });
  }
  return options;
}

/** The option `current` picks: its own row, or the bundled one when blank. */
export function selectedValue(options, current) {
  return options.find((o) => samePath(o.value, current))?.value ?? '';
}

/** Fills the dropdown from the folder again and selects the saved path. */
export async function refreshHookDllPicker() {
  const select = document.querySelector('#goldsrc-hooks-dll-select');
  const input = document.querySelector('#goldsrc-hooks-dll-path-input');
  if (!select || !input) return;
  const refresh = document.querySelector('#goldsrc-hooks-refresh-btn');
  if (refresh) refresh.disabled = true;
  try {
    const list = await listHookDlls();
    const options = pickerOptions(list, input.value.trim());
    select.replaceChildren(...options.map((o) => {
      const el = document.createElement('option');
      el.value = o.value;
      el.textContent = o.text;
      el.title = o.title;
      return el;
    }));
    select.value = selectedValue(options, input.value.trim());
    select.title = list?.folder ? STRINGS.CAPTURE_CONFIG.hookDllFolderHint(list.folder) : '';
  } finally {
    if (refresh) refresh.disabled = false;
  }
}

/** Wires the dropdown and its Refresh button. A pick goes through the path
 *  field's own change handler (config_wiring.js), which checks the path and
 *  saves the settings. */
export function initHookDllPicker() {
  document.querySelector('#goldsrc-hooks-dll-select')?.addEventListener('change', (e) => {
    const input = document.querySelector('#goldsrc-hooks-dll-path-input');
    if (!input) return;
    input.value = e.target.value;
    input.dispatchEvent(new Event('change'));
  });
  document.querySelector('#goldsrc-hooks-refresh-btn')
    ?.addEventListener('click', () => { refreshHookDllPicker(); });
}
