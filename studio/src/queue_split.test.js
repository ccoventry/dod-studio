import { describe, it, expect, vi, beforeEach } from 'vitest';

// Only Start Capture Batch's demo check is tested here; the split itself is
// covered by master-pane.spec.js and split_carry.test.js.
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({ logFrontendEvent: vi.fn(), splitDemoAuto: vi.fn(), demoSplitStatus: vi.fn() }));
vi.mock('./split_pane.js', () => ({ SHORT_MAP_SECONDS: 60 }));
vi.mock('./detail_pane.js', () => ({ updateStreakVisuals: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));
vi.mock('./queue_progress.js', () => ({ SPLIT_SHARE: 70, nest: vi.fn(), placed: vi.fn(), queueProgress: vi.fn(() => ({})) }));

const { createQueueSplit } = await import('./queue_split.js');
const { logFrontendEvent, splitDemoAuto } = await import('./ipc_bridge.js');

const demo = (name, maps, picked) => ({
  path: `C:/demos/${name}`,
  signon_maps: maps,
  streaks: [{ selected: picked }],
});

function queueSplit(demos, present = true) {
  const pickedDemosPresent = vi.fn(async () => present);
  const split = createQueueSplit({
    getDemos: () => demos,
    removeDemo: vi.fn(),
    scan: vi.fn(),
    getTakeIndex: () => ({}),
    refreshQueue: vi.fn(),
    pickedDemosPresent,
  });
  return { ...split, pickedDemosPresent };
}

beforeEach(() => vi.clearAllMocks());

describe('captureDemosReady', () => {
  it('lets a batch with picks in a multi-map demo start, without asking or splitting', async () => {
    const { captureDemosReady, pickedDemosPresent } = queueSplit([demo('two.dem', ['dod_armory', 'dod_anzio'], true)]);
    const onProgress = vi.fn();
    expect(await captureDemosReady({ onProgress })).toBe(true);
    expect(pickedDemosPresent).toHaveBeenCalledWith({ onProgress });
    expect(splitDemoAuto).not.toHaveBeenCalled();
  });

  it('says in the activity log how many picked demos use only their first map', async () => {
    const { captureDemosReady } = queueSplit([
      demo('two.dem', ['dod_armory', 'dod_anzio'], true),
      demo('three.dem', ['dod_flash', 'dod_flash', 'dod_avalanche'], true),
      demo('unpicked.dem', ['dod_armory', 'dod_anzio'], false),
      demo('one.dem', ['dod_anzio'], true),
    ]);
    await captureDemosReady();
    expect(logFrontendEvent).toHaveBeenCalledTimes(1);
    expect(logFrontendEvent).toHaveBeenCalledWith('[capture] Using the first map of 2 demos that recorded more than one map');
  });

  it('logs nothing when no picked demo has more than one map', async () => {
    const { captureDemosReady } = queueSplit([demo('one.dem', ['dod_anzio'], true)]);
    expect(await captureDemosReady()).toBe(true);
    expect(logFrontendEvent).not.toHaveBeenCalled();
  });

  it('passes on a missing demo, and logs nothing for a batch that will not start', async () => {
    const { captureDemosReady } = queueSplit([demo('two.dem', ['dod_armory', 'dod_anzio'], true)], false);
    expect(await captureDemosReady()).toBe(false);
    expect(logFrontendEvent).not.toHaveBeenCalled();
  });
});
