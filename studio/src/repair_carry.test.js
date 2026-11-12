import { describe, it, expect } from 'vitest';
import { matchSplitHighlights } from './split_carry.js';
import { repairCarryReport } from './repair_carry.js';

const hl = (kills, extra = {}) => ({
  player_index: 3,
  target_player: 'm00cat',
  steam_id: '76561197972576011',
  kills,
  victims: kills.map((k) => `shot with ${k[2]}`),
  timeline_string: kills.map((k) => k[2]).join(', '),
  ...extra,
});
const original = (streaks) => ({ path: 'D:/demos/match.dem', map_name: 'dod_anzio', tickrate: 100, streaks });
const repaired = (streaks) => [{ path: 'D:/demos/match_repaired.dem', map_name: 'dod_anzio', streaks, splitIndex: 0 }];

describe('repairCarryReport (#15)', () => {
  it('keeps the work on a highlight the repaired demo has at the same time', () => {
    const from = hl([[9000, 90, 'K98'], [9300, 93, 'BAR']], { curation: 'Keep' });
    const after = hl([[90000, 900, 'Thompson'], [90200, 902, 'Thompson']]);
    const result = matchSplitHighlights(original([from]), repaired([hl(from.kills), after]));
    const report = repairCarryReport(original([from]), result, 'D:/demos/match_repaired.dem');
    expect(report.toast.type).toBe('success');
    expect(report.log).toEqual(['[repair] match.dem -> match_repaired.dem: kept the work on 1 of 1 highlight(s)']);
  });

  it('names each highlight whose work was not kept', () => {
    const from = hl([[9000, 90, 'K98']], { notes: 'nice' });
    const result = matchSplitHighlights(original([from]), repaired([]));
    const report = repairCarryReport(original([from]), result, 'D:/demos/match_repaired.dem');
    expect(report.toast.type).toBe('warning');
    expect(report.log[1]).toMatch(/^\[repair\] {3}NOT KEPT .* m00cat \(K98\): no highlight in the repaired demo/);
  });

  it('says nothing when no highlight had work on it', () => {
    const result = matchSplitHighlights(original([hl([[1, 1, 'K98']])]), repaired([]));
    expect(repairCarryReport(original([]), result, 'x')).toEqual({ toast: null, log: [] });
  });
});
