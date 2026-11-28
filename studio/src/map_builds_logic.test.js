import { describe, it, expect } from 'vitest';
import { addOutcome, buildKey, buildText, fileName, libraryUses } from './map_builds_logic.js';

describe('map builds on the Maps needed banner (#1547)', () => {
  it('writes a build as the 8 hex digits its file is named by', () => {
    expect(buildText(0x61e9d38d)).toBe('61e9d38d');
    expect(buildText(0x0c32)).toBe('00000c32');
    expect(buildText(null)).toBe('?');
  });

  it('takes the file name from either kind of path', () => {
    expect(fileName('C:\\g\\dod\\maps\\dod_armory_b5.61e9d38d.bsp.bak')).toBe('dod_armory_b5.61e9d38d.bsp.bak');
    expect(fileName('D:/x/dod_x.bsp')).toBe('dod_x.bsp');
    expect(fileName('dod_x.bsp')).toBe('dod_x.bsp');
  });

  it('groups the demos playing on a library build by map and build, each demo once', () => {
    const rows = [
      { state: 'fromLibrary', mapName: 'dod_x', expectedChecksum: 2, demoName: 'a.dem' },
      { state: 'fromLibrary', mapName: 'dod_x', expectedChecksum: 2, demoName: 'a.dem' },
      { state: 'fromLibrary', mapName: 'dod_x', expectedChecksum: 3, demoName: 'b.dem' },
      { state: 'ok', mapName: 'dod_x', expectedChecksum: 1, demoName: 'c.dem' },
      { state: 'wrongBuild', mapName: 'dod_y', expectedChecksum: 4, demoName: 'd.dem' },
    ];
    expect(libraryUses(rows)).toEqual([
      { key: buildKey('dod_x', 2), mapName: 'dod_x', checksum: 2, demos: ['a.dem'] },
      { key: buildKey('dod_x', 3), mapName: 'dod_x', checksum: 3, demos: ['b.dem'] },
    ]);
    expect(libraryUses(undefined)).toEqual([]);
  });

  it('says an added build helps the demos only when it is the one they want', () => {
    expect(addOutcome({ checksum: 2 }, 2)).toBe('matches');
    expect(addOutcome({ checksum: 3 }, 2)).toBe('other');
    expect(addOutcome({ checksum: 3 }, null)).toBe('other');
  });
});
