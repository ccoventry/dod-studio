// A project's own settings through the session (#1085, build plan step 4):
// an old project gets Configuration's copied in once and saved with it, a
// project with its own settings resolves to them without touching the form,
// an edit in Configuration reaches the open project, and the Project settings
// page (step 6) edits its block through the session.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

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
  projectFovSources: vi.fn(async () => null),
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
// "Made with another install. Switch?" answered Switch; the switch picks it
// in the form, as Configuration's dropdown does.
vi.mock('./themed_confirm.js', async (original) => ({ ...(await original()), themedConfirm: vi.fn(async () => true) }));
vi.mock('./install_picker.js', async (original) => ({
  ...(await original()),
  switchInstall: vi.fn(async (hlExe) => {
    globalThis.__switchedTo = hlExe;
    return true;
  }),
}));

globalThis.document = { querySelector: () => null };

const { createProjectSession } = await import('./project_session.js');
const snapshots = await import('./project_settings_snapshot.js');
const settings = await import('./project_settings.js');
const newUi = await import('./new_ui.js');
const { SENT_ROWS, SENT_ONLY_WHEN_SET } = await import('./clip_settings_rows.js');
const { startingValues, blockWithPending } = await import('./project_settings_page_logic.js');
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

  it('copies the first Configuration edit after an Apply on a session not saved yet (review 7)', async () => {
    const session = openSession(projectText());
    await session.newSession({ quiet: true });
    settings.applyBlockFromPage({ ...settings.blockForPage().block, clipLength: { beforeSeconds: 2 } });
    form = appForm({ resolution_width: 2560 });
    settings.noteFormSaved(form);
    expect(settings.effectiveSetting('resolution_width', 1)).toBe(2560);
    expect(settings.effectiveSetting('record_start_lead', 5)).toBe(2);
  });

  describe('with the New UI on (review 2)', () => {
    const pageFolders = { ...own, folders: { 'pc-a': { destinations: ['F:/wsod'], exports: ['G:/renders'] } } };
    beforeEach(() => newUi.loadNewUi(true));
    afterEach(() => newUi.loadNewUi(false));

    it("keeps the folders set on the page when Configuration's change, and copies a single value", async () => {
      const session = openSession(projectText({ schema: 3, settings: pageFolders }));
      await session.openProject(PATH);
      form = appForm({ target_drives: ['E:/captures', 'H:/more'], render_export_dirs: ['I:/x'], capture_fps: 300 });
      settings.noteFormSaved(form);
      expect(settings.effectiveSetting('target_drives', [])).toEqual(['F:/wsod']);
      expect(settings.effectiveSetting('render_export_dirs', [])).toEqual(['G:/renders']);
      expect(settings.effectiveSetting('capture_fps', 120)).toBe(300);
      const lines = vi.mocked(ipc.logFrontendEvent).mock.calls.map(([line]) => line);
      expect(lines).toContain('[project] Configuration change not copied into the open project, which sets it on Project Settings: target_drives, render_export_dirs');
    });

    it('keeps the Clip settings set on the page when Configuration adds a line or picks a profile', async () => {
      const clip = { ...own, clipSettings: { preset: null, initCommands: ['ds_hud_hide_scoreboard 1', 'cl_bob 0'], customCommands: [] } };
      const session = openSession(projectText({ schema: 3, settings: clip }));
      await session.openProject(PATH);
      form = appForm({ init_commands: ['mirv_fov 90', 'r_decals 4096'], command_profile_active: 'Clean clips' });
      settings.noteFormSaved(form);
      expect(settings.effectiveSetting('init_commands', [])).toEqual(['ds_hud_hide_scoreboard 1', 'cl_bob 0']);
      expect(settings.effectiveSetting('command_profile_active', 'x')).toBe(null);
    });

    it('opens an older project with Use Clip settings off: its batch sends its lines as they were', async () => {
      const session = openSession(projectText());
      await session.openProject(PATH);
      expect(settings.blockForPage().block.clipSettings.useRows).toBeUndefined();
      expect(settings.effectiveCaptureSettings({ init_commands: ['x 1'] }).init_commands).toEqual(['mirv_fov 90']);
    });

    it('starts a new project with Use Clip settings on: its batch sends every row, and says how many', async () => {
      const session = openSession(projectText());
      await session.newSession({ quiet: true });
      expect(settings.blockForPage().block.clipSettings.useRows).toBe(true);
      vi.mocked(ipc.logFrontendEvent).mockClear();
      settings.applyBlockFromPage(settings.blockForPage().block);
      const sent = settings.effectiveCaptureSettings({ init_commands: [] }).init_commands;
      // Every row but the ones the game decides (ds_hd_enabled) with no line.
      const rows = SENT_ROWS.length - SENT_ONLY_WHEN_SET.length;
      expect(sent).toHaveLength(1 + rows);
      const lines = vi.mocked(ipc.logFrontendEvent).mock.calls.map(([line]) => line);
      expect(lines).toContain(`[project] Clip settings: sending ${rows} row(s) after 1 line(s) of the project's own commands`);
    });

    it('copies the folders as before once it is off', async () => {
      newUi.loadNewUi(false);
      const session = openSession(projectText({ schema: 3, settings: pageFolders }));
      await session.openProject(PATH);
      form = appForm({ target_drives: ['H:/more'] });
      settings.noteFormSaved(form);
      expect(settings.effectiveSetting('target_drives', [])).toEqual(['H:/more']);
    });
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

describe("an older project's field of view, with the New UI on (review 1, 9, 10)", () => {
  const POST = 'D:/Steam/steamapps/common/Half-Life - POST-Anniversary for Movies/hl.exe';
  const INSTALLS = [
    { hl_exe: PRE, name: 'Half-Life - PRE-Anniversary for Movies' },
    { hl_exe: POST, name: 'Half-Life - POST-Anniversary for Movies' },
  ];
  const madeWithPre = { picture: { width: 1920 }, install: { madeWith: { name: INSTALLS[0].name, hlExe: PRE } } };
  const nothingSet = { fromLines: null, fromGame: null, configs: [], unfollowedExecs: [], gameFolder: true };
  const wsodIn = (hlExe) => (hlExe === PRE
    ? { ...nothingSet, fromLines: { value: 110, file: 'dicewsod25.cfg', line: 4 } }
    : { ...nothingSet, unfollowedExecs: ['exec dicewsod25.cfg'] });

  beforeEach(async () => {
    // No project left open by an earlier test for the switch to fill.
    await openSession(projectText()).newSession({ quiet: true });
    newUi.loadNewUi(true);
    globalThis.__switchedTo = undefined;
    // POST picked in Configuration; a switch picks the other one.
    form = appForm({ hl_path: POST, init_commands: ['exec dicewsod25.cfg'] });
    settings.setFormReader(() => ({ ...form, hl_path: globalThis.__switchedTo || form.hl_path }));
    globalThis.document = { querySelector: (s) => (s === '#hl-path-input' ? { value: globalThis.__switchedTo || form.hl_path } : null) };
    vi.mocked(ipc.projectFovSources).mockReset().mockImplementation(async (hlExe) => wsodIn(hlExe));
  });
  afterEach(() => {
    newUi.loadNewUi(false);
    globalThis.document = { querySelector: () => null };
    vi.mocked(ipc.findInstalls).mockResolvedValue([]);
  });

  it('is read from the install the project switches to, after the offer, not the one picked when it opened', async () => {
    vi.mocked(ipc.findInstalls).mockResolvedValue(INSTALLS);
    const session = openSession(projectText({ schema: 3, settings: madeWithPre }));
    await session.openProject(PATH);
    expect(globalThis.__switchedTo).toBe(PRE);
    expect(ipc.projectFovSources).toHaveBeenCalledTimes(1);
    expect(ipc.projectFovSources).toHaveBeenCalledWith(PRE, ['exec dicewsod25.cfg']);
    expect(settings.projectPictureFov()).toBe(110);
  });

  it("is left unset, and logged, when the cfg its lines exec isn't in the install it uses", async () => {
    const session = openSession(projectText({ schema: 3, settings: madeWithPre }));
    await session.openProject(PATH);
    expect(settings.projectPictureFov()).toBe(null);
    const lines = vi.mocked(ipc.logFrontendEvent).mock.calls.map(([line]) => line);
    expect(lines.some((l) => l.includes("keeps none for now: a cfg its Initial Commands exec isn't in the picked install (exec dicewsod25.cfg)"))).toBe(true);
    // Nothing to save: the next open tries again.
    expect(session.needsSavePrompt()).toBe(false);
  });

  it('is not filled by a quiet open (Rescan Recent Sessions)', async () => {
    const session = openSession(projectText({ schema: 3, settings: madeWithPre }));
    await session.loadProjectFile(PATH, { quiet: true });
    expect(ipc.projectFovSources).not.toHaveBeenCalled();
    expect(settings.projectPictureFov()).toBe(null);
  });

  it('is not filled with no hl.exe picked', async () => {
    form = appForm({ hl_path: '', init_commands: ['mirv_fov 100'] });
    const session = openSession(projectText({ schema: 3, settings: { picture: { width: 1920 } } }));
    await session.openProject(PATH);
    expect(ipc.projectFovSources).not.toHaveBeenCalled();
    expect(settings.projectPictureFov()).toBe(null);
  });

  it("takes the start-up configs' value when its lines set none, so its look doesn't change", async () => {
    vi.mocked(ipc.projectFovSources).mockResolvedValue({ ...nothingSet, fromGame: { value: 105, file: 'movie.cfg', line: 1 } });
    form = appForm({ hl_path: POST, init_commands: ['cl_bob 0'] });
    const session = openSession(projectText({ schema: 3, settings: { picture: { width: 1920 } } }));
    await session.openProject(PATH);
    expect(settings.projectPictureFov()).toBe(105);
  });

  it("gives Apply on a session not saved yet what the game uses, not the lines' 90 (review 9)", async () => {
    form = appForm({ hl_path: PRE, init_commands: ['exec dicewsod25.cfg'] });
    const session = openSession(projectText());
    await session.newSession({ quiet: true });
    const { form: now, block, pcId } = settings.blockForPage();
    const fov = await settings.pageFov(block, now);
    expect(fov).toEqual({ shown: 110, fill: 110 });
    const start = { ...startingValues(now, block, pcId), capture_fov: fov.shown };
    const next = blockWithPending(block, start, { ...start, resolution_width: 2560 }, pcId, { fillFov: fov.fill });
    settings.applyBlockFromPage(next);
    expect(settings.effectiveCaptureSettings({ resolution_width: 1 })).toEqual({ resolution_width: 2560, picture_fov: 110 });
  });

  it("writes no field of view on Apply when what the game uses can't be told", async () => {
    const session = openSession(projectText());
    await session.newSession({ quiet: true });
    const { form: now, block, pcId } = settings.blockForPage();
    const fov = await settings.pageFov(block, now);
    expect(fov).toEqual({ shown: 90, fill: null });
    const start = { ...startingValues(now, block, pcId), capture_fov: fov.shown };
    settings.applyBlockFromPage(blockWithPending(block, start, { ...start, resolution_width: 2560 }, pcId, { fillFov: fov.fill }));
    expect(settings.projectPictureFov()).toBe(null);
  });
});
