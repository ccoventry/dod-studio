import { describe, it, expect } from 'vitest';
import { summaryParts } from './capture_summary.js';

const base = {
  mode: 'frame_sequence', codecLabel: 'Ut Video', obsFps: 120, width: 1920, height: 1080, fps: 300,
  scheduledCount: 3, bannedCount: 0, decalFlush: true, destinations: true,
};
const texts = (setup) => summaryParts(setup).map((p) => p.text);

describe('summaryParts', () => {
  it('describes a frame-sequence batch', () => {
    expect(texts(base)).toEqual([
      'Frame sequence', '1920×1080 @ 300 fps', '3 scheduled commands', 'Decals cleared',
    ]);
  });

  it('names the codec in Video mode and the FPS in OBS mode', () => {
    expect(texts({ ...base, mode: 'direct_to_video' })[0]).toBe('Video · Ut Video');
    expect(texts({ ...base, mode: 'obs' })[0]).toBe('OBS @ 120 fps');
  });

  it('marks what blocks Start, and links every part to a setting', () => {
    const parts = summaryParts({ ...base, bannedCount: 2, destinations: false, scheduledCount: 1, decalFlush: false });
    expect(parts.filter((p) => p.blocking).map((p) => p.key)).toEqual(['banned', 'destination']);
    expect(parts.map((p) => p.text)).toContain('1 scheduled command');
    expect(parts.map((p) => p.text)).toContain('Decals kept');
    parts.forEach((p) => {
      expect(p.tab).toMatch(/^tab-/);
      expect(p.field).toBeTruthy();
    });
  });
});
