import { describe, it, expect, vi } from 'vitest';
import { createNameplateCheck } from './nameplate_cell.js';

describe('createNameplateCheck', () => {
  it('marks the files the backend says are gone, and redraws once', async () => {
    const exists = vi.fn(async (paths) => paths.map((p) => p !== 'D:\\np\\gone.png'));
    const onChange = vi.fn();
    const check = createNameplateCheck({ exists, onChange });
    await check.watch(['D:\\np\\a.png', '', 'D:\\np\\gone.png', 'D:\\np\\a.png']);
    expect(exists).toHaveBeenCalledWith(['D:\\np\\a.png', 'D:\\np\\gone.png']);
    expect(check.isMissing('D:\\np\\gone.png')).toBe(true);
    expect(check.isMissing('D:\\np\\a.png')).toBe(false);
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('asks once per set of paths, and again after a reset', async () => {
    const exists = vi.fn(async (paths) => paths.map(() => true));
    const check = createNameplateCheck({ exists, onChange: () => {} });
    await check.watch(['b', 'a']);
    await check.watch(['a', 'b']);
    expect(exists).toHaveBeenCalledTimes(1);
    check.reset();
    await check.watch(['a', 'b']);
    expect(exists).toHaveBeenCalledTimes(2);
  });

  it('marks nothing missing when the check itself failed', async () => {
    const onChange = vi.fn();
    const check = createNameplateCheck({ exists: async () => null, onChange });
    await check.watch(['D:\\np\\a.png']);
    expect(check.isMissing('D:\\np\\a.png')).toBe(false);
    expect(onChange).not.toHaveBeenCalled();
  });
});
