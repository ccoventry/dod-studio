import { describe, it, expect } from 'vitest';
import { captureFieldProblems } from './capture_field_errors.js';

describe('captureFieldProblems', () => {
  it('flags every blank field', () => {
    const p = captureFieldProblems({ hlPath: '', hlaePath: '  ', driveCount: 0 });
    expect(p.hl).toBeTruthy();
    expect(p.hlae).toBeTruthy();
    expect(p.drive).toBeTruthy();
    expect(p.agr).toBe('');
  });
  it('flags a blank, zero or invalid AGR FPS only in AGR mode', () => {
    const ok = { hlPath: 'C:/x/hl.exe', hlaePath: 'C:/y/hlae.exe', driveCount: 1 };
    for (const agrFps of [0, NaN, -3, undefined]) {
      expect(captureFieldProblems({ ...ok, captureMode: 'agr', agrFps }).agr).toMatch(/AGR FPS/);
      expect(captureFieldProblems({ ...ok, captureMode: 'frame_sequence', agrFps }).agr).toBe('');
      expect(captureFieldProblems({ ...ok, captureMode: 'obs', agrFps }).agr).toBe('');
    }
    expect(captureFieldProblems({ ...ok, captureMode: 'agr', agrFps: 30 }).agr).toBe('');
  });
  it('flags nothing when all are set', () => {
    const p = captureFieldProblems({ hlPath: 'C:/x/hl.exe', hlaePath: 'C:/y/hlae.exe', driveCount: 1 });
    expect(p).toEqual({ hl: '', hlae: '', drive: '', agr: '' });
  });
});
