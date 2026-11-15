import { describe, it, expect } from 'vitest';
import {
  sanitizeCrosshairName,
  crosshairFileName,
  defaultCrosshairName,
  crosshairTaken,
  MAX_NAME_CHARS,
  CREATOR_URL,
} from './crosshair_file.js';

describe('crosshair file names (#842)', () => {
  it('sanitizes like the Rust side', () => {
    // The cases of native/src/crosshair_import.rs's names tests.
    const cases = [
      ['Red Dot', 'red-dot'],
      ['  dicE[: :]m00cat :D ', 'dice-m00cat-d'],
      ['customxhair (3)', '3'],
      ['customxhair', ''],
      ['customXHair-blue_v2', 'blue_v2'],
      ['..\\..\\evil/name', 'evil-name'],
      ['crème brûlée', 'crme-brle'],
      ['---', ''],
      ['customxhair.spr', ''],
      ['customxhair-pack.zip', 'pack-zip'],
      ['My Crosshair', 'my-crosshair'],
      ['__a__', 'a'],
    ];
    for (const [raw, want] of cases) expect(sanitizeCrosshairName(raw), raw).toBe(want);
    expect(sanitizeCrosshairName('x'.repeat(100))).toHaveLength(MAX_NAME_CHARS);
    expect(sanitizeCrosshairName(null)).toBe('');
  });

  it('builds the file name, or nothing', () => {
    expect(crosshairFileName('Red Dot')).toBe('customxhair-red-dot.spr');
    expect(crosshairFileName('  ')).toBe('');
    expect(crosshairFileName('customxhair')).toBe('');
  });

  it('prefills the name from the picked file', () => {
    expect(defaultCrosshairName('C:\\Users\\me\\Downloads\\customxhair.spr')).toBe('my-crosshair');
    expect(defaultCrosshairName('C:/Downloads/customxhair (2).zip')).toBe('2');
    expect(defaultCrosshairName('D:\\x\\Clan Pack.ZIP')).toBe('clan-pack');
    expect(defaultCrosshairName('')).toBe('my-crosshair');
  });

  it('checks a name against the listed sprites, ignoring case', () => {
    const sprites = ['customXHair.spr', 'customxhair-Red.spr'];
    expect(crosshairTaken('customxhair-red.spr', sprites)).toBe(true);
    expect(crosshairTaken('customxhair-blue.spr', sprites)).toBe(false);
    expect(crosshairTaken('', sprites)).toBe(false);
    expect(crosshairTaken('a.spr', null)).toBe(false);
  });

  it('links the creator over https', () => {
    expect(CREATOR_URL).toMatch(/^https:\/\//);
  });
});
