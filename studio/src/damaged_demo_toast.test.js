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
  DAMAGED_EVENT, damagedToastText, isWorkingCopy, repairFromNotice, shouldShow, startDamagedDemoToasts,
} = await import('./damaged_demo_toast.js');
const { STRINGS } = await import('./strings.js');

const notice = (over = {}) => ({
  demo: 'C:\\Games\\dod\\monday-wsod25_r07_m1_h1.dem', seconds: 507.75, reason: 'message_too_big', ...over,
});

/** A stand-in for the queue's progress line, recording what it showed. */
function fakeProgress() {
  const shown = [];
  return {
    shown,
    begin: ({ text }) => { shown.push(['begin', text]); return {}; },
    show: (job, view) => shown.push(['show', view.text]),
    ms: () => 1500,
    end: (job, text) => shown.push(['end', text]),
  };
}

beforeEach(() => {
  showToast.mockClear();
  logFrontendEvent.mockClear();
  for (const k of Object.keys(handlers)) delete handlers[k];
});

describe("The game's damaged-demo notice (#1373)", () => {
  it('names the demo and where it stops, or only the demo', () => {
    expect(damagedToastText(notice())).toBe(STRINGS.REPAIR.gameDamaged('monday-wsod25_r07_m1_h1.dem', '8:27'));
    expect(damagedToastText(notice({ seconds: null }))).toBe(STRINGS.REPAIR.gameDamaged('monday-wsod25_r07_m1_h1.dem', ''));
    expect(STRINGS.REPAIR.gameDamaged('a.dem', '')).not.toContain(' at ');
  });

  it('shows a sticky warning toast with Repair, and logs it', async () => {
    startDamagedDemoToasts({ getDemos: () => [], repairQueuedDemo: vi.fn() });
    await Promise.resolve();
    handlers[DAMAGED_EVENT]({ payload: notice() });
    expect(logFrontendEvent).toHaveBeenCalledWith(expect.stringContaining('monday-wsod25_r07_m1_h1.dem is damaged at 8:27'));
    const [text, type, , { action, sticky }] = showToast.mock.calls[0];
    expect(text).toBe(damagedToastText(notice()));
    expect(type).toBe('warning');
    expect(sticky).toBe(true);
    expect(action.label).toBe(STRINGS.REPAIR.GAME_DAMAGED_ACTION);
    // The same demo again straight away is logged but not shown twice.
    handlers[DAMAGED_EVENT]({ payload: notice() });
    expect(showToast).toHaveBeenCalledTimes(1);
    expect(logFrontendEvent).toHaveBeenCalledTimes(2);
  });

  it('offers no Repair while a capture batch runs', async () => {
    startDamagedDemoToasts({ getDemos: () => [], repairQueuedDemo: vi.fn(), isHeld: () => true });
    await Promise.resolve();
    handlers[DAMAGED_EVENT]({ payload: notice() });
    const [text, , , options] = showToast.mock.calls[0];
    expect(text).toBe(damagedToastText(notice(), true));
    expect(text).toContain('capture batch');
    expect(options.action).toBeUndefined();
  });

  it("shows nothing for Studio's own working copies", async () => {
    expect(isWorkingCopy('C:\\Games\\dod\\dodstudio_chain_03.dem')).toBe(true);
    expect(isWorkingCopy('C:\\Games\\dod\\DODSTUDIO_primer.dem')).toBe(true);
    expect(isWorkingCopy('C:\\Games\\dod\\monday.dem')).toBe(false);
    startDamagedDemoToasts({ getDemos: () => [], repairQueuedDemo: vi.fn() });
    await Promise.resolve();
    handlers[DAMAGED_EVENT]({ payload: notice({ demo: 'C:\\Games\\dod\\dodstudio_chain_01.dem' }) });
    expect(showToast).not.toHaveBeenCalled();
    expect(logFrontendEvent).toHaveBeenCalledWith(STRINGS.REPAIR.gameDamagedWorkingCopyLog('dodstudio_chain_01.dem'));
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
    const verdict = await repairFromNotice(notice(), {
      getDemos: () => [queued], repairQueuedDemo, repair, progress: fakeProgress(),
    });
    expect(repairQueuedDemo).toHaveBeenCalledWith(queued);
    expect(repair).not.toHaveBeenCalled();
    expect(verdict).toBe('repaired');
  });

  it('repairs any other demo beside itself, with its progress on the line until it ends', async () => {
    const result = { verdict: 'repaired', percent_kept: 98.2, written: 'C:\\Games\\dod\\x_repaired.dem', holes: [{}], seconds_lost: 3, ends_at_last_hole: false };
    let feed;
    const listenProgress = vi.fn(async (fn) => { feed = fn; return () => {}; });
    const repair = vi.fn(async (path) => {
      feed({ payload: { path, progress: { step: 'writing', fraction: 0.5 } } });
      feed({ payload: { path: 'C:\\other.dem', progress: { step: 'reading', fraction: 0.9 } } });
      return result;
    });
    const progress = fakeProgress();
    const verdict = await repairFromNotice(notice(), {
      getDemos: () => [], repairQueuedDemo: vi.fn(), repair, listenProgress, progress,
    });
    expect(repair).toHaveBeenCalledWith(notice().demo);
    expect(verdict).toBe('repaired');
    expect(progress.shown[0][0]).toBe('begin');
    expect(progress.shown.some(([kind, text]) => kind === 'show' && text.includes('50%'))).toBe(true);
    expect(progress.shown.some(([, text]) => text?.includes('90%'))).toBe(false);
    expect(progress.shown.at(-1)[0]).toBe('end');
    expect(showToast.mock.calls.at(-1)[1]).toBe('success');
    expect(logFrontendEvent).toHaveBeenCalledWith(expect.stringMatching(/repaired in /));
  });

  it('says a failed repair failed, and ends the line', async () => {
    const repair = vi.fn(async () => { throw new Error('locked'); });
    const progress = fakeProgress();
    const verdict = await repairFromNotice(notice(), {
      getDemos: () => [], repairQueuedDemo: vi.fn(), repair, listenProgress: async () => () => {}, progress,
    });
    expect(verdict).toBe('failed');
    expect(showToast.mock.calls.at(-1)[1]).toBe('error');
    expect(progress.shown.at(-1)[0]).toBe('end');
  });
});
