import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { pruneTakeIndex, markBlockCaptured, recordTake, resolveTake, renameDemoInTakeIndex } from './take_index.js';
import {
  captureOf, recordCapture, attachCaptures, capturesToSave, carryCaptures, captureRateOf,
} from './take_record.js';
import * as before439 from './test_fixtures/take_index_before_439.js';

// A project saved before #439: every take is a bare uid list.
const oldProject = () => JSON.parse(readFileSync(new URL('./test_fixtures/project_before_439.json', import.meta.url), 'utf8'));
const FIRST = 'C:/demos/fotw.dem#3#100#400';
const SECOND = 'C:/demos/fotw.dem#4#300#700';

/** What project_session.js writes and reads back, in the parts #439 touches. */
function saveProject(takeIndex, knownUids) {
  const pruned = pruneTakeIndex(takeIndex, knownUids);
  return JSON.stringify({ takeIndex: pruned, takeCaptures: capturesToSave(pruned) });
}
function loadProject(json) {
  const data = JSON.parse(json);
  const takeIndex = data.takeIndex || {};
  attachCaptures(takeIndex, data.takeCaptures);
  return takeIndex;
}

describe('an old project (before #439)', () => {
  it('loads and saves unchanged: no takeCaptures key appears, takeIndex is as it was', () => {
    const project = oldProject();
    const saved = JSON.parse(saveProject(loadProject(JSON.stringify(project)), [FIRST, SECOND, 'C:/demos/removed.dem#1#10#20']));
    expect(saved.takeIndex).toEqual(project.takeIndex);
    expect('takeCaptures' in saved).toBe(false);
  });

  it('a take with no capture facts reads as unknown, and its uids still resolve', () => {
    const takeIndex = loadProject(JSON.stringify(oldProject()));
    expect(captureOf(takeIndex, 'session_a/chain_01_b1')).toBeUndefined();
    expect(resolveTake(takeIndex, 'session_a/chain_01_b1')).toEqual([FIRST, SECOND]);
  });

  it('renames keep working', () => {
    const takeIndex = loadProject(JSON.stringify(oldProject()));
    renameDemoInTakeIndex(takeIndex, 'C:/demos/fotw.dem', 'D:/new/fotw.dem');
    expect(takeIndex['session_a/chain_00_b0']).toEqual(['D:/new/fotw.dem#3#100#400']);
  });
});

describe('a project saved by this build', () => {
  function capturedProject() {
    const project = oldProject();
    const takeIndex = loadProject(JSON.stringify(project));
    const streaks = project.demos[0].streaks;
    const dispatch = { sessionId: 'session_b', streaks, demoPaths: [project.demos[0].path, project.demos[0].path], captureMode: 'direct_to_video', captureFps: 240 };
    markBlockCaptured(
      { take_key: 'session_b/chain_00_b0', take_folder: 'D:/caps/session_b/chain_00_b0', source_streak_indices: [0, 1] },
      dispatch, takeIndex,
    );
    return takeIndex;
  }

  it('keeps takeIndex as bare uid lists and puts the facts in takeCaptures', () => {
    const saved = JSON.parse(saveProject(capturedProject(), [FIRST, SECOND]));
    Object.values(saved.takeIndex).forEach((entry) => expect(Array.isArray(entry)).toBe(true));
    expect(saved.takeIndex['session_b/chain_00_b0']).toEqual([FIRST, SECOND]);
    expect(saved.takeCaptures['session_b/chain_00_b0']).toMatchObject({
      session_id: 'session_b', capture_mode: 'direct_to_video', capture_fps: 240, take_folder: 'D:/caps/session_b/chain_00_b0',
    });
    expect(typeof saved.takeCaptures['session_b/chain_00_b0'].captured_at).toBe('number');
  });

  it('survives a save and load', () => {
    const reloaded = loadProject(saveProject(capturedProject(), [FIRST, SECOND]));
    expect(captureOf(reloaded, 'session_b/chain_00_b0').capture_fps).toBe(240);
  });

  it("works with the code an older build runs (origin/dev's take_index.js)", () => {
    const data = JSON.parse(saveProject(capturedProject(), [FIRST, SECOND]));
    // An older build loads takeIndex as is, resolves, renames, prunes and saves it.
    expect(before439.resolveTake(data.takeIndex, 'session_b/chain_00_b0')).toEqual([FIRST, SECOND]);
    before439.renameDemoInTakeIndex(data.takeIndex, 'C:/demos/fotw.dem', 'D:/new/fotw.dem');
    before439.renameUidsInTakeIndex(data.takeIndex, new Map([['D:/new/fotw.dem#3#100#400', 'x#1#1#1']]));
    const pruned = before439.pruneTakeIndex(data.takeIndex, ['x#1#1#1', 'D:/new/fotw.dem#4#300#700']);
    expect(pruned['session_b/chain_00_b0']).toEqual(['x#1#1#1', 'D:/new/fotw.dem#4#300#700']);
    const recorded = {};
    before439.recordTake(recorded, 'k', ['u']);
    expect(recorded).toEqual({ k: ['u'] });
  });

  it('a catch-up plus a rename still leaves every old reader working', async () => {
    const { applyCaughtUpBatch } = await import('./batch_catch_up.js');
    const { markerRows } = await import('./marker_list.js');
    const { renameUidsInTakeIndex } = await import('./take_index.js');
    const project = oldProject();
    const takeIndex = loadProject(JSON.stringify(project));
    const streaks = project.demos[0].streaks;
    const uids = streaks.map((s) => `${project.demos[0].path}#${s.player_index}#${s.start_tick}#${s.end_tick}`);
    applyCaughtUpBatch({
      session_id: 'session_c', capture_mode: 'obs', obs_capture_fps: 90, streak_uids: uids,
      blocks: [{ take_key: 'session_c/chain_00_b0', captured: true, take_folder: 'D:/caps/c', source_streak_indices: [0] }],
    }, project.demos, takeIndex);
    renameDemoInTakeIndex(takeIndex, 'C:/demos/fotw.dem', 'D:/new/fotw.dem');
    renameUidsInTakeIndex(takeIndex, new Map([['D:/new/fotw.dem#3#100#400', 'D:/new/fotw.dem#3#101#400']]));
    expect(captureOf(takeIndex, 'session_c/chain_00_b0')).toMatchObject({ capture_mode: 'obs', capture_fps: 90 });
    expect(captureOf(takeIndex, 'session_c/chain_00_b0').captured_at).toBeUndefined();

    const data = JSON.parse(saveProject(takeIndex, ['D:/new/fotw.dem#3#101#400', 'D:/new/fotw.dem#4#300#700']));
    Object.values(data.takeIndex).forEach((entry) => expect(Array.isArray(entry)).toBe(true));
    // The readers an older build runs, on what this build saved.
    expect(() => markerRows(project.demos, data.takeIndex)).not.toThrow();
    expect(before439.resolveTake(data.takeIndex, 'session_c/chain_00_b0')).toEqual(['D:/new/fotw.dem#3#101#400']);
    expect(() => before439.pruneTakeIndex(data.takeIndex, [])).not.toThrow();
  });

  it('this build reads what an older build saved back (it drops takeCaptures)', () => {
    const data = JSON.parse(saveProject(capturedProject(), [FIRST, SECOND]));
    delete data.takeCaptures;
    const reloaded = loadProject(JSON.stringify(data));
    expect(resolveTake(reloaded, 'session_b/chain_00_b0')).toEqual([FIRST, SECOND]);
    expect(captureOf(reloaded, 'session_b/chain_00_b0')).toBeUndefined();
  });
});

