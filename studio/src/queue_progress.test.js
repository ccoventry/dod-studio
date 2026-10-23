import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  SCAN_STEPS, inRange, nest, placed, scanStepView, groupingStepView, createQueueProgress,
} from './queue_progress.js';

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
});
