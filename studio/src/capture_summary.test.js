import { describe, it, expect } from 'vitest';
import { summaryParts } from './capture_summary.js';

const base = {
  mode: 'frame_sequence', codecLabel: 'Ut Video', obsFps: 120, width: 1920, height: 1080, fps: 300,
  commandProfile: { name: 'Clean HUD', edited: false },
  scheduledCount: 3, bannedCount: 0, decalFlush: true, destinations: true,
  finish: { enabled: true, codecLabel: 'ProRes 422 HQ' },
};
const texts = (setup) => summaryParts(setup).map((p) => p.text);

describe('summaryParts', () => {
  it('describes a frame-sequence batch', () => {
    expect(texts(base)).toEqual([
      'Frame sequence', '1920×1080 @ 300 fps', 'Commands: Clean HUD', '3 scheduled commands', 'Decals cleared',
      'Finish: ProRes 422 HQ',
    ]);
  });

  it('names the command profile, marked once the lists were edited', () => {
    const part = (setup) => summaryParts(setup).find((p) => p.key === 'commands');
    expect(part({ ...base, commandProfile: { name: 'Clean HUD', edited: true } }).text).toBe('Commands: Clean HUD (edited)');
    expect(part({ ...base, commandProfile: { name: '', edited: false } }).text).toBe('Commands: none');
    expect(part({ ...base, commandProfile: undefined }).text).toBe('Commands: none');
    expect(part(base).field).toBe('#command-profile-select');
  });

  it('says what the finish step will save, or that it is off', () => {
    const part = (setup) => summaryParts(setup).find((p) => p.key === 'finish');
    expect(part({ ...base, finish: { enabled: false, codecLabel: '' } }).text).toBe('Finish: off');
    expect(part({ ...base, finish: { enabled: false } }).field).toBe('#config-finish-clips');
    expect(part({ ...base, mode: 'obs', finish: { enabled: true, codecLabel: 'Keep as captured' } })).toMatchObject({
      text: 'Finish: Keep as captured', tab: 'tab-render-output', field: '#config-finish-codec-obs',
    });
    expect(part({ ...base, mode: 'direct_to_video' }).field).toBe('#config-finish-codec-video');
    expect(part(base).field).toBe('#config-finish-codec-frames');
  });

  it('names the codec in Video mode and the FPS in OBS mode', () => {
    expect(texts({ ...base, mode: 'direct_to_video' })[0]).toBe('Video · Ut Video');
    expect(texts({ ...base, mode: 'obs' })[0]).toBe('OBS @ 120 fps');
  });

  it('names AGR mode with its own FPS, and leaves out the video format', () => {
    const parts = summaryParts({ ...base, mode: 'agr', agrFps: 30 });
    expect(parts[0].text).toBe('AGR for Blender @ 30 fps');
    expect(parts.map((p) => p.key)).not.toContain('format');
    // Nothing a finish step could render either.
    expect(parts.map((p) => p.key)).not.toContain('finish');
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
