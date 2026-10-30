import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({ gameReviewProgress: vi.fn(), gameReviewFinished: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { handleGameReviewRequest, findQueuedDemo, needsRescan, startFromOf } = await import('./review_from_game.js');
const { REVIEW_COVERS } = await import('./review_queue.js');
const { STRINGS } = await import('./strings.js');

const scanned = (path) => ({ path, streaks: [{ viewdemo_times: [5], kills: [[1, 5, 'K98']] }] });

function deps(demos, over = {}) {
  const told = [];
  return {
    told,
    getDemos: () => demos,
    scan: vi.fn(async () => {}),
    isScanning: () => false,
    startReview: vi.fn(async () => null),
    progress: async (text) => told.push(['progress', text]),
    finish: async (error) => told.push(['finish', error]),
    ...over,
  };
}

describe('A review the game asks for (#1142)', () => {
  it('finds a queued demo whatever its slashes and case', () => {
    const demos = [scanned('C:\\Steam\\dod\\A.dem')];
    expect(findQueuedDemo(demos, 'c:/steam/dod/a.dem')).toBe(demos[0]);
    expect(findQueuedDemo(demos, 'C:/other.dem')).toBe(null);
  });

  it('reads where to start: the first highlight, or from a demo time', () => {
    expect(startFromOf({ start: { kind: 'first' } })).toBe(null);
    expect(startFromOf({ start: { kind: 'at', seconds: 356.5 } })).toBe(356.5);
    expect(startFromOf({})).toBe(null);
  });

  it('a queued demo is reviewed whole, from the time asked, without a scan', async () => {
    const d = deps([scanned('C:/dod/a.dem')]);
    await handleGameReviewRequest({ id: 1, demo: 'C:\\dod\\a.dem', start: { kind: 'at', seconds: 400 } }, d);
    expect(d.scan).not.toHaveBeenCalled();
    expect(d.startReview).toHaveBeenCalledWith(['C:/dod/a.dem'], { covers: REVIEW_COVERS.ALL, startFrom: 400, fromGame: true });
    expect(d.told).toEqual([['progress', STRINGS.REVIEW.FROM_GAME_STARTING], ['finish', null]]);
  });

  it('a demo not in the queue is scanned in first, and the game hears each step', async () => {
    const demos = [];
    const d = deps(demos, { scan: vi.fn(async ([p]) => { demos.push(scanned(p)); }) });
    await handleGameReviewRequest({ id: 2, demo: 'D:/demos/b.dem', start: { kind: 'first' } }, d);
    expect(d.scan).toHaveBeenCalledWith(['D:/demos/b.dem']);
    expect(d.told.map((t) => t[1])).toEqual([STRINGS.REVIEW.FROM_GAME_SCANNING, STRINGS.REVIEW.FROM_GAME_STARTING, null]);
  });

  it('highlights from before demo-player times are scanned again', () => {
    expect(needsRescan({ streaks: [{ kills: [[1, 5, 'K98']] }] })).toBe(true);
    expect(needsRescan(scanned('a'))).toBe(false);
    expect(needsRescan({ streaks: [] })).toBe(false);
  });

  it('says why when it cannot start', async () => {
    const busy = deps([], { isScanning: () => true });
    await handleGameReviewRequest({ id: 3, demo: 'C:/x.dem' }, busy);
    expect(busy.told).toEqual([['finish', STRINGS.REVIEW.FROM_GAME_BUSY]]);

    const lost = deps([]);
    await handleGameReviewRequest({ id: 4, demo: 'C:/x.dem' }, lost);
    expect(lost.told.at(-1)).toEqual(['finish', STRINGS.REVIEW.fromGameNotAdded('x.dem')]);

    const empty = deps([{ path: 'C:/x.dem', streaks: [] }]);
    await handleGameReviewRequest({ id: 5, demo: 'C:/x.dem' }, empty);
    expect(empty.told).toEqual([['finish', STRINGS.REVIEW.fromGameNoHighlights('x.dem')]]);

    const refused = deps([scanned('C:/x.dem')], { startReview: async () => 'the game is not from Studio' });
    await handleGameReviewRequest({ id: 6, demo: 'C:/x.dem' }, refused);
    expect(refused.told.at(-1)).toEqual(['finish', 'the game is not from Studio']);
  });
});
