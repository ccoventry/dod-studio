// hook_dll_picker.js
// Configuration > Paths > GoldSrc Hooks DLL as a list of the test builds in
// the checkout's local\ folder (hook_dll_list.rs), so trying another build
// is one pick instead of editing a path: a box with a datalist, so typing
// `91` narrows it to the matching builds. The path itself stays in
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

/**
 * The option text typed into the box picks: the row with exactly that text,
 * else the one whose label is it (`910` for `910 · Oct 22 17:54`), else the
 * only row containing it. `null` when none or several match.
 */
export function resolveTyped(options, typed) {
  const want = String(typed || '').trim().toLowerCase();
  if (!want) return null;
  const exact = options.find((o) => o.text.toLowerCase() === want);
  if (exact) return exact;
  const byLabel = options.filter((o) => o.text.split(' · ')[0].toLowerCase() === want);
  if (byLabel.length === 1) return byLabel[0];
  const containing = options.filter((o) => o.text.toLowerCase().includes(want));
  return containing.length === 1 ? containing[0] : null;
}

/** The rows the box offers, kept for resolving what is typed. */
let shown = [];

const choiceBox = () => document.querySelector('#goldsrc-hooks-dll-choice');
const pathInput = () => document.querySelector('#goldsrc-hooks-dll-path-input');

/** The text the box shows for the saved path. */
function savedText() {
  const value = selectedValue(shown, pathInput()?.value.trim() || '');
  return shown.find((o) => o.value === value)?.text ?? '';
}

/** Fills the list from the folder again and shows the saved path's row. */
export async function refreshHookDllPicker() {
  const box = choiceBox();
  const list = document.querySelector('#goldsrc-hooks-dll-options');
  const input = pathInput();
  if (!box || !list || !input) return;
  const refresh = document.querySelector('#goldsrc-hooks-refresh-btn');
  if (refresh) refresh.disabled = true;
  try {
    const found = await listHookDlls();
    shown = pickerOptions(found, input.value.trim());
    list.replaceChildren(...shown.map((o) => {
      const el = document.createElement('option');
      el.value = o.text;
      return el;
    }));
    if (document.activeElement !== box) box.value = savedText();
    box.title = found?.folder ? STRINGS.CAPTURE_CONFIG.hookDllFolderHint(found.folder) : '';
  } finally {
    if (refresh) refresh.disabled = false;
  }
}

/** Wires the box and its Refresh button. Clicking the box empties it so the
 *  whole list shows; typing narrows it. A pick goes through the path field's
 *  own change handler (config_wiring.js), which checks the path and saves the
 *  settings; text that matches no single row puts the saved pick back. */
export function initHookDllPicker() {
  const box = choiceBox();
  if (box) {
    box.addEventListener('focus', () => {
      box.placeholder = box.value;
      box.value = '';
    });
    box.addEventListener('change', () => {
      const input = pathInput();
      const picked = resolveTyped(shown, box.value);
      if (!input || !picked) return;
      box.value = picked.text;
      if (input.value.trim() !== picked.value) {
        input.value = picked.value;
        input.dispatchEvent(new Event('change'));
      }
    });
    box.addEventListener('blur', () => {
      if (!resolveTyped(shown, box.value)) box.value = savedText();
      box.placeholder = STRINGS.CAPTURE_CONFIG.HOOK_DLL_TYPE_PLACEHOLDER;
    });
    box.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        box.value = savedText();
        box.blur();
      }
    });
  }
  document.querySelector('#goldsrc-hooks-refresh-btn')
    ?.addEventListener('click', () => { refreshHookDllPicker(); });
}
