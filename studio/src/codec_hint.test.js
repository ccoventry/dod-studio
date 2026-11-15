import { describe, it, expect } from 'vitest';
import { codecHint, startCodecHint } from './codec_hint.js';
import { STRINGS } from './strings.js';

describe('codecHint', () => {
  it('has its own line for each ProRes choice', () => {
    const hints = ['prores', 'prores_422', 'prores_lt', 'prores_4444'].map(codecHint);
    hints.forEach((h) => expect(h).toBeTruthy());
    expect(new Set(hints).size).toBe(4);
    expect(codecHint('prores')).toBe(STRINGS.RENDER.CODEC_HINT_PRORES);
  });

  it('has none for other codecs', () => {
    expect(codecHint('dnxhr')).toBe('');
    expect(codecHint('custom')).toBe('');
    expect(codecHint(undefined)).toBe('');
  });
});

describe('startCodecHint', () => {
  it('follows the select and hides when there is nothing to say', () => {
    const listeners = [];
    const select = { value: 'prores', addEventListener: (type, fn) => listeners.push([type, fn]) };
    const hint = { textContent: '', hidden: false };
    const change = (value) => {
      select.value = value;
      listeners.filter(([type]) => type === 'change').forEach(([, fn]) => fn());
    };

    startCodecHint(select, hint);
    expect(hint.textContent).toBe(STRINGS.RENDER.CODEC_HINT_PRORES);
    expect(hint.hidden).toBe(false);

    change('prores_lt');
    expect(hint.textContent).toBe(STRINGS.RENDER.CODEC_HINT_PRORES_LT);

    change('dnxhr');
    expect(hint.hidden).toBe(true);
  });
});
