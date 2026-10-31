import { describe, it, expect } from 'vitest';
import { clampSize, pxToPermille } from './split_resize.js';

describe('split_resize', () => {
  it('keeps a size inside the limits', () => {
    expect(clampSize(50, 100, 400)).toBe(100);
    expect(clampSize(500, 100, 400)).toBe(400);
    expect(clampSize(250, 100, 400)).toBe(250);
  });

  it('prefers the minimum when the window is too small for both limits', () => {
    expect(clampSize(300, 360, 200)).toBe(360);
  });

  it('stores a pixel size as a permille of the container', () => {
    expect(pxToPermille(768, 1920)).toBe(400);
  });

  it('stores nothing sensible for a zero-width container', () => {
    expect(pxToPermille(100, 0)).toBe(0);
  });
});
