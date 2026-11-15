//! Is a grenade's screen shake recorded into a POV demo's view (#910)?
//!
//! In POV playback the engine draws the recorded `ref_params_s`, not the one
//! `V_CalcRefdef` works out live (`goldsrc-hooks/src/pov_live_bob.rs`), so a
//! shake the recorder's client applied would be in the recorded view itself,
//! where dropping the `ScreenShake` message at playback can't reach it. This
//! lines up every `ScreenShake` in a POV demo with the recorded view around it:
//!
//! - the eye offset `vieworg - simorg - viewheight - 1/32`, split along the
//!   recorded `forward`/`right`/`up` (bob is the sideways part; the engine's
//!   shake moves all three axes),
//! - the recorded roll, and `viewangles - cl_viewangles` (what `V_CalcRefdef`
//!   added to the angles: punch, roll from strafing, shake).
//!
//! Then it compares the offset's size inside the shakes against the rest of
//! the demo.
//!
//!     cargo run --release -p analysis --example screen_shake_probe -- <pov demo>... [--rows]

use dem::open_demo_from_bytes;
use dem::types::{FrameData, MessageData, NetMessage};

/// How long after a shake starts to look (the DoD blast shake lasts 1 s).
const AFTER: f32 = 1.2;
const NUDGE: f32 = 1.0 / 32.0;

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

struct Row {
    time: f32,
    fwd: f32,
    side: f32,
    up: f32,
    roll: f32,
    added: [f32; 3],
    /// The recorded gun origin minus the recorded `vieworg`, along
    /// forward/right/up.
    gun: [f32; 3],
    health: i32,
    spectator: i32,
}

fn summary(label: &str, xs: &mut [f32]) {
    if xs.is_empty() {
        println!("    {label:<26} none");
        return;
    }
    for x in xs.iter_mut() {
        *x = x.abs();
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    println!(
        "    {label:<26} median {:.3}  p95 {:.3}  max {:.3}  ({} frames)",
        xs[xs.len() / 2],
        xs[xs.len() * 95 / 100],
        xs[xs.len() - 1],
        xs.len()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rows_wanted = args.iter().any(|a| a == "--rows");
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            println!("{path}: does not parse");
            continue;
        };
        let mut rows: Vec<Row> = Vec::new();
        // (time, amplitude, duration s, frequency)
        let mut shakes: Vec<(f32, f32, f32, f32)> = Vec::new();
        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                    continue;
                };
                let rp = &bt.1.info.refparams;
                let eye: Vec<f32> = (0..3)
                    .map(|i| rp.view_origin[i] - rp.sim_org[i] - rp.view_height[i] - NUDGE)
                    .collect();
                rows.push(Row {
                    time: frame.time,
                    fwd: dot(&eye, &rp.forward),
                    side: dot(&eye, &rp.right),
                    up: dot(&eye, &rp.up),
                    roll: rp.view_angles[2],
                    added: [0, 1, 2].map(|i| angle_diff(rp.view_angles[i], rp.cl_viewangles[i])),
                    gun: {
                        let g: Vec<f32> = (0..3)
                            .map(|i| bt.1.info.view[i] - rp.view_origin[i])
                            .collect();
                        [dot(&g, &rp.forward), dot(&g, &rp.right), dot(&g, &rp.up)]
                    },
                    health: rp.health,
                    spectator: rp.spectator,
                });
                if let MessageData::Parsed(msgs) = &bt.1.messages {
                    for m in msgs {
                        let NetMessage::UserMessage(um) = m else {
                            continue;
                        };
                        let name = String::from_utf8_lossy(&um.name);
                        if !name.trim_end_matches('\0').ends_with("ScreenShake")
                            || um.data.len() < 6
                        {
                            continue;
                        }
                        let u = |i: usize| u16::from_le_bytes([um.data[i], um.data[i + 1]]) as f32;
                        shakes.push((frame.time, u(0) / 4096.0, u(2) / 4096.0, u(4) / 256.0));
                    }
                }
            }
        }
        let spectating = rows.iter().filter(|r| r.spectator != 0).count();
        println!(
            "=== {path}: {} frames, {} ScreenShake",
            rows.len(),
            shakes.len()
        );
        if spectating > rows.len() / 2 {
            println!("    HLTV/spectator demo, skipped");
            continue;
        }
        let in_shake = |t: f32| shakes.iter().any(|(s, _, d, _)| t >= *s && t <= s + d);
        let alive = |r: &&Row| r.health > 0 && r.spectator == 0;
        for &(t, amp, dur, freq) in &shakes {
            println!("  shake at {t:.3}: amplitude {amp:.2}, {dur:.2} s, frequency {freq:.1}");
            if !rows_wanted {
                continue;
            }
            println!(
                "      time   +dt    fwd    side     up    roll | added pitch   yaw  roll  hp | gun fwd  side    up"
            );
            for r in rows
                .iter()
                .filter(|r| r.time >= t - 0.15 && r.time <= t + AFTER)
            {
                println!(
                    "    {:8.3} {:+.3} {:+6.2} {:+6.2} {:+6.2} {:+6.2} |      {:+6.2} {:+6.2} {:+5.2} {:3} | {:+6.2} {:+6.2} {:+6.2}",
                    r.time,
                    r.time - t,
                    r.fwd,
                    r.side,
                    r.up,
                    r.roll,
                    r.added[0],
                    r.added[1],
                    r.added[2],
                    r.health,
                    r.gun[0],
                    r.gun[1],
                    r.gun[2]
                );
            }
        }
        let pick = |inside: bool, f: &dyn Fn(&Row) -> f32| -> Vec<f32> {
            rows.iter()
                .filter(alive)
                .filter(|r| in_shake(r.time) == inside)
                .map(f)
                .collect()
        };
        let eye = |r: &Row| (r.fwd * r.fwd + r.side * r.side + r.up * r.up).sqrt();
        let fwd_up = |r: &Row| (r.fwd * r.fwd + r.up * r.up).sqrt();
        println!("  alive, inside a shake:");
        summary("eye offset (all axes)", &mut pick(true, &eye));
        summary("eye offset (fwd+up only)", &mut pick(true, &fwd_up));
        summary("roll", &mut pick(true, &|r| r.roll));
        println!("  alive, outside every shake:");
        summary("eye offset (all axes)", &mut pick(false, &eye));
        summary("eye offset (fwd+up only)", &mut pick(false, &fwd_up));
        summary("roll", &mut pick(false, &|r| r.roll));
    }
}
