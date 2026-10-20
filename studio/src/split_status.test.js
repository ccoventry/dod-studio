import { describe, it, expect } from 'vitest';
import {
  startRows, applyProgress, applyOutcome, applyError, statusText, summaryText,
} from './split_status.js';

const ev = (stage, index, mapFraction, extra = {}) => ({
  fraction: 0.5, stage, map: 'dod_anzio', part: 1, parts: 2, index, map_fraction: mapFraction, done: 0, file: null, error: null, ...extra,
});
const file = (index) => ({ index, path: `C:\\Demos\\m_split${index + 1}_anzio.dem`, map: 'dod_anzio', seconds: 754, size_bytes: 41_200_000 });

describe('split status rows', () => {
  it('start Waiting, one per ticked map', () => {
    const rows = startRows([0, 2]);
    expect(Object.keys(rows)).toEqual(['0', '2']);
    expect(statusText(rows[0])).toBe('Waiting');
  });

  it('all read together, then each map moves on its own', () => {
    let rows = startRows([0, 1]);
    rows = applyProgress(rows, { stage: 'reading', index: null, map_fraction: 0.42 });
    expect(statusText(rows[0])).toBe('Reading the demo… 42%');
    expect(statusText(rows[1])).toBe('Reading the demo… 42%');
    rows = applyProgress(rows, ev('waiting', 0, 0));
    rows = applyProgress(rows, ev('waiting', 1, 0));
    rows = applyProgress(rows, ev('writing', 0, 0));
    expect(statusText(rows[0])).toBe('Writing…');
    expect(statusText(rows[1])).toBe('Waiting');
    // The check is the second half of the map's own progress.
    rows = applyProgress(rows, ev('checking', 0, 0.8));
    expect(statusText(rows[0])).toBe('Checking… 60%');
    expect(rows[0].bar).toBe(80);
    rows = applyProgress(rows, ev('writing', 1, 0));
    rows = applyProgress(rows, ev('finished', 0, 1, { file: file(0) }));
    expect(statusText(rows[0])).toBe('Finished');
    expect(rows[0].file.path).toMatch(/m_split1_anzio\.dem$/);
    expect(rows[0].bar).toBe(100);
    rows = applyProgress(rows, ev('failed', 1, 1, { error: 'map 2 (dod_anzio) read back as 2 map(s)' }));
    expect(statusText(rows[1])).toBe('Failed: map 2 (dod_anzio) read back as 2 map(s)');
    expect(summaryText(rows)).toBe('Wrote 1 demo, 1 failed.');
  });

  it('a map that has ended stays ended, and other maps\' events leave it be', () => {
    let rows = startRows([0, 1]);
    rows = applyProgress(rows, ev('finished', 0, 1, { file: file(0) }));
    rows = applyProgress(rows, ev('checking', 0, 0.7));
    rows = applyProgress(rows, { stage: 'reading', index: null, map_fraction: 0.1 });
    expect(rows[0].stage).toBe('finished');
    expect(rows[1].stage).toBe('reading');
    // An event for a map that wasn't ticked changes nothing.
    expect(applyProgress(rows, ev('writing', 5, 0))).toBe(rows);
  });

  it('Cancel ends the maps still going as Cancelled, keeping the ones written', () => {
    let rows = startRows([0, 1, 2]);
    rows = applyProgress(rows, ev('finished', 0, 1, { file: file(0) }));
    rows = applyProgress(rows, ev('checking', 1, 0.6));
    rows = applyOutcome(rows, { written: [file(0)], failed: [], cancelled: true });
    expect(rows[0].stage).toBe('finished');
    expect(statusText(rows[1])).toBe('Cancelled');
    expect(statusText(rows[2])).toBe('Cancelled');
    expect(summaryText(rows)).toBe('Wrote 1 demo, 2 cancelled.');
  });

  it('the outcome fills in what the events missed', () => {
    let rows = startRows([0, 1]);
    rows = applyOutcome(rows, {
      written: [file(0)],
      failed: [{ index: 1, map: 'dod_flash', error: 'could not write X' }],
      cancelled: false,
    });
    expect(rows[0]).toMatchObject({ stage: 'finished', file: file(0) });
    expect(statusText(rows[1])).toBe('Failed: could not write X');
  });

  it('a split that fails outright fails every map still going', () => {
    let rows = startRows([0, 1]);
    rows = applyProgress(rows, ev('finished', 0, 1, { file: file(0) }));
    rows = applyError(rows, 'could not read the demo: bad frame');
    expect(rows[0].stage).toBe('finished');
    expect(statusText(rows[1])).toBe('Failed: could not read the demo: bad frame');
    expect(summaryText(rows)).toBe('Wrote 1 demo, 1 failed.');
  });
});
