import { describe, it, expect, vi } from 'vitest';
import { uncapturedUids, countListed, applySelectOnly, createBatchResume } from './batch_resume.js';
import { streakUid } from './take_index.js';

const streak = (player, first, last, extra = {}) => ({
  player_index: player,
  kills: [[first], [last]],
  start_tick: first,
  end_tick: last,
  ...extra,
});

const demos = () => [
  { path: 'a.dem', streaks: [streak(1, 10, 20, { selected: true }), streak(2, 30, 40), streak(3, 50, 60, { selected: true })] },
  { path: 'b.dem', streaks: [streak(1, 70, 80, { selected: true })] },
];

// A batch over a.dem's first highlight (captured), a.dem's third and b.dem's
// (one block each, neither captured).
function batch(d, extra = {}) {
  return {
    session_id: 'session_x',
    project_path: null,
    streak_uids: [streakUid('a.dem', d[0].streaks[0]), streakUid('a.dem', d[0].streaks[2]), streakUid('b.dem', d[1].streaks[0])],
    blocks: [
      { take_key: 'session_x/chain_01_b0', captured: true, source_streak_indices: [0] },
      { take_key: 'session_x/chain_01_b1', captured: false, source_streak_indices: [1] },
      { take_key: 'session_x/chain_02_b0', captured: false, source_streak_indices: [2, 5] },
    ],
    ...extra,
  };
}

const result = { captured: 1, total: 3 };

describe('uncapturedUids', () => {
  it('lists the highlights of the blocks with no take, skipping indices out of range', () => {
    const d = demos();
    expect(uncapturedUids(batch(d))).toEqual([streakUid('a.dem', d[0].streaks[2]), streakUid('b.dem', d[1].streaks[0])]);
  });

  it('is empty when every block was captured', () => {
    const d = demos();
    const b = batch(d);
    b.blocks.forEach((block) => { block.captured = true; });
    expect(uncapturedUids(b)).toEqual([]);
  });
});

describe('applySelectOnly', () => {
  it('ticks exactly the given highlights and unticks the rest', () => {
    const d = demos();
    const ticked = applySelectOnly(d, uncapturedUids(batch(d)));
    expect(ticked).toBe(2);
    expect(d[0].streaks.map((s) => s.selected)).toEqual([false, false, true]);
    expect(d[1].streaks[0].selected).toBe(true);
  });

  it('counts how many of the ids the queue lists', () => {
    const d = demos();
    expect(countListed(d, uncapturedUids(batch(d)))).toBe(2);
    expect(countListed([], uncapturedUids(batch(d)))).toBe(0);
  });
});

function setup({ confirm = true, sessionPath = null, opens = true, queue = demos() } = {}) {
  const app = {
    queue,
    openProject: vi.fn(async () => opens),
    getSessionPath: () => sessionPath,
    getDemos: () => app.queue,
    onSelectionChange: vi.fn(),
    startCapture: vi.fn(async () => {}),
    io: { confirm: vi.fn(async () => confirm), toast: vi.fn(), showHighlights: vi.fn() },
  };
  return { app, resume: createBatchResume(app) };
}

describe('createBatchResume', () => {
  it('on yes, opens the batch project, ticks the missing highlights and starts the batch', async () => {
    const saved = demos();
    const { app, resume } = setup({ queue: [] });
    app.openProject.mockImplementation(async () => { app.queue = saved; return true; });
    await resume.offer(batch(saved, { project_path: 'C:/projects/match.dodstudio' }), result);

    expect(app.io.confirm).toHaveBeenCalledWith(expect.stringContaining('match.dodstudio'), expect.objectContaining({ confirmLabel: 'Capture them now' }));
    expect(app.io.confirm.mock.calls[0][0]).toContain('1 of 3 clips are on disk');
    expect(app.io.confirm.mock.calls[0][0]).toContain('2 clips were not captured');
    expect(app.openProject).toHaveBeenCalledWith('C:/projects/match.dodstudio');
    expect(saved[0].streaks.map((s) => s.selected)).toEqual([false, false, true]);
    expect(app.onSelectionChange).toHaveBeenCalled();
    expect(app.startCapture).toHaveBeenCalledTimes(1);
  });

  it('does not reopen the project that is already open', async () => {
    const d = demos();
    const { app, resume } = setup({ queue: d, sessionPath: 'p.dodstudio' });
    await resume.offer(batch(d, { project_path: 'p.dodstudio' }), result);
    expect(app.openProject).not.toHaveBeenCalled();
    expect(app.startCapture).toHaveBeenCalled();
  });

  it('changes nothing on Not now', async () => {
    const d = demos();
    const { app, resume } = setup({ confirm: false, queue: d });
    await resume.offer(batch(d, { project_path: 'p.dodstudio' }), result);
    expect(app.openProject).not.toHaveBeenCalled();
    expect(d[0].streaks[0].selected).toBe(true);
    expect(app.startCapture).not.toHaveBeenCalled();
  });

  it('starts nothing when the project did not open', async () => {
    const d = demos();
    const { app, resume } = setup({ opens: false, queue: [] });
    await resume.offer(batch(d, { project_path: 'p.dodstudio' }), result);
    expect(app.startCapture).not.toHaveBeenCalled();
  });

  it('does not ask about a batch with no project whose highlights are not in the queue', async () => {
    const d = demos();
    const { app, resume } = setup({ queue: [] });
    await resume.offer(batch(d), result);
    expect(app.io.confirm).not.toHaveBeenCalled();
  });

  it('says so when the opened project no longer has those highlights', async () => {
    const d = demos();
    const { app, resume } = setup({ queue: [] });
    await resume.offer(batch(d, { project_path: 'p.dodstudio' }), result);
    expect(app.io.toast).toHaveBeenCalledWith(expect.stringContaining('Could not find'), 'error');
    expect(app.startCapture).not.toHaveBeenCalled();
  });
});

describe('the question', () => {
  it('says plainly when none of the clips are on disk', async () => {
    const d = demos();
    const { app, resume } = setup({ queue: d, sessionPath: 'p.dodstudio' });
    await resume.offer(batch(d, { project_path: 'p.dodstudio' }), { captured: 0, total: 3 });
    expect(app.io.confirm.mock.calls[0][0]).toContain('none of its 3 clips are on disk');
  });
});
