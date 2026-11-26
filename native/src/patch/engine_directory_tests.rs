use super::*;
use crate::patch::types::{CaptureStreak, PatchJob, PatcherConfig};
use crate::patch::{DEMO_HEADER_SIZE, DIR_ENTRY_SIZE, DIRECTORY_OFFSET_POS, NETMSG_INFO_SIZE};
use crate::test_support::Scratch;
use dem::protocol::{NETWORK_PROTOCOL_47, NETWORK_PROTOCOL_48};

fn frame(out: &mut Vec<u8>, kind: u8, tick: i32) {
    out.push(kind);
    out.extend_from_slice(&(tick as f32 * 0.01).to_le_bytes());
    out.extend_from_slice(&tick.to_le_bytes());
}

fn network(out: &mut Vec<u8>, tick: i32) {
    frame(out, 1, tick);
    out.extend_from_slice(&[0u8; NETMSG_INFO_SIZE]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(0x01); // svc_nop
}

fn entry(out: &mut Vec<u8>, kind: i32, description: &[u8], offset: usize, length: usize) {
    let mut e = [0u8; DIR_ENTRY_SIZE];
    e[0..4].copy_from_slice(&kind.to_le_bytes());
    e[4..4 + description.len()].copy_from_slice(description);
    e[84..88].copy_from_slice(&(offset as i32).to_le_bytes());
    e[88..92].copy_from_slice(&(length as i32).to_le_bytes());
    out.extend_from_slice(&e);
}

/// Two segments, as every real demo has: a LOADING one, then playback
/// opening with `DemoStart`, each closed by `NextSection`.
fn two_segment_demo() -> Vec<u8> {
    let mut d = vec![0u8; DEMO_HEADER_SIZE];
    d[0..6].copy_from_slice(b"HLDEMO");
    let loading = d.len();
    network(&mut d, 0);
    frame(&mut d, 5, 0);
    let playback = d.len();
    frame(&mut d, 2, 0);
    for tick in 1..=10 {
        network(&mut d, tick);
    }
    frame(&mut d, 5, 10);
    let directory = d.len();
    d.extend_from_slice(&2i32.to_le_bytes());
    entry(&mut d, 0, b"LOADING", loading, playback - loading);
    entry(&mut d, 1, b"Playback", playback, directory - playback);
    d[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(directory as i32).to_le_bytes());
    d
}

/// Where each segment really starts, found by walking the frames up to
/// the directory, plus where the last one ends.
fn segment_starts(data: &[u8], directory: usize) -> Vec<usize> {
    let mut starts = vec![DEMO_HEADER_SIZE];
    let mut pos = DEMO_HEADER_SIZE;
    while pos < directory {
        let kind = data[pos];
        let body = match kind {
            0 | 1 => {
                let at = pos + 9 + NETMSG_INFO_SIZE;
                NETMSG_INFO_SIZE
                    + 4
                    + u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize
            }
            2 | 5 => 0,
            3 => crate::patch::CMD_FRAME_SIZE,
            other => panic!("unexpected frame type {other} at {pos}"),
        };
        pos += 9 + body;
        if kind == 5 {
            starts.push(pos);
        }
    }
    assert_eq!(pos, directory, "the frames run exactly up to the directory");
    starts
}

#[test]
fn every_directory_entry_still_points_at_its_segment_after_injection() {
    let scratch = Scratch::new("patch_directory");
    let input = scratch.path().join("in.dem");
    let output = scratch.path().join("out.dem");
    let original = two_segment_demo();
    std::fs::write(&input, &original).unwrap();

    let job = PatchJob {
        source_demo: input.to_string_lossy().to_string(),
        output_demo: output.clone(),
        // Built from JSON rather than a struct literal, so the test keeps
        // compiling when `CaptureStreak` gains an optional field (#417
        // adds `source_key`). Missing optional fields deserialize to their
        // defaults, and `frame_times` is `serde(skip)`.
        streaks: vec![
            serde_json::from_value::<CaptureStreak>(serde_json::json!({
                "start_tick": 3,
                "end_tick": 8,
                "source_demo": input.to_string_lossy(),
                "target_player": null,
                "kill_count": 0,
                "timeline_string": "",
                "duration_string": "",
                "player_index": 0,
                "kills": [],
                "start_index": 0,
                "end_index": 0,
                "total_demo_frames": 14,
                "demo_fps": 100.0,
            }))
            .unwrap(),
        ],
        target_player: None,
        // Every kind the patcher writes: ConsoleCommand frames at
        // DemoStart and mid-playback, and a director event, which is a
        // whole injected network frame.
        init_commands: vec!["sys_autodir".to_string(), "mirv_movie_fps 60".to_string()],
        scheduled_commands: vec![(6, "echo six".to_string()), (9, "echo nine".to_string())],
        director_events: vec![(7, "echo [dod-studio] CLIP".to_string())],
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };
    let config = PatcherConfig {
        decal_flush: false,
        ..PatcherConfig::default()
    };
    let cancel = Arc::new(AtomicBool::new(false));
    StreamPatcher::new(&input, &output)
        .patch(&job, &config, &cancel)
        .unwrap();

    let patched = std::fs::read(&output).unwrap();
    assert!(patched.len() > original.len(), "nothing was injected");
    let read_i32 = |at: usize| i32::from_le_bytes(patched[at..at + 4].try_into().unwrap());
    let directory = read_i32(DIRECTORY_OFFSET_POS) as usize;
    let starts = segment_starts(&patched, directory);
    assert_eq!(read_i32(directory), 2);
    for (k, window) in starts.windows(2).enumerate() {
        let e = directory + 4 + k * DIR_ENTRY_SIZE;
        assert_eq!(read_i32(e + 84) as usize, window[0], "entry {k}'s offset");
        assert_eq!(
            read_i32(e + 88) as usize,
            window[1] - window[0],
            "entry {k}'s length"
        );
    }
    assert_eq!(directory + 4 + 2 * DIR_ENTRY_SIZE, patched.len());
}

/// A protocol-47 demo whose sign-on frame holds `sign_on`, then a few
/// network frames; no directory.
fn protocol_47_demo(sign_on: &[u8]) -> Vec<u8> {
    let mut d = vec![0u8; DEMO_HEADER_SIZE];
    d[0..8].copy_from_slice(b"HLDEMO\0\0");
    d[12..16].copy_from_slice(&NETWORK_PROTOCOL_47.to_le_bytes());
    frame(&mut d, 0, 0);
    d.extend_from_slice(&[0u8; NETMSG_INFO_SIZE]);
    d.extend_from_slice(&(sign_on.len() as u32).to_le_bytes());
    d.extend_from_slice(sign_on);
    frame(&mut d, 5, 0);
    frame(&mut d, 2, 0);
    for tick in 1..=3 {
        network(&mut d, tick);
    }
    frame(&mut d, 5, 3);
    d[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&0i32.to_le_bytes());
    d
}

/// Patches `input` into `output` with an empty job.
fn patch_plain(input: &std::path::Path, output: &std::path::Path) -> std::io::Result<()> {
    let job = PatchJob {
        source_demo: input.to_string_lossy().to_string(),
        output_demo: output.to_path_buf(),
        streaks: Vec::new(),
        target_player: None,
        init_commands: Vec::new(),
        scheduled_commands: Vec::new(),
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };
    let config = PatcherConfig {
        decal_flush: false,
        ..PatcherConfig::default()
    };
    StreamPatcher::new(input, output).patch(&job, &config, &Arc::new(AtomicBool::new(false)))
}

/// A protocol-47 demo's patched copy says 48 in its header and its
/// `svc_serverinfo`, so the game plays it without a warning (#239).
#[test]
fn a_protocol_47_demo_s_patched_copy_says_48() {
    let scratch = Scratch::new("patch_protocol47");
    let input = scratch.path().join("in.dem");
    let output = scratch.path().join("out.dem");
    let mut sign_on = vec![11u8];
    sign_on.extend_from_slice(&NETWORK_PROTOCOL_47.to_le_bytes());
    sign_on.extend_from_slice(&[0u8; 4 + 4 + 16 + 3]);
    sign_on.extend_from_slice(b"dod\0an HLTV\0maps/dod_donner.bsp\0\0\0");
    let d = protocol_47_demo(&sign_on);
    std::fs::write(&input, &d).unwrap();
    assert_eq!(
        crate::demo_split::quick_maps(&d, NETWORK_PROTOCOL_47),
        ["dod_donner"]
    );

    patch_plain(&input, &output).unwrap();

    let patched = std::fs::read(&output).unwrap();
    assert_eq!(
        crate::demo_split::header_protocol(&patched),
        Some(NETWORK_PROTOCOL_48)
    );
    assert_eq!(
        crate::demo_split::quick_maps(&patched, NETWORK_PROTOCOL_48),
        ["dod_donner"]
    );
    assert!(crate::demo_split::quick_maps(&patched, NETWORK_PROTOCOL_47).is_empty());
}

/// A protocol-47 demo with no server info to convert is patched all the
/// same, and its copy stays a whole protocol-47 demo, header included:
/// the game plays that with its warning under Studio's launch.
#[test]
fn a_protocol_47_demo_with_no_server_info_is_patched_and_left_at_47() {
    let scratch = Scratch::new("patch_protocol47_none");
    let input = scratch.path().join("in.dem");
    let output = scratch.path().join("out.dem");
    std::fs::write(&input, protocol_47_demo(&[1, 1, 1])).unwrap();

    patch_plain(&input, &output).unwrap();
    let patched = std::fs::read(&output).unwrap();
    assert_eq!(
        crate::demo_split::header_protocol(&patched),
        Some(NETWORK_PROTOCOL_47)
    );
}
