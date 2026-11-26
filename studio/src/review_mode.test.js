import { describe, it, expect, vi, beforeEach } from 'vitest';

// The game's `review_event`s as review_mode.js turns them into toasts and
// row changes.

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({
  startHighlightReview: vi.fn(),
  stopHighlightReview: vi.fn(() => Promise.resolve()),
}));
vi.mock('./running_game_guard.js', () => ({ closeGameWithOtherSettings: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { listen } = await import('@tauri-apps/api/event');
const { stopHighlightReview } = await import('./ipc_bridge.js');
const { showToast } = await import('./toast.js');
const { STRINGS } = await import('./strings.js');
const { initReviewMode } = await import('./review_mode.js');
const { streakUid } = await import('./take_index.js');

/** Wires review mode against a page with only its button, and returns the
 *  `review_event` handler it registered. */
function wire({ demos = [], onChanged = vi.fn() } = {}) {
  const button = { addEventListener: vi.fn(), disabled: false };
  globalThis.document = {
    querySelector: (selector) => (selector === '#review-highlights-btn' ? button : null),
  };
  initReviewMode({
    getDemos: () => demos,
    getCheckedPaths: () => [],
    getShownPaths: () => [],
    onChanged,
  });
  const call = listen.mock.calls.find(([name]) => name === 'review_event');
  return call[1];
}

describe('review_event toasts', () => {
  beforeEach(() => {
    listen.mockClear();
    showToast.mockClear();
    stopHighlightReview.mockClear();
  });

  it('says a review is running when the game refused what Studio sent (#880)', () => {
    const onEvent = wire();
    onEvent({ payload: { kind: 'refused', command: 'viewdemo' } });
    expect(showToast).toHaveBeenCalledTimes(1);
    const [message, type, , options] = showToast.mock.calls[0];
    expect(message).toBe(STRINGS.REVIEW.refusedToast('viewdemo'));
    expect(type).toBe('warning');
    expect(options.action.label).toBe(STRINGS.REVIEW.STOP);
    options.action.onClick();
    expect(stopHighlightReview).toHaveBeenCalledTimes(1);
  });

  it("puts the game form's kill range and note on the row, quietly (#1139)", () => {
    const streak = { player_index: 2, kills: [[10, 1, 'K98'], [20, 2, 'K98'], [30, 3, 'K98']], curation: 'Keep' };
    const onChanged = vi.fn();
    const onEvent = wire({ demos: [{ path: 'a.dem', streaks: [streak] }], onChanged });
    onEvent({ payload: { kind: 'edit', demo: 'a.dem', key: streakUid('a.dem', streak), from: 2, to: 3, note: 'flick' } });
    expect(streak).toMatchObject({ start_index: 1, end_index: 2, notes: 'flick', curation: 'Keep' });
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(showToast).not.toHaveBeenCalled();
  });

  it('shows nothing for an end this Studio did not start', () => {
    const onEvent = wire();
    onEvent({ payload: { kind: 'ended', reason: 'stopped' } });
    expect(showToast).not.toHaveBeenCalled();
  });
});

describe('startReview with a demo whose file is missing (#1376)', () => {
  const streak = (player) => ({ player_index: player, kills: [[10, 1, 'K98'], [20, 2, 'K98'], [30, 3, 'K98']], curation: 'Keep', viewdemo_times: [1, 2, 3] });
  const demos = () => ['a.dem', 'b.dem'].map((name) => ({ path: `C:/demos/${name}`, name, streaks: [streak(1)] }));

  async function review(settle) {
    const { startHighlightReview } = await import('./ipc_bridge.js');
    startHighlightReview.mockReset();
    startHighlightReview.mockResolvedValue({ count: 1, launched: true });
    (await import('./running_game_guard.js')).closeGameWithOtherSettings.mockResolvedValue(true);
    const list = demos();
    const button = { addEventListener: vi.fn(), disabled: false };
    globalThis.document = { querySelector: (s) => (s === '#review-highlights-btn' ? button : null) };
    const { startReview } = initReviewMode({
      getDemos: () => list, getCheckedPaths: () => [], getShownPaths: () => [], onChanged: vi.fn(), settleMissingDemos: settle(list),
    });
    const result = await startReview(list.map((d) => d.path), { covers: 'all' });
    return { result, sent: startHighlightReview.mock.calls[0]?.[0], list };
  }

  it('reviews the rest of the queue when the missing demo was skipped', async () => {
    const { sent, result } = await review((list) => async () => ({ ok: true, touched: true, dropped: new Set([list[0]]) }));
    expect(result).toBeNull();
    expect(sent.map((h) => h.demo)).toEqual(['C:/demos/b.dem']);
  });

  it('sends nothing when the pop-up was closed', async () => {
    const { sent, result } = await review(() => async () => ({ ok: false, touched: false, dropped: new Set() }));
    expect(result).toBe(STRINGS.REVIEW.NOT_STARTED);
    expect(sent).toBeUndefined();
  });

  it('reviews a located demo under its new path', async () => {
    const { sent } = await review((list) => async () => {
      list[0].path = 'C:/demos/moved/a.dem';
      return { ok: true, touched: true, dropped: new Set() };
    });
    expect(sent.map((h) => h.demo).sort()).toEqual(['C:/demos/b.dem', 'C:/demos/moved/a.dem']);
  });

  it('says nothing is left when the only demo was skipped', async () => {
    const { sent, result } = await review((list) => async () => ({ ok: true, touched: true, dropped: new Set(list) }));
    expect(sent).toBeUndefined();
    expect(result).toBe(STRINGS.REVIEW.NOTHING_LEFT_AFTER_SKIP);
  });
});
