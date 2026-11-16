import { describe, it, expect, vi, beforeEach } from 'vitest';

const handlers = {};
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name, fn) => { handlers[name] = fn; return Promise.resolve(() => {}); }),
}));
vi.mock('./ipc_bridge.js', () => ({ logFrontendEvent: vi.fn(), repairDemo: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));
vi.mock('./repair_status.js', () => ({ noteRepairResult: vi.fn() }));

const { showToast } = await import('./toast.js');
const { logFrontendEvent } = await import('./ipc_bridge.js');
const {
  DAMAGED_EVENT, damagedToastText, repairFromNotice, shouldShow, startDamagedDemoToasts,
} = await import('./damaged_demo_toast.js');
const { STRINGS } = await import('./strings.js');

const notice = (over = {}) => ({
  demo: 'C:\\Games\\dod\\monday-wsod25_r07_m1_h1.dem', seconds: 507.75, reason: 'message_too_big', ...over,
});

beforeEach(() => {
  showToast.mockClear();
  logFrontendEvent.mockClear();
});

describe("The game's damaged-demo notice (#1373)", () => {
  it('names the demo and where it stops, or only the demo', () => {
    expect(damagedToastText(notice())).toBe(STRINGS.REPAIR.gameDamaged('monday-wsod25_r07_m1_h1.dem', '8:27'));
    expect(damagedToastText(notice({ seconds: null }))).toBe(STRINGS.REPAIR.gameDamaged('monday-wsod25_r07_m1_h1.dem', ''));
    expect(STRINGS.REPAIR.gameDamaged('a.dem', '')).not.toContain(' at ');
  });

  it('shows a warning toast with Repair, and logs it', async () => {
    startDamagedDemoToasts({ getDemos: () => [], repairQueuedDemo: vi.fn() });
    await Promise.resolve();
    handlers[DAMAGED_EVENT]({ payload: notice() });
    expect(logFrontendEvent).toHaveBeenCalledWith(expect.stringContaining('monday-wsod25_r07_m1_h1.dem is damaged at 8:27'));
    const [text, type, , { action }] = showToast.mock.calls[0];
    expect(text).toBe(damagedToastText(notice()));
    expect(type).toBe('warning');
    expect(action.label).toBe(STRINGS.REPAIR.GAME_DAMAGED_ACTION);
    // The same demo again straight away is logged but not shown twice.
    handlers[DAMAGED_EVENT]({ payload: notice() });
    expect(showToast).toHaveBeenCalledTimes(1);
    expect(logFrontendEvent).toHaveBeenCalledTimes(2);
  });

  it('shows a notice again only after a while', () => {
    const shown = new Map([[notice().demo, 1000]]);
    expect(shouldShow(notice(), shown, 2000)).toBe(false);
    expect(shouldShow(notice(), shown, 1000 + 31000)).toBe(true);
    expect(shouldShow(notice({ demo: 'C:\\other.dem' }), shown, 2000)).toBe(true);
  });

  it("repairs a queued demo through the queue's Repair", async () => {
    const queued = { path: 'c:/games/dod/monday-wsod25_r07_m1_h1.dem' };
    const repairQueuedDemo = vi.fn(async () => true);
    const repair = vi.fn();
    const verdict = await repairFromNotice(notice(), { getDemos: () => [queued], repairQueuedDemo, repair });
    expect(repairQueuedDemo).toHaveBeenCalledWith(queued);
    expect(repair).not.toHaveBeenCalled();
    expect(verdict).toBe('repaired');
  });

  it('repairs any other demo beside itself and says how it went', async () => {
    const result = { verdict: 'repaired', percent_kept: 98.2, written: 'C:\\Games\\dod\\x_repaired.dem', holes: [{}], seconds_lost: 3, ends_at_last_hole: false };
    const repair = vi.fn(async () => result);
    const verdict = await repairFromNotice(notice(), { getDemos: () => [], repairQueuedDemo: vi.fn(), repair });
    expect(repair).toHaveBeenCalledWith(notice().demo);
    expect(verdict).toBe('repaired');
    expect(showToast.mock.calls.at(-1)[1]).toBe('success');
    expect(logFrontendEvent).toHaveBeenCalledWith(expect.stringMatching(/repaired in /));
  });

  it('says a failed repair failed', async () => {
    const repair = vi.fn(async () => { throw new Error('locked'); });
    const verdict = await repairFromNotice(notice(), { getDemos: () => [], repairQueuedDemo: vi.fn(), repair });
    expect(verdict).toBe('failed');
    expect(showToast.mock.calls.at(-1)[1]).toBe('error');
  });
});
