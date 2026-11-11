import { describe, it, expect } from 'vitest';
import { buildReviewQueue, applyReviewAnswer, applyReviewEdit, fastForwardGap, MIN_FAST_FORWARD_GAP, REVIEW_COVERS, coveredByReview, reviewDemoPaths, hltvPlayersFor, HLTV_PLAYERS, REVIEW_ORDER, reviewKills } from './review_queue.js';
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

describe('HLTV demos in a review (#822)', () => {
  const life = (player, index, at, extra = {}) =>
    streak({ target_player: player, player_index: index, kills: [[at, at, 'K98']], viewdemo_times: [at], kill_count: 1, ...extra });
  const hltv = {
    path: 'h.dem', is_pov: false, local_player_index: null,
    streaks: [
      life('m00cat', 4, 10),
      life('DICE | stealth', 6, 20, { steam_id: '76561197960265729', spectated: true }),
      life('gskiLL greetz', 9, 30),
    ],
  };
  const pov = demo('p.dem', [streak()]);
  const recorders = [
    { id: '76561197960265729', names: ['stealth'], label: 'stealth' },
    { id: '76561197960265730', names: ['m00cat'], label: 'm00cat' },
  ];
  const queue = (players) => buildReviewQueue([hltv, pov], ['h.dem', 'p.dem'], REVIEW_COVERS.ALL, players).highlights;

  it('plays only the chosen players, matched by SteamID or else by name, and every POV highlight', () => {
    const rows = queue(hltvPlayersFor(HLTV_PLAYERS.RECORDERS, recorders));
    expect(rows.map((h) => [h.demo, h.player])).toEqual([['h.dem', 'm00cat'], ['h.dem', 'DICE | stealth'], ['p.dem', '']]);
  });

  describe('order', () => {
    // gorilla 2:10, m00cat 2:15 (inside gorilla's clip), gorilla 3:00,
    // and Alpha at the same moment as gorilla's first.
    const mixed = {
      path: 'o.dem', is_pov: false, streaks: [
        life('m00cat', 1, 135), life('gorilla', 2, 130), life('gorilla', 2, 180), life('Alpha', 3, 130),
      ],
    };
    const played = (order) => buildReviewQueue([mixed], ['o.dem'], REVIEW_COVERS.ALL, null, order)
      .highlights.map((h) => `${h.player}@${h.kill_times[0]}`);

    it('chronological by default: a later clip starting earlier than one ends still goes by its start, ties A to Z', () => {
      expect(played(undefined)).toEqual(['Alpha@130', 'gorilla@130', 'm00cat@135', 'gorilla@180']);
      expect(played(REVIEW_ORDER.TIME)).toEqual(played(undefined));
    });

    it('by player: A to Z regardless of case, each player by time', () => {
      expect(played(REVIEW_ORDER.PLAYER)).toEqual(['Alpha@130', 'gorilla@130', 'gorilla@180', 'm00cat@135']);
    });
  });

  it('sends the camera player numbered from 1, and none for a POV demo', () => {
    const rows = queue(null);
    expect(rows.map((h) => h.spectate)).toEqual([5, 7, 10, null]);
  });

  it('the box: everyone, every recorder, or one', () => {
    expect(hltvPlayersFor(HLTV_PLAYERS.ALL, recorders)).toBeNull();
    expect(hltvPlayersFor(HLTV_PLAYERS.RECORDERS, recorders)).toEqual(recorders);
    expect(queue(hltvPlayersFor('76561197960265730', recorders)).map((h) => h.player)).toEqual(['m00cat', '']);
  });

  describe('the default: players missing their POV for this half', () => {
    const half = (match, h) => ({ match, half: h });
    const withInfo = (d, info) => Object.assign({ ...d }, { match_info: info });
    // stealth recorded this half; m00cat recorded only another one.
    const here = withInfo(hltv, half(2, 1));
    const stealthHere = withInfo(demo('s1.dem', [streak()]), half(2, 1));
    const m00catElsewhere = withInfo(demo('m2.dem', [streak()]), half(3, 2));
    const demos = [here, stealthHere, m00catElsewhere];
    const recorderOf = (d) => ({ 's1.dem': { id: '76561197960265729' }, 'm2.dem': { id: '76561197960265730' } })[d.path] || null;
    const players = (choice) =>
      buildReviewQueue(demos, ['h.dem'], REVIEW_COVERS.ALL, hltvPlayersFor(choice, recorders, { demos, recorderOf }))
        .highlights.map((h) => h.player);

    it('is what an empty box means, and leaves out whoever recorded this half', () => {
      expect(players(HLTV_PLAYERS.MISSING)).toEqual(['m00cat']);
      expect(players('')).toEqual(['m00cat']);
      expect(players(HLTV_PLAYERS.RECORDERS)).toEqual(['m00cat', 'DICE | stealth']);
    });

    it('counts what the box left out, so an empty review can say why', () => {
      const result = buildReviewQueue(demos, ['h.dem'], REVIEW_COVERS.ALL,
        hltvPlayersFor(HLTV_PLAYERS.MISSING, recorders, { demos, recorderOf }));
      expect(result.notChosen).toBe(2);
    });

    it('an HLTV demo whose half is not known reviews every recorder', () => {
      const unplaced = [withInfo(hltv, null), stealthHere, m00catElsewhere];
      const rows = buildReviewQueue(unplaced, ['h.dem'], REVIEW_COVERS.ALL,
        hltvPlayersFor(HLTV_PLAYERS.MISSING, recorders, { demos: unplaced, recorderOf })).highlights;
      expect(rows.map((h) => h.player)).toEqual(['m00cat', 'DICE | stealth']);
    });
  });
});

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

  it("carries each kill's victim and weapon for the game's kill list (#1140)", () => {
    const s = streak({ kills: [[10, 1, 'K98'], [20, 2, 'Colt']], viewdemo_times: [5, 6], victims: ['gskiLL'] });
    const [h] = buildReviewQueue([demo('a.dem', [s])], ['a.dem']).highlights;
    expect(h.kills).toEqual([{ victim: 'gskiLL', weapon: 'K98' }, { victim: '', weapon: 'Colt' }]);
    expect(reviewKills({})).toEqual([]);
  });

  it('leaves out highlights with no demo-player times', () => {
    const demos = [demo('a.dem', [
      streak({ kill_count: 1, kills: [[1, 1, 'K98']], viewdemo_times: [1] }),
      streak({ viewdemo_times: undefined }),
    ])];
    const { highlights, skipped } = buildReviewQueue(demos, ['a.dem']);
    expect(highlights.map((h) => h.kill_times)).toEqual([[1]]);
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

  it('Maybe marks Maybe, never ticks, and leaves an earlier tick alone (#661)', () => {
    const fresh = streak();
    const ticked = streak({ kills: [[5, 1, 'K98'], [6, 2, 'K98'], [7, 3, 'K98']], selected: true });
    const d = demo('a.dem', [fresh, ticked]);
    applyReviewAnswer([d], answerFor(d, fresh, { verdict: 'maybe' }), { tickYes: true });
    applyReviewAnswer([d], answerFor(d, ticked, { verdict: 'maybe' }), { tickYes: true });
    expect(fresh).toMatchObject({ curation: 'Maybe', notes: 'nice' });
    expect(fresh.selected).toBeUndefined();
    expect(ticked).toMatchObject({ curation: 'Maybe', selected: true });
    expect(buildReviewQueue([d], ['a.dem']).highlights.map((h) => h.answered)).toEqual(['maybe', 'maybe']);
  });

  it('a range past the kills is pulled in', () => {
    const s = streak();
    const d = demo('a.dem', [s]);
    applyReviewAnswer([d], answerFor(d, s, { from: 3, to: 9 }));
    expect([s.start_index, s.end_index]).toEqual([2, 2]);
  });

  it("the game's kill range and note form sets both and leaves the mark and tick alone (#1139)", () => {
    const s = streak({ curation: 'Maybe', selected: true, notes: 'old' });
    const d = demo('a.dem', [s]);
    const { verdict: _, ...edit } = answerFor(d, s, { note: 'the flick' });
    expect(applyReviewEdit([d], edit)).toBe(s);
    expect(s).toMatchObject({ curation: 'Maybe', selected: true, start_index: 1, end_index: 2, notes: 'the flick' });
    expect(applyReviewEdit([d], { ...edit, key: 'nope' })).toBeNull();
  });

  it('an answer for a highlight no longer in the queue changes nothing', () => {
    const s = streak();
    const d = demo('a.dem', [s]);
    expect(applyReviewAnswer([d], { ...answerFor(d, s), demo: 'gone.dem' })).toBeNull();
    expect(applyReviewAnswer([d], { ...answerFor(d, s), key: 'nope' })).toBeNull();
    expect(s.curation).toBeUndefined();
  });
});

