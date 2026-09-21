//! Where can a GoldSrc demo be cut without breaking delta continuity? (#58)
//!
//! Naive truncation parses fine and plays back wrong: entity updates are
//! delta-compressed against earlier state, and the decoders and baselines that
//! make them readable are established once, early. This surveys a demo's
//! structure so the minimal viable trim can be chosen from evidence:
//!
//!   * what the directory entries hold, and where the init messages live
//!   * how often a *full* SvcPacketEntities appears -- a full snapshot needs no
//!     predecessor, so it is a candidate cut point -- versus a delta one
//!
//!     cargo run --release -p analysis --example trim_survey_probe -- <demo>...
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(demo) = open_demo_from_bytes(&bytes) else { println!("{path}: unparseable"); continue };
        println!("\n{path}  ({:.1} MB)", bytes.len() as f64 / 1e6);
        println!("  directory entries: {}", demo.directory.entries.len());

        for (ei, entry) in demo.directory.entries.iter().enumerate() {
            let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
            let mut init: Vec<(usize, f32, &str)> = Vec::new();
            let mut full_pe: Vec<(usize, f32)> = Vec::new();
            let mut delta_pe: usize = 0;
            let mut tmin = f32::MAX;
            let mut tmax = f32::MIN;

            for (fi, frame) in entry.frames.iter().enumerate() {
                tmin = tmin.min(frame.time);
                tmax = tmax.max(frame.time);
                let k = match &frame.frame_data {
                    FrameData::NetworkMessage(_) => "NetworkMessage",
                    FrameData::DemoStart => "DemoStart",
                    FrameData::ConsoleCommand(_) => "ConsoleCommand",
                    FrameData::ClientData(_) => "ClientData",
                    FrameData::NextSection => "NextSection",
                    FrameData::Event(_) => "Event",
                    FrameData::WeaponAnimation(_) => "WeaponAnimation",
                    FrameData::Sound(_) => "Sound",
                    FrameData::DemoBuffer(_) => "DemoBuffer",
                };
                *kinds.entry(k).or_insert(0) += 1;

                let FrameData::NetworkMessage(bt) = &frame.frame_data else { continue };
                let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
                for m in msgs {
                    let NetMessage::EngineMessage(em) = m else { continue };
                    let name = match &**em {
                        EngineMessage::SvcServerInfo(_) => "SvcServerInfo",
                        EngineMessage::SvcDeltaDescription(_) => "SvcDeltaDescription",
                        EngineMessage::SvcSpawnBaseline(_) => "SvcSpawnBaseline",
                        EngineMessage::SvcResourceList(_) => "SvcResourceList",
                        EngineMessage::SvcNewUserMsg(_) => "SvcNewUserMsg",
                        EngineMessage::SvcPacketEntities(_) => { full_pe.push((fi, frame.time)); continue }
                        EngineMessage::SvcDeltaPacketEntities(_) => { delta_pe += 1; continue }
                        _ => continue,
                    };
                    if init.len() < 4000 { init.push((fi, frame.time, name)); }
                }
            }

            println!("\n  --- entry {ei}: {} frames, t {:.2}..{:.2} ---", entry.frames.len(), tmin, tmax);
            let mut ks: Vec<_> = kinds.iter().collect();
            ks.sort_by(|a,b| b.1.cmp(a.1));
            println!("     frame kinds: {}", ks.iter().map(|(k,n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", "));

            let mut counts: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new(); // name -> (count, first_frame, last_frame)
            for (fi, _, n) in &init {
                let e = counts.entry(n).or_insert((0, *fi, *fi));
                e.0 += 1; e.2 = *fi;
            }
            for (n,(c,f,l)) in &counts {
                println!("     {n}: x{c}, frames {f}..{l}");
            }
            println!("     packet entities: {} full / {} delta", full_pe.len(), delta_pe);
            if full_pe.len() > 1 {
                let mut gaps: Vec<f32> = full_pe.windows(2).map(|w| w[1].1 - w[0].1).filter(|g| *g>0.0).collect();
                gaps.sort_by(|a,b| a.partial_cmp(b).unwrap());
                if !gaps.is_empty() {
                    println!("     full-snapshot spacing: median {:.2}s  min {:.2}s  max {:.2}s",
                        gaps[gaps.len()/2], gaps[0], gaps[gaps.len()-1]);
                }
                println!("     first full snapshots at frames: {:?}",
                    full_pe.iter().take(6).map(|(f,t)| format!("{f}@{t:.1}s")).collect::<Vec<_>>());
            }
        }
    }
}
