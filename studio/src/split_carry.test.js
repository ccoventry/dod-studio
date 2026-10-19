import { describe, it, expect } from 'vitest';
import { matchSplitHighlights, applyCarriedWork, carryReport, hasWork, UNCARRIED_REASON } from './split_carry.js';
import { renameUidsInTakeIndex, streakUid, recordTake, resolveTake } from './take_index.js';

const hl = (kills, extra = {}) => ({
  player_index: 3,
  target_player: 'm00cat',
  steam_id: '76561197972576011',
  kills,
  victims: kills.map((k) => `shot with ${k[2]}`),
  timeline_string: kills.map((k) => k[2]).join(', '),
  ...extra,
});
const demo = (path, streaks, extra = {}) => ({ path, map_name: 'dod_lennon2', tickrate: 100, streaks, ...extra });
const piece = (path, streaks, splitIndex = 0) => ({ ...demo(path, streaks), splitIndex });

describe('matching a split demo\'s highlights (#217)', () => {
  it('finds the same kills when the piece restarts the clock', () => {
    const from = hl([[9000, 900, 'K98'], [9300, 903, 'BAR'], [9500, 905.2, 'K98']], { curation: 'Keep', notes: 'nice' });
    const to = hl([[100, 10, 'K98'], [400, 13, 'BAR'], [600, 15.1, 'K98']]);
    const result = matchSplitHighlights(demo('a.dem', [from]), [piece('a_map.dem', [to])]);
    expect(result.withWork).toBe(1);
    expect(result.carried).toHaveLength(1);
    expect(result.carried[0]).toMatchObject({ from, to, start_index: 0, end_index: 2, narrowed: false });
  });

  it('ignores highlights with no work on them', () => {
    const result = matchSplitHighlights(demo('a.dem', [hl([[1, 1, 'K98']])]), [piece('p.dem', [hl([[1, 1, 'K98']])])]);
    expect(result).toEqual({ carried: [], uncarried: [], withWork: 0 });
  });

  it('follows a life the piece split into two highlights, by kills not index', () => {
    const kills = [[100, 10, 'K98'], [200, 12, 'K98'], [900, 40, 'BAR'], [1000, 42, 'Colt']];
    const from = hl(kills, { start_index: 2, end_index: 3, status: 'Pending' });
    const first = hl(kills.slice(0, 2));
    const second = hl(kills.slice(2));
    const result = matchSplitHighlights(demo('a.dem', [from]), [piece('p.dem', [first, second])]);
    expect(result.carried[0]).toMatchObject({ to: second, start_index: 0, end_index: 1, narrowed: false });
  });

  it('keeps a range across two piece highlights on the one with most of its kills, and says so', () => {
    const kills = [[100, 10, 'K98'], [200, 12, 'K98'], [900, 40, 'BAR'], [1000, 42, 'Colt']];
    const from = hl(kills, { start_index: 1, end_index: 3, notes: 'x' });
    const first = hl(kills.slice(0, 2));
    const second = hl(kills.slice(2));
    const result = matchSplitHighlights(demo('a.dem', [from]), [piece('p.dem', [first, second])]);
    expect(result.carried[0]).toMatchObject({ to: second, start_index: 0, end_index: 1, narrowed: true });
  });

  it('tells two players with the same kill pattern apart', () => {
    const kills = [[100, 10, 'K98'], [300, 12, 'K98']];
    const a = hl(kills, { curation: 'Keep' });
    const b = hl(kills, { steam_id: '765611980000', target_player: 'other', player_index: 4, curation: 'Skip' });
    const pa = hl(kills);
    const pb = hl(kills, { steam_id: '765611980000', target_player: 'other', player_index: 4 });
    const result = matchSplitHighlights(demo('a.dem', [a, b]), [piece('p.dem', [pb, pa])]);
    expect(result.carried.map((c) => [c.from, c.to])).toEqual([[a, pa], [b, pb]]);
  });

  it('reports kills it cannot find', () => {
    const from = hl([[100, 10, 'K98'], [200, 12, 'K98']], { notes: 'gone' });
    const other = hl([[100, 10, 'K98'], [200, 12, 'Garand']]);
    const result = matchSplitHighlights(demo('a.dem', [from]), [piece('p.dem', [other])]);
    expect(result.uncarried).toEqual([{ from, reason: UNCARRIED_REASON.NOT_FOUND }]);
  });

  it('a different victim is a different kill', () => {
    const from = hl([[100, 10, 'K98']], { notes: 'n' });
    const other = hl([[100, 10, 'K98']], { victims: ['someone else'] });
    expect(matchSplitHighlights(demo('a.dem', [from]), [piece('p.dem', [other])]).uncarried[0].reason)
      .toBe(UNCARRIED_REASON.NOT_FOUND);
  });

  it('does not guess between two places a single kill fits', () => {
    const from = hl([[100, 10, 'K98']], { victims: [], curation: 'Maybe' });
    const p = piece('p.dem', [hl([[50, 5, 'K98']], { victims: [] }), hl([[500, 50, 'K98']], { victims: [] })]);
    expect(matchSplitHighlights(demo('a.dem', [from]), [p]).uncarried[0].reason).toBe(UNCARRIED_REASON.AMBIGUOUS);
  });

  it('settles a single kill by the piece\'s time offset from the other highlights', () => {
    const anchor = hl([[1000, 100, 'BAR'], [1100, 101, 'BAR']], { victims: [] });
    const from = hl([[3000, 300, 'K98']], { victims: [], curation: 'Keep' });
    const p = piece('p.dem', [
      hl([[100, 10, 'BAR'], [200, 11, 'BAR']], { victims: [] }),
      hl([[1000, 100, 'K98']], { victims: [] }),
      hl([[2100, 210, 'K98']], { victims: [] }),
    ]);
    const result = matchSplitHighlights(demo('a.dem', [anchor, from]), [p]);
    expect(result.carried).toHaveLength(1);
    expect(result.carried[0].to.kills[0][0]).toBe(2100);
  });

  it('only looks in the first map\'s piece', () => {
    const from = hl([[100, 10, 'K98']], { notes: 'n' });
    const result = matchSplitHighlights(demo('a.dem', [from]), [piece('b.dem', [hl([[100, 10, 'K98']])], 1)]);
    expect(result.uncarried[0].reason).toBe(UNCARRIED_REASON.NO_PIECE);
  });

  it('puts two highlights\' work on one piece highlight only once', () => {
    const one = hl([[100, 10, 'K98']], { notes: 'first', victims: [] });
    const two = hl([[100, 10, 'K98']], { notes: 'second', victims: [] });
    const result = matchSplitHighlights(demo('a.dem', [one, two]), [piece('p.dem', [hl([[100, 10, 'K98']], { victims: [] })])]);
    expect(result.carried).toHaveLength(1);
    expect(result.uncarried).toEqual([{ from: two, reason: UNCARRIED_REASON.TAKEN }]);
  });
});

