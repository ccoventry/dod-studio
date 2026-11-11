// The one snapshot of Configuration every older project starts from (#1085,
// decision 6): its layout, the per-PC folders, schema migration, and taking
// it once.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({
  getAppVersion: vi.fn(async () => '0.12.0'),
  keepProjectSettingsSnapshot: vi.fn(),
  logFrontendEvent: vi.fn(),
  takeCaptureStates: vi.fn(async () => []),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const {
  snapshotFromForm, usableSnapshot, blockFromSnapshot, snapshotDay, startSnapshot, snapshotInEffect, resetSnapshotForTests,
} = await import('./project_settings_snapshot.js');
const migrations = await import('./project_migrations.js');
const { CURRENT_SCHEMA } = migrations;
const { resolveSettings } = await import('./project_settings_logic.js');
const ipc = await import('./ipc_bridge.js');

const PRE = 'D:/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/hl.exe';
const form = (extra = {}) => ({
  capture_mode: 'frame_sequence', resolution_width: 1920, resolution_height: 1080, capture_fps: 120,
  clip_name_template: '{player}', target_drives: ['E:/captures'], render_export_dirs: ['F:/clips'], hl_path: PRE,
  init_commands: ['mirv_fov 90'], custom_commands: [], ...extra,
});
const TAKEN = '2026-11-11T02:00:00.000Z';

beforeEach(() => {
  resetSnapshotForTests();
  vi.mocked(ipc.logFrontendEvent).mockClear();
});

describe('snapshotFromForm', () => {
  it('holds the block, stamped, with the folders apart from it and under no PC id', () => {
    const snap = snapshotFromForm(form(), { takenAt: TAKEN, appVersion: '0.12.0', schema: CURRENT_SCHEMA });
    expect(snap).toMatchObject({ schema: CURRENT_SCHEMA, takenAt: TAKEN, appVersion: '0.12.0' });
    expect(snap.settings.picture).toMatchObject({ width: 1920, height: 1080, captureFps: 120 });
    expect(snap.settings.install.madeWith.hlExe).toBe(PRE);
    expect(snap.settings.folders).toBeUndefined();
    expect(snap.folders).toEqual({ destinations: ['E:/captures'], exports: ['F:/clips'] });
  });

  it('is a copy: a later change to the form does not reach it', () => {
    const f = form();
    const snap = snapshotFromForm(f, { takenAt: TAKEN });
    f.init_commands.push('r_decals 4096');
    expect(snap.settings.clipSettings.initCommands).toEqual(['mirv_fov 90']);
  });
});

describe('blockFromSnapshot', () => {
  const snap = snapshotFromForm(form(), { takenAt: TAKEN });

  it('puts the folders under this PC\'s id, which resolves them on this PC only', () => {
    const block = blockFromSnapshot(snap, 'pc-a');
    expect(block.folders).toEqual({ 'pc-a': { destinations: ['E:/captures'], exports: ['F:/clips'] } });
    expect(resolveSettings({ target_drives: ['G:/now'] }, block, 'pc-a').target_drives).toEqual(['E:/captures']);
    expect(resolveSettings({ target_drives: ['G:/now'] }, block, 'pc-b').target_drives).toEqual(['G:/now']);
  });

  it('gives each project its own copy', () => {
    const a = blockFromSnapshot(snap, 'pc-a');
    a.picture.width = 640;
    expect(blockFromSnapshot(snap, 'pc-a').picture.width).toBe(1920);
  });

  it('drops folders a hand-edited snapshot put inside its block', () => {
    const block = blockFromSnapshot({ ...snap, settings: { ...snap.settings, folders: { other: {} } } }, 'pc-a');
    expect(Object.keys(block.folders)).toEqual(['pc-a']);
  });
});

describe('usableSnapshot', () => {
  const snap = snapshotFromForm(form(), { takenAt: TAKEN, schema: CURRENT_SCHEMA });
  const usableSnapshotOf = (raw) => usableSnapshot(raw, migrations);

  it('takes one of this schema as it is', () => {
    expect(usableSnapshotOf(snap)).toEqual(snap);
  });

  it('migrates one from an older schema rather than taking it again', () => {
    const old = usableSnapshotOf({ ...snap, schema: 2 });
    expect(old.schema).toBe(CURRENT_SCHEMA);
    expect(old.settings).toEqual(snap.settings);
    expect(old.takenAt).toBe(TAKEN);
  });

  it('uses one from a newer build as it is, unknown fields kept', () => {
    const newer = { ...snap, schema: CURRENT_SCHEMA + 1, settings: { ...snap.settings, picture: { ...snap.settings.picture, fov: 90 } } };
    expect(usableSnapshotOf(newer).settings.picture.fov).toBe(90);
    expect(usableSnapshotOf(newer).schema).toBe(CURRENT_SCHEMA + 1);
  });

  it('refuses something with no block, or a schema no step leads on from', () => {
    expect(usableSnapshotOf(null)).toBe(null);
    expect(usableSnapshotOf({ takenAt: TAKEN })).toBe(null);
    expect(usableSnapshotOf({ ...snap, schema: 0 })).toBe(null);
  });
});

describe('startSnapshot', () => {
  it('reads the form at once and keeps it when none was taken before', async () => {
    const keep = vi.fn(async (snapshot) => ({ snapshot, taken: true }));
    const f = form();
    const started = startSnapshot(() => f, { keep, now: () => new Date(TAKEN) });
    f.resolution_width = 640;
    const snap = await started;
    expect(snap.settings.picture.width).toBe(1920);
    expect(snap.takenAt).toBe(TAKEN);
    expect(snap.appVersion).toBe('0.12.0');
    expect(snap.schema).toBe(CURRENT_SCHEMA);
    expect(await snapshotInEffect()).toBe(snap);
  });

  it('answers with the snapshot already kept, not the form', async () => {
    const earlier = snapshotFromForm(form({ capture_fps: 60 }), { takenAt: '2026-11-01T00:00:00.000Z', schema: CURRENT_SCHEMA });
    const keep = vi.fn(async () => ({ snapshot: earlier, taken: false }));
    const snap = await startSnapshot(() => form(), { keep });
    expect(snap.settings.picture.captureFps).toBe(60);
  });

  it('is null, and says so in the log, when the backend fails', async () => {
    const keep = vi.fn(async () => { throw new Error('disk full'); });
    expect(await startSnapshot(() => form(), { keep })).toBe(null);
    expect(vi.mocked(ipc.logFrontendEvent).mock.calls[0][0]).toContain('disk full');
  });

  it('is null with no form, and before it is started', async () => {
    expect(await snapshotInEffect()).toBe(null);
    expect(await startSnapshot(() => null, { keep: vi.fn() })).toBe(null);
  });
});

describe('snapshotDay', () => {
  it('names the day in plain words', () => {
    expect(snapshotDay('2026-11-11T18:00:00.000Z')).toBe('November 11, 2026');
    expect(snapshotDay('not a date')).toBe('not a date');
  });
});
