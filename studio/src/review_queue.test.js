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
    const s = streak({ start_index: 1, end_index: 2, notes: 'flick', review: 'no' });
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

  it('Yes sets Pending by hand, the kill range and the note', () => {
    const s = streak();
    const d = demo('a.dem', [s]);
    expect(applyReviewAnswer([d], answerFor(d, s))).toBe(s);
    expect(s).toMatchObject({ status: 'Pending', statusByHand: true, start_index: 1, end_index: 2, notes: 'nice', review: 'yes' });
  });

  it('No takes a Pending back to None but leaves a Captured row alone', () => {
    const pending = streak({ status: 'Pending' });
    const captured = streak({ status: 'Captured', kills: [[5, 1, 'K98'], [6, 2, 'K98'], [7, 3, 'K98']] });
    const d = demo('a.dem', [pending, captured]);
    applyReviewAnswer([d], answerFor(d, pending, { verdict: 'no' }));
    applyReviewAnswer([d], answerFor(d, captured, { verdict: 'no' }));
    expect(pending.status).toBe('None');
    expect(captured.status).toBe('Captured');
    expect(captured.review).toBe('no');
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
    expect(s.review).toBeUndefined();
  });
});
