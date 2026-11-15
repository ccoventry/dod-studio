import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));

import { themedConfirm } from './themed_confirm.js';
import { hasNoCameraData, confirmNoCameraData, noCameraDataNotice } from './camera_data.js';
import { STRINGS } from './strings.js';

describe('hasNoCameraData', () => {
  it('is true only for a demo the scan flagged', () => {
    expect(hasNoCameraData({ no_camera_data: true })).toBe(true);
    expect(hasNoCameraData({ no_camera_data: false })).toBe(false);
    // Scanned before the check existed, or nothing at all.
    expect(hasNoCameraData({})).toBe(false);
    expect(hasNoCameraData(null)).toBe(false);
  });
});

describe('confirmNoCameraData', () => {
  beforeEach(() => {
    themedConfirm.mockReset();
    themedConfirm.mockResolvedValue(false);
  });

  it('goes ahead without asking when no demo is flagged', async () => {
    expect(await confirmNoCameraData([{ name: 'a.dem', no_camera_data: false }, { name: 'b.dem' }])).toBe(true);
    expect(themedConfirm).not.toHaveBeenCalled();
  });

  it('lists the flagged demos and returns the answer', async () => {
    const demos = [
      { name: 'good.dem', no_camera_data: false },
      { name: 'frozen.dem', no_camera_data: true },
    ];
    expect(await confirmNoCameraData(demos)).toBe(false);
    const [message, options] = themedConfirm.mock.calls[0];
    expect(message).toBe(STRINGS.CAMERA_DATA.batchMessage(1));
    expect(options.title).toBe('1 demo has no camera data');
    expect(options.confirmLabel).toBe(STRINGS.CAMERA_DATA.START_ANYWAY);
    expect(options.details).toEqual([{ primary: 'frozen.dem', secondary: STRINGS.CAMERA_DATA.DETAIL }]);
  });

  it('asks in preview words for a Launch Preview', async () => {
    themedConfirm.mockResolvedValue(true);
    expect(await confirmNoCameraData([{ name: 'frozen.dem', no_camera_data: true }], { preview: true })).toBe(true);
    const [message, options] = themedConfirm.mock.calls[0];
    expect(message).toBe(STRINGS.CAMERA_DATA.PREVIEW_MESSAGE);
    expect(options.title).toBe(STRINGS.CAMERA_DATA.PREVIEW_TITLE);
    expect(options.confirmLabel).toBe(STRINGS.CAMERA_DATA.PREVIEW_ANYWAY);
  });

  it('counts several in the title', () => {
    expect(STRINGS.CAMERA_DATA.batchTitle(2)).toBe('2 demos have no camera data');
  });
});

describe('noCameraDataNotice', () => {
  it('reads the analyzer report\'s demo_info', () => {
    expect(noCameraDataNotice({ demo_info: { no_camera_data: true } })).toBe(STRINGS.CAMERA_DATA.ANALYZER_NOTICE);
    expect(noCameraDataNotice({ demo_info: { no_camera_data: false } })).toBeNull();
    expect(noCameraDataNotice({ demo_info: {} })).toBeNull();
    expect(noCameraDataNotice(null)).toBeNull();
  });
});
