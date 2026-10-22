// render_presets.js
// Named render setups (#108): codec (with its custom args), source FPS and
// concurrency, saved under a name and applied in one pick. Pure, so it can
// be tested on its own; render_presets_ui.js wires it to Configuration >
// Render Output.

/** The settings a preset carries, in a normalised shape. */
export function presetValues(values) {
  const codec = String(values?.codec || 'prores');
  return {
    codec,
    // Only meaningful for the custom codec, so ignored for every other one.
    custom_codec_args: codec === 'custom' ? String(values?.custom_codec_args || '').trim() : '',
    fps: Math.max(1, parseInt(values?.fps, 10) || 300),
    max_concurrent: Math.min(8, Math.max(1, parseInt(values?.max_concurrent, 10) || 2)),
  };
}

function sameValues(a, b) {
  const x = presetValues(a);
  const y = presetValues(b);
  return x.codec === y.codec && x.custom_codec_args === y.custom_codec_args
    && x.fps === y.fps && x.max_concurrent === y.max_concurrent;
}

/** The first preset whose settings equal `current`, or null. */
export function matchingPreset(presets, current) {
  return (presets || []).find((p) => sameValues(p, current)) || null;
}

/** `presets` with `name` saved as `values`, replacing a preset of the same
 *  name (ignoring case), sorted by name. Empty names are refused. */
export function savePreset(presets, name, values) {
  const trimmed = String(name || '').trim();
  if (!trimmed) return presets || [];
  const kept = (presets || []).filter((p) => p.name.toLowerCase() !== trimmed.toLowerCase());
  return [...kept, { name: trimmed, ...presetValues(values) }]
    .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
}

/** `presets` without the one named `name`. */
export function deletePreset(presets, name) {
  return (presets || []).filter((p) => p.name !== name);
}
