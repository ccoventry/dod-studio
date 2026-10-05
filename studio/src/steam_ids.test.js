import { describe, it, expect } from 'vitest';
import { steamIdForms, deathmsgShowOnlyLine } from './steam_ids.js';

describe('steamIdForms (#536)', () => {
  it('gives all three forms of a real player', () => {
    expect(steamIdForms('76561197972576011')).toEqual({
      id64: '76561197972576011',
      classic: 'STEAM_0:1:6155141',
      id3: '[U:1:12310283]',
    });
  });

  it('matches the deathmsg docs example', () => {
    const forms = steamIdForms('76561197977930126');
    expect(forms.classic).toBe('STEAM_0:0:8832199');
    expect(forms.id3).toBe('[U:1:17664398]');
  });

  it('refuses ids that are not a user account', () => {
    // HLTV proxy
    expect(steamIdForms('90071996842377216')).toBeNull();
    // below the individual range, and account 0
    expect(steamIdForms('76561197960265727')).toBeNull();
    expect(steamIdForms('76561197960265728')).toBeNull();
    // bots and stand-ins
    expect(steamIdForms('PLAYER_3')).toBeNull();
    expect(steamIdForms('BOT')).toBeNull();
    expect(steamIdForms('')).toBeNull();
    expect(steamIdForms(null)).toBeNull();
  });

  it('builds the show-only console line from the SteamID64', () => {
    expect(deathmsgShowOnlyLine(steamIdForms('76561197972576011')))
      .toBe('dodstudio_deathmsg block !76561197972576011');
  });
});
