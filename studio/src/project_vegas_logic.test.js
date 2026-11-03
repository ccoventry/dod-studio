import { describe, it, expect } from 'vitest';
import { tickedUids, videosToExport, clipsOfVideos } from './project_vegas_logic.js';
import { streakUid } from './take_index.js';

const streak = (start_tick, selected) => ({ start_tick, end_tick: start_tick + 10, selected });
const demos = [{ path: 'a.dem', streaks: [streak(1, true), streak(2, false)] }];
const [s1, s2] = demos[0].streaks;
const u1 = streakUid('a.dem', s1);
const u2 = streakUid('a.dem', s2);
const videos = [
  // The streams a take folder's history records (`attempt_stream` in
  // native's take_cleanup.rs): the image folder, `hud:`-prefixed for a
  // HUD-only render.
  { takeKey: 't1', uids: [u1], output_path: 'C:/v/one.mov', stream: 'all' },
  { takeKey: 't2', uids: [u2], output_path: 'C:/v/two.mov', stream: 'all' },
  { takeKey: 't2', uids: [u2], output_path: 'C:/v/two_hud.mov', stream: 'hud:hudcolor' },
];

describe('project VEGAS export', () => {
  it('finds the ticked highlights', () => {
    expect([...tickedUids(demos)]).toEqual([u1]);
  });
  it('exports only the ticked highlights videos when some are ticked', () => {
    const r = videosToExport(videos, demos);
    expect(r.scope).toBe('ticked');
    expect(r.videos.map((v) => v.output_path)).toEqual(['C:/v/one.mov']);
  });
  it('exports every render, HUD passes too, when nothing is ticked', () => {
    const r = videosToExport(videos, [{ path: 'a.dem', streaks: [streak(1, false)] }]);
    expect(r.scope).toBe('all');
    expect(r.videos).toHaveLength(3);
  });
  it('falls back to all when the ticked highlights have no video', () => {
    const r = videosToExport(videos.slice(1), demos);
    expect(r.scope).toBe('all');
    expect(r.videos).toHaveLength(2);
  });
  it('makes clips, marks the HUD pass and counts unreadable lengths', () => {
    const takeIndex = { t2: [u2] };
    Object.defineProperty(takeIndex, 'takeCaptures', { value: { t2: { take_folder: 'C:/takes/t2' } }, enumerable: false });
    const lengths = new Map([['C:/v/two.mov', 4], ['C:/v/two_hud.mov', 4]]);
    const { clips, skipped } = clipsOfVideos(videos, takeIndex, lengths);
    expect(skipped).toBe(1);
    expect(clips.map((c) => [c.name, c.hud, c.takeFolder])).toEqual([
      ['two.mov', false, 'C:/takes/t2'],
      ['two_hud.mov', true, 'C:/takes/t2'],
    ]);
  });
});
