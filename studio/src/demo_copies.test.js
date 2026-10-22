import { describe, it, expect } from 'vitest';
import { splitIdenticalCopies } from './demo_copies.js';

const demo = (path, file_key) => ({ path, file_key });

describe('splitIdenticalCopies', () => {
  it('skips a copy of a queued demo under another name', () => {
    const queued = [demo('C:\\demos\\a.dem', '10-aa')];
    const copy = demo('C:\\elsewhere\\a copy.dem', '10-aa');
    const other = demo('C:\\demos\\b.dem', '20-bb');
    const { keep, copies } = splitIdenticalCopies(queued, [copy, other]);
    expect(keep).toEqual([other]);
    expect(copies).toEqual([{ demo: copy, sameAs: queued[0] }]);
  });

  it('keeps the first of two copies found in one scan', () => {
    const first = demo('C:\\demos\\a.dem', '10-aa');
    const second = demo('C:\\demos\\a2.dem', '10-aa');
    const { keep, copies } = splitIdenticalCopies([], [first, second]);
    expect(keep).toEqual([first]);
    expect(copies).toEqual([{ demo: second, sameAs: first }]);
  });

  it('keeps a rescan of a queued path, and demos with no key', () => {
    const queued = [demo('C:\\demos\\a.dem', '10-aa')];
    const rescan = demo('C:\\demos\\a.dem', '10-aa');
    const noKey = demo('C:\\demos\\old.dem', '');
    const { keep, copies } = splitIdenticalCopies(queued, [rescan, noKey]);
    expect(keep).toEqual([rescan, noKey]);
    expect(copies).toEqual([]);
  });
});
