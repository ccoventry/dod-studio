import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({
  filesExist: vi.fn(), readProjectFile: vi.fn(), writeProjectFile: vi.fn(), takeCaptureStates: vi.fn(), logFrontendEvent: vi.fn(),
}));

const {
  SAVE_LEVEL, ProjectFileError, schemaOf, saveNeedsOf, openProjectText, isNewerProject, canSaveOver,
  buildProjectFile, localDate, backupPath, freeBackupPath, writeProjectBackup, saveProjectFile,
} = await import('./project_file.js');
const { CURRENT_SCHEMA } = await import('./project_migrations.js');
const { projectSchema1 } = await import('./test_fixtures/project_schema1.js');
const { STRINGS } = await import('./strings.js');

/** An in-memory disk with the io saveProjectFile takes. */
function fakeDisk(files = {}) {
  const disk = new Map(Object.entries(files));
  return {
    disk,
    exists: async (paths) => paths.map((p) => disk.has(p)),
    read: async (p) => disk.get(p),
    write: async (p, text) => { disk.set(p, text); },
    log: vi.fn(),
    now: () => new Date(2026, 10, 6, 12).getTime(),
  };
}

const PATH = 'D:/p/monday.dodstudio';
const BACKUP = `D:/p/monday.before-v${CURRENT_SCHEMA}-2026-11-06.dodstudio`;
const SCHEMA1_TEXT = JSON.stringify({ version: '0.12.0', demos: [] });

describe('the file says what it is', () => {
  it('reads a file with no schema as schema 1, needing level 1', () => {
    expect(schemaOf({})).toBe(1);
    expect(saveNeedsOf({})).toBe(1);
  });

  it('reads a bad schema as 1 rather than trust it', () => {
    ['3', 1.5, 0, -2, null].forEach((schema) => expect(schemaOf({ schema })).toBe(1));
  });

  it('this build saves at level 1, and writes saveNeeds 1', () => {
    expect(SAVE_LEVEL).toBe(1);
    expect(JSON.parse(buildProjectFile({}).json).saveNeeds).toBe(1);
  });
});

describe('openProjectText', () => {
  it('returns a schema 1 file migrated, saying it was schema 1', () => {
    const opened = openProjectText(JSON.stringify(projectSchema1()));
    expect(opened.schema).toBe(1);
    expect(opened.data).toEqual(projectSchema1());
    expect(opened.unknownKeys).toEqual({});
  });

  it('holds the top-level keys this build does not know', () => {
    const opened = openProjectText(JSON.stringify({ demos: [], settings: { a: 1 }, profile: 'FOTW' }));
    expect(opened.unknownKeys).toEqual({ settings: { a: 1 }, profile: 'FOTW' });
  });

  it('does not hold the old install paths: a project carrying them switched the game on open', () => {
    const opened = openProjectText(JSON.stringify({ hlaePath: 'C:/x/hlae.exe', hlPath: 'C:/x/hl.exe' }));
    expect(opened.unknownKeys).toEqual({});
  });

  it('reads a file holding just null as no project', () => {
    expect(openProjectText('null').data).toBe(null);
  });

  it('throws on text that is not JSON', () => {
    expect(() => openProjectText('{ not json')).toThrow();
  });
});

describe('a file from a newer build', () => {
  it('counts as newer only past this build\'s schema', () => {
    expect(isNewerProject({ schema: CURRENT_SCHEMA + 1 })).toBe(true);
    expect(isNewerProject({ schema: CURRENT_SCHEMA })).toBe(false);
    expect(isNewerProject(null)).toBe(false);
  });

  it('can be saved over until it asks for a higher level', () => {
    expect(canSaveOver({ saveNeeds: SAVE_LEVEL })).toBe(true);
    expect(canSaveOver({ saveNeeds: SAVE_LEVEL + 1 })).toBe(false);
    expect(canSaveOver(null)).toBe(true);
  });

  it('keeps its own schema and saveNeeds on save, so the newer build runs no step twice', () => {
    const opened = openProjectText(JSON.stringify({ schema: CURRENT_SCHEMA + 3, saveNeeds: 2, settings: { a: 1 } }));
    const { json, schema } = buildProjectFile({ demos: [] }, opened);
    expect(schema).toBe(CURRENT_SCHEMA + 3);
    expect(JSON.parse(json)).toMatchObject({ schema: CURRENT_SCHEMA + 3, saveNeeds: 2, settings: { a: 1 } });
  });
});

