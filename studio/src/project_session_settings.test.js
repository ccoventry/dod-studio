// A project's own settings through the session (#1085, build plan step 4):
// an old project gets Configuration's copied in once and saved with it, a
// project with its own settings resolves to them without touching the form,
// an edit in Configuration reaches the open project, and the Project settings
// page (step 6) edits its block through the session.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({
  filesExist: vi.fn(async (paths) => paths.map((p) => p === 'D:/p/movie.dodstudio')),
  readProjectFile: vi.fn(),
  writeProjectFile: vi.fn(async () => {}),
  defaultProjectsDir: vi.fn(),
  takeCaptureStates: vi.fn(async () => []),
  logFrontendEvent: vi.fn(),
  findInstalls: vi.fn(async () => []),
  getAppVersion: vi.fn(async () => '0.12.0'),
  // The backend keeps the first snapshot it is given (project_settings_snapshot.rs).
  keepProjectSettingsSnapshot: vi.fn(async (snapshot) => {
    const taken = !globalThis.__keptSnapshot;
    if (taken) globalThis.__keptSnapshot = snapshot;
    return { snapshot: globalThis.__keptSnapshot, taken };
  }),
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
const snapshots = await import('./project_settings_snapshot.js');
const settings = await import('./project_settings.js');
const ipc = await import('./ipc_bridge.js');
const { showToast } = await import('./toast.js');

const PATH = 'D:/p/movie.dodstudio';
const PRE = 'D:/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/hl.exe';

// The Configuration form, as settings_sync.js's reader returns it.
let form;
const appForm = (extra = {}) => ({
  capture_mode: 'frame_sequence', resolution_width: 1920, resolution_height: 1080, capture_fps: 120,
  clip_name_template: '{player}', target_drives: ['E:/captures'], render_export_dirs: [], hl_path: PRE,
  init_commands: ['mirv_fov 90'], custom_commands: [], ...extra,
});

function projectText(extra = {}) {
  return JSON.stringify({ version: '0.12.0', demos: [{ path: 'C:/demos/a.dem', name: 'a.dem', streaks: [] }], ...extra });
}

function openSession(text) {
  let demos = [];
  const session = createProjectSession({
    getDemos: () => demos,
    replaceDemos: (d) => { demos = d; },
    loadDemos: (d) => { demos = d; },
    getTakeIndex: () => ({}),
    setTakeIndex: () => {},
    getProjectTeams: () => null,
    setProjectTeams: () => {},
    checkMissingDemos: async () => {},
  });
  ipc.readProjectFile.mockResolvedValue(text);
  return session;
}

const lastWritten = () => JSON.parse(ipc.writeProjectFile.mock.calls.at(-1)[1]);

beforeEach(() => {
  form = appForm();
  settings.setFormReader(() => form);
  settings.loadPcId('pc-a');
  ipc.writeProjectFile.mockClear();
  vi.mocked(showToast).mockClear();
  snapshots.resetSnapshotForTests();
  globalThis.__keptSnapshot = undefined;
});

describe('a project made before #1085', () => {
  it('gets Configuration\'s settings copied in, and the save writes them', async () => {
    const session = openSession(projectText());
    await session.openProject(PATH);
    session.markProjectDirty();
    expect(await session.saveProjectSession()).toBe(true);
    const saved = lastWritten();
    expect(saved.schema).toBe(3);
    expect(saved.settings.picture).toMatchObject({ width: 1920, height: 1080, captureMode: 'frame_sequence' });
    expect(saved.settings.folders['pc-a'].destinations).toEqual(['E:/captures']);
    expect(saved.settings.install.madeWith.hlExe).toBe(PRE);
  });

  it('says so in the one-time summary', async () => {
    const session = openSession(projectText());
    await session.openProject(PATH);
    session.markProjectDirty();
    await session.saveProjectSession();
    const toasts = vi.mocked(showToast).mock.calls.map(([text]) => text);
    expect(toasts.some((t) => t.startsWith('Updated:') && t.includes('copied from Configuration'))).toBe(true);
  });

  it('captures as before: everything resolves to the form', async () => {
    const session = openSession(projectText());
    await session.openProject(PATH);
    expect(settings.effectiveSettings(appForm())).toEqual(appForm());
  });

  it('does not mark the project unsaved just by opening', async () => {
    const session = openSession(projectText());
    await session.openProject(PATH);
    expect(session.needsSavePrompt()).toBe(false);
  });
});

