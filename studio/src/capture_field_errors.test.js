import { describe, it, expect } from 'vitest';
import { captureFieldProblems } from './capture_field_errors.js';

describe('captureFieldProblems', () => {
  it('flags every blank field', () => {
    const p = captureFieldProblems({ hlPath: '', hlaePath: '  ', driveCount: 0 });
    expect(p.hl).toBeTruthy();
    expect(p.hlae).toBeTruthy();
    expect(p.drive).toBeTruthy();
  });
  it('flags nothing when all are set', () => {
    const p = captureFieldProblems({ hlPath: 'C:/x/hl.exe', hlaePath: 'C:/y/hlae.exe', driveCount: 1 });
    expect(p).toEqual({ hl: '', hlae: '', drive: '' });
  });
});
