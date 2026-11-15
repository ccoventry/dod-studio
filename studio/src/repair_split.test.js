import { describe, it, expect } from 'vitest';
import { REPAIR_SHARE, repairStepView, splitStepView } from './repair_split.js';

describe('Repair & Split\'s one bar (#15)', () => {
  it('runs the repair over the first share, saying which step it is on', () => {
    expect(repairStepView({ step: 'reading', fraction: 0.5 })).toEqual({ pct: 25, text: 'Repairing: Reading the recording… 50%' });
    expect(repairStepView(null).pct).toBe(0);
  });

  it('runs the split over the rest', () => {
    expect(splitStepView({ fraction: 0, stage: 'reading' }).pct).toBe(REPAIR_SHARE);
    expect(splitStepView({ fraction: 1, stage: 'checking', map: 'dod_anzio', part: 1, parts: 1 }))
      .toEqual({ pct: 100, text: 'Splitting the repaired demo: Checking dod_anzio… 100%' });
  });
});
