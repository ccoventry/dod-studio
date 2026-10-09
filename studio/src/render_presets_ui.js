// render_presets_ui.js
// Configuration > Render Output's preset row (#108): pick a saved setup to
// apply it, or name the current one and save it.

import { presetValues, matchingPreset, savePreset, deletePreset } from './render_presets.js';
import { STRINGS } from './strings.js';

let presets = [];
let onPresetsChange = () => {};

const $ = (selector) => document.querySelector(selector);

/** The render settings currently in the fields. */
function currentValues() {
  return presetValues({
    codec: $('#render-codec-select')?.value,
    custom_codec_args: $('#render-custom-codec-input')?.value,
    fps: $('#render-fps-input')?.value,
    max_concurrent: $('#render-max-concurrent-input')?.value,
  });
}

/** Sets a field and fires the event its own listeners (saving, the custom
 *  codec box) already react to. */
function setField(selector, value, eventName) {
  const el = $(selector);
  if (!el) return;
  el.value = value;
  el.dispatchEvent(new Event(eventName));
}

/** The saved presets, for the settings file. */
export function getRenderPresets() {
  return presets;
}

/** Loads presets from saved settings. */
export function setRenderPresets(list) {
  presets = Array.isArray(list) ? list.map((p) => ({ name: String(p.name || ''), ...presetValues(p) })).filter((p) => p.name) : [];
  syncPresetSelect();
}

/** Shows the preset the fields currently match, or "—" when they match none. */
export function syncPresetSelect() {
  const select = $('#render-preset-select');
  if (!select) return;
  const none = document.createElement('option');
  none.value = '';
  none.textContent = STRINGS.RENDER.PRESET_NONE;
  select.replaceChildren(none, ...presets.map((p) => {
    const option = document.createElement('option');
    option.value = p.name;
    option.textContent = p.name;
    return option;
  }));
  select.value = matchingPreset(presets, currentValues())?.name || '';
  const deleteBtn = $('#render-preset-delete');
  if (deleteBtn) deleteBtn.disabled = !select.value;
}

function applyPreset(name) {
  const preset = presets.find((p) => p.name === name);
  if (!preset) return;
  setField('#render-codec-select', preset.codec, 'change');
  setField('#render-custom-codec-input', preset.custom_codec_args, 'input');
  setField('#render-fps-input', preset.fps, 'input');
  setField('#render-max-concurrent-input', preset.max_concurrent, 'input');
  syncPresetSelect();
}

/** Wires the row. `onChange()` saves settings after presets change. */
export function initRenderPresets({ onChange } = {}) {
  if (onChange) onPresetsChange = onChange;
  $('#render-preset-select')?.addEventListener('change', (e) => {
    if (e.target.value) applyPreset(e.target.value);
    else syncPresetSelect();
  });
  $('#render-preset-save')?.addEventListener('click', () => {
    const nameEl = $('#render-preset-name');
    const name = nameEl?.value.trim() || $('#render-preset-select')?.value;
    if (!name) {
      nameEl?.focus();
      return;
    }
    presets = savePreset(presets, name, currentValues());
    if (nameEl) nameEl.value = '';
    syncPresetSelect();
    onPresetsChange();
  });
  $('#render-preset-delete')?.addEventListener('click', () => {
    const name = $('#render-preset-select')?.value;
    if (!name) return;
    presets = deletePreset(presets, name);
    syncPresetSelect();
    onPresetsChange();
  });
  ['#render-codec-select', '#render-custom-codec-input', '#render-fps-input', '#render-max-concurrent-input']
    .forEach((selector) => {
      $(selector)?.addEventListener('input', syncPresetSelect);
      $(selector)?.addEventListener('change', syncPresetSelect);
    });
  syncPresetSelect();
}
