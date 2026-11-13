import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ logFrontendEvent: vi.fn() }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));

const { themedConfirm } = await import('./themed_confirm.js');
const { createReviewMissingDemos, askAboutMissingDemo, MISSING_CHOICE: C } = await import('./review_missing_demos.js');
const { STRINGS } = await import('./strings.js');

const gone = (name, extra = {}) => ({ name, path: `C:/demos/${name}`, missing: true, foundAt: null, ...extra });

// A queue of `demos`, and the choices the user makes, in order.
function setUp(demos, choices, over = {}) {
  let queue = [...demos];
  const log = vi.fn();
  const app = {
    getDemos: () => queue,
    lookMissing: vi.fn(async (list) => list.filter((d) => d.missing)),
    locateDemoByHand: vi.fn(async (d) => { d.missing = false; }),
    useFoundCopies: vi.fn(async ([d]) => { d.missing = false; d.foundAt = null; }),
    confirmTrackedRemove: vi.fn(async () => true),
    replaceDemos: vi.fn((next) => { queue = next; }),
    ask: vi.fn(async () => choices.shift()),
    log,
    ...over,
  };
  return { app, settle: createReviewMissingDemos(app), log, queue: () => queue };
}

beforeEach(() => vi.clearAllMocks());

describe('settling the missing demos of a review', () => {
  it('asks about nothing when every file is there', async () => {
    const a = gone('a.dem', { missing: false });
    const { app, settle } = setUp([a], []);
    expect(await settle([a])).toEqual({ ok: true, touched: false, dropped: new Set() });
    expect(app.ask).not.toHaveBeenCalled();
  });

  it('Skip leaves the demo out of the review and the rest of the queue intact', async () => {
    const [a, b, c] = [gone('a.dem'), gone('b.dem', { missing: false }), gone('c.dem', { missing: false })];
    const { settle, queue, log } = setUp([a, b, c], [C.SKIP]);
    const result = await settle([a, b, c]);
    expect(result.ok).toBe(true);
    expect(result.touched).toBe(true);
    expect([...result.dropped]).toEqual([a]);
    // The row stays in the queue: skipping is for this review only.
    expect(queue()).toEqual([a, b, c]);
    expect(a.missing).toBe(true);
    expect(log.mock.calls.map(([line]) => line).some((l) => l.includes('skipped a.dem') && l.includes('other 2 demo'))).toBe(true);
  });

  it('closing the pop-up cancels the review and asks about no more demos', async () => {
    const [a, b] = [gone('a.dem'), gone('b.dem')];
    const { app, settle } = setUp([a, b], [C.CANCEL]);
    const result = await settle([a, b]);
    expect(result.ok).toBe(false);
    expect(app.ask).toHaveBeenCalledTimes(1);
  });

  it('Locate points the row at the file picked and the review goes on with it', async () => {
    const a = gone('a.dem');
    const { app, settle } = setUp([a], [C.LOCATE]);
    const result = await settle([a]);
    expect(app.locateDemoByHand).toHaveBeenCalledWith(a);
    expect(result).toEqual({ ok: true, touched: true, dropped: new Set() });
  });

  it('asks again when Locate was cancelled and the file is still missing', async () => {
    const a = gone('a.dem');
    const { app, settle } = setUp([a], [C.LOCATE, C.SKIP], { locateDemoByHand: vi.fn(async () => {}) });
    const result = await settle([a]);
    expect(app.ask).toHaveBeenCalledTimes(2);
    expect(result.dropped.has(a)).toBe(true);
  });

  it('Use found copy relocates the row', async () => {
    const a = gone('a.dem', { foundAt: 'C:/demos/sub/a.dem' });
    const { app, settle } = setUp([a], [C.USE_FOUND]);
    const result = await settle([a]);
    expect(app.useFoundCopies).toHaveBeenCalledWith([a]);
    expect(result.ok).toBe(true);
    expect(result.dropped.size).toBe(0);
  });

  it('Remove takes the row out of the queue and out of the review', async () => {
    const [a, b] = [gone('a.dem'), gone('b.dem', { missing: false })];
    const { app, settle, queue } = setUp([a, b], [C.REMOVE]);
    const result = await settle([a, b]);
    expect(queue()).toEqual([b]);
    expect(result.dropped.has(a)).toBe(true);
    expect(app.replaceDemos).toHaveBeenCalledTimes(1);
  });

  it('keeps the row and asks again when the tracked-work confirm is declined', async () => {
    const a = gone('a.dem', { streaks: [{ notes: 'my note' }] });
    const { app, settle, queue } = setUp([a], [C.REMOVE, C.SKIP], { confirmTrackedRemove: vi.fn(async () => false) });
    const result = await settle([a]);
    expect(queue()).toEqual([a]);
    expect(app.ask).toHaveBeenCalledTimes(2);
    expect(result.dropped.has(a)).toBe(true);
  });

  it('asks about each missing demo in turn', async () => {
    const [a, b] = [gone('a.dem'), gone('b.dem')];
    const { app, settle } = setUp([a, b], [C.SKIP, C.LOCATE]);
    const result = await settle([a, b]);
    expect(app.ask.mock.calls.map(([d]) => d)).toEqual([a, b]);
    expect([...result.dropped]).toEqual([a]);
  });
});

describe('the pop-up', () => {
  it('offers Use found copy only when a copy was found, and Skip and keep reviewing', async () => {
    themedConfirm.mockResolvedValue(C.SKIP);
    await askAboutMissingDemo(gone('a.dem'));
    let options = themedConfirm.mock.calls[0][1];
    expect(options.choices.map((c) => c.label)).toEqual([
      STRINGS.REVIEW.MISSING_LOCATE, STRINGS.REVIEW.MISSING_REMOVE, STRINGS.REVIEW.MISSING_SKIP,
    ]);
    await askAboutMissingDemo(gone('a.dem', { foundAt: 'C:/x/a.dem' }));
    options = themedConfirm.mock.calls[1][1];
    expect(options.choices.map((c) => c.id)).toContain(C.USE_FOUND);
  });

  it('answers cancel when it is closed', async () => {
    themedConfirm.mockResolvedValue(false);
    expect(await askAboutMissingDemo(gone('a.dem'))).toBe(C.CANCEL);
  });

  it('says so when the search for a copy stopped early', async () => {
    themedConfirm.mockResolvedValue(false);
    await askAboutMissingDemo(gone('a.dem', { searchStopped: true }));
    expect(themedConfirm.mock.calls[0][0]).toContain('stopped early');
    await askAboutMissingDemo(gone('a.dem'));
    expect(themedConfirm.mock.calls[1][0]).not.toContain('stopped early');
  });
});
