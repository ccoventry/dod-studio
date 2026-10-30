import { describe, it, expect } from 'vitest';
import { clipManifest, manifestPathFor, takePlayer, nameplateCount, MANIFEST_FORMAT } from './clip_manifest.js';
import { streakUid } from './take_index.js';

const M00CAT = '76561197972576011';
const GORILLA = '76561197960265729';

const streak = (player_index, target_player, steam_id, firstKill) => ({
  player_index, target_player, steam_id, start_tick: firstKill, end_tick: firstKill + 100,
  kills: [[firstKill, 1, 'Garand'], [firstKill + 100, 2, 'Garand']],
});

function project() {
  const a = streak(3, 'dicE[: :]m00cat :D', M00CAT, 1000);
  const b = streak(5, 'dicE[: :]Gorilla[bc]', GORILLA, 5000);
  const c = streak(3, 'dicE m00cat', M00CAT, 9000);
  const demo = { path: 'D:\\demos\\x.dem', streaks: [a, b, c] };
  const takeIndex = {
    'session_1/chain_01_b0': [streakUid(demo.path, a)],
    // An overlap merge of two players' highlights: no one nameplate fits.
    'session_1/chain_02_b0': [streakUid(demo.path, a), streakUid(demo.path, b)],
    // The same player twice, under two names: still one player.
    'session_1/chain_03_b0': [streakUid(demo.path, a), streakUid(demo.path, c)],
  };
  return { demos: [demo], takeIndex };
}

const lookups = {
  useAsName: (id) => (id === M00CAT ? 'Chris' : null),
  nameplatePath: (id) => (id === M00CAT ? 'D:\\np\\m00cat.png' : null),
};

describe('manifestPathFor', () => {
  it('sits beside the EDL with the same name', () => {
    expect(manifestPathFor('D:\\movie\\dod_highlights.edl')).toBe('D:\\movie\\dod_highlights.dodstudio.json');
    expect(manifestPathFor('D:\\movie\\cut.EDL')).toBe('D:\\movie\\cut.dodstudio.json');
    expect(manifestPathFor('D:\\movie\\noext')).toBe('D:\\movie\\noext.dodstudio.json');
  });
});

describe('takePlayer', () => {
  const { demos, takeIndex } = project();
  const byUid = new Map();
  demos[0].streaks.forEach((s) => byUid.set(streakUid(demos[0].path, s), s));

  it("finds the take's one player by its highlights", () => {
    expect(takePlayer('session_1/chain_01_b0', takeIndex, byUid))
      .toEqual({ steam_id: M00CAT, in_game_name: 'dicE[: :]m00cat :D' });
  });

  it('gives no player for a take of two players, or one never recorded', () => {
    expect(takePlayer('session_1/chain_02_b0', takeIndex, byUid)).toBeNull();
    expect(takePlayer('session_9/chain_01_b0', takeIndex, byUid)).toBeNull();
    expect(takePlayer('', takeIndex, byUid)).toBeNull();
  });

  it('counts one player seen under two names once, by SteamID', () => {
    expect(takePlayer('session_1/chain_03_b0', takeIndex, byUid)?.steam_id).toBe(M00CAT);
  });
});

describe('clipManifest', () => {
  const clips = [
    { name: 'clip1', path: 'D:\\r\\clip1.mov', seconds: 2, take_key: 'session_1/chain_01_b0' },
    { name: 'clip2', path: 'D:\\r\\clip2.mov', seconds: 3, take_key: 'session_1/chain_02_b0' },
    { name: 'clip3', path: 'D:\\r\\clip3.mov', seconds: 1.5, take_key: '' },
  ];

  it('names each clip\'s player by their Use as name, with their nameplate', () => {
    const m = clipManifest(clips, { edlName: 'dod_highlights.edl', ...project(), ...lookups });
    expect(m.format).toBe(MANIFEST_FORMAT);
    expect(m.version).toBe(1);
    expect(m.edl).toBe('dod_highlights.edl');
    expect(m.clips[0]).toEqual({
      name: 'clip1', path: 'D:\\r\\clip1.mov', seconds: 2,
      player: { steam_id: M00CAT, name: 'Chris', in_game_name: 'dicE[: :]m00cat :D' },
      nameplate: 'D:\\np\\m00cat.png',
    });
    expect(nameplateCount(m)).toBe(1);
  });

  it('leaves a clip with no one player, or none known, without player or nameplate', () => {
    const m = clipManifest(clips, { ...project(), ...lookups });
    expect(m.clips[1]).toMatchObject({ name: 'clip2', player: null, nameplate: null });
    expect(m.clips[2]).toMatchObject({ name: 'clip3', player: null, nameplate: null });
  });

  it('falls back to the in-game name when no Use as is set', () => {
    const { demos, takeIndex } = project();
    const m = clipManifest(clips.slice(0, 1), { demos, takeIndex });
    expect(m.clips[0].player.name).toBe('dicE[: :]m00cat :D');
    expect(m.clips[0].nameplate).toBeNull();
  });

  it('round-trips through JSON as it is saved', () => {
    const m = clipManifest(clips, { ...project(), ...lookups });
    expect(JSON.parse(JSON.stringify(m))).toEqual(m);
  });
});