describe('a project made before #1085, with the snapshot taken (decision 6)', () => {
  // Configuration set up for another movie after the snapshot was taken.
  const otherMovie = () => appForm({ resolution_width: 2560, resolution_height: 1440, target_drives: ['F:/other'] });

  it('takes the snapshot, not Configuration as it is when it opens', async () => {
    await settings.startProjectSettingsSnapshot();
    form = otherMovie();
    const session = openSession(projectText());
    await session.openProject(PATH);
    expect(settings.effectiveSetting('resolution_width', 2560)).toBe(1920);
    expect(settings.effectiveSetting('target_drives', ['F:/other'])).toEqual(['E:/captures']);
    expect(session.needsSavePrompt()).toBe(false);
    session.markProjectDirty();
    await session.saveProjectSession();
    const saved = lastWritten();
    expect(saved.settings.picture).toMatchObject({ width: 1920, height: 1080 });
    expect(saved.settings.folders).toEqual({ 'pc-a': { destinations: ['E:/captures'], exports: [] } });
    expect(saved.settings.install.madeWith.hlExe).toBe(PRE);
  });

  it('uses a snapshot still being kept when a project opens at once (a fresh install)', async () => {
    settings.startProjectSettingsSnapshot();
    const session = openSession(projectText());
    await session.openProject(PATH);
    expect(ipc.keepProjectSettingsSnapshot).toHaveBeenCalled();
    expect(settings.effectiveSetting('resolution_width', 1)).toBe(1920);
  });

  it('gives every older project the same settings, whichever opens first', async () => {
    await settings.startProjectSettingsSnapshot();
    const session = openSession(projectText());
    form = otherMovie();
    await session.openProject(PATH);
    form = appForm({ resolution_width: 800 });
    await session.openProject(PATH);
    expect(settings.effectiveSetting('resolution_width', 800)).toBe(1920);
  });

  it('keeps the snapshot another window took first', async () => {
    globalThis.__keptSnapshot = snapshots.snapshotFromForm(appForm({ capture_fps: 60 }), { takenAt: '2026-11-01T12:00:00.000Z', schema: 3 });
    await settings.startProjectSettingsSnapshot();
    const session = openSession(projectText());
    await session.openProject(PATH);
    expect(settings.effectiveSetting('capture_fps', 120)).toBe(60);
  });

  it('names the day of the snapshot in the one-time summary and the log', async () => {
    globalThis.__keptSnapshot = snapshots.snapshotFromForm(appForm(), { takenAt: '2026-11-11T18:00:00.000Z', appVersion: '0.12.0', schema: 3 });
    await settings.startProjectSettingsSnapshot();
    const session = openSession(projectText());
    await session.openProject(PATH);
    session.markProjectDirty();
    await session.saveProjectSession();
    const toasts = vi.mocked(showToast).mock.calls.map(([text]) => text);
    expect(toasts.some((t) => t.includes('the ones Configuration had on November 11, 2026, the same for every older project'))).toBe(true);
    const lines = vi.mocked(ipc.logFrontendEvent).mock.calls.map(([line]) => line);
    expect(lines.some((l) => l.includes('took the snapshot of Configuration taken 2026-11-11T18:00:00.000Z by 0.12.0'))).toBe(true);
    expect(lines.some((l) => l.includes('settings from the snapshot taken 2026-11-11T18:00:00.000Z'))).toBe(true);
  });

  it('leaves a project with its own settings alone', async () => {
    await settings.startProjectSettingsSnapshot();
    const session = openSession(projectText({ schema: 3, settings: { picture: { width: 640 } } }));
    await session.openProject(PATH);
    expect(settings.effectiveSetting('resolution_width', 1920)).toBe(640);
  });
});

