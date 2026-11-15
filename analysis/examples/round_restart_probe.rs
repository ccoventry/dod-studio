//! When does a DoD demo start a new round? A timeline of every `RoundState`
//! with the round-related text around it, for `ds_hud_deathmsg clear_on
//! restart` (which clears the kill feed on `RoundState` 0, the reset).
//!
//! Prints, on the viewdemo bar's clock (the last `svc_time`, `m:ss:cc`):
//! every `RoundState`, every `TextMsg`/`HudText`/`svc_print`/`svc_centerprint`
//! that mentions a round, a restart, a win, going live or a half, and how many
//! `DeathMsg`s arrived since the previous `RoundState`.
//!
//!     cargo run --release -p analysis --example round_restart_probe -- a.dem [b.dem ...]

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use dod::UserMessage;

fn clock(t: f32) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

fn interesting(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    // The once-a-second clock; never a round event.
    if lower.contains("game_time_left") {
        return false;
    }
    [
        "round", "restart", "win", "live", "half", "captured", "time", "draw", "match", "enough",
        "ready",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn clean(text: &str) -> String {
    text.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .replace('\n', " | ")
}

fn probe(path: &str) {
    let Ok(bytes) = std::fs::read(path) else {
        println!("=== {path}: could not read");
        return;
    };
    let Ok(demo) = open_demo_from_bytes(&bytes) else {
        println!("=== {path}: could not parse");
        return;
    };
    println!("=== {path}");
    let mut svc_time = 0f32;
    let mut deaths = 0u32;
    let mut last_text = String::new();
    let mut last_kill: Option<f32> = None;
    let mut awaiting_first_kill = false;
    // WINDOW=from,to (seconds on the viewdemo clock) also lists every user
    // message in that span.
    let window = std::env::var("WINDOW").ok().and_then(|w| {
        let (a, b) = w.split_once(',')?;
        Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))
    });
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &boxed.1.messages else {
                continue;
            };
            for message in messages {
                let mut text = None;
                match message {
                    NetMessage::EngineMessage(engine) => match &**engine {
                        EngineMessage::SvcTime(t) => svc_time = t.time,
                        EngineMessage::SvcPrint(p) => {
                            text = Some(format!(
                                "print {}",
                                clean(&String::from_utf8_lossy(&p.message.0))
                            ))
                        }
                        EngineMessage::SvcCenterPrint(p) => {
                            text = Some(format!(
                                "centerprint {}",
                                clean(&String::from_utf8_lossy(&p.message))
                            ))
                        }
                        _ => {}
                    },
                    NetMessage::UserMessage(user) => {
                        let name = String::from_utf8_lossy(&user.name)
                            .trim_end_matches('\0')
                            .to_string();
                        if window.is_some_and(|(from, to)| svc_time >= from && svc_time <= to) {
                            println!("  {}        . {name} {:?}", clock(svc_time), user.data);
                        }
                        if name == "DeathMsg" {
                            deaths += 1;
                            if awaiting_first_kill {
                                println!("  {}      first kill after the start", clock(svc_time));
                                awaiting_first_kill = false;
                            }
                            last_kill = Some(svc_time);
                            continue;
                        }
                        if name == "RoundState" {
                            let last = last_kill.map_or("none".to_string(), |t| {
                                format!("{} ({:.2} s before)", clock(t), svc_time - t)
                            });
                            println!(
                                "  {}  RoundState {:?}   ({deaths} kills since the last one, last kill {last})",
                                clock(svc_time),
                                user.data
                            );
                            deaths = 0;
                            awaiting_first_kill = user.data.first() == Some(&1);
                            // A repeat after a round change is news again.
                            last_text.clear();
                            continue;
                        }
                        match UserMessage::new(&user.name, &user.data) {
                            Ok(UserMessage::HudText(t)) => {
                                text = Some(format!("HudText {}", clean(&t.text)))
                            }
                            Ok(UserMessage::TextMsg(t)) => {
                                let args: Vec<&str> = [&t.arg1, &t.arg2, &t.arg3, &t.arg4]
                                    .iter()
                                    .filter_map(|a| a.as_deref())
                                    .collect();
                                text = Some(format!("TextMsg {} {:?}", clean(&t.text), args))
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(text) = text
                    && interesting(&text)
                    && text != last_text
                {
                    println!("  {}      {text}", clock(svc_time));
                    last_text = text;
                }
            }
        }
    }
    println!(
        "  end {}  ({deaths} kills after the last RoundState)",
        clock(svc_time)
    );
}

fn main() {
    for path in std::env::args().skip(1) {
        probe(&path);
    }
}