describe('buildProjectFile', () => {
  it('stamps the format at the top of the file, version staying 0.12.0', () => {
    const data = JSON.parse(buildProjectFile({ demos: [] }).json);
    expect(Object.keys(data).slice(0, 3)).toEqual(['version', 'schema', 'saveNeeds']);
    expect(data).toMatchObject({ version: '0.12.0', schema: CURRENT_SCHEMA });
  });

  it('writes the held keys back after this build\'s own', () => {
    const opened = openProjectText(JSON.stringify({ settings: { a: 1 } }));
    const data = JSON.parse(buildProjectFile({ demos: [] }, opened).json);
    expect(Object.keys(data)).toEqual(['version', 'schema', 'saveNeeds', 'demos', 'settings']);
  });

  it('stamps over the old values of a whole file edited in place', () => {
    const opened = openProjectText(JSON.stringify({ schema: 1, demos: [] }));
    expect(JSON.parse(buildProjectFile(opened.data, opened).json).schema).toBe(CURRENT_SCHEMA);
  });
});

describe('backup names', () => {
  const rescan = { label: 'rescan', date: '2026-10-23', extension: 'json' };

  it('go beside the file, named after it and the date', () => {
    expect(backupPath('H:\\movies\\WSOD25 main.json', rescan)).toBe('H:\\movies\\WSOD25 main.before-rescan-2026-10-23.json');
    expect(backupPath('D:/p/monday.json', rescan)).toBe('D:/p/monday.before-rescan-2026-10-23.json');
  });

  it('take the format they came before, in Studio\'s own file type, from either kind of project', () => {
    const name = { label: 'v2', date: '2026-11-06', extension: 'dodstudio' };
    expect(backupPath('D:/p/monday.json', name)).toBe('D:/p/monday.before-v2-2026-11-06.dodstudio');
    expect(backupPath('D:/p/monday.dodstudio', name)).toBe('D:/p/monday.before-v2-2026-11-06.dodstudio');
  });

  it('count up for a second one the same day, so the first is kept', () => {
    expect(backupPath('D:/p/monday.json', { ...rescan, n: 2 })).toBe('D:/p/monday.before-rescan-2026-10-23-2.json');
  });

  it('never overwrite an earlier backup: no free name, or no way to tell, means no backup', () => {
    const tries = ['a.1.json', 'a.2.json', 'a.3.json'];
    expect(freeBackupPath(tries, [true, false, false])).toBe('a.2.json');
    expect(freeBackupPath(tries, [true, true, true])).toBe(null);
    expect(freeBackupPath(tries, null)).toBe(null);
  });

  it('date in local time, zero-padded', () => {
    expect(localDate(new Date(2026, 0, 2))).toBe('2026-01-02');
  });
});

describe('writeProjectBackup', () => {
  it('fails rather than overwrite when it can\'t tell which names are taken', async () => {
    const write = vi.fn();
    await expect(writeProjectBackup(PATH, 'x', { label: 'v2', extension: 'dodstudio' }, { date: '2026-11-06', exists: async () => null, write }))
      .rejects.toThrow('overwriting');
    expect(write).not.toHaveBeenCalled();
  });
});

