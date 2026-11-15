// The whole path a project file takes (#1085): the real project_session.js and
// rescan_recent.js on an in-memory disk, against a schema 1 file shaped like a
// real movie project and the frozen save logic of a build from before #1085.

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { projectSchema1, SCHEMA1_TAKES } from './test_fixtures/project_schema1.js';
import { loadBefore1085, saveBefore1085 } from './test_fixtures/project_save_before_1085.js';

const disk = vi.hoisted(() => new Map());
const saveDialog = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: saveDialog }));
vi.mock('./ipc_bridge.js', () => ({
  filesExist: vi.fn(async (paths) => paths.map((p) => disk.has(p))),
  readProjectFile: vi.fn(async (path) => disk.get(path)),
  writeProjectFile: vi.fn(async (path, text) => { disk.set(path, text); }),
  defaultProjectsDir: vi.fn(async () => 'D:/projects'),
  takeCaptureStates: vi.fn(async (folders) => folders.map(() => ({ exists: true, renders: [] }))),
  logFrontendEvent: vi.fn(),
  estimateRescan: vi.fn(),
  getAppVersion: vi.fn(),
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

const indicator = { textContent: '', title: '', classList: { toggle: vi.fn() } };
// The offer's Rescan all button: focused as the dialog shows, so the click
// comes as soon as the dialog waits for an answer.
const rescanStart = {
  addEventListener(_, handler) { this.click = handler; },
  focus() { queueMicrotask(() => this.click()); },
};
const PAGE = { '#session-file-indicator': indicator, '#rescan-recent-start-btn': rescanStart };
globalThis.document = { querySelector: (selector) => PAGE[selector] ?? null };

const { createProjectSession } = await import('./project_session.js');
const { createRescanRecent } = await import('./rescan_recent.js');
const { showToast } = await import('./toast.js');
const { logFrontendEvent } = await import('./ipc_bridge.js');
const { CURRENT_SCHEMA } = await import('./project_migrations.js');
const { HIGHLIGHTS_VERSION } = await import('./scan_version.js');
const { STRINGS } = await import('./strings.js');

const PATH = 'D:\\p\\movie.json';
const backupOf = (date) => `D:\\p\\movie.before-v${CURRENT_SCHEMA}-${date}.dodstudio`;
const today = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

/** A session as main.js wires it, on the in-memory disk. */
function openSession() {
  const app = { demos: [], takeIndex: {}, teams: null, review: null };
  const session = createProjectSession({
    getDemos: () => app.demos,
    replaceDemos: (demos) => { app.demos = demos; },
    loadDemos: (demos) => { app.demos = demos; },
    getTakeIndex: () => app.takeIndex,
    setTakeIndex: (index) => { app.takeIndex = index; },
    getProjectTeams: () => app.teams,
    setProjectTeams: (teams) => { app.teams = teams; },
    checkMissingDemos: async () => {},
    getReviewSettings: () => app.review,
    setReviewSettings: (review) => { app.review = review; },
  });
  return { app, session };
}

/** Opens `project` from the disk, changes nothing but the unsaved mark, saves. */
async function openAndSave(project, path = PATH) {
  disk.set(path, JSON.stringify(project, null, 2));
  const { app, session } = openSession();
  await session.loadProjectFile(path);
  session.markProjectDirty();
  const saved = await session.saveProjectSession();
  return { app, session, saved, data: JSON.parse(disk.get(path)) };
}

const toasts = () => showToast.mock.calls.map(([message]) => message);
const clipsOf = (data) => data.demos.flatMap((demo) => demo.streaks);

beforeEach(() => {
  disk.clear();
  showToast.mockClear();
  logFrontendEvent.mockClear();
  saveDialog.mockReset();
});

describe('a schema 1 project in this build', () => {
  it('changes nothing on disk by being opened', async () => {
    const text = JSON.stringify(projectSchema1());
    disk.set(PATH, text);
    await openSession().session.loadProjectFile(PATH);
    expect(disk.get(PATH)).toBe(text);
    expect(disk.size).toBe(1);
  });

  it('saves everything it held, plus the two new numbers', async () => {
    const { data } = await openAndSave(projectSchema1());
    const { schema, saveNeeds, ...rest } = data;
    expect(schema).toBe(CURRENT_SCHEMA);
    expect(saveNeeds).toBe(1);
    expect(rest).toEqual(projectSchema1());
  });

  it('backs the old file up, byte for byte, on the first save only', async () => {
    const text = JSON.stringify(projectSchema1(), null, 2);
    const { app, session } = await openAndSave(projectSchema1());
    expect(disk.get(backupOf(today()))).toBe(text);
    app.demos[0].streaks[0].notes = 'changed';
    session.markProjectDirty();
    await session.saveProjectSession();
    expect([...disk.keys()].filter((p) => p.includes('.before-'))).toHaveLength(1);
  });

  it('says once what the update came to and where the backup is', async () => {
    const { session } = await openAndSave(projectSchema1());
    const summary = STRINGS.PROJECT_FILE.updatedToast({ clips: 5, takesFound: 2, takesNotFound: 1 }, backupOf(today()));
    expect(toasts()).toContain(summary);
    showToast.mockClear();
    session.markProjectDirty();
    await session.saveProjectSession();
    expect(toasts().join()).not.toContain('Updated:');
  });

  it('logs the update in the activity log, a quiet save too', async () => {
    disk.set(PATH, JSON.stringify(projectSchema1()));
    const { session } = openSession();
    await session.loadProjectFile(PATH, { quiet: true });
    session.markProjectDirty();
    await session.saveProjectSession({ quiet: true });
    expect(showToast).not.toHaveBeenCalled();
    const lines = logFrontendEvent.mock.calls.map(([line]) => line);
    expect(lines.some((line) => line.includes('[project] updated') && line.includes(backupOf(today())))).toBe(true);
  });

  it('keeps the project unsaved, and says why, when the backup can\'t be made', async () => {
    disk.set(PATH, JSON.stringify(projectSchema1()));
    const { writeProjectFile } = await import('./ipc_bridge.js');
    const { session } = openSession();
    await session.loadProjectFile(PATH);
    session.markProjectDirty();
    writeProjectFile.mockImplementationOnce(async () => { throw new Error('disk full'); });
    expect(await session.saveProjectSession()).toBe(false);
    expect(session.needsSavePrompt()).toBe(true);
    expect(toasts().join()).toContain('disk full');
    expect(JSON.parse(disk.get(PATH)).schema).toBeUndefined();
  });

  it('offers Save a copy when the backup can\'t be made, and carries on in the copy', async () => {
    const copy = 'D:\\p\\movie copy.dodstudio';
    const text = JSON.stringify(projectSchema1());
    disk.set(PATH, text);
    const { writeProjectFile } = await import('./ipc_bridge.js');
    saveDialog.mockResolvedValue(copy);
    const { session } = openSession();
    await session.loadProjectFile(PATH);
    session.markProjectDirty();
    writeProjectFile.mockImplementationOnce(async () => { throw new Error('disk full'); });
    expect(await session.saveProjectSession()).toBe(true);
    expect(saveDialog.mock.calls[0][0]).toMatchObject({ title: STRINGS.PROJECT_FILE.SAVE_COPY_DIALOG_TITLE, defaultPath: copy });
    expect(toasts().join()).toContain('disk full');
    expect(disk.get(PATH)).toBe(text);
    expect(clipsOf(JSON.parse(disk.get(copy)))).toHaveLength(clipsOf(projectSchema1()).length);
    expect(session.currentPath()).toBe(copy);
    expect(session.needsSavePrompt()).toBe(false);
  });

  it('offers Save a copy when the write fails, with a quiet save simply failing', async () => {
    disk.set(PATH, JSON.stringify({ ...projectSchema1(), schema: CURRENT_SCHEMA, saveNeeds: 1 }));
    const { writeProjectFile } = await import('./ipc_bridge.js');
    const { session } = openSession();
    await session.loadProjectFile(PATH, { quiet: true });
    session.markProjectDirty();
    writeProjectFile.mockImplementationOnce(async () => { throw new Error('rename refused'); });
    expect(await session.saveProjectSession({ quiet: true })).toBe(false);
    expect(saveDialog).not.toHaveBeenCalled();
    expect(session.needsSavePrompt()).toBe(true);
  });

  it('stays unsaved when a mark lands while the save writes', async () => {
    disk.set(PATH, JSON.stringify(projectSchema1()));
    const { writeProjectFile } = await import('./ipc_bridge.js');
    const { session } = openSession();
    await session.loadProjectFile(PATH);
    session.markProjectDirty();
    const write = writeProjectFile.getMockImplementation();
    writeProjectFile.mockImplementation(async (path, text) => {
      if (path === PATH) session.markProjectDirty();
      return write(path, text);
    });
    await session.saveProjectSession();
    writeProjectFile.mockImplementation(write);
    expect(session.needsSavePrompt()).toBe(true);
    expect(indicator.textContent).toContain('unsaved');
  });

  it('reports a save that worked as saved, even when its summary fails', async () => {
    const { takeCaptureStates } = await import('./ipc_bridge.js');
    takeCaptureStates.mockImplementationOnce(async (folders) => folders);
    takeCaptureStates.mockImplementationOnce(async () => { throw new Error('no answer'); });
    const { saved } = await openAndSave(projectSchema1());
    expect(saved).toBe(true);
  });

  it('counts only the takes it saved in the summary', async () => {
    const project = projectSchema1();
    project.takeIndex['session_gone/chain_01_b0'] = ['D:\\demos\\removed.dem#1#1#2'];
    await openAndSave(project);
    expect(toasts().join()).toContain('2 takes found, 1 not found');
  });

  it('refuses to erase what a newer build saved since this window opened the file', async () => {
    const copy = 'D:\\p\\movie copy.dodstudio';
    disk.set(PATH, JSON.stringify(projectSchema1()));
    const { session } = openSession();
    await session.loadProjectFile(PATH);
    const theirs = JSON.stringify({ ...projectSchema1(), schema: CURRENT_SCHEMA + 1, saveNeeds: 1, settings: { a: 1 } });
    disk.set(PATH, theirs);
    saveDialog.mockResolvedValue(copy);
    session.markProjectDirty();
    expect(await session.saveProjectSession()).toBe(true);
    expect(disk.get(PATH)).toBe(theirs);
    expect(disk.has(copy)).toBe(true);
    expect(toasts().join()).toContain(STRINGS.PROJECT_FILE.newerOnDisk(PATH));
  });

  it('forgets the file\'s held keys on New Session', async () => {
    disk.set(PATH, JSON.stringify({ ...projectSchema1(), settings: { a: 1 } }));
    const { app, session } = openSession();
    await session.loadProjectFile(PATH);
    await session.newSession({ quiet: true });
    app.demos = projectSchema1().demos;
    saveDialog.mockResolvedValue('D:/p/other.dodstudio');
    session.markProjectDirty();
    await session.saveProjectSession();
    expect(JSON.parse(disk.get('D:/p/other.dodstudio')).settings).toBeUndefined();
  });
});

describe('old build to new build to old build', () => {
  const statuses = (data) => clipsOf(data).map((s) => [s.status, s.curation, s.notes, s.clipName, s.start_index]);

  it('a project an older build saved opens here with everything', async () => {
    const oldText = saveBefore1085(loadBefore1085(JSON.stringify(projectSchema1())));
    disk.set(PATH, oldText);
    const { app, session } = openSession();
    await session.loadProjectFile(PATH);
    expect(app.demos).toEqual(projectSchema1().demos);
    expect(Object.keys(app.takeIndex)).toEqual(Object.values(SCHEMA1_TAKES));
    expect(session.currentPath()).toBe(PATH);
  });

  it('a project this build saved reads back in the older build unchanged', async () => {
    const { data } = await openAndSave(projectSchema1());
    const old = loadBefore1085(JSON.stringify(data));
    expect(old.demos).toEqual(projectSchema1().demos);
    expect(Object.keys(old.takeIndex)).toEqual(Object.values(SCHEMA1_TAKES));
    expect(old.teams).toEqual(projectSchema1().teams);
    expect(old.review).toEqual(projectSchema1().review);
  });

  it('the older build saves it back with clips, takes, captures, renders and statuses intact', async () => {
    const { data } = await openAndSave(projectSchema1());
    const back = JSON.parse(saveBefore1085(loadBefore1085(JSON.stringify(data))));
    expect(statuses(back)).toEqual(statuses(projectSchema1()));
    expect(back.takeIndex).toEqual(projectSchema1().takeIndex);
    expect(back.takeCaptures).toEqual(projectSchema1().takeCaptures);
    expect(back.takeRenders).toEqual(projectSchema1().takeRenders);
  });

  it('coming back from the older build, the file is schema 1 again and gets a second backup', async () => {
    const { data } = await openAndSave(projectSchema1());
    const back = saveBefore1085(loadBefore1085(JSON.stringify(data)));
    await openAndSave(JSON.parse(back));
    expect(disk.has(backupOf(`${today()}-2`))).toBe(true);
  });
});

describe('the compatibility table, row by row', () => {
  const later = (extra) => ({ ...projectSchema1(), schema: CURRENT_SCHEMA + 1, saveNeeds: 1, ...extra });

  it('new top-level keys: an older build drops them', () => {
    const back = JSON.parse(saveBefore1085(loadBefore1085(JSON.stringify(later({ settings: { a: 1 }, profile: 'FOTW' })))));
    expect(back.settings).toBeUndefined();
    expect(back.profile).toBeUndefined();
  });

  it('new top-level keys: a build from step 1 on keeps them, unused', async () => {
    const { data } = await openAndSave(later({ settings: { a: 1 }, profile: 'FOTW' }));
    expect(data).toMatchObject({ settings: { a: 1 }, profile: 'FOTW' });
  });

  const variantRow = (data) => clipsOf(data).find((row) => row.variantOf);

  const withVariant = () => {
    const project = later();
    const clip = project.demos[0].streaks[0];
    project.demos[0].streaks.push({
      ...clip, uid: 'clip~v1', variantOf: 'clip', variant: { name: 'X-ray', commands: [] }, status: 'Pending',
    });
    project.takeIndex['session_y/chain_01_b0'] = ['clip~v1'];
    return project;
  };

  it('variant rows: an older build keeps the row, its saved uid and its take', () => {
    const back = JSON.parse(saveBefore1085(loadBefore1085(JSON.stringify(withVariant()))));
    expect(variantRow(back)).toMatchObject({ uid: 'clip~v1', variantOf: 'clip', variant: { name: 'X-ray' } });
    expect(back.takeIndex['session_y/chain_01_b0']).toEqual(['clip~v1']);
  });

  it('variant rows: a build from step 1 on keeps the row and its take', async () => {
    const { data } = await openAndSave(withVariant());
    expect(variantRow(data)).toMatchObject({ uid: 'clip~v1', variant: { name: 'X-ray' } });
    expect(data.takeIndex['session_y/chain_01_b0']).toEqual(['clip~v1']);
  });

  const withTakeSettings = () => {
    const project = later();
    project.takeCaptures[SCHEMA1_TAKES.FIRST_TAKE].settings = { hash: 'abc', width: 2560 };
    return project;
  };

  it('fields inside takeCaptures: an older build keeps them', () => {
    const back = JSON.parse(saveBefore1085(loadBefore1085(JSON.stringify(withTakeSettings()))));
    expect(back.takeCaptures[SCHEMA1_TAKES.FIRST_TAKE].settings).toEqual({ hash: 'abc', width: 2560 });
  });

  it('fields inside takeCaptures: a build from step 1 on keeps them', async () => {
    const { data } = await openAndSave(withTakeSettings());
    expect(data.takeCaptures[SCHEMA1_TAKES.FIRST_TAKE].settings).toEqual({ hash: 'abc', width: 2560 });
  });
});

describe('a project from a newer build', () => {
  const newer = (extra) => ({ ...projectSchema1(), schema: CURRENT_SCHEMA + 2, saveNeeds: 1, settings: { a: 1 }, ...extra });

  it('says so on the header line', async () => {
    disk.set(PATH, JSON.stringify(newer()));
    await openSession().session.loadProjectFile(PATH);
    expect(indicator.textContent).toContain(STRINGS.PROJECT_FILE.MADE_WITH_NEWER);
    expect(indicator.title).toContain(STRINGS.PROJECT_FILE.NEWER_TITLE);
  });

  it('saves normally, keeping its schema and new settings, with no backup', async () => {
    const { data } = await openAndSave(newer());
    expect(data).toMatchObject({ schema: CURRENT_SCHEMA + 2, settings: { a: 1 } });
    expect(disk.size).toBe(1);
  });

  it('toasts what happens to its new settings on the first save only', async () => {
    const { session } = await openAndSave(newer());
    expect(toasts()).toContain(STRINGS.PROJECT_FILE.NEWER_SAVED_TOAST);
    showToast.mockClear();
    session.markProjectDirty();
    await session.saveProjectSession();
    expect(toasts()).not.toContain(STRINGS.PROJECT_FILE.NEWER_SAVED_TOAST);
  });

  it('with saveNeeds past this build, Save asks for a copy and leaves the file alone', async () => {
    const copy = 'D:\\p\\movie copy.dodstudio';
    const text = JSON.stringify(newer({ saveNeeds: 2 }));
    disk.set(PATH, text);
    saveDialog.mockResolvedValue(copy);
    const { session } = openSession();
    await session.loadProjectFile(PATH);
    session.markProjectDirty();
    expect(await session.saveProjectSession()).toBe(true);
    expect(saveDialog.mock.calls[0][0]).toMatchObject({ title: STRINGS.PROJECT_FILE.SAVE_COPY_DIALOG_TITLE, defaultPath: copy });
    expect(disk.get(PATH)).toBe(text);
    expect(JSON.parse(disk.get(copy))).toMatchObject({ saveNeeds: 2, settings: { a: 1 } });
    expect(indicator.title).toContain(STRINGS.PROJECT_FILE.NEEDS_COPY_TITLE);
  });

  it('with saveNeeds past this build, Save a copy onto the file itself is refused', async () => {
    const text = JSON.stringify(newer({ saveNeeds: 2 }));
    disk.set(PATH, text);
    saveDialog.mockResolvedValue(PATH);
    const { session } = openSession();
    await session.loadProjectFile(PATH);
    session.markProjectDirty();
    expect(await session.saveProjectSession()).toBe(false);
    expect(disk.get(PATH)).toBe(text);
    expect(toasts().join()).toContain('newer DoD Studio');
  });

  it('with saveNeeds past this build, a quiet save is refused', async () => {
    const text = JSON.stringify(newer({ saveNeeds: 2 }));
    disk.set(PATH, text);
    const { session } = openSession();
    await session.loadProjectFile(PATH, { quiet: true });
    session.markProjectDirty();
    expect(await session.saveProjectSession({ quiet: true })).toBe(false);
    expect(saveDialog).not.toHaveBeenCalled();
    expect(disk.get(PATH)).toBe(text);
  });
});

describe('Rescan Recent Sessions', () => {
  it('backs each project up, and unknown keys survive, across a run over several', async () => {
    const paths = ['D:\\p\\a.json', 'D:\\p\\b.dodstudio'];
    const outdated = (demo) => ({ ...demo, highlights_version: HIGHLIGHTS_VERSION - 1, map_name: 'x', teams: [], signon_maps: ['x'] });
    paths.forEach((path) => {
      const project = { ...projectSchema1(), settings: { from: path } };
      project.demos = project.demos.map(outdated);
      disk.set(path, JSON.stringify(project));
      disk.set(project.demos[0].path, 'demo');
    });
    const originals = paths.map((path) => disk.get(path));
    const { app, session } = openSession();
    const scan = vi.fn(async () => {
      app.demos[0].streaks[0].notes = 'rescanned';
      session.markProjectDirty();
      return true;
    });
    const rescan = createRescanRecent({
      getRecent: () => paths,
      session,
      getDemos: () => app.demos,
      scan,
      persistAppSettings: async () => {},
      estimate: async () => ({}),
    });
    const result = await rescan.offer();
    expect(result).toMatchObject({ done: 2, failed: 0 });
    paths.forEach((path, i) => {
      const data = JSON.parse(disk.get(path));
      expect(data.settings).toEqual({ from: path });
      expect(data.schema).toBe(CURRENT_SCHEMA);
      expect(data.demos[0].streaks[0].notes).toBe('rescanned');
      const stem = path.replace(/\.(json|dodstudio)$/, '');
      expect(disk.get(`${stem}.before-v${CURRENT_SCHEMA}-${today()}.dodstudio`)).toBe(originals[i]);
      expect(disk.get(`${stem}.before-rescan-${today()}.json`)).toBe(originals[i]);
    });
  });
});

