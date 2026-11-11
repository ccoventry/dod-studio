import { describe, it, expect } from 'vitest';
import { recordCapture } from './take_record.js';
import { recordRender } from './take_renders.js';
import { capturesOf, folderList, describeCapture, renderEntriesOf, videoPathList } from './capture_history_logic.js';

const ME = 'd.dem#1#10#20';
const OTHER = 'd.dem#2#12#30';
const labelOfUid = (uid) => (uid === OTHER ? 'Ann at 0:12' : null);
const formatSize = (b) => `${b} B`;
const context = { labelOfUid, formatSize };

const index = {
  'old/chain_00_b0': [ME],
  'new1/chain_00_b0': [ME],
  'new2/chain_01_b1': [ME, OTHER, 'gone#1#1#1'],
  'unrelated/chain_00_b0': [OTHER],
};
recordCapture(index, 'new1/chain_00_b0', { session_id: 'new1', captured_at: 1_700_000_000_000, capture_mode: 'frame_sequence', capture_fps: 300, take_folder: 'D:/caps/new1/chain_00_b0' });
recordCapture(index, 'new2/chain_01_b1', { session_id: 'new2', captured_at: 1_700_100_000_000, capture_mode: 'obs', capture_fps: 120, take_folder: 'D:/caps/new2/chain_01_b1' });
recordCapture(index, 'unrelated/chain_00_b0', { captured_at: 1_700_200_000_000 });

describe('capturesOf', () => {
  it('lists the takes holding the highlight, newest first, undated old ones last', () => {
    expect(capturesOf(index, ME).map((c) => c.takeKey)).toEqual(['new2/chain_01_b1', 'new1/chain_00_b0', 'old/chain_00_b0']);
  });

  it('a take recorded before #439 has no capture facts', () => {
    expect(capturesOf(index, ME).at(-1)).toMatchObject({ uids: [ME], record: {} });
  });

  it('nothing for a highlight never captured, or no index', () => {
    expect(capturesOf(index, 'x#9#9#9')).toEqual([]);
    expect(capturesOf(undefined, ME)).toEqual([]);
  });

  it('folderList keeps the order, with a blank for an unrecorded folder', () => {
    expect(folderList(capturesOf(index, ME))).toEqual(['D:/caps/new2/chain_01_b1', 'D:/caps/new1/chain_00_b0', '']);
  });
});

describe('describeCapture', () => {
  const [shared, plain, old] = capturesOf(index, ME);

  it('names the mode and rate, the take, and who it is shared with', () => {
    const line = describeCapture(shared, ME, { exists: true, renders: [] }, context);
    expect(line.settings).toBe('OBS (real time) · 120 fps');
    expect(line.takeName).toBe('chain_01_b1');
    expect(line.status).toBe('exists');
    expect(line.shared).toEqual(['Ann at 0:12', 'a highlight no longer in the queue']);
    expect(line.when).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/);
  });

  it('a take that is not there is marked gone and its line stays', () => {
    const line = describeCapture(plain, ME, { exists: false, renders: [] }, context);
    expect(line.status).toBe('gone');
    expect(line.shared).toEqual([]);
    expect(line.settings).toBe('Frame sequence · 300 fps');
  });

  it('an old record has no date, settings or folder, and says so rather than guessing', () => {
    const line = describeCapture(old, ME, undefined, context);
    expect(line.status).toBe('unknown');
    expect(line.when).toBe('Captured earlier (date not recorded)');
    expect(line.settings).toBe('settings not recorded');
    expect(line.folder).toBe(null);
  });

  it('is checking until the disk answers, unchecked when the check failed', () => {
    expect(describeCapture(plain, ME, undefined, context).status).toBe('checking');
    expect(describeCapture(plain, ME, null, context).status).toBe('unchecked');
  });

  it('lists the renders newest first, with a video only for a finished one still there', () => {
    const attempt = (started, outcome, extra = {}) => ({ started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome, output_path: '', ...extra });
    const state = {
      exists: true,
      renders: [
        attempt(1_700_000_100_000, 'finished', { output_path: 'D:/out/a.mov', output_exists: true, output_size_bytes: 10 }),
        attempt(1_700_000_200_000, 'finished', { output_path: 'D:/out/b.mov', output_exists: false }),
        attempt(1_700_000_300_000, 'failed', { error: 'boom' }),
      ],
    };
    const { renders } = describeCapture(plain, ME, state, context);
    expect(renders).toHaveLength(3);
    expect(renders[0].text).toContain('Failed: boom');
    expect(renders[0].videoPath).toBe(null);
    expect(renders[1].text).toContain('b.mov');
    expect(renders[1].videoPath).toBe(null);
    expect(renders[2].videoPath).toBe('D:/out/a.mov');
  });
});

describe('the renders the project recorded (#1264)', () => {
  const recorded = (started, extra = {}) => ({ started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished', output_path: `D:/out/${started}.mov`, ...extra });
  const project = { 'del/chain_00_b0': [ME] };
  recordCapture(project, 'del/chain_00_b0', { captured_at: 1_700_000_000_000, take_folder: 'D:/caps/del/chain_00_b0' });
  recordRender(project, 'del/chain_00_b0', recorded(1_700_000_100_000));
  recordRender(project, 'del/chain_00_b0', recorded(1_700_000_200_000));
  const [capture] = capturesOf(project, ME);

  it('a deleted take still lists the videos rendered from it', () => {
    const line = describeCapture(capture, ME, { exists: false, renders: [] }, context);
    expect(line.status).toBe('gone');
    expect(line.renders).toHaveLength(2);
  });

  it('a video still there gets Show video; one moved is shown where it was found; one not found is missing', () => {
    const located = new Map([['D:/out/1700000100000.mov', 'E:/sorted/1700000100000.mov'], ['D:/out/1700000200000.mov', null]]);
    const { renders } = describeCapture(capture, ME, { exists: false, renders: [] }, { ...context, located });
    expect(renders[0]).toMatchObject({ videoPath: null, missing: true });
    expect(renders[0].text).toContain('was moved or deleted');
    expect(renders[1]).toMatchObject({ videoPath: 'E:/sorted/1700000100000.mov', missing: false });
  });

  it('before the lookup answers, a recorded video has no button and is not called missing', () => {
    const { renders } = describeCapture(capture, ME, undefined, context);
    expect(renders.every((r) => r.videoPath === null && !r.missing)).toBe(true);
  });

  it("a render both the take folder and the project hold is listed once, with the folder's outcome", () => {
    const state = { exists: true, renders: [recorded(1_700_000_100_000, { output_exists: true, output_size_bytes: 10 }), recorded(1_700_000_300_000, { outcome: 'failed', output_path: '' })] };
    const entries = renderEntriesOf(capture, state);
    expect(entries.map((e) => e.started_unix_ms)).toEqual([1_700_000_100_000, 1_700_000_200_000, 1_700_000_300_000]);
    expect(videoPathList([capture], [state])).toEqual(['D:/out/1700000100000.mov', 'D:/out/1700000200000.mov']);
  });
});
