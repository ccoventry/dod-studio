// The owner's own project through old, new and old again (#1085), on a copy.
// Local only, the frontend's `#[ignore]`: skipped unless DS_ROUNDTRIP_PROJECT
// is set, and never under CI. Set, it fails loudly when the file isn't there.
// It only reads that file; everything else happens in memory.
//
//   DS_ROUNDTRIP_PROJECT=<path to a copy> npx vitest run --dir src project_roundtrip_local

import { describe, it, expect, vi } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({
  filesExist: vi.fn(), readProjectFile: vi.fn(), writeProjectFile: vi.fn(), takeCaptureStates: vi.fn(), logFrontendEvent: vi.fn(),
}));

const { openProjectText, buildProjectFile, saveProjectFile } = await import('./project_file.js');
const { CURRENT_SCHEMA } = await import('./project_migrations.js');
const { attachCaptures, capturesToSave } = await import('./take_record.js');
const { attachRenders, rendersToSave } = await import('./take_renders.js');
const { pruneTakeIndex, streakUid } = await import('./take_index.js');
const { loadBefore1085, saveBefore1085 } = await import('./test_fixtures/project_save_before_1085.js');

const PATH = process.env.DS_ROUNDTRIP_PROJECT;

/** What project_session.js saves for a project just opened from `opened`. */
function saveInThisBuild(opened) {
  const { data } = opened;
  const takeIndex = data.takeIndex || {};
  attachCaptures(takeIndex, data.takeCaptures);
  attachRenders(takeIndex, data.takeRenders);
  const uids = data.demos.flatMap((demo) => demo.streaks.map((streak) => streakUid(demo.path, streak)));
  const pruned = pruneTakeIndex(takeIndex, uids);
  return buildProjectFile({
    scanPaths: data.scanPaths, demos: data.demos, takeIndex: pruned, takeCaptures: capturesToSave(pruned),
    takeRenders: rendersToSave(pruned), teams: data.teams, review: data.review, mode: 'workspace',
  }, opened);
}

const facts = (data) => ({
  demos: data.demos.length,
  clips: data.demos.flatMap((demo) => demo.streaks).length,
  rows: data.demos.flatMap((demo) => demo.streaks).map((s) => [s.status, s.curation, s.notes, s.selected, s.start_index, s.end_index]),
  takeIndex: data.takeIndex,
  takeCaptures: data.takeCaptures,
  takeRenders: data.takeRenders,
  teams: data.teams,
  review: data.review,
});

describe.runIf(PATH !== undefined && !process.env.CI)('a real project round trip', () => {
  it('finds the file DS_ROUNDTRIP_PROJECT names', () => {
    expect(PATH && existsSync(PATH), `DS_ROUNDTRIP_PROJECT is "${PATH}": no such file`).toBeTruthy();
  });

  it('survives old to new to old with its clips, takes and statuses', async () => {
    const original = readFileSync(PATH, 'utf8');
    const before = facts(JSON.parse(original));

    const built = saveInThisBuild(openProjectText(original));
    const written = new Map([['copy.dodstudio', original]]);
    await saveProjectFile('copy.dodstudio', built, {
      exists: async (paths) => paths.map((p) => written.has(p)),
      read: async (p) => written.get(p),
      write: async (p, text) => { written.set(p, text); },
      log: () => {},
    });
    const saved = written.get('copy.dodstudio');
    expect(JSON.parse(saved).schema).toBe(CURRENT_SCHEMA);
    expect([...written.keys()].some((p) => p.includes('.before-v'))).toBe(true);
    expect(facts(JSON.parse(saved))).toEqual(before);

    const back = saveBefore1085(loadBefore1085(saved));
    expect(facts(JSON.parse(back))).toEqual(before);
    expect(facts(openProjectText(back).data)).toEqual(before);
  }, 60000);
});
