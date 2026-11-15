import { describe, it, expect, vi } from 'vitest';
import { dropLine, startObsDropRate } from './obs_drop_rate.js';
import { obsForRows } from './batch_results.js';
import { STRINGS } from './strings.js';

const report = (over = {}) => ({
  clip: 2,
  recording: true,
  skipped_pct: 0.4,
  missed_pct: 0,
  recent_skipped_pct: 0.2,
  recent_missed_pct: 0,
  lagging: false,
  capture_fps: 120,
  ...over,
});

describe('OBS live drop rate (#172)', () => {
  it('shows the clip so far while it records, and its totals when done', () => {
    expect(dropLine(report()).text).toBe('OBS, clip 2: 0.4% of frames skipped, 0% missed');
    expect(dropLine(report({ recording: false, skipped_pct: 1.5 })).text).toBe('OBS, clip 2 done: 1.5% of frames skipped, 0% missed');
    expect(dropLine(report()).lagging).toBe(false);
  });

  it('warns with the last seconds\' figure and names the fix', () => {
    const line = dropLine(report({ lagging: true, recent_skipped_pct: 34.2, skipped_pct: 20 }));
    expect(line.lagging).toBe(true);
    expect(line.text).toContain('34.2%');
    expect(line.text).toContain("encoder can't keep up");
    expect(line.text).toContain('to 60');
  });

  it('names drawing as the cause when render lag is the bigger loss', () => {
    const line = dropLine(report({ lagging: true, recent_skipped_pct: 0, recent_missed_pct: 12 }));
    expect(line.text).toContain('12%');
    expect(line.text).toContain("can't draw them in time");
  });

  it('at 60 fps or below, does not suggest 60', () => {
    expect(STRINGS.OBS_DROP_RATE.fix(60)).not.toContain('to 60');
    expect(STRINGS.OBS_DROP_RATE.fix(120)).toContain('to 60');
  });

  it('warns once per batch, hides when the batch ends, and starts only once', () => {
    const handlers = {};
    const listen = vi.fn((name, fn) => {
      handlers[name] = fn;
      return Promise.resolve(() => {});
    });
    const classes = new Set();
    const el = {
      hidden: true,
      textContent: '',
      title: '',
      classList: {
        toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
        remove: (c) => classes.delete(c),
      },
    };
    const toast = vi.fn();
    startObsDropRate({ el, listen, toast });
    startObsDropRate({ el, listen, toast });
    expect(listen).toHaveBeenCalledTimes(2);

    handlers.capture_status({ payload: { running: true } });
    handlers.capture_obs_drop_rate({ payload: report() });
    expect(el.hidden).toBe(false);
    expect(classes.has('lagging')).toBe(false);

    handlers.capture_obs_drop_rate({ payload: report({ lagging: true, recent_skipped_pct: 40 }) });
    handlers.capture_obs_drop_rate({ payload: report({ lagging: true, recent_skipped_pct: 41 }) });
    expect(classes.has('lagging')).toBe(true);
    expect(toast).toHaveBeenCalledTimes(1);

    handlers.capture_status({ payload: { running: false } });
    expect(el.hidden).toBe(true);

    // The next batch warns again.
    handlers.capture_status({ payload: { running: true } });
    handlers.capture_obs_drop_rate({ payload: report({ lagging: true, recent_skipped_pct: 40 }) });
    expect(toast).toHaveBeenCalledTimes(2);
  });
});

describe('Last Batch: OBS figures per take (#172)', () => {
  const rows = [{ folder: 'D:\\raw\\s1\\b0' }, { folder: 'D:\\raw\\s1\\b1' }, { folder: 'D:\\raw\\s1\\b2' }];

  it('matches a clip to its take by folder, in either spelling', () => {
    const clips = [{ clip: 1, take_folder: 'd:/RAW/s1/b1/' }];
    expect(obsForRows(rows, clips)).toEqual([null, clips[0], null]);
  });

  it('falls back to the order when a clip has no folder', () => {
    const clips = [{ clip: 1, take_folder: null }, { clip: 3, take_folder: null }];
    expect(obsForRows(rows, clips)).toEqual([clips[0], null, clips[1]]);
  });

  it('has nothing for a batch that did not use OBS', () => {
    expect(obsForRows(rows, [])).toEqual([null, null, null]);
  });
});
