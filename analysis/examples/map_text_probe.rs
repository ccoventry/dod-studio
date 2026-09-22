//! Which channel a map's own on-screen text arrives on, and what it says.
//!
//! `dod_anzio` warns about enemy mortars near a spawn exit (#287). That text
//! lands over recorded footage and there is no setting for it, but before
//! anything can suppress it, one question has to be answered: which of the four
//! ways GoldSrc and DoD put words on screen is actually carrying it?
//!
//!   `svc_temp_entity` / `TE_TEXTMESSAGE`  what a `game_text` entity sends
//!   `svc_centerprint`                     the engine's own centre print
//!   `HudText` (user message)              what `env_message` ends up as
//!   `TextMsg` (user message)              DoD's own, with a destination byte
//!
//! All four are ordinary recorded messages, so the answer is in the demo and
//! this costs a parse rather than a live test.
//!
//!     cargo run --release -p analysis --example map_text_probe -- <demo>
//!
//! Text is reported with its frame range, because that is what separates a
//! message the map fires at a trigger from one sent once at connect.

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage, TempEntity};
use dod::UserMessage;
use std::collections::BTreeMap;

/// One distinct string, however it arrived.
#[derive(Default)]
struct Seen {
    count: usize,
    first: usize,
    last: usize,
    /// Whatever the carrier says about placement: a channel, a destination, a
    /// position. Free text, because the four carriers describe it differently.
    detail: String,
}

#[derive(Default)]
struct Channel {
    strings: BTreeMap<String, Seen>,
}

impl Channel {
    fn record(&mut self, text: &str, frame: usize, detail: String) {
        let entry = self.strings.entry(clean(text)).or_insert(Seen {
            count: 0,
            first: frame,
            last: frame,
            detail,
        });
        entry.count += 1;
        entry.last = frame;
    }

    fn report(&self, label: &str) {
        if self.strings.is_empty() {
            println!("{label}: nothing");
            return;
        }
        println!("{label}: {} distinct", self.strings.len());
        for (text, seen) in &self.strings {
            println!(
                "  {:>5}x  frames {}-{}  {}  {:?}",
                seen.count, seen.first, seen.last, seen.detail, text
            );
        }
    }
}

fn clean(text: &str) -> String {
    text.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: map_text_probe <demo>");
    let bytes = std::fs::read(&path).expect("read demo");
    let demo = open_demo_from_bytes(&bytes).expect("parse demo");

    let mut te_text = Channel::default();
    let mut center_print = Channel::default();
    let mut hud_text = Channel::default();
    let mut text_msg = Channel::default();
    let mut frame_no = 0usize;

    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            frame_no += 1;
            let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &boxed.1.messages else {
                continue;
            };
            for message in messages {
                match message {
                    NetMessage::EngineMessage(engine) => match &**engine {
                        EngineMessage::SvcTempEntity(te) => {
                            if let TempEntity::TeTextMessage(text) = &te.entity {
                                te_text.record(
                                    &String::from_utf8_lossy(&text.message.0),
                                    frame_no,
                                    format!("channel {} at ({}, {})", text.channel, text.x, text.y),
                                );
                            }
                        }
                        EngineMessage::SvcCenterPrint(print) => {
                            center_print.record(
                                &String::from_utf8_lossy(&print.message),
                                frame_no,
                                "centre".to_string(),
                            );
                        }
                        _ => {}
                    },
                    NetMessage::UserMessage(user) => {
                        match UserMessage::new(&user.name, &user.data) {
                            Ok(UserMessage::HudText(text)) => hud_text.record(
                                &text.text,
                                frame_no,
                                format!("style {}", text.init_hud_style),
                            ),
                            Ok(UserMessage::TextMsg(text)) => {
                                let args: Vec<&str> =
                                    [&text.arg1, &text.arg2, &text.arg3, &text.arg4]
                                        .iter()
                                        .filter_map(|a| a.as_deref())
                                        .collect();
                                text_msg.record(
                                    &text.text,
                                    frame_no,
                                    format!("dest {} args {:?}", text.destination, args),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    println!("=== {path} ===");
    println!("{frame_no} frames\n");
    te_text.report("svc_temp_entity / TE_TEXTMESSAGE");
    println!();
    center_print.report("svc_centerprint");
    println!();
    hud_text.report("HudText (user message)");
    println!();
    text_msg.report("TextMsg (user message)");
}
