import { describe, it, expect, vi } from 'vitest';
import { capturedTakes, applyCaughtUpBatch, createBatchCatchUp, isBatchStillRunningInGame } from './batch_catch_up.js';
import { streakUid } from './take_index.js';
import { HIGHLIGHT_STATUS } from './status_colors.js';

const streak = (player, first, last, extra = {}) => ({
  player_index: player,
  kills: [[first], [last]],
  start_tick: first,
  end_tick: last,
  ...extra,
});

const demos = () => [
  { path: 'a.dem', streaks: [streak(1, 10, 20), streak(2, 30, 40), streak(3, 50, 60)] },
  { path: 'b.dem', streaks: [streak(1, 70, 80)] },
];

const uid = (path, s) => streakUid(path, { ...s });

// A batch over a.dem's first two highlights (merged into one take) and
// b.dem's, whose take is missing.
function batch(d, extra = {}) {
  return {
    session_id: 'session_x',
    project_path: null,
    streak_uids: [uid('a.dem', d[0].streaks[0]), uid('a.dem', d[0].streaks[1]), uid('b.dem', d[1].streaks[0])],
    blocks: [
      { take_key: 'session_x/chain_01_b0', captured: true, source_streak_indices: [0, 1] },
      { take_key: 'session_x/chain_02_b0', captured: false, source_streak_indices: [2] },
    ],
    ...extra,
  };
}

describe('capturedTakes', () => {
  it('maps each captured block to the highlights it covers', () => {
    const d = demos();
    expect(capturedTakes(batch(d))).toMatchObject([
      { takeKey: 'session_x/chain_01_b0', uids: [uid('a.dem', d[0].streaks[0]), uid('a.dem', d[0].streaks[1])] },
    ]);
  });

  it('skips a block whose indices are out of range', () => {
    const b = { streak_uids: ['u0'], blocks: [{ take_key: 'k', captured: true, source_streak_indices: [5] }] };
    expect(capturedTakes(b)).toEqual([]);
  });
});

describe('applyCaughtUpBatch', () => {
  it('marks the captured highlights and records the take', () => {
    const d = demos();
    const takeIndex = {};
    const result = applyCaughtUpBatch(batch(d), d, takeIndex);
    expect(result).toEqual({ captured: 1, total: 2, marked: 2, changed: true });
    expect(d[0].streaks.map((s) => s.status)).toEqual([HIGHLIGHT_STATUS.CAPTURED, HIGHLIGHT_STATUS.CAPTURED, undefined]);
    expect(d[1].streaks[0].status).toBeUndefined();
    expect(Object.keys(takeIndex)).toEqual(['session_x/chain_01_b0']);
  });

  it('keeps a verified Rendered and overrules a hand-set one', () => {
    const d = demos();
    d[0].streaks[0].status = HIGHLIGHT_STATUS.RENDERED;
    d[0].streaks[1].status = HIGHLIGHT_STATUS.RENDERED;
    d[0].streaks[1].statusByHand = true;
    applyCaughtUpBatch(batch(d), d, {});
    expect(d[0].streaks[0].status).toBe(HIGHLIGHT_STATUS.RENDERED);
    expect(d[0].streaks[1].status).toBe(HIGHLIGHT_STATUS.CAPTURED);
    expect(d[0].streaks[1].statusByHand).toBeUndefined();
  });
});

const fakeResume = () => ({ offer: vi.fn(async () => {}) });

function fakeIo(overrides = {}) {
  return {
    catchUp: vi.fn(async () => ({ batches: [], waiting_for_game: false })),
    readProject: vi.fn(),
    writeProject: vi.fn(async () => {}),
    listen: vi.fn(async () => () => {}),
    toast: vi.fn(),
    ...overrides,
  };
}

