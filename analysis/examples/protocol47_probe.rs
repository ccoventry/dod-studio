//! What differs between protocol-47 and protocol-48 demos on the wire? One
//! line per demo in a folder: header protocol, the protocol each
//! `svc_serverinfo` and `svc_version` carries, the server build its sign-on
//! print names, the VAC byte, which engine messages occur, a signature of
//! the delta descriptions, whether every frame re-encodes byte for byte, and
//! what the analyzer makes of it (#239, `docs/goldsrc_protocol_47.md`).
//!
//!     cargo run -p analysis --release --example protocol47_probe -- <folder>
//!
//! Columns are tab-separated; compare the two groups with a spreadsheet or
//! `sort | uniq -c` on a column.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "dev-only probe run by hand: a panic on bad input is its error report (#1179)"
)]

use analysis::Analysis;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

#[derive(Default)]
struct Wire {
    server_info_protocols: BTreeSet<i32>,
    server_infos: usize,
    max_players: BTreeSet<u8>,
    vac: BTreeSet<u8>,
    legacy_trailers: usize,
    versions: BTreeSet<u32>,
    build: String,
    engine_ids: BTreeSet<u8>,
    deltas: BTreeMap<String, u16>,
}

fn engine_id(m: &EngineMessage) -> u8 {
    // SAFETY: `EngineMessage` is `#[repr(u8)]` with explicit discriminants
    // equal to the wire ids (`types.rs`), and the Rust reference guarantees a
    // primitive-repr enum starts with its discriminant.
    unsafe { *(m as *const EngineMessage as *const u8) }
}

fn survey(demo: &dem::types::Demo) -> Wire {
    let mut wire = Wire::default();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &boxed.1.messages else {
                continue;
            };
            for message in messages {
                let NetMessage::EngineMessage(m) = message else {
                    continue;
                };
                wire.engine_ids.insert(engine_id(m));
                match m.as_ref() {
                    EngineMessage::SvcServerInfo(s) => {
                        wire.server_infos += 1;
                        wire.server_info_protocols.insert(s.protocol);
                        wire.max_players.insert(s.max_players);
                        wire.vac.insert(s.unknown);
                        wire.legacy_trailers += usize::from(s.legacy_trailer.is_some());
                    }
                    EngineMessage::SvcVersion(v) => {
                        wire.versions.insert(v.protocol_version);
                    }
                    EngineMessage::SvcPrint(p) if wire.build.is_empty() => {
                        let text = String::from_utf8_lossy(&p.message.0);
                        if let Some(at) = text.find("BUILD") {
                            wire.build = text[at..]
                                .split_whitespace()
                                .take(2)
                                .collect::<Vec<_>>()
                                .join(" ");
                        }
                    }
                    EngineMessage::SvcDeltaDescription(d) => {
                        let name = String::from_utf8_lossy(&d.name)
                            .trim_end_matches('\0')
                            .to_string();
                        wire.deltas.insert(name, d.total_fields);
                    }
                    _ => {}
                }
            }
        }
    }
    wire
}

fn main() {
    let folder = PathBuf::from(std::env::args().nth(1).expect("usage: <folder>"));
    let mut demos: Vec<_> = fs::read_dir(&folder)
        .expect("read folder")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
        .collect();
    demos.sort();
    println!(
        "demo\theader\tserverinfo\tcount\tmaxplayers\tvac\ttrailers\tsvc_version\tbuild\tengine_ids\tdeltas\treencode\tanalysis"
    );
    for path in demos {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = fs::read(&path).expect("read demo");
        let protocol = bytes
            .get(12..16)
            .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(-1);
        let (wire, reencode) = match dem::open_demo_from_bytes(&bytes) {
            Ok(demo) => {
                let wire = survey(&demo);
                let directory = bytes
                    .get(540..544)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
                    .unwrap_or(0);
                let written = demo.write_to_bytes().expect("re-encode");
                let first = written.iter().zip(&bytes).position(|(a, b)| a != b);
                let same = first.is_none_or(|at| at >= directory);
                (
                    Some(wire),
                    if same { "same" } else { "DIFFERS" }.to_string(),
                )
            }
            Err(e) => (
                None,
                format!("parse ERR {}", e.to_string().lines().next().unwrap_or("")),
            ),
        };
        let analysis = match Analysis::try_from_bytes(&bytes) {
            Ok(a) => {
                let kills: usize = a.state.players.iter().map(|p| p.stats.1 as usize).sum();
                format!(
                    "{} {} players={} kills={} rounds={}",
                    a.demo_info.demo_type,
                    a.demo_info.map_name,
                    a.state.players.len(),
                    kills,
                    a.state.rounds.len()
                )
            }
            Err(e) => format!("ERR {}", e.lines().next().unwrap_or("")),
        };
        let w = wire.unwrap_or_default();
        let deltas: Vec<String> = w.deltas.iter().map(|(n, f)| format!("{n}:{f}")).collect();
        println!(
            "{name}\t{protocol}\t{:?}\t{}\t{:?}\t{:?}\t{}\t{:?}\t{}\t{:?}\t{}\t{reencode}\t{analysis}",
            w.server_info_protocols,
            w.server_infos,
            w.max_players,
            w.vac,
            w.legacy_trailers,
            w.versions,
            w.build,
            w.engine_ids,
            deltas.join(","),
        );
    }
}
