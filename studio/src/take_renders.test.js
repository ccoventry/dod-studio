import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { pruneTakeIndex, markBlockCaptured, resolveTake } from './take_index.js';
import { attachCaptures, capturesToSave, recordCapture } from './take_record.js';
import {
  rendersOf, recordRender, recordTakeFolderRenders, attachRenders, rendersToSave, carryRenders,
  projectRenderedVideos, existingVideos,
} from './take_renders.js';
import * as before1264Index from './test_fixtures/take_index_before_1264.js';
import * as before1264Record from './test_fixtures/take_record_before_1264.js';

const oldProject = () => JSON.parse(readFileSync(new URL('./test_fixtures/project_before_439.json', import.meta.url), 'utf8'));
const FIRST = 'C:/demos/fotw.dem#3#100#400';
const SECOND = 'C:/demos/fotw.dem#4#300#700';
const KEY = 'session_b/chain_00_b0';

const finished = (started, extra = {}) => ({
  started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished',
  output_path: `D:/exports/clip_${started}.mov`, output_size_bytes: 10, ...extra,
});

/** What project_session.js writes and reads, in the parts #439 and #1264 touch. */
function saveProject(takeIndex, knownUids) {
  const pruned = pruneTakeIndex(takeIndex, knownUids);
  return JSON.stringify({ takeIndex: pruned, takeCaptures: capturesToSave(pruned), takeRenders: rendersToSave(pruned) }, null, 2);
}
function loadProject(json) {
  const data = JSON.parse(json);
  const takeIndex = data.takeIndex || {};
  attachCaptures(takeIndex, data.takeCaptures);
  attachRenders(takeIndex, data.takeRenders);
  return takeIndex;
}

/** What an older build (origin/dev before #1264) writes and reads, verbatim readers. */
function oldBuildSave(takeIndex, knownUids) {
  const pruned = before1264Index.pruneTakeIndex(takeIndex, knownUids);
  return JSON.stringify({ takeIndex: pruned, takeCaptures: before1264Record.capturesToSave(pruned) }, null, 2);
}
function oldBuildLoad(json) {
  const data = JSON.parse(json);
  const takeIndex = data.takeIndex || {};
  before1264Record.attachCaptures(takeIndex, data.takeCaptures);
  return takeIndex;
}

function capturedProject() {
  const project = oldProject();
  const takeIndex = loadProject(JSON.stringify(project));
  const streaks = project.demos[0].streaks;
  markBlockCaptured(
    { take_key: KEY, take_folder: 'D:/caps/session_b/chain_00_b0', source_streak_indices: [0, 1] },
    { sessionId: 'session_b', streaks, demoPaths: [project.demos[0].path, project.demos[0].path], captureMode: 'direct_to_video', captureFps: 240 },
    takeIndex,
  );
  return takeIndex;
}

describe('a project that never rendered', () => {
  it('saves byte for byte as a build from before #1264 saves it', () => {
    const json = JSON.stringify(oldProject());
    expect(saveProject(loadProject(json), [FIRST, SECOND])).toBe(oldBuildSave(oldBuildLoad(json), [FIRST, SECOND]));
    const captured = capturedProject();
    expect(saveProject(captured, [FIRST, SECOND])).not.toContain('takeRenders');
  });
});

describe('a project saved with renders, opened by an older build', () => {
  function renderedProjectJson() {
    const takeIndex = capturedProject();
    recordRender(takeIndex, KEY, finished(1_700_000_100_000));
    recordRender(takeIndex, KEY, finished(1_700_000_200_000, { stream: 'hudcolor', codec: 'png' }));
    return saveProject(takeIndex, [FIRST, SECOND]);
  }

  it('keeps takeIndex as bare uid lists and takeCaptures as they were', () => {
    const data = JSON.parse(renderedProjectJson());
    Object.values(data.takeIndex).forEach((entry) => expect(Array.isArray(entry)).toBe(true));
    expect(data.takeRenders[KEY]).toHaveLength(2);
    expect(data.takeCaptures[KEY]).toMatchObject({ capture_fps: 240, take_folder: 'D:/caps/session_b/chain_00_b0' });
  });

  it("works with origin/dev's readers: resolve, captures, rename, prune, record", () => {
    const takeIndex = oldBuildLoad(renderedProjectJson());
    expect(before1264Index.resolveTake(takeIndex, KEY)).toEqual([FIRST, SECOND]);
    expect(before1264Record.captureOf(takeIndex, KEY)).toMatchObject({ capture_mode: 'direct_to_video', capture_fps: 240 });
    before1264Index.renameDemoInTakeIndex(takeIndex, 'C:/demos/fotw.dem', 'D:/new/fotw.dem');
    const pruned = before1264Index.pruneTakeIndex(takeIndex, ['D:/new/fotw.dem#3#100#400']);
    expect(pruned[KEY]).toEqual(['D:/new/fotw.dem#3#100#400']);
    expect(before1264Record.captureOf(pruned, KEY).capture_fps).toBe(240);
  });

  it('the older build saves it without takeRenders and otherwise unchanged (the accepted loss)', () => {
    const json = renderedProjectJson();
    const resaved = JSON.parse(oldBuildSave(oldBuildLoad(json), [FIRST, SECOND]));
    const original = JSON.parse(json);
    expect('takeRenders' in resaved).toBe(false);
    expect(resaved.takeIndex).toEqual(original.takeIndex);
    expect(resaved.takeCaptures).toEqual(original.takeCaptures);
  });

  it('this build reads that back with no renders, and the take folders fill them again', () => {
    const json = oldBuildSave(oldBuildLoad(renderedProjectJson()), [FIRST, SECOND]);
    const takeIndex = loadProject(json);
    expect(rendersOf(takeIndex, KEY)).toEqual([]);
    expect(recordTakeFolderRenders(takeIndex, KEY, [finished(1_700_000_100_000, { output_exists: true })])).toBe(1);
  });

  it('survives a save and load in this build', () => {
    const takeIndex = loadProject(renderedProjectJson());
    expect(rendersOf(takeIndex, KEY).map((r) => r.stream)).toEqual(['all', 'hudcolor']);
    expect(resolveTake(takeIndex, KEY)).toEqual([FIRST, SECOND]);
  });
});

