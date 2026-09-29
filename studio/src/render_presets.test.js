import { describe, it, expect } from 'vitest';
import { presetValues, matchingPreset, savePreset, deletePreset } from './render_presets.js';

describe('render presets', () => {
  it('normalises values, dropping custom args for a built-in codec', () => {
    expect(presetValues({ codec: 'h264', custom_codec_args: '-x', fps: '240', max_concurrent: 20 }))
      .toEqual({ codec: 'h264', custom_codec_args: '', fps: 240, max_concurrent: 8 });
    expect(presetValues({ codec: 'custom', custom_codec_args: ' -c:v mpeg4 ' }).custom_codec_args).toBe('-c:v mpeg4');
  });

  it('saves by name, replacing one of the same name, sorted', () => {
    let presets = savePreset([], 'YouTube', { codec: 'h264', fps: 300, max_concurrent: 2 });
    presets = savePreset(presets, 'archive', { codec: 'prores', fps: 300, max_concurrent: 1 });
    presets = savePreset(presets, 'youtube', { codec: 'h264_nvenc', fps: 300, max_concurrent: 3 });
    expect(presets.map((p) => p.name)).toEqual(['archive', 'youtube']);
    expect(presets[1].codec).toBe('h264_nvenc');
    expect(savePreset(presets, '  ', { codec: 'prores' })).toBe(presets);
  });

  it('finds the preset matching the current settings, or none', () => {
    const presets = savePreset([], 'Discord', { codec: 'h264', fps: 240, max_concurrent: 2 });
    expect(matchingPreset(presets, { codec: 'h264', fps: '240', max_concurrent: '2', custom_codec_args: 'ignored' }).name).toBe('Discord');
    expect(matchingPreset(presets, { codec: 'h264', fps: 300, max_concurrent: 2 })).toBe(null);
  });

  it('deletes by name', () => {
    const presets = savePreset(savePreset([], 'a', {}), 'b', {});
    expect(deletePreset(presets, 'a').map((p) => p.name)).toEqual(['b']);
  });
});
