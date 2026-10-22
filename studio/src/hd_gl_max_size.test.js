import { describe, it, expect } from 'vitest';
import { glMaxSizeLine } from './hd_gl_max_size.js';

describe('glMaxSizeLine', () => {
  it('says nothing without a report', () => {
    expect(glMaxSizeLine(null, 1024)).toBe(null);
    expect(glMaxSizeLine(undefined, 1024)).toBe(null);
  });

  it('is fine at or above the build size, and names where the value comes from', () => {
    const line = glMaxSizeLine({ value: '2048', shows_at: 2048, source: { kind: 'config', file: 'movie.cfg', line: 3 } }, 1024);
    expect(line.low).toBe(false);
    expect(line.text).toContain('gl_max_size 2048');
    expect(line.text).toContain('movie.cfg line 3');
  });

  it('warns when the game would shrink HD files below the build size', () => {
    const line = glMaxSizeLine({ value: '256', shows_at: 256, source: { kind: 'engine_default' } }, 1024);
    expect(line.low).toBe(true);
    expect(line.text).toContain('256 px');
    expect(line.text).toContain('1024');
    expect(line.text).toContain('movie.cfg');
  });

  it('compares with the build size picked, not a fixed number', () => {
    const gl = { value: '1024', shows_at: 1024, source: { kind: 'config', file: 'movie.cfg', line: 1 } };
    expect(glMaxSizeLine(gl, 1024).low).toBe(false);
    expect(glMaxSizeLine(gl, 2048).low).toBe(true);
  });

  it('points at Initial Commands when they set the low value', () => {
    const line = glMaxSizeLine({ value: '512', shows_at: 512, source: { kind: 'initial_commands' } }, 1024);
    expect(line.low).toBe(true);
    expect(line.text).toContain('Initial Commands');
    expect(line.text).not.toContain('into movie.cfg');
  });
});
