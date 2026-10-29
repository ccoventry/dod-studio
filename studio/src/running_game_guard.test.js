import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({
  checkRunningGame: vi.fn(),
  closeRunningGame: vi.fn(async () => {}),
}));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn(async () => false) }));
vi.mock('./toast.js', () => ({ showToast: vi.fn(() => ({ remove: () => {} })) }));

import { closeGameWithOtherSettings, runningGameDetails } from './running_game_guard.js';
import { checkRunningGame, closeRunningGame } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

const PRE = 'C:\\Games\\Half-Life - PRE-Anniversary for Movies\\hl.exe';
const POST = 'C:\\Games\\Half-Life - POST-Anniversary for Movies\\hl.exe';

function mismatch(differs, running = {}, wanted = {}) {
  return {
    state: 'mismatch',
    pid: 4242,
    running: { install: 'Half-Life - PRE-Anniversary for Movies', exe: PRE, width: 1920, height: 1080, ...running },
    wanted: { install: 'Half-Life - POST-Anniversary for Movies', exe: POST, width: 3440, height: 1440, ...wanted },
    differs,
  };
}

describe('the running-game message (#666)', () => {
  it('names the install and the resolution when both differ', () => {
    expect(STRINGS.RUNNING_GAME.message(mismatch(['install', 'resolution']))).toBe(
      'Day of Defeat is running from "Half-Life - PRE-Anniversary for Movies" at 1920×1080; '
      + 'DoD Studio is set to "Half-Life - POST-Anniversary for Movies" at 3440×1440. '
      + 'The game only takes these when it starts. Close it and start again?',
    );
  });

  it('names only the install when only it differs', () => {
    const text = STRINGS.RUNNING_GAME.message(mismatch(['install']));
    expect(text).toContain('running from "Half-Life - PRE-Anniversary for Movies";');
    expect(text).toContain('set to "Half-Life - POST-Anniversary for Movies".');
    expect(text).not.toContain('×');
  });

  it('names only the resolution when only it differs', () => {
    const text = STRINGS.RUNNING_GAME.message(mismatch(['resolution']));
    expect(text).toContain('running at 1920×1080; DoD Studio is set to 3440×1440.');
    expect(text).not.toContain('Anniversary');
  });

  it('names the game folder when it differs (#402)', () => {
    const folders = mismatch(['game_folder'], { game_folder: 'dod_movies' }, { game_folder: 'dod' });
    expect(STRINGS.RUNNING_GAME.message(folders)).toContain(
      'running with the "dod_movies" game folder; DoD Studio is set to the "dod" game folder.',
    );
    const all = mismatch(['install', 'game_folder', 'resolution'], { game_folder: 'dod_movies' }, { game_folder: 'dod' });
    expect(STRINGS.RUNNING_GAME.message(all)).toContain(
      'DoD Studio is set to "Half-Life - POST-Anniversary for Movies" with the "dod" game folder at 3440×1440.',
    );
  });

  it('lists both hl.exe paths only when the install differs', () => {
    expect(runningGameDetails(mismatch(['install']))).toEqual([
      { primary: STRINGS.RUNNING_GAME.RUNNING_EXE, secondary: PRE },
      { primary: STRINGS.RUNNING_GAME.STUDIO_EXE, secondary: POST },
    ]);
    expect(runningGameDetails(mismatch(['resolution']))).toEqual([]);
  });
});

const HOOK_A = 'C:\\Repos\\dod-studio\\local\\test-660\\dodstudio_goldsrc_hooks.dll';
const HOOK_B = 'C:\\Repos\\dod-studio\\local\\test-660b\\dodstudio_goldsrc_hooks.dll';

function hookMismatch(differs, running = {}, wanted = {}) {
  return mismatch(
    differs,
    { hook_dll: HOOK_A, hook_label: 'test-660', hook_build: '0.10.0+aaa', ...running },
    { hook_dll: HOOK_B, hook_label: 'test-660b', hook_build: '0.10.0+bbb', ...wanted },
  );
}

