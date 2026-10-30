//! Who was standing in a capture area when a kill happened, told from where
//! the players were (#1132).
//!
//! The demo's own capture-area messages name only the recorder (`ClientAreas`
//! is sent to one player) and only count attackers on flags that need two or
//! more (`PlayersIn`); `analysis/examples/capture_area_probe.rs` measures
//! them. This probe tries the route that covers every player: the analyzer's
//! kill positions (#448) tested against the map's `dod_capture_area` brushes.
//! A player touches an area when any of 27 points spread over his 32 x 32 x
//! 72 hull is inside the brush, which is about how the engine's trigger touch
//! works.
//!
//! Per demo it prints how many kills had a position for the victim, how many
//! victims stood in an area their own team could capture ("killed while
//! capping"), how many of those died to the other team ("flag defence"), how
//! many killers stood in an area their team could capture, and for a POV
//! demo how the recorder's deaths in an area compare with `ClientAreas`, the
//! server's own word for the recorder.
//!
//!     cargo run -p native --release --example capture_area_kills_probe -- <install> demo.dem...

use analysis::Analysis;
use analysis::Connection;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use dod::Team;
use native::overview::files::bsp_path;
use native::overview::level::Level;
use std::path::Path;

/// `CONTENTS_EMPTY` in the BSP's leaves: anything else is inside the brush.
const CONTENTS_EMPTY: i32 = -1;
/// How far before a death the recorder's `ClientAreas` state is read: the
/// leave message (status 0) can arrive in the death's own packet.
const STATE_LEAD_SECONDS: f32 = 0.05;

struct Area {
    model: usize,
    offset: [f32; 3],
    allies_can_cap: bool,
    axis_can_cap: bool,
}

fn capture_areas(level: &Level) -> Vec<Area> {
    level
        .entities
        .iter()
        .filter(|e| e.classname() == "dod_capture_area")
        .filter_map(|e| {
            let model = e.brush_submodel()? as usize;
            let flag = |key: &str| e.get(key).map(|v| v.trim() != "0").unwrap_or(true);
            Some(Area {
                model,
                offset: level.models.get(model)?.offset,
                allies_can_cap: flag("area_allies_cancap"),
                axis_can_cap: flag("area_axis_cancap"),
            })
        })
        .collect()
}

/// The areas whose brush the player's hull at `origin` overlaps.
fn areas_touched(level: &Level, areas: &[Area], origin: [f32; 3]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, area) in areas.iter().enumerate() {
        let hit = [-16.0f32, 0.0, 16.0].iter().any(|dx| {
            [-16.0f32, 0.0, 16.0].iter().any(|dy| {
                [-36.0f32, 0.0, 36.0].iter().any(|dz| {
                    let p = [
                        origin[0] + dx - area.offset[0],
                        origin[1] + dy - area.offset[1],
                        origin[2] + dz - area.offset[2],
                    ];
                    level.model_point_contents(area.model, p) != CONTENTS_EMPTY
                })
            })
        });
        if hit {
            out.push(i);
        }
    }
    out
}

fn can_cap(area: &Area, team: &Option<Team>) -> bool {
    match team {
        Some(Team::Allies) | Some(Team::British) => area.allies_can_cap,
        Some(Team::Axis) => area.axis_can_cap,
        _ => false,
    }
}

/// The recorder's `ClientAreas` changes as (svc time, in an area), in order.
fn recorder_area_states(bytes: &[u8]) -> Vec<(f32, bool)> {
    let Ok(demo) = open_demo_from_bytes(bytes) else {
        return Vec::new();
    };
    let mut time = 0.0f32;
    let mut inside = std::collections::BTreeSet::new();
    let mut out = Vec::new();
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
                    NetMessage::EngineMessage(em) => {
                        if let EngineMessage::SvcTime(t) = &**em {
                            time = t.time;
                        }
                    }
                    NetMessage::UserMessage(um) => {
                        let name = String::from_utf8_lossy(&um.name);
                        if name.trim_end_matches('\0') != "ClientAreas" || um.data.len() < 2 {
                            continue;
                        }
                        match um.data[1] {
                            1 => {
                                inside.insert(um.data[0]);
                            }
                            0 => {
                                inside.remove(&um.data[0]);
                            }
                            2 => inside.clear(),
                            _ => continue,
                        }
                        out.push((time, !inside.is_empty()));
                    }
                }
            }
        }
    }
    out
}

