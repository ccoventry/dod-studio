import { describe, it, expect } from 'vitest';
import { zoneAt, cssPoint } from './demo_drop.js';

const scan = { el: 'scan', rect: { left: 0, top: 0, right: 400, bottom: 100 }, shown: true };
const queue = { el: 'queue', rect: { left: 0, top: 110, right: 400, bottom: 500 }, shown: true };

describe('zoneAt (#530)', () => {
  it('finds the zone under the pointer', () => {
    expect(zoneAt([scan, queue], { x: 10, y: 50 })).toBe('scan');
    expect(zoneAt([scan, queue], { x: 399, y: 499 })).toBe('queue');
  });

  it('is null between zones, outside them, or with no position', () => {
    expect(zoneAt([scan, queue], { x: 10, y: 105 })).toBe(null);
    expect(zoneAt([scan, queue], { x: 600, y: 50 })).toBe(null);
    expect(zoneAt([scan, queue], null)).toBe(null);
  });

  it('never matches a zone that is not shown (another page is open)', () => {
    expect(zoneAt([{ ...scan, shown: false }], { x: 10, y: 50 })).toBe(null);
  });
});

describe('cssPoint', () => {
  it('turns physical pixels into CSS pixels', () => {
    expect(cssPoint({ x: 300, y: 150 }, 1.5)).toEqual({ x: 200, y: 100 });
    expect(cssPoint({ x: 300, y: 150 }, 0)).toEqual({ x: 300, y: 150 });
    expect(cssPoint(undefined, 2)).toBe(null);
  });
});