describe('createBatchCatchUp', () => {
  it('marks a batch with no project in the open queue', async () => {
    const d = demos();
    const onStatusChange = vi.fn();
    const io = fakeIo({ catchUp: vi.fn(async () => ({ batches: [batch(d)], waiting_for_game: false })) });
    const takeIndex = {};
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => d, getTakeIndex: () => takeIndex, getSessionPath: () => null, onStatusChange, io });
    await c.start();
    expect(d[0].streaks[0].status).toBe(HIGHLIGHT_STATUS.CAPTURED);
    expect(onStatusChange).toHaveBeenCalledTimes(1);
    expect(io.toast).toHaveBeenCalledWith(expect.stringContaining('1 of 2 clips'), 'info');
    expect(io.listen).toHaveBeenCalledWith('batch_catch_up', expect.any(Function));
  });

  it('marks a batch in its project file when that project is not open', async () => {
    const saved = demos();
    const file = JSON.stringify({ version: '0.12.0', demos: saved });
    const io = fakeIo({ readProject: vi.fn(async () => file) });
    const onStatusChange = vi.fn();
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => [], getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange, io });
    await c.applyBatches([batch(saved, { project_path: 'C:/projects/match.json' })]);

    expect(io.readProject).toHaveBeenCalledWith('C:/projects/match.json');
    const [path, written] = io.writeProject.mock.calls[0];
    expect(path).toBe('C:/projects/match.json');
    const data = JSON.parse(written);
    expect(data.demos[0].streaks[0].status).toBe(HIGHLIGHT_STATUS.CAPTURED);
    expect(Object.keys(data.takeIndex)).toEqual(['session_x/chain_01_b0']);
    expect(onStatusChange).not.toHaveBeenCalled();
    expect(io.toast).toHaveBeenCalledWith(expect.stringContaining('match.json'), 'info');
  });

  it('marks the open queue when the batch came from the open project', async () => {
    const d = demos();
    const io = fakeIo();
    const onStatusChange = vi.fn();
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => d, getTakeIndex: () => ({}), getSessionPath: () => 'p.json', onStatusChange, io });
    await c.applyBatches([batch(d, { project_path: 'p.json' })]);
    expect(io.readProject).not.toHaveBeenCalled();
    expect(d[0].streaks[1].status).toBe(HIGHLIGHT_STATUS.CAPTURED);
    expect(onStatusChange).toHaveBeenCalled();
  });

  it('says so when the project file cannot be read', async () => {
    const io = fakeIo({ readProject: vi.fn(async () => { throw new Error('gone'); }) });
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => [], getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io });
    await c.applyBatches([batch(demos(), { project_path: 'gone.json' })]);
    expect(io.writeProject).not.toHaveBeenCalled();
    expect(io.toast).toHaveBeenCalledWith(expect.stringContaining('Could not mark them in the project gone.json'), 'error');
  });

  it('toasts when the check itself fails', async () => {
    const io = fakeIo({ catchUp: vi.fn(async () => { throw 'boom'; }) });
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => [], getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io });
    await c.start();
    expect(io.toast).toHaveBeenCalledWith(expect.stringContaining('boom'), 'error');
  });
  it('offers the clips the newest unfinished batch never got to, once', async () => {
    const d = demos();
    const resume = fakeResume();
    const done = { ...batch(d, { session_id: 'session_y' }), blocks: batch(d).blocks.map((b) => ({ ...b, captured: true })) };
    const c = createBatchCatchUp({ resume, getDemos: () => d, getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io: fakeIo() });
    const unfinished = batch(d);
    await c.applyBatches([unfinished, done]);
    expect(resume.offer).toHaveBeenCalledTimes(1);
    expect(resume.offer).toHaveBeenCalledWith(unfinished, expect.objectContaining({ captured: 1, total: 2 }));
  });

  it('offers nothing when every clip is on disk', async () => {
    const d = demos();
    const resume = fakeResume();
    const done = { ...batch(d), blocks: batch(d).blocks.map((b) => ({ ...b, captured: true })) };
    const c = createBatchCatchUp({ resume, getDemos: () => d, getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io: fakeIo() });
    await c.applyBatches([done]);
    expect(resume.offer).not.toHaveBeenCalled();
  });

  it('holds Start while a game may still be capturing, until the check after it closes', async () => {
    let onEvent = null;
    const io = fakeIo({
      catchUp: vi.fn(async () => ({ batches: [], waiting_for_game: true })),
      listen: vi.fn(async (_name, handler) => { onEvent = handler; return () => {}; }),
    });
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => [], getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io });
    await c.start();
    expect(isBatchStillRunningInGame()).toBe(true);
    expect(io.toast).toHaveBeenCalledWith(expect.stringContaining('may still be capturing'), 'info', expect.any(Number));
    await onEvent({ payload: [] });
    expect(isBatchStillRunningInGame()).toBe(false);
  });

  it('does not hold Start when the check after the game came before the reply', async () => {
    let onEvent = null;
    const io = fakeIo({
      listen: vi.fn(async (_name, handler) => { onEvent = handler; return () => {}; }),
      catchUp: vi.fn(async () => {
        await onEvent({ payload: [] });
        return { batches: [], waiting_for_game: true };
      }),
    });
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => [], getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io });
    await c.start();
    expect(isBatchStillRunningInGame()).toBe(false);
  });

  it('does not hold Start when it could not listen for the end of the wait', async () => {
    const io = fakeIo({
      listen: vi.fn(async () => { throw new Error('no events'); }),
      catchUp: vi.fn(async () => ({ batches: [], waiting_for_game: true })),
    });
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    const c = createBatchCatchUp({ resume: fakeResume(), getDemos: () => [], getTakeIndex: () => ({}), getSessionPath: () => null, onStatusChange: vi.fn(), io });
    await c.start();
    err.mockRestore();
    expect(isBatchStillRunningInGame()).toBe(false);
  });
});

describe('a caught-up batch keeps how it was captured (#439)', () => {
  it('records the batch settings and the take folder, but no capture time it cannot know', () => {
    const d = demos();
    const takeIndex = {};
    const b = batch(d, { session_id: 'session_x', capture_mode: 'agr', capture_fps: 300, agr_fps: 120 });
    b.blocks[0].take_folder = 'D:/caps/session_x/chain_01_b0';
    applyCaughtUpBatch(b, d, takeIndex);
    const record = takeIndex['session_x/chain_01_b0'];
    expect(record).toMatchObject({ session_id: 'session_x', capture_mode: 'agr', capture_fps: 120, take_folder: 'D:/caps/session_x/chain_01_b0' });
    expect(record.captured_at).toBeUndefined();
  });
});
