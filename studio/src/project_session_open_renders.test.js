// Opening a project picks up renders its take folders hold and the file
// doesn't (#1264). Those were marking the project unsaved on every open,
// though the user had changed nothing; now they're saved with the open when
// they're the only change, and their highlights become Rendered (#1345).

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({
  // The project file exists; no backup of it does yet.
  filesExist: vi.fn(async (paths) => paths.map((p) => p === 'D:/p/movie.dodstudio')),
  readProjectFile: vi.fn(),
  writeProjectFile: vi.fn(async () => {}),
  defaultProjectsDir: vi.fn(),
  takeCaptureStates: vi.fn(),
  logFrontendEvent: vi.fn(),
}));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));
vi.mock('./pick_path.js', () => ({ pickPath: vi.fn() }));
vi.mock('./dialog_wait.js', () => ({ whileDialogOpens: (open) => open() }));
vi.mock('./detail_pane.js', () => ({ updateStreakVisuals: () => {} }));
vi.mock('./master_pane.js', () => ({ clearCheckedPaths: () => {} }));
vi.mock('./nav.js', () => ({ switchNavTab: () => {} }));
vi.mock('./teams_pane.js', () => ({ refreshTeamsPane: () => {} }));
vi.mock('./queue_filters.js', () => ({ dropOtherPlayersSavedStreaks: () => false }));
vi.mock('./project_open_progress.js', () => ({
  createProjectOpenProgress: () => ({ opening() {}, opened() {}, searching() {}, searched() {} }),
}));

globalThis.document = { querySelector: () => null };

const { createProjectSession } = await import('./project_session.js');
const { streakUid } = await import('./take_index.js');
const ipc = await import('./ipc_bridge.js');

const PATH = 'D:/p/movie.dodstudio';
const finished = { started_unix_ms: 5, stream: 'all', outcome: 'finished', output_path: 'C:/clips/a.mov' };

/** A project with one demo, two highlights, both Captured from one take. */
function projectText() {
  const streaks = [
    { player_index: 1, start_tick: 10, end_tick: 20, kills: [[10], [20]], status: 'Captured', curation: 'Keep' },
    { player_index: 1, start_tick: 30, end_tick: 40, kills: [[30], [40]], status: 'Captured', curation: 'Keep' },
  ];
  const demo = { path: 'C:/demos/a.dem', name: 'a.dem', streaks };
  const uids = streaks.map((s) => streakUid(demo.path, { ...s }));
  return JSON.stringify({
    version: '0.12.0',
    demos: [demo],
    takeIndex: { 's/b0': uids },
    takeCaptures: { 's/b0': { session_id: 's', take_folder: 'D:/caps/s/b0' } },
  });
}

function open({ renders = [finished], beforePickUp } = {}) {
  let demos = [];
  let takeIndex = {};
  const onStatusChange = vi.fn();
  const session = createProjectSession({
    getDemos: () => demos,
    replaceDemos: (d) => { demos = d; },
    loadDemos: (d) => { demos = d; },
    getTakeIndex: () => takeIndex,
    setTakeIndex: (i) => { takeIndex = i; },
    getProjectTeams: () => null,
    setProjectTeams: () => {},
    checkMissingDemos: async () => {},
    onStatusChange,
  });
  ipc.readProjectFile.mockResolvedValue(projectText());
  ipc.takeCaptureStates.mockImplementation(async () => {
    if (beforePickUp) beforePickUp(session);
    return [{ exists: true, renders }];
  });
  return { session, onStatusChange, demos: () => demos };
}

/** Lets the open's background pick-up and any save it makes finish. */
const settle = () => new Promise((resolve) => { setTimeout(resolve, 0); });

beforeEach(() => {
  ipc.writeProjectFile.mockClear();
  ipc.logFrontendEvent.mockClear();
});

describe('opening a project whose take folders hold new renders', () => {
  it('marks their highlights Rendered and opens saved', async () => {
    const t = open();
    expect(await t.session.openProject(PATH)).toBe(true);
    await settle();
    expect(t.demos()[0].streaks.map((s) => s.status)).toEqual(['Rendered', 'Rendered']);
    expect(t.onStatusChange).toHaveBeenCalled();
    // A project from an older format is backed up first, so the file is the last write.
    expect(ipc.writeProjectFile.mock.calls.at(-1)[0]).toBe(PATH);
    expect(t.session.needsSavePrompt()).toBe(false);
    expect(ipc.logFrontendEvent.mock.calls.flat().some((l) => String(l).includes('found in its take folders'))).toBe(true);
  });

  it('leaves it unsaved, writing nothing, when something else changed since the open', async () => {
    const t = open({ beforePickUp: (session) => session.markProjectDirty() });
    await t.session.openProject(PATH);
    await settle();
    expect(t.demos()[0].streaks.map((s) => s.status)).toEqual(['Rendered', 'Rendered']);
    expect(ipc.writeProjectFile).not.toHaveBeenCalled();
    expect(t.session.needsSavePrompt()).toBe(true);
  });

  it("finding nothing new does not save someone else's change made meanwhile", async () => {
    const t = open({ renders: [], beforePickUp: (session) => session.markProjectDirty() });
    await t.session.openProject(PATH);
    await settle();
    expect(ipc.writeProjectFile).not.toHaveBeenCalled();
    expect(t.session.needsSavePrompt()).toBe(true);
  });

  it('opens saved and writes nothing when there is nothing new', async () => {
    const t = open({ renders: [] });
    await t.session.openProject(PATH);
    await settle();
    expect(ipc.writeProjectFile).not.toHaveBeenCalled();
    expect(t.session.needsSavePrompt()).toBe(false);
    expect(t.demos()[0].streaks.map((s) => s.status)).toEqual(['Captured', 'Captured']);
  });
});

describe('saveIfOnlyChange (#1344: a batch caught up on in the open project)', () => {
  it('saves when its change is the only one', async () => {
    const t = open({ renders: [] });
    await t.session.openProject(PATH);
    await settle();
    const { result, saved } = await t.session.saveIfOnlyChange(() => { t.session.markProjectDirty(); return 'marks'; });
    expect(result).toBe('marks');
    expect(saved).toBe(true);
    expect(t.session.needsSavePrompt()).toBe(false);
  });

  it('a change that marked nothing does not save a mark made while it awaited', async () => {
    const t = open({ renders: [] });
    await t.session.openProject(PATH);
    await settle();
    ipc.writeProjectFile.mockClear();
    const { saved } = await t.session.saveIfOnlyChange(async () => {
      t.session.markProjectDirty();
      return { changed: false };
    }, { madeChange: (marks) => marks.changed });
    expect(saved).toBe(false);
    expect(ipc.writeProjectFile).not.toHaveBeenCalled();
    expect(t.session.needsSavePrompt()).toBe(true);
  });

  it("leaves the user's own unsaved edits, and its change, for the user to save", async () => {
    const t = open({ renders: [] });
    await t.session.openProject(PATH);
    await settle();
    t.session.markProjectDirty();
    ipc.writeProjectFile.mockClear();
    const { saved } = await t.session.saveIfOnlyChange(() => t.session.markProjectDirty());
    expect(saved).toBe(false);
    expect(ipc.writeProjectFile).not.toHaveBeenCalled();
    expect(t.session.needsSavePrompt()).toBe(true);
  });
});