describe('capture facts', () => {
  it('a newer capture into the same key overwrites older facts, an unknown one leaves them', () => {
    const index = {};
    recordCapture(index, 's/b0', { captured_at: 1, capture_fps: 120, session_id: 's' });
    recordCapture(index, 's/b0', { captured_at: 2, capture_fps: undefined, take_folder: '' });
    expect(captureOf(index, 's/b0')).toEqual({ captured_at: 2, capture_fps: 120, session_id: 's' });
  });

  it('are not part of the index when it is serialised or listed', () => {
    const index = { k: ['u'] };
    recordTake(index, 'k', ['u'], { captured_at: 1 });
    expect(JSON.stringify(index)).toBe('{"k":["u"]}');
    expect(Object.keys(index)).toEqual(['k']);
  });

  it('pruning carries the facts of the takes it keeps', () => {
    const index = { keep: ['u'], drop: ['gone'] };
    recordCapture(index, 'keep', { captured_at: 1 });
    recordCapture(index, 'drop', { captured_at: 2 });
    expect(capturesToSave(pruneTakeIndex(index, ['u']))).toEqual({ keep: { captured_at: 1 } });
    const to = {};
    carryCaptures(index, to);
    expect(capturesToSave(to)).toBeUndefined();
  });

  it('a bad takeCaptures in a project file is ignored', () => {
    const index = {};
    attachCaptures(index, 'nope');
    attachCaptures(index, [1]);
    expect(capturesToSave(index)).toBeUndefined();
  });
});

describe('captureRateOf (#439)', () => {
  it('names the rate the mode really records at, not the frame-sequence setting', () => {
    const settings = { capture_fps: 300, obs_capture_fps: 120, agr_fps: 60 };
    expect(captureRateOf('obs', settings)).toBe(120);
    expect(captureRateOf('obs', { capture_fps: 300, obs_capture_fps: 0 })).toBe(60);
    expect(captureRateOf('obs', { capture_fps: 300 })).toBeUndefined();
    expect(captureRateOf('agr', settings)).toBe(60);
    expect(captureRateOf('agr', { capture_fps: 300, agr_fps: 0 })).toBe(300);
    expect(captureRateOf('direct_to_video', settings)).toBe(300);
  });

  it("a dispatch's own rate wins over the clip payload's capture_fps", () => {
    const takeIndex = {};
    const streaks = [{ player_index: 1, start_tick: 1, end_tick: 2 }];
    markBlockCaptured(
      { take_key: 's/b0', capture_fps: 300, source_streak_indices: [0] },
      { streaks, demoPaths: ['d.dem'], captureMode: 'obs', captureFps: 120 }, takeIndex,
    );
    expect(captureOf(takeIndex, 's/b0').capture_fps).toBe(120);
  });
});
