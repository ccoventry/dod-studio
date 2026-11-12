import { describe, it, expect } from 'vitest';
import { DEFAULT_AGR_FPS, effectiveAgrFps } from './agr_rate.js';

describe('effectiveAgrFps', () => {
  it('is 30 when AGR FPS is unset, as the backend reads it', () => {
    expect(DEFAULT_AGR_FPS).toBe(30);
    expect(effectiveAgrFps(0)).toBe(30);
    expect(effectiveAgrFps(undefined)).toBe(30);
    expect(effectiveAgrFps(null)).toBe(30);
    expect(effectiveAgrFps(-5)).toBe(30);
    expect(effectiveAgrFps(NaN)).toBe(30);
  });

  it('is the set rate otherwise', () => {
    expect(effectiveAgrFps(120)).toBe(120);
    expect(effectiveAgrFps(1)).toBe(1);
  });
});
