import { describe, it, expect } from 'vitest';
import { isDamagedDemo, repairProgressView, gapsText, repairResultText, repairGapsTitle } from './repair_view.js';

const repaired = {
  verdict: 'repaired',
  written: 'D:/demos/match_repaired.dem',
  holes: [
    { before_secs: 39.2, after_secs: 68.6, lost_secs: 29.4, bytes_skipped: 1 },
    { before_secs: 816.3, after_secs: 828.8, lost_secs: 12.5, bytes_skipped: 1 },
  ],
  seconds_lost: 41.9,
  percent_kept: 96.71,
  ends_at_last_hole: false,
  reasons: [],
};

describe('isDamagedDemo', () => {
  it('is a demo the scan found damaged', () => {
    expect(isDamagedDemo({ damage: { read_up_to_secs: 507 } })).toBe(true);
  });
  it('is not a whole demo, nor one from an older scan', () => {
    expect(isDamagedDemo({ damage: null })).toBe(false);
    expect(isDamagedDemo({})).toBe(false);
  });
});

describe('repairProgressView', () => {
  it('names the step and the whole repair\'s percentage', () => {
    expect(repairProgressView({ step: 'reading', fraction: 0.355 }))
      .toEqual({ pct: 36, text: 'Reading the recording… 36%' });
  });
  it('starts on the first step before any progress arrives', () => {
    expect(repairProgressView(null)).toEqual({ pct: 0, text: 'Finding the gaps… 0%' });
  });
});

describe('gapsText', () => {
  it('lists each gap with the seconds it lost', () => {
    expect(gapsText(repaired.holes)).toBe('0:39–1:08 (29 s), 13:36–13:48 (13 s)');
  });
});

describe('repairResultText', () => {
  it('says how many gaps, how much was lost and the file it saved', () => {
    expect(repairResultText(repaired))
      .toBe('Repaired: 2 gaps cut out (in all 42 s of recording lost, 96.7% kept). Saved as match_repaired.dem.');
  });
  it('says when the recording breaks once more after the last gap and never resumes', () => {
    expect(repairResultText({ ...repaired, ends_at_last_hole: true })).toMatch(/breaks once more and never resumes/);
  });
  it('lists no gap for damage that runs to the end, only where the repair ends', () => {
    const text = repairResultText({ ...repaired, holes: [], seconds_lost: 0, percent_kept: 98.2, ends_at_last_hole: true });
    expect(text).toBe('Repaired: the recording breaks and never resumes, so the repair ends at the break (98.2% kept). Saved as match_repaired.dem.');
  });
  it('says nothing was saved for a demo with no damage, a failed check or a cancel', () => {
    expect(repairResultText({ verdict: 'not_damaged' })).toMatch(/nothing saved/);
    expect(repairResultText({ verdict: 'refused', reasons: ['3 entity counts disagree'] }))
      .toBe('Not saved: the repaired demo failed a check (3 entity counts disagree).');
    expect(repairResultText({ verdict: 'cancelled' })).toMatch(/Nothing was saved/);
  });
  it('lists the gaps in its tooltip', () => {
    expect(repairGapsTitle(repaired)).toMatch(/^Gaps, in time since the recording started: 0:39/);
    expect(repairGapsTitle({ verdict: 'not_damaged', holes: [] })).toBe('');
  });
});