fn in_area_at(states: &[(f32, bool)], t: f32) -> bool {
    states
        .iter()
        .take_while(|(when, _)| *when <= t - STATE_LEAD_SECONDS)
        .last()
        .map(|(_, inside)| *inside)
        .unwrap_or(false)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((install, demos)) = args.split_first() else {
        eprintln!("usage: capture_area_kills_probe <install> demo.dem...");
        std::process::exit(2);
    };
    for path in demos {
        let name = Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(bytes) = std::fs::read(path) else {
            println!("{name}\tunreadable");
            continue;
        };
        let Ok(analysis) = Analysis::try_from_bytes(&bytes) else {
            println!("{name}\tdoes not parse");
            continue;
        };
        let map = analysis
            .demo_info
            .map_name
            .trim_start_matches("maps/")
            .trim_end_matches(".bsp")
            .to_string();
        let level = match Level::from_file(&bsp_path(Path::new(install), &map)) {
            Ok(level) => level,
            Err(e) => {
                println!("{name}\t{map}: {e}");
                continue;
            }
        };
        let areas = capture_areas(&level);
        let hltv = analysis.demo_info.demo_type == "HLTV";
        let recorder = analysis.state.players.iter().find(|p| {
            !hltv && matches!(p.connection, Connection::Connected { client_id } if Some(client_id) == analysis.state.pov_player_index)
        });
        let states = if hltv {
            Vec::new()
        } else {
            recorder_area_states(&bytes)
        };
        let (
            mut kills,
            mut victim_known,
            mut capping,
            mut defence,
            mut killer_known,
            mut killer_capping,
        ) = (0, 0, 0, 0, 0, 0);
        let (mut rec_deaths, mut rec_geo, mut rec_msg, mut rec_both) = (0, 0, 0, 0);
        for k in &analysis.state.kill_positions {
            if k.killer.is_none() || k.teamkill {
                continue;
            }
            kills += 1;
            let victim_cap = k.victim_origin.map(|o| {
                areas_touched(&level, &areas, o)
                    .iter()
                    .any(|&i| can_cap(&areas[i], &k.victim_team))
            });
            if let Some(cap) = victim_cap {
                victim_known += 1;
                if cap {
                    capping += 1;
                    if k.killer_team != k.victim_team {
                        defence += 1;
                    }
                }
            }
            if let Some(o) = k.killer_origin {
                killer_known += 1;
                if areas_touched(&level, &areas, o)
                    .iter()
                    .any(|&i| can_cap(&areas[i], &k.killer_team))
                {
                    killer_capping += 1;
                }
            }
            if let (Some(rec), Some(victim)) = (recorder, &k.victim)
                && &rec.id == victim
            {
                rec_deaths += 1;
                let by_message = in_area_at(&states, k.time.viewdemo_offset.as_secs_f32());
                let by_geometry = victim_cap.unwrap_or(false);
                rec_msg += by_message as u32;
                rec_geo += by_geometry as u32;
                rec_both += (by_message && by_geometry) as u32;
            }
        }
        println!(
            "{name}\t{}\t{map}\tareas {}\tkills {kills}\tvictim pos {victim_known}\tkilled while capping {capping} (by other team {defence})\tkiller pos {killer_known}\tkiller capping {killer_capping}\trecorder deaths {rec_deaths}: in area by ClientAreas {rec_msg}, by position {rec_geo}, both {rec_both}",
            if hltv { "HLTV" } else { "POV" },
            areas.len(),
        );
    }
}
