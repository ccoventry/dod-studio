// codec_hint.js
// The line under Render Settings' Codec picker that says what the selected
// ProRes choice is for, so picking between four sizes of the same codec
// isn't a guess. Other codecs have no line.
import { STRINGS } from './strings.js';

const HINT_KEYS = {
  prores: 'CODEC_HINT_PRORES',
  prores_422: 'CODEC_HINT_PRORES_422',
  prores_lt: 'CODEC_HINT_PRORES_LT',
  prores_4444: 'CODEC_HINT_PRORES_4444',
};

/** The hint for a `RenderCodec` string id, or '' when it has none. */
export function codecHint(codec) {
  const key = HINT_KEYS[codec];
  return key ? STRINGS.RENDER[key] : '';
}

/** Keeps `hint` showing the hint for `select`'s value, hidden when empty. */
export function startCodecHint(select, hint) {
  if (!select || !hint) return;
  const sync = () => {
    const text = codecHint(select.value);
    hint.textContent = text;
    hint.hidden = !text;
  };
  select.addEventListener('change', sync);
  sync();
}
