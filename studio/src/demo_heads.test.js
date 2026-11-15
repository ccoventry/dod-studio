import { describe, it, expect, vi } from 'vitest';
import { readDemoHeadsInto, mapCellText } from './demo_heads.js';

// The Demos list shows a folder from its file names and fills in each demo's
// map and type after: on a cloud drive reading the start of every demo first
// held the list back.

const demos = () => ['a', 'b', 'c', 'd', 'e'].map((n) => ({ path: `D:/demos/${n}.dem`, map_name: null, demo_type: 'POV' }));
const headFor = (path) => ({ path, map_name: `dod_${path.slice(-5, -4)}`, demo_type: path.includes('c.dem') ? 'HLTV' : 'POV' });

describe('readDemoHeadsInto', () => {
  it('reads a few at a time and shows each batch as it arrives', async () => {
    const list = demos();
    const read = vi.fn(async (paths) => paths.map(headFor));
    const shown = [];
    const onRead = () => shown.push(list.map((d) => d.map_name));
    expect(await readDemoHeadsInto(list, { read, stillWanted: () => true, onRead, size: 2 })).toBe(true);
    expect(read).toHaveBeenCalledTimes(3);
    expect(read.mock.calls[0][0]).toEqual(['D:/demos/a.dem', 'D:/demos/b.dem']);
    expect(shown[0]).toEqual(['dod_a', 'dod_b', null, null, null]);
    expect(list.map((d) => d.map_name)).toEqual(['dod_a', 'dod_b', 'dod_c', 'dod_d', 'dod_e']);
    expect(list[2].demo_type).toBe('HLTV');
  });

  it('stops once another folder is open, and writes nothing late', async () => {
    const list = demos();
    let wanted = true;
    const read = vi.fn(async (paths) => { wanted = false; return paths.map(headFor); });
    const onRead = vi.fn();
    expect(await readDemoHeadsInto(list, { read, stillWanted: () => wanted, onRead, size: 2 })).toBe(false);
    expect(read).toHaveBeenCalledTimes(1);
    expect(onRead).not.toHaveBeenCalled();
    expect(list[0].map_name).toBeNull();
  });

  it('a failed read leaves the map unknown, not still reading', async () => {
    const list = demos();
    await readDemoHeadsInto(list, { read: async () => [], stillWanted: () => true, onRead: () => {}, size: 10 });
    expect(list.map((d) => mapCellText(d, { reading: '…', dash: '—' }))).toEqual(['—', '—', '—', '—', '—']);
  });
});

describe('mapCellText', () => {
  it('says it is reading until the head is in', () => {
    expect(mapCellText({ map_name: null }, { reading: '…', dash: '—' })).toBe('…');
    expect(mapCellText({}, { reading: '…', dash: '—' })).toBe('…');
    expect(mapCellText({ map_name: 'dod_anzio' }, { reading: '…', dash: '—' })).toBe('dod_anzio');
    expect(mapCellText({ map_name: '' }, { reading: '…', dash: '—' })).toBe('—');
  });
});
