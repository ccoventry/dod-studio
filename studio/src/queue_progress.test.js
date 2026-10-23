import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  SCAN_STEPS, inRange, nest, placed, scanStepView, groupingStepView, createQueueProgress, scanFoldersKey,
} from './queue_progress.js';
import { lastRunKey } from './progress_view.js';

// The queue's line, spinner and bar, enough for createQueueProgress.
function fakeDoc(line = 'Ready') {
  const classes = new Set();
  const fill = { style: { width: '0%' } };
  const els = {
    '#scan-status': { textContent: line },
    '#scan-spinner': { style: { display: 'none' } },
    '#scan-progress': {
      hidden: true,
      classList: {
        toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
        remove: (c) => classes.delete(c),
        contains: (c) => classes.has(c),
      },
      querySelector: () => fill,
    },
  };
  return {
    querySelector: (sel) => els[sel] ?? null,
    line: () => els['#scan-status'].textContent,
    spinner: () => els['#scan-spinner'].style.display,
    bar: () => ({
      hidden: els['#scan-progress'].hidden,
      sliding: classes.has('indeterminate'),
      width: fill.style.width,
    }),
  };
}

afterEach(() => vi.useRealTimers());

describe('queue_progress (#951)', () => {
  it('remembers the folder count apart for a scan without subfolders (#531)', () => {
    const paths = ['C:\\demos'];
    expect(scanFoldersKey(paths, true)).toBe(lastRunKey('queue_scan_last_folders', paths));
    expect(scanFoldersKey(paths, false)).not.toBe(scanFoldersKey(paths, true));
    expect(scanFoldersKey(paths)).toBe(scanFoldersKey(paths, true));
  });

  it('places a step on its share of the bar, nested shares included', () => {
    expect(inRange([10, 90], 50)).toBe(50);
    expect(inRange([10, 90], 150)).toBe(90);
    const split = { job: 'j', range: [50, 100] };
    // A split's scan's grouping: the last tenth of the scan's 70-100 share.
    const scan = nest(split, [70, 100], 'Splitting a.dem: adding its maps');
    expect(scan.range).toEqual([85, 100]);
    expect(scan.job).toBe('j');
    const grouping = nest(scan, SCAN_STEPS.grouping);
    expect(grouping.range[0]).toBeCloseTo(98.5);
    expect(grouping.range[1]).toBe(100);
    expect(grouping.text).toBe('Splitting a.dem: adding its maps');
    expect(placed(grouping, { text: 'Grouping by match: 1 of 2', pct: 50 }).text).toBe('Splitting a.dem: adding its maps');
    expect(placed({ range: [10, 90] }, { text: 'x', pct: 50 })).toEqual({ text: 'x', pct: 50 });
  });

  it('words looking for demos against the last scan, or slides', () => {
    const first = scanStepView({ listing: true, folders: 1240, demos: 85 });
    expect(first).toEqual({
      step: 'looking', text: 'Looking for demos: 1,240 folders, 85 demos found', pct: 0, indeterminate: true,
    });
    const again = scanStepView({ listing: true, folders: 620, demos: 1 }, { expectedFolders: 1240 });
    expect(again.text).toBe('Looking for demos: 620 folders (about 1,240 last time), 1 demo found');
    expect(again.pct).toBe(50);
    expect(again.indeterminate).toBe(false);
  });

  it('counts the demos checked for being new, between looking and reading', () => {
    const view = scanStepView({ keying: true, keyed: 300, total: 1200, scanned: 0, status: 'x' });
    expect(view).toEqual({ step: 'keying', text: 'Checking which demos are new: 300 of 1,200', pct: 25, indeterminate: false });
    // After looking's share of the bar and before reading's, so it never goes back.
    expect(placed(nest({ range: [0, 100] }, SCAN_STEPS.keying), view).pct).toBe(6.25);
    expect(SCAN_STEPS.looking[1]).toBe(SCAN_STEPS.keying[0]);
    expect(SCAN_STEPS.keying[1]).toBe(SCAN_STEPS.reading[0]);
  });

  it('shows reading demos as the backend words it, with the time left', () => {
    const view = scanStepView(
      { scanned: 12, total: 48, status: 'Reading demos: 12 of 48, a.dem' },
      { timeLeft: ' · about 40 s left' },
    );
    expect(view).toEqual({ step: 'reading', text: 'Reading demos: 12 of 48, a.dem · about 40 s left', pct: 25, indeterminate: false });
    // On the whole scan's bar: a quarter of the reading step's 10-90%.
    expect(placed(nest({ range: [0, 100] }, SCAN_STEPS.reading), view).pct).toBe(30);
    // The last event says nothing the scan's end line doesn't.
    expect(scanStepView({ status: 'Complete', found: 3 })).toBeNull();
    expect(scanStepView({ status: 'Cancelled', cancelled: true })).toBeNull();
  });

  it('counts grouping by match', () => {
    expect(groupingStepView({ done: 30, total: 60 })).toEqual({
      step: 'grouping', text: 'Grouping by match: 30 of 60', pct: 50, indeterminate: false,
    });
    expect(groupingStepView(null).text).toBe('Grouping by match…');
  });

  it('gives the line to one job at a time, and puts the old line back', () => {
    vi.useFakeTimers();
    const doc = fakeDoc('Done in 2.0 s: 3 demos added');
    const progress = createQueueProgress({ doc });
    const scan = progress.begin();
    expect(doc.line()).toBe('Looking for demos… · 0:00');
    expect(doc.spinner()).toBe('inline-block');
    expect(doc.bar()).toEqual({ hidden: false, sliding: true, width: '0%' });

    // A regroup from Group by waits for the line rather than taking it.
    expect(progress.begin({ ifIdle: true })).toBeNull();

    progress.show(scan, { text: 'Reading demos: 1 of 4', pct: 30, indeterminate: false });
    expect(doc.bar()).toEqual({ hidden: false, sliding: false, width: '30%' });
    vi.advanceTimersByTime(2_000);
    expect(doc.line()).toBe('Reading demos: 1 of 4 · 0:02');

    progress.end(scan, 'Done in 2.0 s: 4 demos added');
    expect(doc.line()).toBe('Done in 2.0 s: 4 demos added');
    expect(doc.spinner()).toBe('none');
    expect(doc.bar().hidden).toBe(true);
    // A job that no longer has the line changes nothing.
    progress.show(scan, { text: 'late' });
    progress.end(scan, 'late');
    expect(doc.line()).toBe('Done in 2.0 s: 4 demos added');

    // With no line of its own, the job puts back the one it found.
    const regroup = progress.begin({ ifIdle: true, text: 'Grouping by match…' });
    expect(doc.line()).toBe('Grouping by match… · 0:00');
    progress.end(regroup);
    expect(doc.line()).toBe('Done in 2.0 s: 4 demos added');
  });

  it('a job that takes the line gives it back to the one it took it from (#958)', () => {
    vi.useFakeTimers();
    const doc = fakeDoc('Ready');
    const progress = createQueueProgress({ doc });
    const grouping = progress.begin({ ifIdle: true, text: 'Grouping by match: 0 of 0' });
    vi.advanceTimersByTime(3_000);
    const search = progress.begin({ text: 'Looking for 3 missing demos…' });
    expect(doc.line()).toBe('Looking for 3 missing demos… · 0:00');
    // The grouping carries on under it; its line is kept, not drawn.
    progress.show(grouping, { text: 'Grouping by match: 5 of 40', pct: 12.5, indeterminate: false });
    expect(doc.line()).toBe('Looking for 3 missing demos… · 0:00');
    expect(doc.bar().sliding).toBe(true);
    expect(progress.ms(grouping)).toBe(3_000);

    progress.end(search);
    expect(doc.line()).toBe('Grouping by match: 5 of 40 · 0:03');
    expect(doc.bar()).toEqual({ hidden: false, sliding: false, width: '12.5%' });
    expect(doc.spinner()).toBe('inline-block');
    progress.end(grouping);
    expect(doc.line()).toBe('Ready');
    expect(doc.bar().hidden).toBe(true);
  });

  it('a job under another that ends meanwhile leaves its last line for the end', () => {
    const doc = fakeDoc('Ready');
    const progress = createQueueProgress({ doc });
    const scan = progress.begin();
    const split = progress.begin({ text: 'Splitting a.dem' });
    progress.end(scan, 'Done in 2.0 s: 4 demos added');
    expect(doc.line()).toBe('Splitting a.dem · 0:00');
    progress.end(split);
    expect(doc.line()).toBe('Done in 2.0 s: 4 demos added');
    expect(doc.spinner()).toBe('none');
  });
});
