//! How close does the hook's schedule run a command to where an injected
//! frame would? (#434, step 3: "measure before building on it")
//!
//! Writes a copy of a demo with `echo [dod-studio] INJ <n>` injected as a
//! `ConsoleCommand` frame at several points, the way a capture batch injects
//! Scheduled Commands, and a schedule file for `dodstudio_schedule` with
//! `echo [dod-studio] HOOK <n>` at the viewdemo clock each injected frame
//! runs at. Play the copy with `viewdemo` after `dodstudio_schedule load
//! "<schedule>"`: the hook log then has a `marker at <clock>` line for every
//! INJ and HOOK, on the same clock, and the difference per pair is the gap
//! between the two routes.
//!
//! Usage:
//!
//!     cargo run --release -p native --example schedule_timing_probe -- <in.dem> <out.dem> <schedule.txt> [seconds...]
//!
//! `seconds` (default 5 30 60 90 120) are offsets past the first `svc_time`
//! after the demo's `DemoStart`.

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use native::patch::StreamPatcher;
use native::patch::types::{PatchJob, PatcherConfig};
use std::sync::{Arc, atomic::AtomicBool};

fn last_svc_time(frame_data: &FrameData) -> Option<f32> {
    let FrameData::NetworkMessage(boxed) = frame_data else {
        return None;
    };
    let MessageData::Parsed(messages) = &boxed.1.messages else {
        return None;
    };
    messages.iter().rev().find_map(|m| match m {
        NetMessage::EngineMessage(eng) => match eng.as_ref() {
            EngineMessage::SvcTime(t) => Some(t.time),
            _ => None,
        },
        _ => None,
    })
}

/// For each offset: the 1-based frame ordinal to inject ahead of, and the
/// viewdemo clock the injected frame runs at (the last `svc_time` before it).
fn find_targets(demo: &dem::types::Demo, offsets: &[f32]) -> Vec<(i32, f32)> {
    let mut out = Vec::new();
    let mut ordinal = 0i32;
    let mut started = false;
    let mut first_svc: Option<f32> = None;
    let mut svc_time = 0.0f32;
    let mut want = offsets.iter().copied().peekable();
    for frame in demo.directory.entries.iter().flat_map(|e| e.frames.iter()) {
        ordinal += 1;
        if matches!(frame.frame_data, FrameData::DemoStart) {
            started = true;
        }
        let here = last_svc_time(&frame.frame_data);
        if started && let (Some(first), Some(t)) = (first_svc, here) {
            while let Some(&offset) = want.peek() {
                if t >= first + offset {
                    out.push((ordinal, svc_time));
                    want.next();
                } else {
                    break;
                }
            }
        }
        if let Some(t) = here {
            if started && first_svc.is_none() {
                first_svc = Some(t);
            }
            svc_time = t;
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: schedule_timing_probe <in.dem> <out.dem> <schedule.txt> [seconds...]");
        std::process::exit(2);
    }
    let input = std::path::PathBuf::from(&args[1]);
    let output = std::path::PathBuf::from(&args[2]);
    let schedule = std::path::PathBuf::from(&args[3]);
    let mut offsets: Vec<f32> = args[4..]
        .iter()
        .map(|s| s.parse().expect("seconds must be numbers"))
        .collect();
    if offsets.is_empty() {
        offsets = vec![5.0, 30.0, 60.0, 90.0, 120.0];
    }
    offsets.sort_by(f32::total_cmp);

    let bytes =
        std::fs::read(&input).unwrap_or_else(|e| panic!("could not read {}: {e}", input.display()));
    let demo = open_demo_from_bytes(&bytes)
        .unwrap_or_else(|e| panic!("could not parse {}: {e}", input.display()));
    let targets = find_targets(&demo, &offsets);
    assert_eq!(
        targets.len(),
        offsets.len(),
        "the demo is shorter than the last offset"
    );
    drop(demo);

    let job = PatchJob {
        source_demo: input.to_string_lossy().to_string(),
        output_demo: output.clone(),
        streaks: Vec::new(),
        target_player: None,
        init_commands: Vec::new(),
        scheduled_commands: targets
            .iter()
            .enumerate()
            .map(|(i, &(ordinal, _))| (ordinal, format!("echo [dod-studio] INJ {}", i + 1)))
            .collect(),
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };
    let config = PatcherConfig {
        decal_flush: false,
        ..Default::default()
    };
    StreamPatcher::new(&input, &output)
        .patch(&job, &config, &Arc::new(AtomicBool::new(false)))
        .unwrap_or_else(|e| panic!("patching {} failed: {e}", input.display()));

    let mut text = String::from("dodstudio-schedule 1\n");
    for (i, &(_, clock)) in targets.iter().enumerate() {
        text.push_str(&format!("{clock}\techo [dod-studio] HOOK {}\n", i + 1));
    }
    std::fs::write(&schedule, text)
        .unwrap_or_else(|e| panic!("could not write {}: {e}", schedule.display()));

    println!("Wrote {} and {}", output.display(), schedule.display());
    for (i, (ordinal, clock)) in targets.iter().enumerate() {
        println!(
            "  INJ/HOOK {}: frame {ordinal}, viewdemo clock {clock:.3}",
            i + 1
        );
    }
}
