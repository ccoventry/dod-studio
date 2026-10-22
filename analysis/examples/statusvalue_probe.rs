//! Finds every `StatusValue` user message in a demo and reports the
//! `viewdemo` window's own timer at each one -- the crosshair-on-a-live-
//! teammate readout `CHudStatusBar` draws, gated on `g_iUser1 == 0` (not
//! spectating). There is no server during playback, so this only ever fires
//! at moments the *original recording* had a crosshair on a teammate; you
//! cannot manufacture it by looking around during playback. This probe finds
//! those moments instead of hunting for them by eye.
//!
//! The timestamp reported is `SVC_TIME`'s own value (see `analysis::time::
//! GameTime::viewdemo_offset`), not the frame's wall-clock recording time --
//! those two drift apart on a spliced/chained demo, and `SVC_TIME` is what's
//! actually shown in the `viewdemo` timer, i.e. what you type into
//! `demo_jump`/scrub to.
//!
//!     cargo run --release -p analysis --example statusvalue_probe -- path/to/demo.dem

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: statusvalue_probe <demo>");
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    // (viewdemo_offset seconds, raw payload bytes)
    let mut hits: Vec<(f32, Vec<u8>)> = Vec::new();
    let mut viewdemo_offset = 0.0f32;

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
                        if let EngineMessage::SvcTime(t) = em.as_ref() {
                            viewdemo_offset = t.time;
                        }
                    }
                    NetMessage::UserMessage(um) => {
                        let mut n: Vec<u8> = um.name.clone();
                        while n.last() == Some(&0) {
                            n.pop();
                        }
                        if n == b"StatusValue" {
                            hits.push((viewdemo_offset, um.data.clone()));
                        }
                    }
                }
            }
        }
    }

    println!("== {} ==", path);
    println!("{} StatusValue message(s)\n", hits.len());
    if hits.is_empty() {
        println!("Never fires in this demo -- pick a different one.");
        return;
    }

    // Group hits into runs: DoD only resends StatusValue when the health
    // value *changes*, not every frame, so a single sustained gaze can have
    // gaps of a second or two between updates. 2.5s is the widest gap still
    // clearly "the same look", picked by eye from this file's own gap sizes.
    const MAX_GAP: f32 = 2.5;
    let mut windows: Vec<(f32, f32, usize)> = Vec::new(); // (start, end, hit count)
    let mut start = hits[0].0;
    let mut prev = hits[0].0;
    let mut count = 1;
    for &(offset, _) in &hits[1..] {
        if offset - prev > MAX_GAP {
            windows.push((start, prev, count));
            start = offset;
            count = 0;
        }
        prev = offset;
        count += 1;
    }
    windows.push((start, prev, count));

    println!(
        "{} window(s) of sustained ID (gap <= {}s)\n",
        windows.len(),
        MAX_GAP
    );
    let mut by_length = windows.clone();
    by_length.sort_by(|a, b| (b.1 - b.0).partial_cmp(&(a.1 - a.0)).unwrap());
    println!("Longest windows -- best ones to test against:");
    for (start, end, count) in by_length.iter().take(10) {
        println!(
            "  {:.2}s .. {:.2}s  ({:.1}s, {} update(s))",
            start,
            end,
            end - start,
            count
        );
    }

    println!("\nAll windows, in order:");
    for (start, end, count) in &windows {
        println!("  {:.2}s .. {:.2}s  ({} update(s))", start, end, count);
    }
}
