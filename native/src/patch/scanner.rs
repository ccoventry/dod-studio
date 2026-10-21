// patch/scanner.rs
// Life-bounded highlight scanner, HLTV detection, and the check that a
// scanned demo is still the same file when a batch starts.
// Every function here performs std::fs I/O — native-only.

use crate::patch::types::{CaptureStreak, HighlightStatus};
use crate::patch::{MAX_PAYLOAD_LIMIT_BYTES, NETWORK_HEADER_ALIGNMENT, SCANNER_SECTION_BOUNDARY};

// ── HLTV detection ────────────────────────────────────────────────────────────

/// How much of a demo [`is_hltv_demo`] reads.
const HLTV_MARKER_WINDOW: u64 = 4096;

/// Whether an HLTV proxy recorded the demo, from its first few KB and no
/// parse (#566). The proxy's connect message ends `Spawn count N (HLTV)`,
/// about 1,060 bytes into the file: in all 113 HLTV demos of 984 surveyed,
/// and in no POV demo. `HLTV Proxy` in the first 512 bytes, what this looked
/// for before, is in none of them, so it never matched.
pub fn is_hltv_demo(path: &std::path::Path) -> Result<bool, std::io::Error> {
    use std::io::Read;
    let mut head = Vec::with_capacity(HLTV_MARKER_WINDOW as usize);
    std::fs::File::open(path)?
        .take(HLTV_MARKER_WINDOW)
        .read_to_end(&mut head)?;
    Ok(is_hltv_head(&head))
}

/// [`is_hltv_demo`] on bytes already read from the start of a demo.
pub fn is_hltv_head(head: &[u8]) -> bool {
    head.windows(6).any(|window| window == b"(HLTV)")
}

// ── Life-bounded highlight scanner ───────────────────────────────────────────
// Reads player.kill_streaks directly from the completed Analysis — the authoritative,
// already-segmented output of use_kill_streak_updates (analysis/src/kill.rs).
// Segmentation boundaries (death, round reset, map change) are handled by the
// analysis crate, including the grenade kill-after-death edge case.

/// How much of a demo the leading-frame walk reads first. The opening segment
/// it walks is the connection's signon: tens to hundreds of KB, well inside.
const LEADING_FRAMES_PREFIX_BYTES: usize = 8 * 1024 * 1024;