describe('a project with its own settings', () => {
  const own = { picture: { width: 2560, height: 1440 }, clipNames: { template: '{map}' } };

  it('resolves to them while the form keeps the app\'s', async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    expect(settings.effectiveSetting('resolution_width', 1920)).toBe(2560);
    expect(settings.effectiveSetting('clip_name_template', '{player}')).toBe('{map}');
    expect(form.resolution_width).toBe(1920);
  });

  it('starts the game at its size', async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    expect(settings.withProjectLaunchSize({ game_path: PRE })).toEqual({ game_path: PRE, resolution_width: 2560, resolution_height: 1440 });
    expect(settings.withProjectLaunchSize({ resolution_width: 800, resolution_height: 600 }))
      .toEqual({ resolution_width: 800, resolution_height: 600 });
  });

  it('takes a Configuration edit, unsaved until the project is saved', async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    form = appForm({ capture_fps: 300 });
    settings.noteFormSaved(form);
    expect(session.needsSavePrompt()).toBe(true);
    expect(settings.effectiveSetting('capture_fps', 120)).toBe(300);
    await session.saveProjectSession();
    expect(lastWritten().settings.picture).toEqual({ width: 2560, height: 1440, captureFps: 300 });
  });

  it('is unaffected by a settings save that changed none of its settings', async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    settings.noteFormSaved(appForm({ notify_error: false }));
    expect(session.needsSavePrompt()).toBe(false);
  });

  it('is gone after New Session: everything resolves to the form again', async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    await session.newSession();
    expect(settings.effectiveSetting('resolution_width', 1920)).toBe(1920);
    expect(settings.withProjectLaunchSize(null)).toBe(null);
  });
});

describe('loadPcId', () => {
  it('keeps a saved id', () => {
    expect(settings.loadPcId('abc')).toBe(false);
    expect(settings.getPcId()).toBe('abc');
  });

  it('makes one for a file without, and says the file needs saving', () => {
    expect(settings.loadPcId(undefined)).toBe(true);
    expect(settings.getPcId()).not.toBe('');
  });
});

describe('the Project settings page (#1085, step 6)', () => {
  const own = { picture: { width: 2560, height: 1440 } };

  it("gets the open project's block to edit", async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    expect(settings.blockForPage()).toMatchObject({ form, pcId: 'pc-a', block: own });
  });

  it('gets a block made from Configuration for a session not saved yet', async () => {
    const session = openSession(projectText());
    await session.newSession({ quiet: true });
    const { block } = settings.blockForPage();
    expect(block.picture).toMatchObject({ width: 1920, height: 1080 });
    expect(block.folders['pc-a'].destinations).toEqual(['E:/captures']);
  });

  it('applies a block: the project is unsaved, captures use it, and the save writes it', async () => {
    const session = openSession(projectText({ schema: 3, settings: own }));
    await session.openProject(PATH);
    settings.applyBlockFromPage({ ...own, clipLength: { beforeSeconds: 2 } });
    expect(session.needsSavePrompt()).toBe(true);
    expect(settings.effectiveSetting('record_start_lead', 5)).toBe(2);
    expect(form.record_start_lead).toBeUndefined();
    await session.saveProjectSession();
    expect(lastWritten().settings.clipLength).toEqual({ beforeSeconds: 2 });
  });

  it('keeps another project from opening while the page says Stay', async () => {
    const session = openSession(projectText());
    settings.setProjectSettingsPageLeave(async () => false);
    ipc.readProjectFile.mockClear();
    try {
      expect(await session.openProject(PATH)).toBe(false);
      expect(ipc.readProjectFile).not.toHaveBeenCalled();
    } finally {
      settings.setProjectSettingsPageLeave(async () => true);
    }
  });

  it('tells the page when a project opens under it', async () => {
    const session = openSession(projectText());
    let told = 0;
    settings.onProjectBlockReplaced(() => { told += 1; });
    try {
      await session.openProject(PATH);
      expect(told).toBe(1);
    } finally {
      settings.onProjectBlockReplaced(() => {});
    }
  });
});
