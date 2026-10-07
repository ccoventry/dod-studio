import { describe, it, expect } from 'vitest';
import { buildReviewQueue, applyReviewAnswer } from './review_queue.js';
import { streakUid } from './take_index.js';

const streak = (overrides = {}) => ({
  player_index: 1,
  kill_count: 3,
  start_tick: 100,
  end_tick: 300,
  kills: [[100, 10, 'K98'], [200, 12, 'K98'], [300, 15, 'Colt']],
  viewdemo_times: [40.5, 42.5, 45.5],
  ...overrides,
});

const demo = (path, streaks) => ({ path, local_player_index: 1, streaks });

describe('Review highlights queue (#623)', () => {
  it('takes only ticked demos, in queue order, each demo by time', () => {
    const late = streak({ kills: [[900, 90, 'K98']], viewdemo_times: [120], kill_count: 1 });
    const early = streak();
    const demos = [demo('a.dem', [late, early]), demo('b.dem', [streak()]), demo('c.dem', [streak()])];
    const { highlights, skipped } = buildReviewQueue(demos, ['c.dem', 'a.dem']);
    expect(skipped).toBe(0);
    expect(highlights.map((h) => [h.demo, h.kill_times[0]])).toEqual([
      ['a.dem', 40.5], ['a.dem', 120], ['c.dem', 40.5],
    ]);
  });

  it('carries the kill range from 1, the note and an earlier answer', () => {
    const s = streak({ start_index: 1, end_index: 2, notes: 'flick', curation: 'Skip' });
    const [h] = buildReviewQueue([demo('a.dem', [s])], ['a.dem']).highlights;
    expect(h).toMatchObject({ from: 2, to: 3, note: 'flick', answered: 'no', kill_times: [40.5, 42.5, 45.5] });
    expect(h.key).toBe(streakUid('a.dem', s));
  });

  it('leaves out other players, short lives and highlights with no demo-player times', () => {
    const demos = [demo('a.dem', [
      streak({ player_index: 2 }),
      streak({ kill_count: 1, kills: [[1, 1, 'K98']], viewdemo_times: [1] }),
      streak({ viewdemo_times: undefined }),
    ])];
    const { highlights, skipped } = buildReviewQueue(demos, ['a.dem'], 2);
    expect(highlights).toEqual([]);
    expect(skipped).toBe(1);
  });
});

describe('Review highlights answers (#623)', () => {
  const answerFor = (d, s, overrides) => ({
    demo: d.path, key: streakUid(d.path, s), verdict: 'yes', from: 2, to: 3, note: 'nice', ...overrides,
  });

  it('Yes marks Keep with the kill range and note, and leaves Status and the tick alone', () => {
    const s = streak({ status: 'None' });
    const d = demo('a.dem', [s]);
    expect(applyReviewAnswer([d], answerFor(d, s))).toBe(s);
    expect(s).toMatchObject({ curation: 'Keep', status: 'None', start_index: 1, end_index: 2, notes: 'nice' });
    expect(s.selected).toBeUndefined();
    expect(s.statusByHand).toBeUndefined();
  });

  it('Yes ticks the row only when asked to', () => {
    const s = streak();
    const d = demo('a.dem', [s]);
    applyReviewAnswer([d], answerFor(d, s), { tickYes: true });
    expect(s.selected).toBe(true);
  });

  it('No marks Skip and unticks, even with tick Yes on, and keeps a Captured status', () => {
    const s = streak({ status: 'Captured', selected: true });
    const d = demo('a.dem', [s]);
    applyReviewAnswer([d], answerFor(d, s, { verdict: 'no' }), { tickYes: true });
    expect(s).toMatchObject({ curation: 'Skip', selected: false, status: 'Captured' });
  });

  it('a mark set in either place is the answer the game is sent', () => {
    const kept = streak({ curation: 'Keep' });
    const fresh = streak({ kills: [[5, 1, 'K98'], [6, 2, 'K98'], [7, 3, 'K98']] });
    const answered = buildReviewQueue([demo('a.dem', [kept, fresh])], ['a.dem']).highlights.map((h) => h.answered);
    expect(answered).toEqual(['yes', null]);
  });

  it('a range past the kills is pulled in', () => {
    const s = streak();
    const d = demo('a.dem', [s]);
    applyReviewAnswer([d], answerFor(d, s, { from: 3, to: 9 }));
    expect([s.start_index, s.end_index]).toEqual([2, 2]);
  });

  it('an answer for a highlight no longer in the queue changes nothing', () => {
    const s = streak();
    const d = demo('a.dem', [s]);
    expect(applyReviewAnswer([d], { ...answerFor(d, s), demo: 'gone.dem' })).toBeNull();
    expect(applyReviewAnswer([d], { ...answerFor(d, s), key: 'nope' })).toBeNull();
    expect(s.curation).toBeUndefined();
  });
});
