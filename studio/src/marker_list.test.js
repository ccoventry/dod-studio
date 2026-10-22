import { describe, it, expect } from 'vitest';
import { clockTime, csvField, markerRows, markerCsv, MARKER_COLUMNS } from './marker_list.js';
import { streakUid } from './take_index.js';

describe('clockTime and csvField', () => {
  it('formats demo seconds', () => {
    expect(clockTime(754.2)).toBe('12:34.20');
    expect(clockTime(5.5)).toBe('0:05.50');
    expect(clockTime(3725.25)).toBe('1:02:05.25');
  });

  it('quotes only what needs it', () => {
    expect(csvField('plain')).toBe('plain');
    expect(csvField('a, b')).toBe('"a, b"');
    expect(csvField('say "hi"')).toBe('"say ""hi"""');
  });
});

describe('markerRows', () => {
  const captured = {
    player_index: 1, target_player: 'krod', status: 'Captured', start_tick: 1000, end_tick: 1400,
    kills: [[1000, 700, 'Garand'], [1200, 702, 'Garand'], [1400, 705.5, 'MP40']],
    viewdemo_times: [754.2, 756, 759.7], start_index: 1, end_index: 2,
    timeline_string: 'Garand (+0:02) MP40', notes: 'wallbang, nice',
  };
  const earlier = { ...captured, status: 'Rendered', start_tick: 500, kills: [[500, 600, 'K98']], viewdemo_times: [650], start_index: 0, end_index: 0, notes: '' };
  const pending = { ...captured, status: 'Pending', start_tick: 2000 };
  const demo = { path: 'C:/d/x.dem', name: 'x.dem', streaks: [captured, pending, earlier] };

  it('lists captured and rendered highlights in time order, over the chosen range', () => {
    const takeIndex = { 'sess/dodstudio_chain_01_b0': [streakUid(demo.path, captured)] };
    const rows = markerRows([demo], takeIndex);
    expect(rows).toHaveLength(2);
    expect(rows[0][9]).toBe('Rendered');
    expect(rows[1]).toEqual([
      'x.dem', 'krod', 2, '2-3', '12:36.00', '12:39.70', '3.70', 1200, 1400,
      'Captured', 'sess/dodstudio_chain_01_b0', 'Garand (+0:02) MP40', 'wallbang, nice',
    ]);
    expect(rows[0][10]).toBe('');
  });

  it('writes a header and quoted CSV', () => {
    const csv = markerCsv([demo], {});
    const lines = csv.trimEnd().split('\r\n');
    expect(lines[0]).toBe(MARKER_COLUMNS.join(','));
    expect(lines[2].endsWith('"wallbang, nice"')).toBe(true);
  });
});