describe('putting the work back', () => {
  it('copies what the user set and the mapped range, and renames the take', () => {
    const kills = [[100, 10, 'K98'], [200, 12, 'K98'], [900, 40, 'BAR'], [1000, 42, 'Colt']];
    const from = hl(kills, {
      start_index: 2, end_index: 3, status: 'Captured', statusByHand: true, selected: true,
      notes: 'n', clipName: 'clip', curation: 'Keep',
    });
    const second = hl(kills.slice(2));
    const original = demo('C:/d/a.dem', [from]);
    const p = piece('C:/d/a_dod_lennon2.dem', [hl(kills.slice(0, 2)), second]);
    const takeIndex = {};
    recordTake(takeIndex, 'sess/chain_01_b0', [streakUid(original.path, from), 'C:/d/other.dem#1#5#5']);

    const result = matchSplitHighlights(original, [p]);
    const renames = applyCarriedWork(original, result.carried);
    renameUidsInTakeIndex(takeIndex, renames);

    expect(second).toMatchObject({
      status: 'Captured', statusByHand: true, selected: true, notes: 'n', clipName: 'clip',
      curation: 'Keep', start_index: 0, end_index: 1,
    });
    expect(resolveTake(takeIndex, 'sess/chain_01_b0')).toEqual([streakUid(p.path, second), 'C:/d/other.dem#1#5#5']);
  });

  it('hasWork counts what a split or a rescan carries', () => {
    expect(hasWork(hl([[1, 1, 'K98']]))).toBe(false);
    expect(hasWork(hl([[1, 1, 'K98']], { status: 'None' }))).toBe(false);
    expect(hasWork(hl([[1, 1, 'K98']], { selected: true }))).toBe(true);
    expect(hasWork(hl([[1, 1, 'K98'], [2, 2, 'K98']], { end_index: 0 }))).toBe(true);
  });
});

describe('the split\'s report', () => {
  it('counts carried and not carried, and logs every one left behind with its work', () => {
    const kept = hl([[100, 10, 'K98']], { curation: 'Keep', viewdemo_times: [70] });
    const lost = hl([[5000, 500, 'Garand'], [5100, 501, 'Garand']], {
      curation: 'Skip', notes: 'too far', status: 'Rendered', viewdemo_times: [125, 126],
    });
    const original = demo('C:/d/a.dem', [kept, lost]);
    const result = matchSplitHighlights(original, [piece('C:/d/a_x.dem', [hl([[100, 10, 'K98']])])]);
    const report = carryReport(original, result, ['C:/d/a_x.dem']);
    expect(report.toast.type).toBe('warning');
    expect(report.toast.text).toContain('1 of 2');
    expect(report.log[0]).toContain('kept the work on 1 of 2');
    expect(report.log).toHaveLength(2);
    expect(report.log[1]).toContain('2:05');
    expect(report.log[1]).toContain('m00cat');
    expect(report.log[1]).toContain('Review: Skip');
    expect(report.log[1]).toContain('Status: Rendered');
    expect(report.log[1]).toContain('Note: "too far"');
  });

  it('says nothing when nothing had work on it', () => {
    const original = demo('a.dem', [hl([[1, 1, 'K98']])]);
    expect(carryReport(original, matchSplitHighlights(original, []), [])).toEqual({ toast: null, log: [] });
  });

  it('a clean carry is a success', () => {
    const from = hl([[100, 10, 'K98']], { notes: 'n' });
    const original = demo('a.dem', [from]);
    const report = carryReport(original, matchSplitHighlights(original, [piece('p.dem', [hl([[100, 10, 'K98']])])]), ['p.dem']);
    expect(report.toast.type).toBe('success');
    expect(report.toast.text).toContain('all 1 highlight');
  });
});
