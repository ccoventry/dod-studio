import { describe, it, expect, beforeEach } from 'vitest';
import {
  cleanUseAs, useAsFromTable, setUseAsTable, useAsName, onUseAsChange,
  cleanNameplate, nameplateFromTable, nameplatePath,
} from './player_alias.js';

const table = {
  players: [
    { steam_id: '76561197961831029', name: 'm00cat', use_as: '  Chris ', nameplate: ' D:/np/m00cat.png ' },
    { steam_id: '76561197960265729', name: 'bob', use_as: '' },
    { steam_id: '', name: 'bot', use_as: 'Robot', nameplate: 'D:/np/bot.png' },
  ],
};

describe('player_alias', () => {
  beforeEach(() => setUseAsTable(null));

  it('trims, and treats empty as unset', () => {
    expect(cleanUseAs('  x ')).toBe('x');
    expect(cleanUseAs('   ')).toBe('');
    expect(cleanUseAs(undefined)).toBe('');
  });

  it('finds the alias by SteamID only', () => {
    expect(useAsFromTable(table, '76561197961831029')).toBe('Chris');
    expect(useAsFromTable(table, '76561197960265729')).toBeNull();
    expect(useAsFromTable(table, '76561197960000000')).toBeNull();
    // A row with no SteamID is never matched, even by an empty id.
    expect(useAsFromTable(table, '')).toBeNull();
    expect(useAsFromTable(table, null)).toBeNull();
  });

  it('answers from the table it was given and tells listeners', () => {
    let calls = 0;
    const off = onUseAsChange(() => { calls += 1; });
    expect(useAsName('76561197961831029')).toBeNull();
    setUseAsTable(table);
    expect(useAsName('76561197961831029')).toBe('Chris');
    expect(calls).toBe(1);
    off();
    setUseAsTable(null);
    expect(calls).toBe(1);
  });

  it('finds the nameplate by SteamID only, trimmed (#429)', () => {
    expect(cleanNameplate('  a.png ')).toBe('a.png');
    expect(cleanNameplate(42)).toBe('');
    expect(nameplateFromTable(table, '76561197961831029')).toBe('D:/np/m00cat.png');
    expect(nameplateFromTable(table, '76561197960265729')).toBeNull();
    expect(nameplateFromTable(table, '')).toBeNull();
  });

  it('answers nameplatePath from the same table as useAsName', () => {
    expect(nameplatePath('76561197961831029')).toBeNull();
    setUseAsTable(table);
    expect(nameplatePath('76561197961831029')).toBe('D:/np/m00cat.png');
  });
});
