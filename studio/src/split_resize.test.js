import { describe, it, expect, vi } from 'vitest';
import { attachSplitResize, clampSize, pxToPermille } from './split_resize.js';

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

  it('ends the drag when a move arrives with no button held (mouseup lost to a blur)', () => {
    const docHandlers = {};
    vi.stubGlobal('document', {
      addEventListener: (t, f) => { docHandlers[t] = f; },
      removeEventListener: (t) => { delete docHandlers[t]; },
    });
    const handleHandlers = {};
    const handle = {
      classList: { add() {}, remove() {} },
      addEventListener: (t, f) => { handleHandlers[t] = f; },
    };
    const apply = vi.fn();
    const commit = vi.fn();
    attachSplitResize({
      handle, axis: 'x', measure: () => 100, limits: () => ({ min: 0, max: 1000 }),
      apply, commit, reset() {},
    });
    handleHandlers.mousedown({ preventDefault() {}, clientX: 0 });
    docHandlers.mousemove({ clientX: 50, buttons: 1 });
    expect(commit).not.toHaveBeenCalled();
    docHandlers.mousemove({ clientX: 80, buttons: 0 });
    expect(commit).toHaveBeenCalledWith(150);
    expect(apply).toHaveBeenCalledTimes(1);
    expect(docHandlers.mousemove).toBeUndefined();
    vi.unstubAllGlobals();
  });
});
