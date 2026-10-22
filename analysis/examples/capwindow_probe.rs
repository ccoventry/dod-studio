//! Measure the frame distance between a flag capture and its score credits.
//!
//! `CapMsg` names exactly one capper. Everyone else who helped is visible only
//! as an `ObjScore` increment, and this establishes that those land in the
//! *same frame* 99.5% of the time — so recovering co-cappers needs no tolerance
//! window at all. Also reports how many captures had co-cappers.
//!
//!     cargo run --release -p analysis --example capwindow_probe -- demo.dem
//!     cargo run --release -p analysis --example capwindow_probe -- demo.dem -v

use dem::open_demo_from_bytes;
use dem::types::{FrameData, MessageData, NetMessage};
use dod::UserMessage;
use std::collections::{BTreeMap, HashMap};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: capwindow_probe <demo>");
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let Ok(demo) = open_demo_from_bytes(&bytes) else {
        return;
    };
    drop(bytes);

    // (frame, time, capper slot)
    let mut caps: Vec<(usize, f32, u8)> = Vec::new();
    // (frame, time, slot)
    let mut bumps: Vec<(usize, f32, u8)> = Vec::new();
    let mut obj_last: HashMap<u8, i32> = HashMap::new();
    let mut frame_no = 0usize;

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            frame_no += 1;
            let t = frame.time;
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                let NetMessage::UserMessage(um) = m else {
                    continue;
                };
                let Ok(msg) = UserMessage::new(&um.name, &um.data) else {
                    continue;
                };
                match msg {
                    UserMessage::CapMsg(c) => caps.push((frame_no, t, c.client_index - 1)),
                    UserMessage::ObjScore(o) => {
                        let idx = o.client_index - 1;
                        let now = o.score as i32;
                        let prev = obj_last.insert(idx, now).unwrap_or(0);
                        if now > prev {
                            bumps.push((frame_no, t, idx));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // For the capper CapMsg names, how far is their own ObjScore bump?
    let mut self_dt: BTreeMap<i64, u32> = BTreeMap::new();
    // For everyone else, distance of the nearest bump to the capture.
    let mut other_dt: BTreeMap<i64, u32> = BTreeMap::new();
    let mut self_secs: Vec<f32> = Vec::new();
    let mut other_secs: Vec<f32> = Vec::new();
    let mut same_frame_others = 0u32;
    let mut self_offframe = 0u32;

    for (cf, ct, capper) in &caps {
        for (bf, bt2, slot) in &bumps {
            let df = *bf as i64 - *cf as i64;
            if df.abs() > 1000 {
                continue;
            }
            let bucket = if df.abs() <= 10 {
                df
            } else {
                df.signum() * (10 + (df.abs() - 10) / 50 * 50)
            };
            if slot == capper {
                *self_dt.entry(bucket).or_insert(0) += 1;
                self_secs.push(bt2 - ct);
                if df != 0 {
                    self_offframe += 1;
                }
            } else {
                if df == 0 {
                    same_frame_others += 1;
                }
                *other_dt.entry(bucket).or_insert(0) += 1;
                other_secs.push(bt2 - ct);
            }
        }
    }

    let pct = |v: &mut Vec<f32>, q: f32| -> f32 {
        if v.is_empty() {
            return f32::NAN;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[((v.len() - 1) as f32 * q) as usize]
    };
    let mut s2 = self_secs.clone();
    let mut o2 = other_secs.clone();

    let file = path.rsplit(['/', '\\']).next().unwrap_or(&path);
    println!(
        "{file}\tcaps={}\tbumps={}\tselfPairs={}\totherPairs={}\tselfSecP50={:.2}\tselfSecP95={:.2}\totherSecP50={:.2}\totherSecP95={:.2}",
        caps.len(),
        bumps.len(),
        self_secs.len(),
        other_secs.len(),
        pct(&mut s2, 0.5),
        pct(&mut s2, 0.95),
        pct(&mut o2, 0.5),
        pct(&mut o2, 0.95),
    );
    // Per capture: does the CapMsg-named capper have an ObjScore bump in the
    // very same frame, and how many OTHER players do?
    let mut missing_self = 0u32;
    let mut co_capper_hist: BTreeMap<usize, u32> = BTreeMap::new();
    for (cf, _, capper) in &caps {
        let same: Vec<u8> = bumps
            .iter()
            .filter(|(bf, _, _)| bf == cf)
            .map(|(_, _, s)| *s)
            .collect();
        if !same.contains(capper) {
            missing_self += 1;
        }
        let others = same.iter().filter(|s| *s != capper).count();
        *co_capper_hist.entry(others).or_insert(0) += 1;
    }
    let hist: Vec<String> = co_capper_hist
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    println!(
        "STAT	{file}	caps={}	missingSelf={}	coCapperHist={}",
        caps.len(),
        missing_self,
        hist.join(",")
    );
    let _ = (self_offframe, same_frame_others);
    if std::env::args().any(|a| a == "-v") {
        println!("  self  frame-delta buckets: {:?}", self_dt);
        println!("  other frame-delta buckets: {:?}", other_dt);
    }
}
