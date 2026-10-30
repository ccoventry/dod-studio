import { describe, it, expect, beforeEach } from 'vitest';
import { cleanUseAs, useAsFromTable, setUseAsTable, useAsName, onUseAsChange } from './player_alias.js';

const table = {
  players: [
    { steam_id: '76561197961831029', name: 'm00cat', use_as: '  Chris ' },
    { steam_id: '76561197960265729', name: 'bob', use_as: '' },
    { steam_id: '', name: 'bot', use_as: 'Robot' },
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
});
