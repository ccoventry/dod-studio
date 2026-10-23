// hook_dll_picker.js
// Configuration > Paths > GoldSrc Hooks DLL as a list of the test builds in
// the checkout's local\ folder (hook_dll_list.rs), so trying another build
// is one pick instead of editing a path. A plain dropdown, with a filter box
// before it: typing `91` narrows the dropdown to the matching builds. The
// path itself stays in #goldsrc-hooks-dll-path-input (hidden), which every
// launch reads and settings_sync.js saves; this only sets it. Browse still
// picks any file. Only in builds that use the override (hook_dll_row.js).

import { listHookDlls } from './ipc_bridge.js';
import { makeClearable } from './clearable_input.js';
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

/** The rows that contain `typed` (any case), with the one `current` picks
 *  always kept so the dropdown never shows a row that isn't the saved one. */
export function filterOptions(options, typed, current) {
  const want = String(typed || '').trim().toLowerCase();
  if (!want) return options;
  const keep = selectedValue(options, current);
  return options.filter((o) => o.value === keep || o.text.toLowerCase().includes(want));
}

/** Every row from the last listing. */
let all = [];

const pathInput = () => document.querySelector('#goldsrc-hooks-dll-path-input');
const select = () => document.querySelector('#goldsrc-hooks-dll-select');
const filterBox = () => document.querySelector('#goldsrc-hooks-dll-filter');

/** Shows the rows the filter leaves, the saved one selected. */
function render() {
  const el = select();
  if (!el) return;
  const current = pathInput()?.value.trim() || '';
  const rows = filterOptions(all, filterBox()?.value, current);
  el.replaceChildren(...rows.map((o) => {
    const option = document.createElement('option');
    option.value = o.value;
    option.textContent = o.text;
    option.title = o.title;
    return option;
  }));
  el.value = selectedValue(rows, current);
}

/** Saves `value` as the path, through the path field's own change handler
 *  (config_wiring.js), which checks it and saves the settings. */
function pick(value) {
  const input = pathInput();
  if (!input || input.value.trim() === value) return;
  input.value = value;
  input.dispatchEvent(new Event('change'));
}

/** Fills the dropdown from the folder again and selects the saved path. */
export async function refreshHookDllPicker() {
  const input = pathInput();
  if (!select() || !input) return;
  const refresh = document.querySelector('#goldsrc-hooks-refresh-btn');
  if (refresh) refresh.disabled = true;
  try {
    const found = await listHookDlls();
    all = pickerOptions(found, input.value.trim());
    render();
    const hint = found?.folder ? STRINGS.CAPTURE_CONFIG.hookDllFolderHint(found.folder) : '';
    select().title = hint;
  } finally {
    if (refresh) refresh.disabled = false;
  }
}

/** Wires the dropdown, its filter box and the Refresh button. Typing in the
 *  filter narrows the dropdown; Enter picks the one row it matches. */
export function initHookDllPicker() {
  select()?.addEventListener('change', (e) => pick(e.target.value));
  const box = filterBox();
  if (box) {
    makeClearable(box, STRINGS.CAPTURE_CONFIG.HOOK_DLL_FILTER_CLEAR);
    box.addEventListener('input', render);
    box.addEventListener('keydown', (e) => {
      if (e.key !== 'Enter') return;
      const match = resolveTyped(all, box.value);
      if (match) {
        pick(match.value);
        box.value = '';
        render();
      }
    });
  }
  document.querySelector('#goldsrc-hooks-refresh-btn')
    ?.addEventListener('click', () => { refreshHookDllPicker(); });
}
