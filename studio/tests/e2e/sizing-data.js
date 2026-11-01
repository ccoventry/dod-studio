// Realistic, awkward data for the widget-sizing checks (#46): long file names,
// long paths, many rows, long player names. Mocked IPC only.

const LONG_DIR = 'D:/Demos/Clan Matches/2026/Season 14 - Regular Season Playoff Round/Week 07 vs The Extraordinarily Long Clan Name Gaming';

export const PLAYERS = [
  '[WSOD25]=m00cat=The Extremely Long Player Name',
  'xX_ProSniperKillerOfDoom_Xx',
  'Sgt. Hartmann der Unbeugsame',
  'a',
  '{$T} Brain',
  'Wolfgang "Der Panzerknacker" Müller-Lüdenscheid',
];

export function makeStreak(i, source) {
  const base = 1000 + i * 3000;
  return {
    start_tick: base, end_tick: base + 900, source_demo: source,
    target_player: PLAYERS[i % PLAYERS.length], kill_count: 2 + (i % 5),
    timeline_string: `0${i}:12 - 0${i}:40`, duration_string: '28.4 s', player_index: i % 6,
    kills: [[base + 100, 100 + i, 'k98'], [base + 300, 103 + i, 'garand'], [base + 500, 105 + i, 'spring']],
    start_index: i * 10, end_index: i * 10 + 9, total_demo_frames: 40000, demo_fps: 100,
    viewdemo_times: [], match_start_tick: 100, frame_times: [],
    faction: i % 2 ? 'Axis' : 'Allies', victims: ['Victim With A Very Long Name Indeed', 'b', 'c'],
    victim_factions: ['Axis', 'Axis', 'Allies'], headshots: [], source_key: 'k', spectated: false,
    steam_id: '76561198000000001', recorded_this_demo: true,
  };
}

export function makeDemos(count) {
  return Array.from({ length: count }, (_, n) => {
    const name = n % 3 === 0
      ? `wsod25_group_stage_round_${n}_vs_the_extraordinarily_long_clan_name_gaming_dod_anzio_final_pov_m00cat.dem`
      : `match${n}.dem`;
    const path = `${LONG_DIR}/${name}`;
    return {
      path, name, tickrate: 100, is_pov: n % 4 !== 0, local_player_index: 2, playback_frames: 40000,
      streaks: Array.from({ length: 6 }, (__, i) => makeStreak(i, path)),
      file_key: `k${n}`, map_name: n % 2 ? 'dod_anzio' : 'dod_lennon_beta_special_edition_final',
      modified_unix_secs: 1760000000 + n * 86400, peak_packet_entities: 100, no_camera_data: false,
      recorder_id: '76561198000000001', recorder_name: PLAYERS[n % PLAYERS.length],
      teams: [], signon_maps: ['dod_anzio'], highlights_version: 99,
    };
  });
}

const LONG_INSTALL = 'C:/Program Files (x86)/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies with an extra long folder name';

