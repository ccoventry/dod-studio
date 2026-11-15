//! Which commands does the engine's demo command filter drop? (#679)
//!
//! Writes a copy of a demo with a handful of `ConsoleCommand` frames injected
//! at one frame a few seconds into playback, through the same `StreamPatcher`
//! a capture batch uses. Play the copy with `viewdemo` and read the console
//! (or `qconsole.log`, since Studio launches with `-condebug`) at the demo
//! time this prints:
//!
//! - `echo stufftext_probe_start` / `_end` bracket the test.
//! - `cvarlist gl_max` is a control the filter lets through: it prints the
//!   matching cvars. (An unknown name is no control: during playback the
//!   engine prints nothing for it either way, measured 2026-10-10.)
//! - `bind f1` (name contains `bind`) prints F1's binding if it runs and
//!   nothing if dropped.
//! - `mirv_matte_setcolor` is the real case from the issue: with no arguments
//!   it prints HLAE's usage if it runs, nothing if dropped.
//! - `echo stufftext_probe exit_in_args` tests a line rule on an argument
//!   (`exit` anywhere on the line): dropped means nothing is printed.
//! - `echo stufftext_probe_chain; bind f2` asks whether a filtered name behind
//!   a `;` gets through, since the name test sees only `echo`: F2's binding
//!   printed means it does.
//!
//! Usage:
//!
//!     cargo run --release -p native --example stufftext_probe -- <in.dem> <out.dem> [seconds]
//!
//! `seconds` (default 5) is how far past the first `svc_time` after the
//! demo's `DemoStart` to inject, on the viewdemo clock.

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use native::patch::types::{PatchJob, PatcherConfig};
use native::patch::{StreamPatcher, cfg_scan};
use std::sync::{Arc, atomic::AtomicBool};

const COMMANDS: &[&str] = &[
    "echo stufftext_probe_start",
    "cvarlist gl_max",
    "bind f1",
    "mirv_matte_setcolor",
    "echo stufftext_probe exit_in_args",
    "echo stufftext_probe_chain; bind f2",
    "echo stufftext_probe_end",
];

/// The injection point: the 1-based frame ordinal `StreamPatcher` counts in,
/// with the clocks around it.
struct Target {
    ordinal: i32,
    /// The viewdemo clock when the commands run: the last `svc_time` before
    /// the target frame, since the injected frames are read ahead of it.
    svc_time_before: f32,
    /// The `svc_time` the target frame itself carries, if any.
    svc_time_at: Option<f32>,
    /// The frame header's own clock, which the injected frames copy.
    header_time: f32,
}

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

fn find_target(demo: &dem::types::Demo, seconds: f32) -> Option<Target> {
    let mut ordinal = 0i32;
    let mut started = false;
    let mut first_svc: Option<f32> = None;
    let mut svc_time = 0.0f32;

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            ordinal += 1;
            if matches!(frame.frame_data, FrameData::DemoStart) {
                started = true;
            }
            let here = last_svc_time(&frame.frame_data);
            if started
                && let (Some(first), Some(t)) = (first_svc, here)
                && t >= first + seconds
            {
                return Some(Target {
                    ordinal,
                    svc_time_before: svc_time,
                    svc_time_at: Some(t),
                    header_time: frame.time,
                });
            }
            if let Some(t) = here {
                if started && first_svc.is_none() {
                    first_svc = Some(t);
                }
                svc_time = t;
            }
        }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: stufftext_probe <in.dem> <out.dem> [seconds]");
        std::process::exit(2);
    }
    let input = std::path::PathBuf::from(&args[1]);
    let output = std::path::PathBuf::from(&args[2]);
    let seconds: f32 = args
        .get(3)
        .map(|s| s.parse().expect("seconds must be a number"))
        .unwrap_or(5.0);

    let bytes =
        std::fs::read(&input).unwrap_or_else(|e| panic!("could not read {}: {e}", input.display()));
    let demo = open_demo_from_bytes(&bytes)
        .unwrap_or_else(|e| panic!("could not parse {}: {e}", input.display()));
    let target = find_target(&demo, seconds).unwrap_or_else(|| {
        panic!(
            "{} has no svc_time {seconds}s past its DemoStart",
            input.display()
        )
    });
    drop(demo);

    let job = PatchJob {
        source_demo: input.to_string_lossy().to_string(),
        output_demo: output.clone(),
        streaks: Vec::new(),
        target_player: None,
        init_commands: Vec::new(),
        scheduled_commands: COMMANDS
            .iter()
            .map(|c| (target.ordinal, c.to_string()))
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

    // Read the copy back: every command must be in it, in order.
    let out_bytes = std::fs::read(&output)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", output.display()));
    let out_demo = open_demo_from_bytes(&out_bytes)
        .unwrap_or_else(|e| panic!("could not parse {}: {e}", output.display()));
    // The clock is read from the copy itself, so it is where the frames
    // actually landed rather than where they were aimed.
    let mut written: Vec<String> = Vec::new();
    let mut svc_time = 0.0f32;
    let mut landed: Option<(f32, f32)> = None; // (svc_time before, header time)
    let mut svc_after: Option<f32> = None;
    for frame in out_demo
        .directory
        .entries
        .iter()
        .flat_map(|e| e.frames.iter())
    {
        if let FrameData::ConsoleCommand(c) = &frame.frame_data {
            let text = c
                .command
                .to_str()
                .unwrap_or("")
                .trim_end_matches('\0')
                .to_string();
            if COMMANDS.contains(&text.as_str()) {
                landed.get_or_insert((svc_time, frame.time));
                written.push(text);
            }
        }
        if let Some(t) = last_svc_time(&frame.frame_data) {
            if landed.is_some() && svc_after.is_none() {
                svc_after = Some(t);
            }
            svc_time = t;
        }
    }
    assert_eq!(
        written, COMMANDS,
        "the copy does not carry the probe commands in order"
    );
    let (landed_svc, landed_header) = landed.expect("checked above");
    assert_eq!(
        landed_svc, target.svc_time_before,
        "the commands landed somewhere other than the frame they were aimed at"
    );
    assert_eq!(svc_after, target.svc_time_at);
    assert_eq!(landed_header, target.header_time);

    let clock = |t: f32| format!("{}:{:05.2}", (t / 60.0) as u32, t % 60.0);
    println!("Wrote {}", output.display());
    println!(
        "Injected {} ConsoleCommand frames ahead of frame {} (1-based ordinal).",
        COMMANDS.len(),
        target.ordinal
    );
    println!(
        "Viewdemo clock when they run: {:.2}s ({}); the frame after them carries svc_time {}.",
        target.svc_time_before,
        clock(target.svc_time_before),
        target
            .svc_time_at
            .map(|t| format!("{t:.2}s ({})", clock(t)))
            .unwrap_or_else(|| "none".to_string())
    );
    println!(
        "Frame header time (file clock): {:.2}s.",
        target.header_time
    );
    println!();
    println!("Expected console output if the filter is as cfg_scan models it:");
    for command in COMMANDS {
        let verdict = match cfg_scan::demo_filter_rule(command) {
            Some(rule) => format!("dropped ({} '{}')", rule.kind(), rule.pattern()),
            None => "runs".to_string(),
        };
        println!("  {command:<50} {verdict}");
    }
}
