//! What a demo says about who stands in a capture area (#1132).
//!
//! Decodes the three capture-area user messages the analyzer drops, with the
//! meanings `dod.so` and `client.so` give them (`docs/goldsrc_linux_debug_symbols.md`):
//!
//! - `ClientAreas {area, status[, icon]}`, sent with `MSG_ONE` to one player:
//!   status 1 = that player entered the area (`CBasePlayer::SetClientAreaIcon`
//!   from the area's touch), 0 = left it (`HandleSignals`), 2 = clear every
//!   area (round reset), 254/255 = the area's HUD icon name follows.
//! - `PlayersIn {point, team, count, required}`, sent to everyone by
//!   `CAreaCapture::Think`: how many of the team that does not own the point
//!   stand in it, uncontested. Team 0 with count 0 when nobody (or both teams,
//!   or only the owners) is in it; nothing at all for points one player can
//!   take.
//! - `CurMarker {marker}`: the player's own map-marker choice (UI only).
//!
//! Per demo it prints the counts, the recorder's own deaths and kills while
//! `ClientAreas` had them in an area, and how many deaths a `PlayersIn` count
//! for the victim's team dropped with (within [`DROP_WINDOW_SECONDS`]), which
//! is the only per-death area evidence for players other than the recorder.
//!
//!     cargo run --release -p analysis --example capture_area_probe -- demo.dem...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// How long after a death a `PlayersIn` drop for the victim's team still
/// counts as the victim leaving the area. The server recounts in the area's
/// think, a fraction of a second after the body stops touching it.
const DROP_WINDOW_SECONDS: f32 = 1.5;

#[derive(Default)]
struct Survey {
    is_hltv: bool,
    map: String,
    recorder_slot: Option<u8>,
    areas_by_status: BTreeMap<u8, u32>,
    area_indices: BTreeSet<u8>,
    players_in: u32,
    players_in_nonzero: u32,
    players_in_points: BTreeSet<u8>,
    required_seen: BTreeSet<u8>,
    cur_marker: u32,
    deaths: u32,
    deaths_with_drop: u32,
    recorder_deaths: u32,
    recorder_deaths_in_area: u32,
    recorder_kills: u32,
    recorder_kills_in_area: u32,
    players: BTreeSet<u8>,
}

/// A death waiting for its victim's team's `PlayersIn` count to drop.
struct OpenDeath {
    time: f32,
    team: u8,
    counts: HashMap<u8, u8>,
}

fn survey(bytes: &[u8]) -> Option<Survey> {
    let demo = open_demo_from_bytes(bytes).ok()?;
    let mut s = Survey::default();
    let mut time = 0.0f32;
    let mut team_of: HashMap<u8, u8> = HashMap::new();
    // point -> (team, count) as last sent
    let mut points: HashMap<u8, (u8, u8)> = HashMap::new();
    let mut recorder_areas: BTreeSet<u8> = BTreeSet::new();
    let mut open: Vec<OpenDeath> = Vec::new();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => match &**em {
                        EngineMessage::SvcTime(t) => time = t.time,
                        EngineMessage::SvcHltv(_) => s.is_hltv = true,
                        EngineMessage::SvcServerInfo(info) => {
                            s.map = String::from_utf8_lossy(&info.map_file_name)
                                .trim_end_matches('\0')
                                .to_string();
                            s.recorder_slot = Some(info.player_index);
                        }
                        _ => {}
                    },
                    NetMessage::UserMessage(um) => {
                        let name = String::from_utf8_lossy(&um.name);
                        let d = &um.data[..];
                        match name.trim_end_matches('\0') {
                            "ClientAreas" if d.len() >= 2 => {
                                *s.areas_by_status.entry(d[1]).or_default() += 1;
                                s.area_indices.insert(d[0]);
                                match d[1] {
                                    1 => {
                                        recorder_areas.insert(d[0]);
                                    }
                                    0 => {
                                        recorder_areas.remove(&d[0]);
                                    }
                                    2 => recorder_areas.clear(),
                                    _ => {}
                                }
                            }
                            "PlayersIn" if d.len() == 4 => {
                                s.players_in += 1;
                                s.players_in_points.insert(d[0]);
                                if d[1] != 0 {
                                    s.players_in_nonzero += 1;
                                    s.required_seen.insert(d[3]);
                                }
                                let before = points.insert(d[0], (d[1], d[2]));
                                close_deaths(&mut open, &mut s, d, before, time);
                            }
                            "CurMarker" => s.cur_marker += 1,
                            "PTeam" if d.len() >= 2 => {
                                team_of.insert(d[0], d[1]);
                            }
                            "DeathMsg" if d.len() >= 3 => {
                                let (killer, victim) = (d[0], d[1]);
                                s.deaths += 1;
                                s.players.insert(victim);
                                if killer != 0 {
                                    s.players.insert(killer);
                                }
                                let recorder = s.recorder_slot.map(|slot| slot + 1);
                                if !s.is_hltv && Some(victim) == recorder {
                                    s.recorder_deaths += 1;
                                    if !recorder_areas.is_empty() {
                                        s.recorder_deaths_in_area += 1;
                                    }
                                }
                                if !s.is_hltv && Some(killer) == recorder && killer != victim {
                                    s.recorder_kills += 1;
                                    if !recorder_areas.is_empty() {
                                        s.recorder_kills_in_area += 1;
                                    }
                                }
                                if let Some(&team) = team_of.get(&victim) {
                                    let counts = points
                                        .iter()
                                        .filter(|(_, (t, _))| *t == team)
                                        .map(|(p, (_, c))| (*p, *c))
                                        .collect();
                                    open.push(OpenDeath { time, team, counts });
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            open.retain(|o| time - o.time <= DROP_WINDOW_SECONDS);
        }
    }
    Some(s)
}

/// Credit the open deaths whose victim's team just lost a player on a point.
fn close_deaths(
    open: &mut Vec<OpenDeath>,
    s: &mut Survey,
    d: &[u8],
    before: Option<(u8, u8)>,
    time: f32,
) {
    let Some((before_team, before_count)) = before else {
        return;
    };
    let dropped = before_team != 0 && (d[1] != before_team || d[2] < before_count);
    if !dropped {
        return;
    }
    if let Some(i) = open.iter().position(|o| {
        o.team == before_team
            && time - o.time <= DROP_WINDOW_SECONDS
            && o.counts.contains_key(&d[0])
    }) {
        open.remove(i);
        s.deaths_with_drop += 1;
    }
}

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else {
            println!("== {path}: unreadable");
            continue;
        };
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let Some(s) = survey(&bytes) else {
            println!("== {name}: does not parse");
            continue;
        };
        println!(
            "{name}\t{}\t{}\tClientAreas {:?} areas {:?}\tPlayersIn {} (nonzero {}) points {:?} required {:?}\tCurMarker {}\tdeaths {} with-drop {}\trecorder deaths {} in-area {} kills {} in-area {}\tplayers {}",
            if s.is_hltv { "HLTV" } else { "POV" },
            s.map,
            s.areas_by_status,
            s.area_indices,
            s.players_in,
            s.players_in_nonzero,
            s.players_in_points,
            s.required_seen,
            s.cur_marker,
            s.deaths,
            s.deaths_with_drop,
            s.recorder_deaths,
            s.recorder_deaths_in_area,
            s.recorder_kills,
            s.recorder_kills_in_area,
            s.players.len(),
        );
    }
}
