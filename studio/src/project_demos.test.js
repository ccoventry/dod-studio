import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ locateMissingDemos: vi.fn(), changedDemos: vi.fn(), logFrontendEvent: vi.fn() }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));
vi.mock('./pick_path.js', () => ({ pickPath: vi.fn() }));
vi.mock('./take_index.js', () => ({ renameDemoInTakeIndex: vi.fn() }));

const { createProjectDemos } = await import('./project_demos.js');
const { locateMissingDemos } = await import('./ipc_bridge.js');
const { themedConfirm } = await import('./themed_confirm.js');

const settle = async () => { for (let i = 0; i < 10; i += 1) await Promise.resolve(); };

function setUp(demos) {
  return createProjectDemos({
    getDemos: () => demos,
    getTakeIndex: () => ({}),
    getScanPaths: () => [],
    refreshQueue: vi.fn(),
    scan: vi.fn(),
    removeDemo: vi.fn(),
  });
}

beforeEach(() => vi.clearAllMocks());

describe('markDemosGone while checkMissingDemos runs', () => {
  // A look on focus while a project's own missing-demo check waits on its
  // "use the copies found?" dialog: the check marks its rows itself, so the
  // look must not mark them first and toast a second time (#1376).
  it('leaves the rows to the check that is already looking at them', async () => {
    const a = { name: 'a.dem', path: 'C:/demos/a.dem', file_key: 'k' };
    const app = setUp([a]);
    locateMissingDemos.mockResolvedValue([{ path: a.path, candidate: 'C:/demos/a2.dem' }]);
    let answer;
    themedConfirm.mockReturnValue(new Promise((resolve) => { answer = resolve; }));
    const check = app.checkMissingDemos('C:/demos/p.dodstudio', []);
    await settle();
    expect(themedConfirm).toHaveBeenCalledTimes(1);

    expect(await app.markDemosGone([a])).toEqual([]);
    expect(a.missing).toBeUndefined();

    answer(false);
    expect(await check).toBe(1);
    expect(a.missing).toBe(true);
    expect(a.foundAt).toBe('C:/demos/a2.dem');
  });

  it('marks them once the check is over', async () => {
    const a = { name: 'a.dem', path: 'C:/demos/a.dem', file_key: 'k' };
    const app = setUp([a]);
    locateMissingDemos.mockResolvedValueOnce([]);
    expect(await app.checkMissingDemos(null, [])).toBe(0);
    locateMissingDemos.mockResolvedValueOnce([{ path: a.path, candidate: null }]);
    expect(await app.markDemosGone([a])).toEqual([a]);
    expect(a.missing).toBe(true);
  });
});
