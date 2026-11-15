import { describe, it, expect } from 'vitest';
import { matchRescanHighlights, applyRescanWork, rescanReport } from './rescan_carry.js';
import { UNCARRIED_REASON } from './split_carry.js';
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
const PATH = 'C:/demos/a.dem';
const demo = (streaks) => ({ path: PATH, map_name: 'dod_anzio', tickrate: 100, streaks });

describe('carrying work across a rescan', () => {
  it('keeps the work on a highlight whose uid is unchanged, range as it was', () => {
    const kills = [[100, 1, 'K98'], [200, 3, 'K98'], [300, 5, 'BAR']];
    const from = hl(kills, { notes: 'n', curation: 'Keep', start_index: 1, end_index: 2 });
    const to = hl(kills);
    const result = matchRescanHighlights(demo([from]), demo([to]));
    expect(result.carried).toEqual([expect.objectContaining({ from, to, exact: true, start_index: 1, end_index: 2 })]);
    expect(applyRescanWork(demo([from]), result).size).toBe(0);
    expect(to).toMatchObject({ notes: 'n', curation: 'Keep', start_index: 1, end_index: 2 });
  });

  it('a life split in two: work on kills 2-4 moves to the 3-kill highlight, range remapped', () => {
    // Older detection: one 4-kill life opening with a grenade. Newer: the
    // grenade kill is its own highlight, so neither new uid is the old one.
    const kills = [[100, 10, 'Grenade'], [400, 13, 'K98'], [600, 15, 'K98'], [800, 17.5, 'BAR']];
    const from = hl(kills, { start_index: 1, end_index: 3, notes: 'the triple', curation: 'Keep', status: 'Captured' });
    const nade = hl(kills.slice(0, 1));
    const triple = hl(kills.slice(1));
    const previous = demo([from]);
    const fresh = demo([nade, triple]);
    const takeIndex = {};
    recordTake(takeIndex, 'sess/chain_01_b0', [streakUid(PATH, from)]);

    const result = matchRescanHighlights(previous, fresh);
    expect(result.uncarried).toEqual([]);
    expect(result.carried).toEqual([expect.objectContaining({ from, to: triple, start_index: 0, end_index: 2, narrowed: false })]);
    expect(result.carried[0].exact).toBeUndefined();
    renameUidsInTakeIndex(takeIndex, applyRescanWork(previous, result));
    expect(triple).toMatchObject({ notes: 'the triple', curation: 'Keep', status: 'Captured', start_index: 0, end_index: 2 });
    expect(nade.notes).toBeUndefined();
    expect(resolveTake(takeIndex, 'sess/chain_01_b0')).toEqual([streakUid(PATH, triple)]);
  });

  it('a whole-life range over the split lands on the bigger part and is logged as narrowed', () => {
    const kills = [[100, 10, 'Grenade'], [400, 13, 'K98'], [600, 15, 'K98'], [800, 17.5, 'BAR']];
    const from = hl(kills, { notes: 'x' });
    const triple = hl(kills.slice(1));
    const previous = demo([from]);
    const result = matchRescanHighlights(previous, demo([hl(kills.slice(0, 1)), triple]));
    expect(result.carried[0]).toMatchObject({ to: triple, start_index: 0, end_index: 2, narrowed: true });
    const report = rescanReport([{ previous, result }]);
    expect(report.toast.type).toBe('success');
    expect(report.log.some((l) => l.includes('splits its Kill Range over two highlights'))).toBe(true);
  });

  it('never guesses: kills the new scan lacks are reported, with all their work', () => {
    const from = hl([[100, 10, 'K98'], [300, 12, 'K98']], { notes: 'gone', curation: 'Maybe' });
    const other = hl([[100, 10, 'K98'], [320, 12.2, 'Colt']]);
    const previous = demo([from]);
    const result = matchRescanHighlights(previous, demo([other]));
    expect(result.carried).toEqual([]);
    expect(result.uncarried).toEqual([{ from, reason: UNCARRIED_REASON.NOT_FOUND }]);
    expect(other.notes).toBeUndefined();
    const report = rescanReport([{ previous, result }]);
    expect(report.toast).toEqual({
      text: expect.stringContaining('kept the work on 0 of 1 highlights'),
      type: 'warning',
    });
    expect(report.log[0]).toBe('[rescan] kept the work on 0 of 1 highlight(s) across 1 demo(s)');
    expect(report.log[1]).toBe('[rescan] a.dem:');
    expect(report.log[2]).toContain('NOT KEPT');
    expect(report.log[2]).toContain('no highlight in the new scan has the same player and kills');
    expect(report.log[2]).toContain('Review: Maybe');
    expect(report.log[2]).toContain('Note: "gone"');
  });

  it('two highlights with the same pattern are not guessed', () => {
    const one = [[100, 10, 'K98']];
    const from = hl(one, { notes: 'which?' });
    const result = matchRescanHighlights(demo([from]), demo([hl([[150, 10, 'K98']]), hl([[900, 80, 'K98']])]));
    expect(result.uncarried.map((u) => u.reason)).toEqual([UNCARRIED_REASON.AMBIGUOUS]);
  });

  it('an exact match is not offered to the kill matcher again', () => {
    const kills = [[100, 10, 'K98'], [300, 12, 'K98']];
    const a = hl(kills, { notes: 'a' });
    const to = hl(kills);
    const result = matchRescanHighlights(demo([a]), demo([to]));
    expect(result.carried).toHaveLength(1);
    expect(result.uncarried).toEqual([]);
  });

  it('no toast and no log when nothing rescanned had work on it', () => {
    const result = matchRescanHighlights(demo([hl([[1, 1, 'K98']])]), demo([hl([[1, 1, 'K98']])]));
    expect(rescanReport([{ previous: demo([]), result }])).toEqual({ toast: null, log: [] });
  });

  it('one toast over every rescanned demo', () => {
    const k = [[100, 10, 'K98'], [300, 12, 'K98']];
    const reports = [1, 2].map(() => {
      const previous = demo([hl(k, { notes: 'n' })]);
      return { previous, result: matchRescanHighlights(previous, demo([hl(k)])) };
    });
    expect(rescanReport(reports).toast).toEqual({
      text: 'Rescan kept the marks, notes, status and kill ranges of all 2 highlights with work on them.',
      type: 'success',
    });
  });
});
