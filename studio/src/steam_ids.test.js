import { describe, it, expect } from 'vitest';
import { steamIdForms, deathmsgShowOnlyLine, parseSteamIdInput, convertSteamId } from './steam_ids.js';

describe('steamIdForms (#536)', () => {
  it('gives all three forms of a real player', () => {
    expect(steamIdForms('76561197962734863')).toEqual({
      id64: '76561197962734863',
      classic: 'STEAM_0:1:1234567',
      id3: '[U:1:2469135]',
    });
  });

  it('matches the deathmsg docs example', () => {
    const forms = steamIdForms('76561197975574370');
    expect(forms.classic).toBe('STEAM_0:0:7654321');
    expect(forms.id3).toBe('[U:1:15308642]');
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
    expect(deathmsgShowOnlyLine(steamIdForms('76561197962734863')))
      .toBe('ds_hud_deathmsg block !76561197962734863');
  });
});

describe('parseSteamIdInput (#1085)', () => {
  const ID64 = '76561197972576011';
  const FORMS = { ok: true, id64: ID64, classic: 'STEAM_0:1:6155141', id3: '[U:1:12310283]' };

  it('reads the three spellings of one account to the same forms', () => {
    for (const s of ['STEAM_0:1:6155141', 'STEAM_1:1:6155141', '[U:1:12310283]', 'U:1:12310283', ID64]) {
      expect(parseSteamIdInput(s)).toEqual(FORMS);
    }
  });

  it('tolerates whitespace and case', () => {
    expect(parseSteamIdInput('  steam_0:1:6155141\n')).toEqual(FORMS);
    expect(parseSteamIdInput('\t[u:1:12310283] ')).toEqual(FORMS);
    expect(parseSteamIdInput(` ${ID64} `)).toEqual(FORMS);
  });

  it('round trips through every form', () => {
    for (const from of ['id64', 'classic', 'id3']) {
      for (const to of ['id64', 'classic', 'id3']) {
        expect(convertSteamId(FORMS[from], to)).toBe(FORMS[to]);
      }
    }
    expect(convertSteamId('STEAM_0:0:7654321', 'id3')).toBe('[U:1:15308642]');
  });

  it('answers not-a-SteamID for garbage, never throwing', () => {
    const bad = { ok: false, reason: 'not_a_steam_id' };
    for (const s of ['', '   ', 'm00cat', 'PLAYER_3', 'BOT', 'STEAM_0:2:5', 'STEAM_0:1:', '[U:1:]',
      'STEAM_0:1:6155141 extra', '[U:1:12310283', '12310283', '7656119797257601x', null, undefined, 42, {}]) {
      expect(parseSteamIdInput(s)).toEqual(bad);
    }
    expect(convertSteamId('nope', 'id3')).toBeNull();
    expect(convertSteamId(ID64, 'ok')).toBeNull();
  });

  it('refuses another universe or account type', () => {
    expect(parseSteamIdInput('STEAM_2:1:6155141').ok).toBe(false);
    expect(parseSteamIdInput('[U:2:12310283]').ok).toBe(false);
    expect(parseSteamIdInput('[G:1:12310283]').ok).toBe(false);
    // the HLTV proxy's id64 is not a user account
    expect(parseSteamIdInput('90071996842377216').ok).toBe(false);
  });

  it('refuses account 0 and overflow past 32 bits', () => {
    expect(parseSteamIdInput('STEAM_0:0:0').ok).toBe(false);
    expect(parseSteamIdInput('[U:1:0]').ok).toBe(false);
    expect(parseSteamIdInput('[U:1:4294967296]').ok).toBe(false);
    expect(parseSteamIdInput('STEAM_0:0:2147483648').ok).toBe(false);
    expect(parseSteamIdInput('[U:1:99999999999]').ok).toBe(false);
    expect(parseSteamIdInput('76561202255233024').ok).toBe(false);
    expect(parseSteamIdInput('[U:1:4294967295]').ok).toBe(true);
  });
});