describe('recordRender', () => {
  it('keeps the settings, path and time, not the outcome or disk state', () => {
    const index = { k: ['u'] };
    expect(recordRender(index, 'k', finished(5, { output_exists: true, error: null }))).toBe(true);
    expect(rendersOf(index, 'k')).toEqual([{ started_unix_ms: 5, stream: 'all', codec: 'prores', fps: 60, output_path: 'D:/exports/clip_5.mov', output_size_bytes: 10 }]);
  });

  it('records each render once, both halves of a HUD pair, oldest first', () => {
    const index = { k: ['u'] };
    recordRender(index, 'k', finished(9));
    expect(recordRender(index, 'k', finished(9))).toBe(false);
    recordRender(index, 'k', finished(9, { stream: 'hudcolor' }));
    recordRender(index, 'k', finished(3));
    expect(rendersOf(index, 'k').map((r) => [r.started_unix_ms, r.stream])).toEqual([[3, 'all'], [9, 'all'], [9, 'hudcolor']]);
  });

  it('ignores a take the project does not know, and unfinished or pathless attempts', () => {
    const index = { k: ['u'] };
    expect(recordRender(index, 'other', finished(1))).toBe(false);
    expect(recordRender(index, 'k', finished(1, { outcome: 'failed' }))).toBe(false);
    expect(recordRender(index, 'k', finished(1, { output_path: '' }))).toBe(false);
    expect(recordRender(index, 'k', null)).toBe(false);
    expect(rendersToSave(index)).toBeUndefined();
  });

  it('is never part of the index when it is serialised or listed', () => {
    const index = { k: ['u'] };
    recordRender(index, 'k', finished(1));
    expect(JSON.stringify(index)).toBe('{"k":["u"]}');
    expect(Object.keys(index)).toEqual(['k']);
  });
});

describe('pruning and loading', () => {
  it('pruning carries the renders of the takes it keeps and drops the rest', () => {
    const index = { keep: ['u'], drop: ['gone'] };
    recordRender(index, 'keep', finished(1));
    recordRender(index, 'drop', finished(2));
    expect(Object.keys(rendersToSave(pruneTakeIndex(index, ['u'])))).toEqual(['keep']);
    const to = {};
    carryRenders(index, to);
    expect(rendersToSave(to)).toBeUndefined();
  });

  it('a bad takeRenders in a project file is ignored', () => {
    const index = { k: ['u'] };
    attachRenders(index, 'nope');
    attachRenders(index, [1]);
    attachRenders(index, { k: 'nope' });
    attachRenders(index, { k: [{ started_unix_ms: 1 }] });
    expect(rendersToSave(index)).toBeUndefined();
  });
});

describe('projectRenderedVideos (the whole-project export list)', () => {
  it('lists takes in capture order, undated last, each take oldest render first', () => {
    const index = { late: ['a'], early: ['b'], undated: ['c'], unrendered: ['d'] };
    recordCapture(index, 'late', { captured_at: 200 });
    recordCapture(index, 'early', { captured_at: 100 });
    recordRender(index, 'late', finished(7));
    recordRender(index, 'early', finished(9));
    recordRender(index, 'early', finished(8, { stream: 'hudcolor' }));
    recordRender(index, 'undated', finished(1));
    expect(projectRenderedVideos(index).map((v) => [v.takeKey, v.started_unix_ms])).toEqual([
      ['early', 8], ['early', 9], ['late', 7], ['undated', 1],
    ]);
    expect(projectRenderedVideos(index)[0].uids).toEqual(['b']);
  });

  it('existingVideos keeps the found ones at their current paths', () => {
    const videos = [{ output_path: 'a' }, { output_path: 'b' }, { output_path: 'c' }];
    expect(existingVideos(videos, ['a', null, 'E:/moved/c'])).toEqual([{ output_path: 'a' }, { output_path: 'E:/moved/c' }]);
    expect(existingVideos(videos, null)).toEqual([]);
  });

  it('nothing for an index with no renders', () => {
    expect(projectRenderedVideos({ k: ['u'] })).toEqual([]);
    expect(projectRenderedVideos(undefined)).toEqual([]);
  });
});
