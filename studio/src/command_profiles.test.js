import { describe, it, expect } from 'vitest';
import {
  profileLists, sameLists, normaliseProfiles, matchingProfile, profileState,
  saveProfile, renameProfile, deleteProfile,
} from './command_profiles.js';

const FOTW = {
  init_commands: ['r_decals 256', 'mirv_fov 90'],
  custom_commands: [{ command: 'host_timescale 0.5', relation: 'Before', offset_seconds: 1.5 }],
};

describe('command profiles', () => {
  it('normalises lists: trims, drops blank rows, keeps order', () => {
    expect(profileLists({
      init_commands: [' mirv_fov 90 ', '', '  ', 'r_decals 256'],
      custom_commands: [
        { command: ' say hi ', relation: 'After', offset_seconds: 0 },
        { command: '', relation: 'Before', offset_seconds: 2 },
        { command: 'x', relation: 'sideways', offset_seconds: NaN },
      ],
    })).toEqual({
      init_commands: ['mirv_fov 90', 'r_decals 256'],
      custom_commands: [
        { command: 'say hi', relation: 'After', offset_seconds: 0 },
        { command: 'x', relation: 'Before', offset_seconds: 2.0 },
      ],
    });
    expect(profileLists(undefined)).toEqual({ init_commands: [], custom_commands: [] });
  });

  it('compares lists ignoring blank rows and whitespace, but not order', () => {
    expect(sameLists(FOTW, { ...FOTW, init_commands: ['r_decals 256 ', '', 'mirv_fov 90'] })).toBe(true);
    expect(sameLists(FOTW, { ...FOTW, init_commands: ['mirv_fov 90', 'r_decals 256'] })).toBe(false);
    expect(sameLists(FOTW, {
      ...FOTW,
      custom_commands: [{ command: 'host_timescale 0.5', relation: 'After', offset_seconds: 1.5 }],
    })).toBe(false);
  });

  it('drops unnamed profiles when loading', () => {
    expect(normaliseProfiles([{ name: ' FOTW ', ...FOTW }, { name: '', ...FOTW }, null]).map((p) => p.name))
      .toEqual(['FOTW']);
    expect(normaliseProfiles('nonsense')).toEqual([]);
  });

  it('saves by name, replacing one of the same name, sorted', () => {
    let profiles = saveProfile([], 'Wallhack', FOTW);
    profiles = saveProfile(profiles, 'clean HUD', { init_commands: ['hud_draw 0'] });
    profiles = saveProfile(profiles, 'wallhack', { init_commands: ['r_drawviewmodel 0'] });
    expect(profiles.map((p) => p.name)).toEqual(['clean HUD', 'wallhack']);
    expect(profiles[1].init_commands).toEqual(['r_drawviewmodel 0']);
    expect(saveProfile(profiles, '  ', FOTW)).toBe(profiles);
  });

  it('finds the profile the lists match', () => {
    const profiles = saveProfile(saveProfile([], 'FOTW', FOTW), 'Empty', {});
    expect(matchingProfile(profiles, FOTW).name).toBe('FOTW');
    expect(matchingProfile(profiles, { init_commands: [''] }).name).toBe('Empty');
    expect(matchingProfile(profiles, { init_commands: ['mirv_fov 100'] })).toBe(null);
  });

  it('reports the applied profile, and "edited" once the lists move away from it', () => {
    const profiles = saveProfile([], 'FOTW', FOTW);
    expect(profileState(profiles, 'FOTW', FOTW)).toEqual({ name: 'FOTW', edited: false });
    expect(profileState(profiles, 'FOTW', { ...FOTW, init_commands: ['mirv_fov 100'] }))
      .toEqual({ name: 'FOTW', edited: true });
    // No applied profile, or a deleted one: a matching profile stands in, never edited.
    expect(profileState(profiles, '', FOTW)).toEqual({ name: 'FOTW', edited: false });
    expect(profileState(profiles, 'Gone', { init_commands: ['x'] })).toEqual({ name: '', edited: false });
  });

  it('renames, refusing a blank name or one another profile has', () => {
    const profiles = saveProfile(saveProfile([], 'A', FOTW), 'B', {});
    expect(renameProfile(profiles, 'A', 'C').map((p) => p.name)).toEqual(['B', 'C']);
    expect(renameProfile(profiles, 'A', 'C')[1].init_commands).toEqual(FOTW.init_commands);
    expect(renameProfile(profiles, 'A', 'a').map((p) => p.name)).toEqual(['a', 'B']);
    expect(renameProfile(profiles, 'A', 'b')).toBe(null);
    expect(renameProfile(profiles, 'A', ' ')).toBe(null);
    expect(renameProfile(profiles, 'Missing', 'C')).toBe(null);
  });

  it('deletes by name', () => {
    const profiles = saveProfile(saveProfile([], 'A', {}), 'B', {});
    expect(deleteProfile(profiles, 'A').map((p) => p.name)).toEqual(['B']);
  });
});