fn read_prefix(path: &std::path::Path, limit: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::with_capacity(limit);
    std::fs::File::open(path)?
        .take(limit as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Each frame's time in the demo's opening segment, walked from `bytes` (the
/// whole file, or its first part; `file_len` is the whole file's length).
/// The flag says whether the walk finished: `false` means `bytes` ran out
/// before a stopping frame, so the caller reads the whole file and asks again.
fn leading_frame_times(
    bytes: &[u8],
    file_len: usize,
    analysis: &analysis::Analysis,
) -> Result<(Vec<f32>, bool), String> {
    let mut frame_times: Vec<f32> = Vec::with_capacity(analysis.demo_info.playback_frames as usize);
    let mut stopped = false;
    let mut truncated = false;
    if bytes.len() >= crate::patch::DEMO_HEADER_SIZE {
        let directory_offset = i32::from_le_bytes(
            bytes[crate::patch::DIRECTORY_OFFSET_POS..crate::patch::DEMO_HEADER_SIZE]
                .try_into()
                .unwrap(),
        ) as usize;
        let mut pos = crate::patch::DEMO_HEADER_SIZE;
        let full_end = if directory_offset > 0 && directory_offset <= file_len {
            directory_offset
        } else {
            file_len
        };
        // What was actually read; the walk asks for more when it runs out
        // before a stopping frame.
        let end = full_end.min(bytes.len());
        truncated = end < full_end;
        while pos + crate::patch::FRAME_HEADER_SIZE <= end {
            let type_byte = bytes[pos];
            if type_byte > 9 && type_byte != 255 {
                stopped = true;
                break;
            }
            if type_byte == SCANNER_SECTION_BOUNDARY {
                stopped = true;
                break;
            }
            if type_byte != 255 {
                let time = f32::from_le_bytes(bytes[pos + 1..pos + 5].try_into().unwrap());
                frame_times.push(time);
            }
            pos += crate::patch::FRAME_HEADER_SIZE;
            match type_byte {
                0 | 1 => {
                    let total_fixed_size = NETWORK_HEADER_ALIGNMENT;
                    if pos + total_fixed_size > end {
                        break;
                    }
                    let len = i32::from_le_bytes(
                        bytes[pos + crate::patch::NETMSG_INFO_SIZE..pos + total_fixed_size]
                            .try_into()
                            .unwrap(),
                    ) as usize;
                    if len > MAX_PAYLOAD_LIMIT_BYTES {
                        return Err(format!(
                            "Scanner alignment lost! Read impossible packet size: {} bytes at pos {}",
                            len, pos
                        ));
                    }
                    pos += total_fixed_size + len;
                }
                2 | 255 => {}
                3 => pos += crate::patch::CMD_FRAME_SIZE,
                4 => pos += crate::patch::CLIENT_DATA_FRAME_SIZE,
                6 => pos += crate::patch::EVENT_FRAME_SIZE,
                7 => pos += 8,
                8 => {
                    if pos + 8 > end {
                        break;
                    }
                    let len =
                        u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
                    pos += 24 + len;
                }
                9 => {
                    if pos + 4 > end {
                        break;
                    }
                    let len = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                    pos += 4 + len;
                }
                _ => {
                    stopped = true;
                    break;
                }
            }
        }
    }
    Ok((frame_times, stopped || !truncated))
}

/// The Capture page's rows: one per highlight, as `analysis::highlights`
/// defines them, each built on `demo` (the fields every row of the demo
/// shares). Whose rows exist is decided here and nowhere later (#573):
/// `Highlights::shown`, the same rule the in-game Highlights tab applies
/// (D18: a POV demo's recorder, an HLTV demo's everyone), so the two list
/// the same highlights; the frontend only narrows them by Min Kills and its
/// Player filter. Saved projects index these rows in this order. The slot is
/// the one the kills were scored from, which capture spectates (#706), so a
/// player who left before the demo ended keeps theirs; only a highlight
/// whose slot is unknown is skipped, as capture needs one to anchor the
/// patcher.
fn capture_streaks(
    highlights: &analysis::highlights::Highlights,
    demo: &CaptureStreak,
) -> Vec<CaptureStreak> {
    let mut streaks = Vec::new();
    for highlight in highlights.shown() {
        let Some(slot) = highlight.slot else {
            continue;
        };
        let kills_raw: Vec<(i32, f32, String)> = highlight
            .kills
            .iter()
            .map(|kill| {
                (
                    kill.frame_index as i32,
                    kill.real_secs,
                    analysis::weapon_display_name(&kill.weapon),
                )
            })
            .collect();
        let viewdemo_times: Vec<f32> = highlight.kills.iter().map(|k| k.viewdemo_secs).collect();

        let end_index = kills_raw.len().saturating_sub(1);
        let mut streak = CaptureStreak {
            start_tick: kills_raw[0].0,
            end_tick: kills_raw[end_index].0,
            target_player: Some(highlight.player.clone()),
            kill_count: kills_raw.len(),
            player_index: slot as usize,
            kills: kills_raw,
            viewdemo_times,
            start_index: 0,
            end_index,
            steam_id: highlight.steam_id(),
            recorded_this_demo: highlight.recorder,
            ..demo.clone()
        };
        streak.update_visuals();
        streaks.push(streak);
    }
    streaks
}

pub fn scan_demo_for_highlights(
    path: &std::path::Path,
) -> Result<
    (
        f32,
        Vec<CaptureStreak>,
        bool,
        Option<usize>,
        i32,
        Option<i32>,
        std::sync::Arc<Vec<f32>>,
    ),
    String,
> {
    scan_demo_for_highlights_with_analysis(path).map(|(result, _analysis)| result)
}

// Same scan as `scan_demo_for_highlights`, but also hands back the full
// `Analysis` it already computed internally (previously always discarded)
// so callers doing a folder-wide scan (e.g. Capture Studio's `scan_directory`)
// can write it straight into the analyzer cache instead of re-parsing the
// same demo from scratch the next time it's opened in the Demo Analyzer.
pub fn scan_demo_for_highlights_with_analysis(
    path: &std::path::Path,
) -> Result<
    (
        (
            f32,
            Vec<CaptureStreak>,
            bool,
            Option<usize>,
            i32,
            Option<i32>,
            std::sync::Arc<Vec<f32>>,
        ),
        analysis::Analysis,
    ),
    String,
> {
    // No HLTV refusal here: the one that stood here looked for a string no
    // HLTV demo has, so it never refused one, and HLTV demos are to list
    // every player's streaks (#247, D18). See #566.

    // The analysis comes from the analyzer cache when the Demo Analyzer (or an
    // earlier scan) already parsed this exact file: ~15 ms instead of a full
    // parse of up to several seconds. A miss parses and fills the cache.
    let (_, analysis, _) = crate::run_analyzer_cached(&path.to_path_buf(), |_, _| {})
        .map_err(|e| format!("Failed to parse demo: {}", e))?;

    // `frame_times` only covers the demo's opening segment (the walk stops at
    // the first section boundary), so it only needs the start of the file.
    let file_len = std::fs::metadata(path)
        .map_err(|e| format!("Failed to read file: {}", e))?
        .len() as usize;
    let prefix = read_prefix(path, LEADING_FRAMES_PREFIX_BYTES)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    let (mut frame_times, complete) = leading_frame_times(&prefix, file_len, &analysis)?;
    if !complete {
        let bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
        frame_times = leading_frame_times(&bytes, file_len, &analysis)?.0;
    }

    let final_demo_frames = if analysis.demo_info.playback_frames > 0 {
        analysis.demo_info.playback_frames
    } else if !frame_times.is_empty() {
        frame_times.len() as i32
    } else {
        0
    };
    let frame_times_arc = std::sync::Arc::new(frame_times);

    let mut tickrate = if analysis.demo_info.playback_time > 0.0 {
        analysis.demo_info.playback_frames as f32 / analysis.demo_info.playback_time
    } else {
        100.0
    };

    // Fallback if the demo header has garbage values
    if !tickrate.is_normal() || !(10.0..=1000.0).contains(&tickrate) {
        tickrate = 100.0;
    }

    let source_key = crate::utils::demo_hasher::key_text(
        crate::utils::demo_hasher::demo_key_of_head(&prefix, file_len as u64),
    );

    let highlights = analysis::highlights::highlights(&analysis);
    let streaks = capture_streaks(
        &highlights,
        &CaptureStreak {
            start_tick: 0,
            end_tick: 0,
            source_demo: path.to_string_lossy().to_string(),
            target_player: None,
            kill_count: 0,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            viewdemo_times: Vec::new(),
            start_index: 0,
            end_index: 0,
            total_demo_frames: final_demo_frames,
            demo_fps: tickrate,
            frame_times: frame_times_arc.clone(),
            status: HighlightStatus::None,
            match_start_tick: analysis.state.match_start_tick,
            source_key: Some(source_key),
            spectated: highlights.hltv,
            steam_id: None,
            recorded_this_demo: false,
        },
    );

    // The recorder's slot, when the rows are theirs alone (D18,
    // `analysis::highlights::Rule`). None when every player's rows are
    // listed: an HLTV demo, whose `svc_serverinfo` names the proxy's slot 0
    // (#247), or a POV demo whose recorder can't be told. The rows above are
    // already the ones to list, so the frontend reads this only to know
    // whether to show whose row is whose, never to filter (#573).
    let local_player_index = match highlights.rule {
        analysis::highlights::Rule::Recorder => {
            analysis.state.pov_player_index.map(|idx| idx as usize)
        }
        analysis::highlights::Rule::Everyone => None,
    };
    let demo_type_is_pov = analysis.demo_info.demo_type == "POV";
    let playback_frames = analysis.demo_info.playback_frames;
    let match_start_tick = analysis.state.match_start_tick;

    Ok((
        (
            tickrate,
            streaks,
            demo_type_is_pov,
            local_player_index,
            playback_frames,
            match_start_tick,
            frame_times_arc,
        ),
        analysis,
    ))
}

// ── Source check ─────────────────────────────────────────────────────────────

/// Refuses a batch whose source demos are no longer the files their streaks
/// were scanned from (#196). Every tick a streak carries counts frames of the
/// scanned file; patched into a different demo saved under the same name,
/// they land on nonsense, and the game crashes minutes later instead of the
/// batch failing now. Reads 64 KiB per distinct demo. Streaks with no
/// `source_key` (saved before it existed) are not checked.
pub fn check_sources_unchanged(streaks: &[CaptureStreak]) -> Result<(), String> {
    let mut checked = std::collections::HashSet::new();
    for streak in streaks {
        let Some(expected) = streak.source_key.as_deref() else {
            continue;
        };
        if !checked.insert((streak.source_demo.as_str(), expected)) {
            continue;
        }
        let path = std::path::Path::new(&streak.source_demo);
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        let Some(key) = crate::utils::demo_hasher::demo_key_text(path) else {
            return Err(crate::messages::source_demo_unreadable(name));
        };
        if key != expected {
            return Err(crate::messages::source_demo_changed(name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod source_check_tests {
    use super::*;
    use crate::utils::demo_hasher::{demo_key_of_head, key_text};

    pub(super) fn streak_for(path: &std::path::Path, key: Option<String>) -> CaptureStreak {
        CaptureStreak {
            start_tick: 0,
            end_tick: 0,
            source_demo: path.to_string_lossy().to_string(),
            target_player: None,
            kill_count: 0,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: Vec::new(),
            start_index: 0,
            end_index: 0,
            total_demo_frames: 0,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: Default::default(),
            status: HighlightStatus::None,
            match_start_tick: None,
            source_key: key,
            spectated: false,
            steam_id: None,
            recorded_this_demo: false,
        }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("source_check_{}_{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_scanned_file_passes() {
        let dir = temp_dir("same");
        let demo = dir.join("match.dem");
        let bytes = b"HLDEMO\0\0the scanned demo".to_vec();
        std::fs::write(&demo, &bytes).unwrap();
        let key = key_text(demo_key_of_head(&bytes, bytes.len() as u64));
        let streaks = [
            streak_for(&demo, Some(key.clone())),
            streak_for(&demo, Some(key)),
        ];
        assert_eq!(check_sources_unchanged(&streaks), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_different_file_under_the_same_name_is_refused() {
        let dir = temp_dir("swapped");
        let demo = dir.join("match.dem");
        let scanned = b"HLDEMO\0\0the scanned demo".to_vec();
        let key = key_text(demo_key_of_head(&scanned, scanned.len() as u64));
        // Same length, different content: the size alone would not catch it.
        std::fs::write(&demo, b"HLDEMO\0\0another demo!!!!").unwrap();
        assert_eq!(
            scanned.len(),
            std::fs::metadata(&demo).unwrap().len() as usize
        );
        let err = check_sources_unchanged(&[streak_for(&demo, Some(key))]).unwrap_err();
        assert!(
            err.contains("match.dem") && err.contains("Scan it again"),
            "{}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_refused() {
        let dir = temp_dir("missing");
        let demo = dir.join("gone.dem");
        let err = check_sources_unchanged(&[streak_for(&demo, Some("1-00".into()))]).unwrap_err();
        assert!(
            err.contains("gone.dem") && err.contains("could not be read"),
            "{}",
            err
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streaks_saved_before_the_key_existed_are_not_checked() {
        let dir = temp_dir("old");
        let demo = dir.join("never_written.dem");
        assert_eq!(check_sources_unchanged(&[streak_for(&demo, None)]), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_hltv_demo_is_known_by_the_proxys_connect_message() {
        let mut head = vec![0u8; 1100];
        head[..6].copy_from_slice(b"HLDEMO");
        assert!(!is_hltv_head(&head));
        head[1030..1051].copy_from_slice(b"Spawn count 19 (HLTV)");
        assert!(is_hltv_head(&head));
        // What the old check looked for marks nothing on its own.
        assert!(!is_hltv_head(b"HLTV Proxy"));
    }

    /// An HLTV analysis with one player, `connection` at the demo's end, who
    /// scored two kills from slot 3.
    fn analysis_of_one_player(connection: serde_json::Value) -> analysis::Analysis {
        let at = |secs: u64, frame: usize| {
            serde_json::json!({
                "real_offset": { "secs": secs, "nanos": 0 },
                "viewdemo_offset": { "secs": secs, "nanos": 0 },
                "frame_index": frame,
            })
        };
        let mut json = serde_json::to_value(analysis::Analysis::default()).unwrap();
        json["demo_info"]["demo_type"] = "HLTV".into();
        json["state"]["players"] = serde_json::json!([{
            "id": "STEAM_0:0:1",
            "connection": connection,
            "name": "leaver",
            "team": "Axis",
            "class": null,
            "stats": [0, 2, 0],
            "session_stats": [0, 2, 0],
            "accumulated_stats": [0, 0, 0],
            "needs_reconnect_sync": true,
            "stats_seeded": true,
            "has_pre_demo_activity": false,
            "has_reconnected": false,
            "kill_streaks": [
                { "kills": [[at(10, 1000), "K98", "STEAM_0:0:2"], [at(12, 1200), "K98", "STEAM_0:0:3"]], "slot": 3 },
                { "kills": [] },
            ],
            "weapon_breakdown": {},
            "mortality": [],
        }]);
        serde_json::from_value(json).expect("the synthetic analysis deserialises")
    }

    fn rows(analysis: &analysis::Analysis) -> Vec<(Option<String>, usize, usize)> {
        let highlights = analysis::highlights::highlights(analysis);
        let demo = source_check_tests::streak_for(std::path::Path::new("x.dem"), None);
        capture_streaks(&highlights, &demo)
            .into_iter()
            .map(|s| (s.target_player, s.player_index, s.kill_count))
            .collect()
    }

    #[test]
    fn a_player_who_left_keeps_the_slot_their_kills_came_from() {
        let left = analysis_of_one_player("Disconnected".into());
        assert_eq!(rows(&left), [(Some("leaver".to_string()), 3, 2)]);
        // Still in the game, now in slot 8: the kills were still slot 3's.
        let moved = analysis_of_one_player(serde_json::json!({ "Connected": { "client_id": 8 } }));
        assert_eq!(rows(&moved), [(Some("leaver".to_string()), 3, 2)]);
    }

    /// Each DeathMsg in `bytes`: (its frame's time, the killer's slot).
    fn death_msgs(bytes: &[u8]) -> Vec<(f32, u8)> {
        use dem::types::{FrameData, MessageData, NetMessage};
        let demo = dem::open_demo_from_bytes(bytes).expect("the demo parses");
        let mut found = Vec::new();
        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(network) = &frame.frame_data else {
                    continue;
                };
                let MessageData::Parsed(messages) = &network.1.messages else {
                    continue;
                };
                for message in messages {
                    if let NetMessage::UserMessage(user) = message
                        && let Ok(dod::UserMessage::DeathMsg(death)) =
                            dod::UserMessage::new(&user.name, &user.data)
                        && let Some(slot) = death.killer_client_index.checked_sub(1)
                    {
                        found.push((frame.time, slot));
                    }
                }
            }
        }
        found
    }

    /// An HLTV demo most of whose players left before it ended keeps every
    /// player's highlights, each at the slot its kills came from. Run with
    /// `DOD_HLTV_DISCONNECT_DEMO=<path> cargo test -p native --lib
    /// players_who_left -- --ignored --nocapture`; without it, the KTP demo
    /// it was found with is looked for in the Downloads folder.
    #[test]
    #[ignore]
    fn the_highlights_of_players_who_left_are_kept() {
        let path = std::env::var_os("DOD_HLTV_DISCONNECT_DEMO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default())
                    .join("Downloads")
                    .join("ktp_1791749434-NY1_h2-2610111632-dod_thunder2.dem")
            });
        assert!(
            path.is_file(),
            "no demo at {}: set DOD_HLTV_DISCONNECT_DEMO",
            path.display()
        );
        let ((_, streaks, ..), analysis) =
            scan_demo_for_highlights_with_analysis(&path).expect("the scan succeeds");

        let mut expected = 0;
        let mut before = 0;
        for player in &analysis.state.players {
            let lives = player
                .kill_streaks
                .iter()
                .filter(|s| !s.kills.is_empty())
                .count();
            expected += lives;
            if matches!(player.connection, analysis::Connection::Connected { .. }) {
                before += lives;
            }
            let listed = streaks
                .iter()
                .filter(|s| s.target_player.as_deref() == Some(player.name.as_str()))
                .count();
            println!(
                "{:<24} {lives:>3} highlights, {listed:>3} listed",
                player.name
            );
            assert_eq!(listed, lives, "{}", player.name);
        }
        println!(
            "{} listed; {before} with the end-of-demo slot",
            streaks.len()
        );
        assert_eq!(streaks.len(), expected);
        assert!(expected > before, "every player was still in the game");

        let deaths = death_msgs(&std::fs::read(&path).unwrap());
        for streak in &streaks {
            for (_, secs, weapon) in &streak.kills {
                assert!(
                    deaths.iter().any(|(t, slot)| (t - secs).abs() < 0.001
                        && *slot as usize == streak.player_index),
                    "{:?}'s {weapon} kill at {secs:.2} s: no DeathMsg from slot {}",
                    streak.target_player,
                    streak.player_index
                );
            }
        }
    }

    /// How many rows the Capture page showed before #573: every highlight
    /// with a known slot was scanned, and the frontend kept the recorder's
    /// slot's (`local_player_index`) when the demo had one.
    fn rows_before_573(analysis: &analysis::Analysis) -> usize {
        let highlights = analysis::highlights::highlights(analysis);
        let recorder_slot = match highlights.rule {
            analysis::highlights::Rule::Recorder => analysis.state.pov_player_index,
            analysis::highlights::Rule::Everyone => None,
        };
        highlights
            .all
            .iter()
            .filter(|h| h.slot.is_some())
            .filter(|h| recorder_slot.is_none() || h.slot == recorder_slot)
            .count()
    }

    #[test]
    fn a_pov_demo_lists_the_recorders_highlights_and_an_hltv_demo_everyones() {
        let mut json = serde_json::to_value(analysis_of_one_player(
            serde_json::json!({ "Connected": { "client_id": 3 } }),
        ))
        .unwrap();
        let mut other = json["state"]["players"][0].clone();
        other["name"] = "milo".into();
        other["id"] = "76561197961831029".into();
        other["connection"] = serde_json::json!({ "Connected": { "client_id": 5 } });
        other["kill_streaks"][0]["slot"] = 5.into();
        json["state"]["players"].as_array_mut().unwrap().push(other);
        json["state"]["pov_player_index"] = 3.into();

        json["demo_info"]["demo_type"] = "POV".into();
        let pov: analysis::Analysis = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(rows(&pov), [(Some("leaver".to_string()), 3, 2)]);
        let highlights = analysis::highlights::highlights(&pov);
        let demo = source_check_tests::streak_for(std::path::Path::new("x.dem"), None);
        let streaks = capture_streaks(&highlights, &demo);
        assert!(streaks.iter().all(|s| s.recorded_this_demo));
        assert_eq!(rows_before_573(&pov), streaks.len());

        json["demo_info"]["demo_type"] = "HLTV".into();
        let hltv: analysis::Analysis = serde_json::from_value(json).unwrap();
        let listed = rows(&hltv);
        assert_eq!(
            listed,
            [
                (Some("leaver".to_string()), 3, 2),
                (Some("milo".to_string()), 5, 2)
            ]
        );
        let highlights = analysis::highlights::highlights(&hltv);
        let streaks = capture_streaks(&highlights, &demo);
        assert!(streaks.iter().all(|s| !s.recorded_this_demo));
        assert_eq!(streaks[1].steam_id.as_deref(), Some("76561197961831029"));
    }

    /// The checked-in POV recording lists as many highlights as it did before
    /// #573, all the recorder's.
    #[test]
    fn the_ci_fixture_lists_the_same_highlights_as_before() {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        ));
        let ((_, streaks, ..), analysis) =
            scan_demo_for_highlights_with_analysis(path).expect("the fixture scans");
        assert!(!streaks.is_empty());
        assert_eq!(streaks.len(), rows_before_573(&analysis));
        assert!(streaks.iter().all(|s| s.recorded_this_demo));
    }

    /// POV demos list as many highlights as before #573. Reads every demo in
    /// `DOD_ANALYSIS_DEMOS` (a folder), else the pre-Anniversary install's
    /// `dod` folder, up to 50 POV demos: `cargo test -p native --lib
    /// pov_highlight_counts -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn pov_highlight_counts_are_unchanged() {
        let dir = std::env::var_os("DOD_ANALYSIS_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    "C:/Program Files (x86)/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/dod",
                )
            });
        let mut demos: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("no demos at {}: {e}", dir.display()))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
            .collect();
        demos.sort();
        let (mut checked, mut before_total, mut after_total) = (0, 0, 0);
        for path in demos {
            if checked == 50 {
                break;
            }
            if is_hltv_demo(&path).unwrap_or(true) {
                continue;
            }
            let Ok(((_, streaks, ..), analysis)) = scan_demo_for_highlights_with_analysis(&path)
            else {
                continue;
            };
            if analysis.demo_info.demo_type == "HLTV" {
                continue;
            }
            let before = rows_before_573(&analysis);
            println!(
                "{:>4} {:>4}  {}",
                before,
                streaks.len(),
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            assert_eq!(streaks.len(), before, "{}", path.display());
            checked += 1;
            before_total += before;
            after_total += streaks.len();
        }
        println!("{checked} POV demos: {before_total} highlights before, {after_total} after");
        assert!(checked > 0, "no POV demo read in {}", dir.display());
    }
}