describe('What a review covers (#661)', () => {
  const marked = (curation, at) => streak({ kills: [[at, 1, 'K98']], viewdemo_times: [at], kill_count: 1, curation });
  const d = demo('a.dem', [marked('Keep', 10), marked(undefined, 20), marked('Maybe', 30), marked('Skip', 40)]);
  const times = (covers) => buildReviewQueue([d], ['a.dem'], covers).highlights.map((h) => h.kill_times[0]);

  it('All plays every highlight, whatever its mark', () => {
    expect(times(REVIEW_COVERS.ALL)).toEqual([10, 20, 30, 40]);
    expect(times(undefined)).toEqual([10, 20, 30, 40]);
  });

  it('each mark, or no mark, picks just those rows', () => {
    expect(times(REVIEW_COVERS.UNMARKED)).toEqual([20]);
    expect(times(REVIEW_COVERS.KEEP)).toEqual([10]);
    expect(times(REVIEW_COVERS.MAYBE)).toEqual([30]);
    expect(times(REVIEW_COVERS.SKIP)).toEqual([40]);
  });

  it('ticked demos win; with none ticked, the shown demos are reviewed', () => {
    expect(reviewDemoPaths(['b.dem'], ['a.dem', 'b.dem'])).toEqual(['b.dem']);
    expect(reviewDemoPaths([], ['a.dem', 'b.dem'])).toEqual(['a.dem', 'b.dem']);
    expect(reviewDemoPaths(undefined, undefined)).toEqual([]);
  });

  it('coveredByReview agrees with the queue', () => {
    expect(coveredByReview({ curation: 'Maybe' }, REVIEW_COVERS.MAYBE)).toBe(true);
    expect(coveredByReview({ curation: 'Maybe' }, REVIEW_COVERS.UNMARKED)).toBe(false);
    expect(coveredByReview({}, REVIEW_COVERS.UNMARKED)).toBe(true);
    expect(coveredByReview({ curation: 'Skip' }, 'something new')).toBe(true);
  });
});

describe('Review highlights fast-forward setting (#665)', () => {
  it('sends nothing when the box is off', () => {
    expect(fastForwardGap(false, '8')).toBeNull();
  });

  it('sends the seconds typed, held to 7..120', () => {
    expect(fastForwardGap(true, '12')).toBe(12);
    expect(fastForwardGap(true, '3')).toBe(MIN_FAST_FORWARD_GAP);
    expect(fastForwardGap(true, '999')).toBe(120);
  });

  it('falls back to 8 for a blank or unreadable box', () => {
    expect(fastForwardGap(true, '')).toBe(8);
    expect(fastForwardGap(true, undefined)).toBe(8);
    expect(fastForwardGap(true, 'abc')).toBe(8);
  });
});
