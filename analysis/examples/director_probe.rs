//! What the HLTV director actually records into a demo, and whether the camera's
//! target switching is in the stream (#206).
//!
//! While spectating an HLTV demo the camera wanders off the player you are
//! watching, most obviously when they die. `spec_autodirector` is DoD's own
//! switch for it, but `follow`/`specmode` reach the server through
//! `pfnServerCmd` and there is no server during playback — so the client's own
//! spectator UI may have no working path to hold a target at all.
//!
//! The lead worth settling first is whether the switching is *recorded*. If the
//! director's decisions arrive as `svc_director` (51) messages in the demo, then
//! this is a demo-patching problem — squarely what `native/src/patch/` already
//! does, and it already carries a parked `write_ineye_hijack_payload` for the
//! same idea — rather than something needing a runtime DLL hook.
//!
//! Command ids are the engine's, from `hltv.h`. `native/src/patch/engine.rs`
//! currently calls 5 `DRC_CMD_INEYE`, which disagrees with that header (5 is
//! `TIMESCALE` there, and `INEYE` is 12); this probe reports raw ids so the
//! demos themselves settle which numbering is real.
//!
//!     cargo run --release -p analysis --example director_probe -- <demo>...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

/// `hltv.h`'s DRC_CMD_* enum.
fn drc_name(c: u8) -> &'static str {
    match c {
        0 => "NONE",
        1 => "START",
        2 => "EVENT",
        3 => "MODE",
        4 => "CAMERA",
        5 => "TIMESCALE",
        6 => "MESSAGE",
        7 => "SOUND",
        8 => "STATUS",
        9 => "BANNER",
        10 => "STUFFTEXT",
        11 => "CHASE",
        12 => "INEYE",
        13 => "MAP",
        14 => "CAMPATH",
        15 => "WAYPOINTS",
        _ => "(unknown)",
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: director_probe <demo>...");
        std::process::exit(2);
    }

    for path in &args {
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("{path}: unreadable");
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            eprintln!("{path}: unparseable");
            continue;
        };

        let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
        let mut samples: Vec<(f32, u8, Vec<u8>)> = Vec::new();
        // For the commands that plausibly carry a target, keep the whole series
        // so switch cadence is visible rather than just a total.
        let mut target_series: Vec<(f32, u8, Vec<u8>)> = Vec::new();

        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(bt) = &frame.frame_data else { continue };
                let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
                for m in msgs {
                    let NetMessage::EngineMessage(em) = m else { continue };
                    if let EngineMessage::SvcDirector(d) = &**em {
                        *counts.entry(d.command).or_insert(0) += 1;
                        if samples.len() < 25 {
                            samples.push((frame.time, d.command, d.message.to_vec()));
                        }
                        if matches!(d.command, 2 | 3 | 11 | 12) {
                            target_series.push((frame.time, d.command, d.message.to_vec()));
                        }
                    }
                }
            }
        }

        let total: usize = counts.values().sum();
        println!("\n{path}");
        if total == 0 {
            println!("  no svc_director messages at all");
            continue;
        }
        println!("  {total} svc_director messages");
        for (cmd, n) in &counts {
            println!("     {:>3} {:<10} x{}", cmd, drc_name(*cmd), n);
        }

        println!("  first messages seen:");
        for (t, c, m) in samples.iter().take(12) {
            println!("     t={:<9.2} cmd={:>3} {:<10} payload[{}]: {}", t, c, drc_name(*c), m.len(), hex(m));
        }

        if !target_series.is_empty() {
            println!("  target-ish commands: {} total", target_series.len());
            // Gaps between consecutive switches say how often the camera moves.
            let mut gaps: Vec<f32> =
                target_series.windows(2).map(|w| w[1].0 - w[0].0).filter(|g| *g > 0.0).collect();
            gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if !gaps.is_empty() {
                let med = gaps[gaps.len() / 2];
                println!(
                    "     interval between them: median {:.1}s, min {:.1}s, max {:.1}s",
                    med,
                    gaps.first().unwrap(),
                    gaps.last().unwrap()
                );
            }
            println!("     first 15:");
            for (t, c, m) in target_series.iter().take(15) {
                println!("       t={:<9.2} {:<8} payload: {}", t, drc_name(*c), hex(m));
            }
        }
    }
}
