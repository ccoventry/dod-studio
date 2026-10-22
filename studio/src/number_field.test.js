import { describe, it, expect } from 'vitest';
import { parseNumber } from './number_field.js';

describe('parseNumber', () => {
  it('keeps a typed 0, which `parseFloat(v) || fallback` threw away (#460)', () => {
    expect(parseNumber('0', 2.0)).toBe(0);
    expect(parseNumber('0.0', 0.6)).toBe(0);
  });

  it('falls back only for an empty or non-numeric field', () => {
    expect(parseNumber('', 3.0)).toBe(3.0);
    expect(parseNumber(undefined, 3.0)).toBe(3.0);
    expect(parseNumber('abc', 3.0)).toBe(3.0);
  });

  it('parses decimals, and integers when asked', () => {
    expect(parseNumber('0.05', 1)).toBe(0.05);
    expect(parseNumber('2.7', 0, { integer: true })).toBe(2);
    expect(parseNumber('300fps', 0, { integer: true })).toBe(300);
  });

  it('with `positive`, 0 and below fall back too', () => {
    expect(parseNumber('0', 300, { integer: true, positive: true })).toBe(300);
    expect(parseNumber('-5', 0.05, { positive: true })).toBe(0.05);
    expect(parseNumber('120', 300, { integer: true, positive: true })).toBe(120);
  });

  it('never returns NaN or Infinity', () => {
    expect(parseNumber('Infinity', 1)).toBe(1);
    expect(parseNumber('NaN', 1)).toBe(1);
  });
});
