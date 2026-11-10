// A project file as builds from before #1085 save it (no `schema`), shaped
// like a real movie project: scanned demos with marks, notes, ranges and
// statuses, takes with and without their capture record, renders, a team name
// and the Review box's pick. The paths are made up.

const POV_DEMO = 'D:\\demos\\movie\\grp1_h1_alpha.dem';
const HLTV_DEMO = 'D:\\demos\\movie\\grp1_hltv.dem';
const FIRST_TAKE = 'session_20261102_185314/dodstudio_chain_01_b0';
const SECOND_TAKE = 'session_20261102_185314/dodstudio_chain_02_b0';
const UNRECORDED_TAKE = 'session_20261026_015608/dodstudio_chain_01_b0';

/** A highlight row over `kills`, each `[tick, seconds, weapon]`. */
function row(player, kills, extra = {}) {
  return {
    start_tick: kills[0][0],
    end_tick: kills[kills.length - 1][0],
    source_demo: extra.source_demo ?? POV_DEMO,
    target_player: 'alpha',
    kill_count: kills.length,
    timeline_string: kills.map(([, , weapon]) => weapon).join(', '),
    duration_string: '0:12',
    player_index: player,
    kills,
    start_index: 0,
    end_index: kills.length - 1,
    total_demo_frames: 966750,
    demo_fps: 668.12,
    faction: 'Axis',
    victims: kills.map((_, i) => `victim${i}`),
    headshots: kills.map(() => 'no'),
    flag_tags: kills.map(() => []),
    source_key: '105137417-36d34d0b3d5973e4',
    spectated: false,
    steam_id: '76561197981202855',
    recorded_this_demo: true,
    selected: false,
    ...extra,
  };
}

const K98 = 'K98';

/** A fresh copy, so a test can change it freely. */
export function projectSchema1() {
  return {
    version: '0.12.0',
    scanPaths: ['D:\\demos\\movie'],
    demos: [
      {
        path: POV_DEMO,
        name: 'grp1_h1_alpha.dem',
        tickrate: 668.12,
        is_pov: true,
        local_player_index: 7,
        playback_frames: 966750,
        file_key: '105137417-36d34d0b3d5973e4',
        map_name: 'dod_armory_b6',
        modified_unix_secs: 1755281537,
        peak_packet_entities: 54,
        no_camera_data: false,
        recorder_id: '76561197981202855',
        recorder_name: 'alpha',
        teams: [{ side: 'Allies', tag: 'gskiLL' }, { side: 'Axis', tag: 'dicE' }],
        signon_maps: ['dod_armory_b6'],
        highlights_version: 2,
        streaks: [
          row(7, [[1000, 10, K98], [1400, 12, K98], [1800, 14, K98]], { curation: 'Keep', status: 'Captured', selected: true }),
          row(7, [[3000, 30, K98], [3300, 32, K98]], { curation: 'Keep', status: 'Rendered' }),
          row(7, [[5000, 50, K98], [5200, 52, K98], [5400, 54, K98]], { curation: 'Keep', status: 'Pending', statusByHand: true, notes: 'slow but maybe', clipName: 'alpha_slow_kar' }),
          row(7, [[7000, 70, K98], [7200, 72, K98], [7400, 74, K98]], { curation: 'Maybe', start_index: 1 }),
          row(7, [[9000, 90, K98], [9100, 91, K98]], { curation: 'Skip' }),
          row(7, [[11000, 110, K98], [11100, 111, K98]]),
        ],
      },
      {
        path: HLTV_DEMO,
        name: 'grp1_hltv.dem',
        tickrate: 100,
        is_pov: false,
        local_player_index: null,
        playback_frames: 200000,
        file_key: '200000-aa',
        map_name: 'dod_railroad2',
        no_camera_data: false,
        teams: [],
        signon_maps: ['dod_railroad2'],
        highlights_version: 2,
        streaks: [
          row(3, [[2000, 20, 'Garand'], [2100, 21, 'Garand']], { source_demo: HLTV_DEMO, curation: 'Keep', spectated: true }),
        ],
      },
    ],
    takeIndex: {
      [FIRST_TAKE]: [`${POV_DEMO}#7#1000#1800`],
      [SECOND_TAKE]: [`${POV_DEMO}#7#3000#3300`],
      [UNRECORDED_TAKE]: [`${HLTV_DEMO}#3#2000#2100`],
    },
    takeCaptures: {
      [FIRST_TAKE]: {
        session_id: 'session_20261102_185314', captured_at: 1793669611372, capture_mode: 'direct_to_video',
        capture_fps: 300, take_folder: 'D:\\raw\\session_20261102_185314\\dodstudio_chain_01_b0',
      },
      [SECOND_TAKE]: {
        session_id: 'session_20261102_185314', captured_at: 1793669700000, capture_mode: 'frames',
        capture_fps: 120, take_folder: 'D:\\raw\\session_20261102_185314\\dodstudio_chain_02_b0',
      },
    },
    takeRenders: {
      [SECOND_TAKE]: [{
        started_unix_ms: 1793667368389, stream: 'all', codec: 'prores_422', fps: 120,
        output_path: 'D:\\clips\\alpha_row02.mov', output_size_bytes: 13143781,
      }],
    },
    teams: { names: { 'puts on a clinic': 'clinic' }, merged: {} },
    review: { hltv: 'missing', hltv_players: [] },
    mode: 'workspace',
  };
}

/** The file's take keys, for a test to look one up. */
export const SCHEMA1_TAKES = { FIRST_TAKE, SECOND_TAKE, UNRECORDED_TAKE };
