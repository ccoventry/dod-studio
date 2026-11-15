import { describe, it, expect } from 'vitest';
import { clampToViewport, normalizeItems, itemState, nextIndex } from './context_menu.js';

const view = { width: 800, height: 600 };
const size = { width: 200, height: 300 };

describe('clampToViewport', () => {
  it('leaves a menu that fits where it was asked for', () => {
    expect(clampToViewport({ x: 100, y: 100 }, size, view)).toEqual({ x: 100, y: 100 });
  });
  it('slides back from the right and bottom edges', () => {
    expect(clampToViewport({ x: 790, y: 590 }, size, view)).toEqual({ x: 596, y: 296 });
  });
  it('never goes past the top or left margin', () => {
    expect(clampToViewport({ x: -5, y: -5 }, size, view)).toEqual({ x: 4, y: 4 });
  });
  it('a menu taller than the window starts at the top margin', () => {
    expect(clampToViewport({ x: 10, y: 300 }, { width: 100, height: 900 }, view).y).toBe(4);
  });
});

describe('normalizeItems', () => {
  const sep = { separator: true };
  const a = { label: 'A' };
  const b = { label: 'B' };
  it('drops falsy slots and stray separators', () => {
    expect(normalizeItems([sep, a, sep, sep, null, false, sep, b, sep])).toEqual([a, sep, b]);
  });
  it('is empty when nothing is left', () => {
    expect(normalizeItems([sep, null])).toEqual([]);
  });
});

describe('itemState', () => {
  it('an enabled item shows its own title', () => {
    expect(itemState({ title: 'hi' })).toEqual({ enabled: true, tooltip: 'hi' });
  });
  it('a dimmed item shows the reason', () => {
    expect(itemState({ disabled: 'no demo', title: 'hi' })).toEqual({ enabled: false, tooltip: 'no demo' });
  });
  it('disabled: true dims without a reason', () => {
    expect(itemState({ disabled: true })).toEqual({ enabled: false, tooltip: '' });
  });
});

describe('nextIndex', () => {
  const items = [{ label: 'a' }, { separator: true }, { label: 'b', disabled: 'x' }, { label: 'c' }];
  it('skips separators and dimmed items', () => {
    expect(nextIndex(items, 0, 1)).toBe(3);
  });
  it('wraps around both ways', () => {
    expect(nextIndex(items, 3, 1)).toBe(0);
    expect(nextIndex(items, 0, -1)).toBe(3);
  });
  it('starts from nothing at either end', () => {
    expect(nextIndex(items, -1, 1)).toBe(0);
    expect(nextIndex(items, items.length, -1)).toBe(3);
  });
  it('is -1 when nothing can be chosen', () => {
    expect(nextIndex([{ label: 'a', disabled: true }], -1, 1)).toBe(-1);
  });
});