/** A settings.json full of long paths, commands and player names. */
export const LONG_SETTINGS = {
  hl_path: `${LONG_INSTALL}/hl.exe`,
  hlae_path: 'C:/Programs/HLAE/Half-Life Advanced Effects Portable Edition Build/hlae.exe',
  ffmpeg_path: 'C:/Programs/ffmpeg-8.0-full_build-shared/ffmpeg-8.0-full_build-shared/bin/ffmpeg.exe',
  game_folder: `${LONG_INSTALL}/dod_movies_english_with_long_name`,
  goldsrc_hooks_dll_path: 'C:/Users/someone/Repos/dod-studio/target/i686-pc-windows-msvc/release/dodstudio_goldsrc_hooks.dll',
  obs_exe_path: 'C:/Program Files/obs-studio/bin/64bit/obs64.exe',
  target_drives: ['D:/dodstudio-captures/very/long/path/to/the/capture/output/folder/for/season/14', 'E:/caps', 'F:/another/long/path/to/captures/folder/number/three/here'],
  render_export_dirs: ['G:/Videos/Day of Defeat/Clan Matches/Season 14 Playoffs/Finished Clips Exported For Editing'],
  init_commands: ['mirv_fov 90; mirv_cvar_unhook_all; echo a very long initial command line goes here to test clipping', 'cl_showfps 1'],
  custom_commands: [
    { command: 'mirv_streams record name "D:/dodstudio-captures/very/long/path/to/take" ; echo another long command', relation: 'Before', offset_seconds: 2.5 },
    { command: 'echo x', relation: 'After', offset_seconds: 10 },
  ],
  clip_name_template: '{date}_{map}_{player}_{kills}-kill-streak_{weapon}_round_{round}_clip_{n}_extra_long_suffix',
  players: { players: PLAYERS.map((name, i) => ({ steam_id: `7656119800000000${i}`, name, png: `D:/sprays/${name}-very-long-spray-file-name-2048.png`, use_as: '', nameplate: '' })) },
};

/** Render jobs (RenderJobView) with long names and paths. */
export function makeRenderJobs(count) {
  return Array.from({ length: count }, (_, i) => ({
    id: String(i),
    name: `wsod25_group_stage_round_${i}_vs_the_extraordinarily_long_clan_name_gaming-chain_0${i % 9}_b${i}-obs`,
    stream: i % 2 ? 'all' : 'hudColor', stream_folder: 'all', frames: 1200 + i,
    date: '2026-10-29 10:00 PM', status: ['Queued', 'Rendering', 'Finished', 'Failed'][i % 4],
    speed: '1.2x', progress: (i * 7) % 100,
    error_log: i % 4 === 3 ? 'ffmpeg exited with code 1: Error while opening encoder for output stream #0:0 - maybe incorrect parameters' : null,
    settings_summary: 'ProRes 422 HQ @ 300fps', output_path: i % 4 === 2 ? `G:/Videos/Day of Defeat/Clan Matches/Season 14 Playoffs/Finished Clips Exported For Editing/clip_${i}.mov` : '',
    output_size_bytes: i % 4 === 2 ? 3_500_000_000 : null,
    take_folder: `D:/dodstudio-captures/very/long/path/to/the/capture/output/folder/for/season/14/demo${i}/chain_01_b0/take0000`,
    take_key: `s/${i}`, codec_id: 'prores', custom_codec_args: '', skip_available: false,
  }));
}

/** Boots the real shell with `count` demos queued through Add Folder. */
export async function bootWithDemos(page, count, extra = {}) {
  const demos = makeDemos(count);
  await page.addInitScript(([list, dir, more, LONG]) => {
    window.__mockInvokeHandlers = {
      get_settings: () => ({ pinned_folders: [], setup_dismissed: true, ...LONG, ...more }),
      diagnose_executable_paths: ({ paths }) => paths.map(() => 'ok'),
      browse_directory: () => ({
        path: dir, parent: null,
        subdirs: [{ name: 'Season 14 - Regular Season Playoff Round with a long name', path: `${dir}/sub`, demo_count: 12 }],
        demos: list.map((d, i) => ({ name: d.name, path: d.path, size_bytes: 80_000_000 + i, modified_unix_secs: 1760000000 + i * 86400, map_name: null })),
      }),
      'plugin:dialog|open': () => dir,
      scan_directory: () => ({ demos: list, unchanged: 0, copies: [], cancelled: false }),
      queue_match_groups: () => ({ demos: [], matches: [] }),
      scan_demo_folders: () => [],
    };
  }, [demos, LONG_DIR, extra, LONG_SETTINGS]);
  await page.goto('/index.html');
  await page.evaluate(() => document.querySelector('#add-folder-btn').click());
  await page.locator('#master-demo-table-body tr').nth(count - 1).waitFor();
}
