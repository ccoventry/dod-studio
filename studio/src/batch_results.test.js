import { describe, it, expect } from 'vitest';
import { resultsOf, takeLabel } from './batch_results.js';

const dispatch = (over = {}) => ({
  streaks: [{ target_player: 'm00cat', kill_count: 4, kills: [[100, 61, 'k98']], start_index: 0, end_index: 0 }],
  ...over,
});
const block = (over = {}) => ({ demo_name: 'wsod25_grp5_h1', source_streak_indices: [0], ...over });

describe('takeLabel', () => {
  it('says nothing about gaps for a take recorded at real time', () => {
    expect(takeLabel(block(), dispatch())).not.toContain('gap');
  });

  it('notes a take whose gap between kills was recorded sped up (#831)', () => {
    const d = dispatch({ kill_gaps: { mode: 'fast_forward', over_seconds: 8, speed: 4 } });
    expect(takeLabel(block({ ramped_gaps: 1 }), d)).toMatch(/\(gap at 4x\)$/);
    expect(takeLabel(block({ ramped_gaps: 2 }), d)).toMatch(/\(2 gaps at 4x\)$/);
  });
});

describe('resultsOf', () => {
  it('shows a take OBS started late as late, with by how much (#1109)', () => {
    const { rows, totals } = resultsOf({ blocks: [block({ captured: true, renderable: true }), block({ started_late_ms: 240 })] }, dispatch());
    expect(rows.map((r) => r.status)).toEqual(['ok', 'late']);
    expect(rows[1].lateMs).toBe(240);
    expect(totals.captured).toBe(1);
  });
});
