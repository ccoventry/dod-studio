// project_session.js's own refusal (#1085): an open from Explorer or a batch
// offer comes through no control, so batch_guard.js's click filter never sees
// it, and this is all that stops it.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({
  readProjectFile: vi.fn(async () => JSON.stringify({ version: '0.12.0', demos: [] })),
  writeProjectFile: vi.fn(),
  defaultProjectsDir: vi.fn(),
  takeCaptureStates: vi.fn(async () => []),
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

const modal = { style: { display: 'none' } };
globalThis.document = { querySelector: (selector) => (selector === '#unsaved-changes-modal' ? modal : null) };

const { createProjectSession } = await import('./project_session.js');
const { readProjectFile } = await import('./ipc_bridge.js');

/** A session with a project open and unsaved work, as a batch would find it. */
function heldSession(held) {
  const app = { replaceDemos: vi.fn(), loadDemos: vi.fn() };
  const session = createProjectSession({
    getDemos: () => [{ path: 'a.dem', streaks: [] }],
    replaceDemos: app.replaceDemos,
    loadDemos: app.loadDemos,
    getTakeIndex: () => ({}),
    setTakeIndex: () => {},
    getProjectTeams: () => null,
    setProjectTeams: () => {},
    checkMissingDemos: async () => {},
    refuseDuringBatch: () => held,
  });
  session.markProjectDirty();
  return { app, session };
}

beforeEach(() => {
  readProjectFile.mockClear();
  modal.style.display = 'none';
});

describe('during a batch', () => {
  it('openProject refuses before the unsaved prompt and reads nothing', async () => {
    const { app, session } = heldSession(true);
    expect(await session.openProject('D:/p/other.dodstudio')).toBe(false);
    expect(readProjectFile).not.toHaveBeenCalled();
    expect(app.loadDemos).not.toHaveBeenCalled();
    expect(modal.style.display).toBe('none');
  });

  it('confirmReplaceProject says no, which stops Load and Rescan Recent Sessions', async () => {
    expect(await heldSession(true).session.confirmReplaceProject()).toBe(false);
    expect(modal.style.display).toBe('none');
  });

  it('New Session leaves the project open, with no prompt', async () => {
    const { app, session } = heldSession(true);
    await session.newSession();
    expect(app.replaceDemos).not.toHaveBeenCalled();
    expect(modal.style.display).toBe('none');
  });

  it('a quiet New Session, Rescan putting back its blank queue, still runs', async () => {
    const { app, session } = heldSession(true);
    await session.newSession({ quiet: true });
    expect(app.replaceDemos).toHaveBeenCalledWith([]);
  });
});

describe('with no batch', () => {
  it('openProject goes on to the unsaved prompt', () => {
    const { session } = heldSession(false);
    session.openProject('D:/p/other.dodstudio');
    return vi.waitFor(() => expect(modal.style.display).toBe('flex'));
  });
});
