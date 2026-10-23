import { describe, it, expect } from 'vitest';
import { stepBarLayout } from './step_bar.js';
import {
  formatSize, setupStartView, setupView, buildStartView, buildView, coverageView,
} from './hd_progress.js';
import { blenderStartView, blenderFramesView } from './blender_progress.js';

const MB = 1024 ** 2;

describe('stepBarLayout (#956)', () => {
  it('fills the steps done and slides over the running one', () => {
    // Step 2 of 6 running: filled to 1/6, the block over 1/6..2/6.
    const at = stepBarLayout(1, 6);
    expect(at.fill).toBeCloseTo(16.67, 1);
    expect(at.slideFrom).toBeCloseTo(16.67, 1);
    expect(at.slideWidth).toBeCloseTo(16.67, 1);
  });

  it('moves the fill into a step whose fraction is known, with no block', () => {
    const at = stepBarLayout(1, 4, { fraction: 0.5 });
    expect(at.fill).toBe(37.5);
    expect(at.slideFrom).toBeNull();
    expect(at.slideWidth).toBe(0);
  });

  it('stays inside the bar for odd input', () => {
    expect(stepBarLayout(9, 4).fill).toBe(100);
    expect(stepBarLayout(9, 4).slideFrom).toBeNull();
    expect(stepBarLayout(-1, 0).fill).toBe(0);
    expect(stepBarLayout(0, 2, { fraction: 3 }).fill).toBe(50);
  });
});

describe('HD Set up tools', () => {
  it('says what it checks first, with the block sliding', () => {
    expect(setupStartView()).toEqual({ text: "Checking what's already installed…", bar: { slide: true } });
  });

  it('a download: name, file count and bytes, the bar by bytes within its file', () => {
    const view = setupView({
      item: 'realesrgan-ncnn-vulkan.zip', step: 2, steps: 4, bytes_done: 12 * MB, bytes_total: 40 * MB, unpacking: false,
    });
    expect(view.text).toBe('Downloading realesrgan-ncnn-vulkan.zip (2 of 4): 12.0 MB of 40.0 MB');
    expect(view.bar).toEqual({ done: 1, total: 4, fraction: 0.3 });
    expect(stepBarLayout(view.bar.done, view.bar.total, view.bar).fill).toBe(32.5);
  });

  it('a download of unknown size slides over its file', () => {
    const view = setupView({ item: 'x.zip', step: 1, steps: 2, bytes_done: 300 * 1024, bytes_total: null, unpacking: false });
    expect(view.text).toBe('Downloading x.zip (1 of 2): 300 KB');
    expect(view.bar).toEqual({ done: 0, total: 2 });
  });

  it('an unpack slides over its file', () => {
    const view = setupView({ item: 'python.zip', step: 3, steps: 3, bytes_done: 0, bytes_total: null, unpacking: true });
    expect(view.text).toBe('Unpacking python.zip (3 of 3)…');
    expect(view.bar).toEqual({ done: 2, total: 3 });
  });

  it('formats sizes as before', () => {
    expect(formatSize(1.5 * 1024 ** 3)).toBe('1.5 GB');
    expect(formatSize(10)).toBe('1 KB');
  });
});

describe('HD Build', () => {
  it('starts with a line and the block sliding over the whole bar', () => {
    expect(buildStartView()).toEqual({ text: 'Starting the build…', bar: { slide: true } });
  });

  it('a step: the line as before, the bar by step', () => {
    const view = buildView({ step: 2, steps: 6, style: 'ultrasharp', asset_type: 'world' }, 180, 'Map textures');
    expect(view.text).toBe('Step 2 of 6: ultrasharp, Map textures (3m 00s so far)');
    expect(view.bar).toEqual({ done: 1, total: 6 });
  });
});

describe('HD map coverage', () => {
  it('slides until the first count, then fills by maps read', () => {
    expect(coverageView(null)).toEqual({ text: 'Reading the maps…', bar: { slide: true } });
    expect(coverageView({ done: 12, total: 40 })).toEqual({ text: 'Reading the maps: 12 of 40', bar: { pct: 30 } });
  });
});

describe('Blender', () => {
  it('names each step, then adds the clock after its first second', () => {
    expect(blenderStartView('import')).toEqual({ text: 'Importing the demo into Blender…', bar: { slide: true } });
    expect(blenderStartView('scene', 0.5).text).toBe('Building the scene…');
    expect(blenderStartView('encode', 65).text).toBe('Encoding the MP4… (1m 05s so far)');
    expect(blenderStartView('unknown').text).toBe('Rendering…');
  });

  it('Render fills by frames', () => {
    const view = blenderFramesView({ done: 120, total: 600, frame_secs: 0.4 }, 60);
    expect(view.text).toBe('Frame 120 of 600 (0.4 s each), 1m 00s so far');
    expect(view.bar).toEqual({ pct: 20 });
  });
});