describe('the running-game message for the hook DLL (#1143)', () => {
  it('names both hook DLL folders when the path differs', () => {
    expect(STRINGS.RUNNING_GAME.message(hookMismatch(['hook_dll']))).toBe(
      'Day of Defeat is running with the hook DLL from "test-660"; '
      + 'DoD Studio is set to the hook DLL from "test-660b". '
      + 'The game only takes these when it starts. Close it and start again?',
    );
  });

  it('names both builds when the same path holds another build', () => {
    const check = hookMismatch(['hook_build'], { hook_dll: HOOK_B, hook_label: 'test-660b' });
    expect(STRINGS.RUNNING_GAME.message(check)).toContain(
      'running with hook build 0.10.0+aaa; DoD Studio is set to hook build 0.10.0+bbb.',
    );
  });

  it('calls a build without an id an older one', () => {
    const check = hookMismatch(['hook_build'], { hook_build: null });
    expect(STRINGS.RUNNING_GAME.message(check)).toContain(
      'running with an older hook build with no build id; DoD Studio is set to hook build 0.10.0+bbb.',
    );
  });

  it('follows the resolution with "with" when both differ', () => {
    expect(STRINGS.RUNNING_GAME.message(hookMismatch(['resolution', 'hook_dll']))).toContain(
      'running at 1920×1080 with the hook DLL from "test-660"; DoD Studio is set to 3440×1440 with the hook DLL from "test-660b".',
    );
  });

  it('says so when the hook DLL could not be read', () => {
    expect(STRINGS.RUNNING_GAME.message(hookMismatch(['hook_unreadable']))).toBe(
      "DoD Studio could not read which hook DLL the running Day of Defeat has, so it can't tell whether it is the one DoD Studio is set to. Close it and start again?",
    );
    const withResolution = STRINGS.RUNNING_GAME.message(hookMismatch(['resolution', 'hook_unreadable']));
    expect(withResolution).toContain('running at 1920×1080; DoD Studio is set to 3440×1440. DoD Studio could not read');
  });

  it('lists both hook DLL paths with their builds', () => {
    expect(runningGameDetails(hookMismatch(['hook_dll']))).toEqual([
      { primary: STRINGS.RUNNING_GAME.RUNNING_HOOK, secondary: `${HOOK_A} (build 0.10.0+aaa)` },
      { primary: STRINGS.RUNNING_GAME.STUDIO_HOOK, secondary: `${HOOK_B} (build 0.10.0+bbb)` },
    ]);
    expect(runningGameDetails(hookMismatch(['hook_build'], { hook_build: null }))[0].secondary)
      .toBe(`${HOOK_A} (no build id)`);
    expect(runningGameDetails(hookMismatch(['hook_unreadable']))).toEqual([
      { primary: STRINGS.RUNNING_GAME.STUDIO_HOOK, secondary: `${HOOK_B} (build 0.10.0+bbb)` },
    ]);
  });
});

describe('closeGameWithOtherSettings (#666)', () => {
  beforeEach(() => vi.clearAllMocks());

  it('goes on without asking when no game runs, it matches, or the check failed', async () => {
    for (const answer of [{ state: 'none' }, { state: 'match', pid: 1 }, null]) {
      checkRunningGame.mockResolvedValueOnce(answer);
      expect(await closeGameWithOtherSettings()).toBe(true);
    }
    expect(themedConfirm).not.toHaveBeenCalled();
    expect(closeRunningGame).not.toHaveBeenCalled();
  });

  it('passes the caller\'s unsaved settings to the check', async () => {
    checkRunningGame.mockResolvedValueOnce({ state: 'none' });
    const request = { game_path: POST, resolution_width: 3440, resolution_height: 1440 };
    await closeGameWithOtherSettings(request);
    expect(checkRunningGame).toHaveBeenCalledWith(request);
  });

  it('Cancel keeps the game and stops the caller', async () => {
    checkRunningGame.mockResolvedValueOnce(mismatch(['install', 'resolution']));
    themedConfirm.mockResolvedValueOnce(false);
    expect(await closeGameWithOtherSettings()).toBe(false);
    expect(themedConfirm).toHaveBeenCalledWith(
      expect.stringContaining('Close it and start again?'),
      expect.objectContaining({ confirmLabel: STRINGS.RUNNING_GAME.CLOSE_AND_RESTART }),
    );
    expect(closeRunningGame).not.toHaveBeenCalled();
  });

  it('yes closes that game by pid, then lets the caller start a new one', async () => {
    checkRunningGame.mockResolvedValueOnce(mismatch(['resolution']));
    themedConfirm.mockResolvedValueOnce(true);
    const button = { disabled: false };
    expect(await closeGameWithOtherSettings(null, { button })).toBe(true);
    expect(closeRunningGame).toHaveBeenCalledWith(4242);
    expect(button.disabled).toBe(false);
  });

  it('a game that would not close stops the caller', async () => {
    checkRunningGame.mockResolvedValueOnce(mismatch(['install']));
    themedConfirm.mockResolvedValueOnce(true);
    closeRunningGame.mockRejectedValueOnce('did not close');
    expect(await closeGameWithOtherSettings()).toBe(false);
  });
});
