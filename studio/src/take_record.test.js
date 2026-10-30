import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import {
  recordTake, resolveTake, pruneTakeIndex, renameDemoInTakeIndex, renameUidsInTakeIndex, markBlockCaptured, streakUid,
} from './take_index.js';
import { uidsOf, mapUids, recordInto, captureRateOf } from './take_record.js';
import { markerRows } from './marker_list.js';

// A project saved before #439: every take is a bare uid list.
const oldProject = () => JSON.parse(readFileSync(new URL('./test_fixtures/project_before_439.json', import.meta.url), 'utf8'));
const FIRST = 'C:/demos/fotw.dem#3#100#400';
const SECOND = 'C:/demos/fotw.dem#4#300#700';

describe('take record shapes (#439)', () => {
  it('reads the uids of either shape', () => {
    expect(uidsOf([FIRST])).toEqual([FIRST]);
    expect(uidsOf({ uids: [FIRST], capture_fps: 300 })).toEqual([FIRST]);
    expect(uidsOf(undefined)).toEqual([]);
  });

  it('recordInto unions uids and writes only the capture facts it was given', () => {
    const record = recordInto([FIRST], [SECOND], { capture_fps: 300, capture_mode: undefined, take_folder: '' });
    expect(record).toEqual({ uids: [FIRST, SECOND], capture_fps: 300 });
  });

  it('a newer capture into the same key overwrites the older facts', () => {
    const first = recordInto(undefined, [FIRST], { captured_at: 1, capture_fps: 120, session_id: 's' });
    expect(recordInto(first, [FIRST], { captured_at: 2, capture_fps: 300 })).toEqual({
      uids: [FIRST], captured_at: 2, capture_fps: 300, session_id: 's',
    });
  });

  it('mapUids keeps the shape it was given', () => {
    expect(mapUids([FIRST], (u) => u)).toEqual([FIRST]);
    expect(mapUids({ uids: [FIRST], capture_fps: 300 }, () => [])).toEqual({ uids: [], capture_fps: 300 });
  });
});

describe('an old project (bare uid lists)', () => {
  it('still resolves its takes and lists its markers', () => {
    const { takeIndex, demos } = oldProject();
    expect(resolveTake(takeIndex, 'session_a/chain_01_b1')).toEqual([FIRST, SECOND]);
    expect(markerRows(demos, takeIndex).map((row) => row[10])).toEqual(['session_a/chain_00_b0', 'session_a/chain_01_b1']);
  });

  it('saving it back loses nothing: pruning keeps the shape and drops only dead uids', () => {
    const { takeIndex } = oldProject();
    const pruned = pruneTakeIndex(takeIndex, [FIRST, SECOND]);
    expect(pruned).toEqual({
      'session_a/chain_00_b0': [FIRST],
      'session_a/chain_01_b1': [FIRST, SECOND],
    });
    expect(JSON.parse(JSON.stringify(pruned))).toEqual(pruned);
  });

  it('renames follow a moved or split demo without changing the shape', () => {
    const { takeIndex } = oldProject();
    renameDemoInTakeIndex(takeIndex, 'C:/demos/fotw.dem', 'D:/new/fotw.dem');
    expect(takeIndex['session_a/chain_00_b0']).toEqual(['D:/new/fotw.dem#3#100#400']);
    renameUidsInTakeIndex(takeIndex, new Map([['D:/new/fotw.dem#3#100#400', 'x#1#1#1']]));
    expect(takeIndex['session_a/chain_00_b0']).toEqual(['x#1#1#1']);
  });

  it('a new capture of a highlight leaves the old takes as they were and adds a record', () => {
    const { takeIndex, demos } = oldProject();
    const before = JSON.stringify(takeIndex);
    const streaks = demos[0].streaks;
    const dispatch = { sessionId: 'session_b', streaks, demoPaths: [demos[0].path, demos[0].path], captureMode: 'direct_to_video' };
    markBlockCaptured(
      { take_key: 'session_b/chain_00_b0', take_folder: 'D:/captures/session_b/chain_00_b0', source_streak_indices: [0] },
      dispatch, takeIndex, 240,
    );
    expect(streakUid(demos[0].path, streaks[0])).toBe(FIRST);
    const record = takeIndex['session_b/chain_00_b0'];
    expect(record).toMatchObject({
      uids: [FIRST], session_id: 'session_b', capture_mode: 'direct_to_video', capture_fps: 240,
      take_folder: 'D:/captures/session_b/chain_00_b0',
    });
    expect(typeof record.captured_at).toBe('number');
    const { 'session_b/chain_00_b0': _added, ...rest } = takeIndex;
    expect(JSON.stringify(rest)).toBe(before);
  });

  it('recording into an old key upgrades just that entry to a record', () => {
    const { takeIndex } = oldProject();
    recordTake(takeIndex, 'session_a/chain_00_b0', [FIRST], { captured_at: 5 });
    expect(takeIndex['session_a/chain_00_b0']).toEqual({ uids: [FIRST], captured_at: 5 });
    expect(Array.isArray(takeIndex['session_a/chain_01_b1'])).toBe(true);
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

  it('a dispatch\'s own rate wins over the clip payload\'s capture_fps', () => {
    const takeIndex = {};
    const streaks = [{ player_index: 1, start_tick: 1, end_tick: 2 }];
    markBlockCaptured(
      { take_key: 's/b0', capture_fps: 300, source_streak_indices: [0] },
      { streaks, demoPaths: ['d.dem'], captureMode: 'obs', captureFps: 120 }, takeIndex,
    );
    expect(takeIndex['s/b0'].capture_fps).toBe(120);
  });
});
