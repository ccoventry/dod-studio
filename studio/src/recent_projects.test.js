import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ filesExist: vi.fn() }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));

const { addRecent, removeRecent, normalizeRecent, createRecentProjects, RECENT_PROJECTS_CAP } =
  await import('./recent_projects.js');

describe('addRecent', () => {
  it('puts a new project at the top', () => {
    expect(addRecent(['D:/p/a.json'], 'D:/p/b.json')).toEqual(['D:/p/b.json', 'D:/p/a.json']);
  });

  it('moves a listed project to the top instead of listing it twice', () => {
    expect(addRecent(['D:/p/a.json', 'D:/p/b.json', 'D:/p/c.json'], 'D:/p/c.json'))
      .toEqual(['D:/p/c.json', 'D:/p/a.json', 'D:/p/b.json']);
  });

  it('treats either slash and any case as the same file, keeping the newest spelling', () => {
    expect(addRecent(['D:/Projects/A.json', 'D:/p/b.json'], 'd:\\projects\\a.json'))
      .toEqual(['d:\\projects\\a.json', 'D:/p/b.json']);
  });

  it(`keeps at most ${RECENT_PROJECTS_CAP}, dropping the oldest`, () => {
    let list = [];
    for (let i = 0; i < 12; i++) list = addRecent(list, `D:/p/${i}.json`);
    expect(list).toHaveLength(10);
    expect(list[0]).toBe('D:/p/11.json');
    expect(list[9]).toBe('D:/p/2.json');
  });

  it('ignores an empty path', () => {
    expect(addRecent(['D:/p/a.json'], '')).toEqual(['D:/p/a.json']);
  });
});

describe('removeRecent', () => {
  it('drops the path, whatever its slashes or case', () => {
    expect(removeRecent(['D:/p/a.json', 'D:/p/b.json'], 'd:\\P\\A.JSON')).toEqual(['D:/p/b.json']);
  });
});

describe('normalizeRecent', () => {
  it('reads an older settings file with no list as empty', () => {
    expect(normalizeRecent(undefined)).toEqual([]);
    expect(normalizeRecent(null)).toEqual([]);
  });

  it('keeps strings only, once each, capped', () => {
    const raw = ['D:/p/a.json', 7, '', 'd:/P/a.json', null, ...Array.from({ length: 12 }, (_, i) => `D:/q/${i}.json`)];
    const list = normalizeRecent(raw);
    expect(list[0]).toBe('D:/p/a.json');
    expect(list).toHaveLength(10);
    expect(list.filter((p) => p.toLowerCase() === 'd:/p/a.json')).toHaveLength(1);
  });
});

describe('createRecentProjects', () => {
  let persisted;
  let openProject;
  let exists;
  let confirm;
  let recent;

  beforeEach(() => {
    // No DOM in the unit tests: the menu itself is covered by the e2e spec.
    globalThis.document = { querySelector: () => null };
    persisted = [];
    openProject = vi.fn(async () => true);
    exists = vi.fn(async (paths) => paths.map(() => true));
    confirm = vi.fn(async () => true);
    recent = createRecentProjects({
      openProject,
      persistAppSettings: async () => { persisted.push(recent.list()); },
      exists,
      confirm,
    });
  });

  it('loads the saved list and saves every change through the settings', async () => {
    recent.load(['D:/p/a.json', 'D:/p/b.json']);
    await recent.remember('D:/p/b.json');
    expect(recent.list()).toEqual(['D:/p/b.json', 'D:/p/a.json']);
    expect(persisted.at(-1)).toEqual(['D:/p/b.json', 'D:/p/a.json']);

    await recent.clearRecent();
    expect(recent.list()).toEqual([]);
    expect(persisted.at(-1)).toEqual([]);
  });

  it('opens a project that is there through openProject', async () => {
    recent.load(['D:/p/a.json']);
    expect(await recent.openRecent('D:/p/a.json')).toBe(true);
    expect(openProject).toHaveBeenCalledWith('D:/p/a.json');
    expect(confirm).not.toHaveBeenCalled();
  });

  it('offers to remove a project whose file is gone, and removes it on yes', async () => {
    recent.load(['D:/p/a.json', 'D:/p/gone.json']);
    exists.mockImplementation(async (paths) => paths.map((p) => p !== 'D:/p/gone.json'));
    expect(await recent.openRecent('D:/p/gone.json')).toBe(false);
    expect(openProject).not.toHaveBeenCalled();
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(recent.list()).toEqual(['D:/p/a.json']);
    expect(persisted.at(-1)).toEqual(['D:/p/a.json']);
  });

  it('keeps a missing project listed when the user says Keep', async () => {
    recent.load(['D:/p/gone.json']);
    exists.mockImplementation(async (paths) => paths.map(() => false));
    confirm.mockImplementation(async () => false);
    await recent.openRecent('D:/p/gone.json');
    expect(recent.list()).toEqual(['D:/p/gone.json']);
    expect(persisted).toEqual([]);
  });

  it('still tries to open when the check itself failed', async () => {
    recent.load(['D:/p/a.json']);
    exists.mockImplementation(async () => null);
    await recent.openRecent('D:/p/a.json');
    expect(openProject).toHaveBeenCalledWith('D:/p/a.json');
  });
});