describe('saveProjectFile', () => {
  const built = (extra) => buildProjectFile({ demos: [], ...extra });

  it('backs up the file byte for byte on the save that raises its schema', async () => {
    const io = fakeDisk({ [PATH]: SCHEMA1_TEXT });
    const raised = await saveProjectFile(PATH, built(), io);
    expect(raised).toEqual({ backup: BACKUP, fromSchema: 1 });
    expect(io.disk.get(BACKUP)).toBe(SCHEMA1_TEXT);
    expect(JSON.parse(io.disk.get(PATH)).schema).toBe(CURRENT_SCHEMA);
  });

  it('logs the backup\'s start and end', async () => {
    const io = fakeDisk({ [PATH]: SCHEMA1_TEXT });
    await saveProjectFile(PATH, built(), io);
    expect(io.log).toHaveBeenCalledTimes(2);
    expect(io.log.mock.calls[1][0]).toContain(BACKUP);
  });

  it('makes no second backup once the file is raised', async () => {
    const io = fakeDisk({ [PATH]: SCHEMA1_TEXT });
    await saveProjectFile(PATH, built(), io);
    expect(await saveProjectFile(PATH, built({ scanPaths: [] }), io)).toBe(null);
    expect(io.disk.size).toBe(2);
  });

  it('makes a new backup, not an overwrite, when an older build brought the file back down', async () => {
    const io = fakeDisk({ [PATH]: SCHEMA1_TEXT, [BACKUP]: 'first' });
    const raised = await saveProjectFile(PATH, built(), io);
    expect(raised.backup).toBe(BACKUP.replace('-2026-11-06', '-2026-11-06-2'));
    expect(io.disk.get(BACKUP)).toBe('first');
  });

  it('makes no backup for a new file', async () => {
    const io = fakeDisk();
    expect(await saveProjectFile(PATH, built(), io)).toBe(null);
    expect([...io.disk.keys()]).toEqual([PATH]);
  });

  it('keeps a file it can\'t parse, as a backup, instead of losing it under the save', async () => {
    const io = fakeDisk({ [PATH]: '{ cut off' });
    expect((await saveProjectFile(PATH, built(), io)).fromSchema).toBe(1);
    expect(io.disk.get(BACKUP)).toBe('{ cut off');
  });

  it('writes nothing when the backup fails', async () => {
    const io = fakeDisk({ [PATH]: SCHEMA1_TEXT });
    io.write = vi.fn(async () => { throw new Error('disk full'); });
    const save = saveProjectFile(PATH, built(), io);
    await expect(save).rejects.toBeInstanceOf(ProjectFileError);
    await expect(save).rejects.toThrow('disk full');
    expect(io.write).toHaveBeenCalledTimes(1);
    expect(io.disk.get(PATH)).toBe(SCHEMA1_TEXT);
  });

  it('writes nothing when it can\'t tell whether the file is there', async () => {
    const io = fakeDisk({ [PATH]: SCHEMA1_TEXT });
    io.exists = async () => null;
    await expect(saveProjectFile(PATH, built(), io)).rejects.toBeInstanceOf(ProjectFileError);
    expect(io.disk.get(PATH)).toBe(SCHEMA1_TEXT);
  });

  it('refuses to save over a file that needs a newer build', async () => {
    const text = JSON.stringify({ schema: CURRENT_SCHEMA + 1, saveNeeds: SAVE_LEVEL + 1 });
    const io = fakeDisk({ [PATH]: text });
    await expect(saveProjectFile(PATH, built(), io)).rejects.toThrow('newer DoD Studio');
    expect(io.disk.get(PATH)).toBe(text);
    expect(io.disk.size).toBe(1);
  });
});

describe('the one-time summary', () => {
  it('names the counts and the backup', () => {
    expect(STRINGS.PROJECT_FILE.updatedToast({ clips: 364, takesFound: 45, takesNotFound: 3 }, 'D:/p/a.before-v2.dodstudio'))
      .toBe('Updated: 364 clips, 45 takes found, 3 not found. Backup: D:/p/a.before-v2.dodstudio');
  });

  it('leaves the takes out when the disk check failed', () => {
    expect(STRINGS.PROJECT_FILE.updatedToast({ clips: 1, takesFound: null, takesNotFound: null }, 'b'))
      .toBe('Updated: 1 clip. Backup: b');
  });
});
