//! Where one player is, frame by frame, over a range of a demo: the path a
//! highlight clip follows. A frag-movie editor reads it to judge a jump cut:
//! two moments 30 s apart can still be a few metres apart on the map, and a
//! cut between them reads as continuous, while a cut across the map makes
//! the viewer wonder whether it is the same clip.
//!
//! Frames are counted the way the analyzer counts them (`GameTime::
//! frame_index`: 1-based, every frame of every directory entry), so a
//! highlight's `start_tick`/`end_tick` and each kill's tick index straight
//! into the output.
//!
//! The position comes from the frame header's view (`refparams`) when that
//! view is the player's, as in their own POV recording: the eye position and
//! the view angles. Otherwise, in an HLTV demo or someone else's POV, it is
//! the player entity's origin from the entity replay, with no angles.
//!
//! The player is given by entity index, which is a highlight's 0-based
//! `player_index` plus one (checked on wsod25_grp3_h1: the POV recorder's
//! view entity and the HLTV demo's entity for the same streak agree to the
//! unit, the HLTV origin sitting at the feet, the view at the eye).
//!
//! CSV on stdout, one row per network frame in the range where the position
//! is known: `frame,time,x,y,z,pitch,yaw,source` (`source` is `view` or
//! `entity`).
//!
//!     cargo run --release -p analysis --example player_path -- <demo.dem> <entity index> <from frame> <to frame>
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "dev-only probe run by hand: a panic on bad input is its error report (#1179)"
)]

use analysis::PlayerPose;
use analysis::entity_replay::EntityReplay;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: player_path <demo.dem> <entity index> <from frame> <to frame>";
    let path = args.first().expect(usage);
    let player: u16 = args.get(1).expect(usage).parse().expect(usage);
    let from: usize = args.get(2).expect(usage).parse().expect(usage);
    let to: usize = args.get(3).expect(usage).parse().expect(usage);
    let bytes = std::fs::read(path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    writeln!(out, "frame,time,x,y,z,pitch,yaw,source").expect("write");
    let mut replay: EntityReplay<PlayerPose> = EntityReplay::default();
    let mut frame_index = 0usize;
    for entry in &demo.directory.entries {
        for f in &entry.frames {
            frame_index += 1;
            let FrameData::NetworkMessage(bt) = &f.frame_data else {
                continue;
            };
            replay.begin_frame(bt.1.sequence_info.incoming_sequence);
            if let MessageData::Parsed(msgs) = &bt.1.messages {
                for m in msgs {
                    if let NetMessage::EngineMessage(em) = m {
                        if let EngineMessage::SvcServerInfo(si) = &**em {
                            replay.reset(1..=u16::from(si.max_players));
                        }
                        replay.apply(em);
                    }
                }
            }
            if frame_index < from {
                continue;
            }
            if frame_index > to {
                return;
            }
            let view = &bt.1.info.refparams;
            // The view entity is 1-based like the player index; 0 means none.
            if i64::from(view.view_entity) == i64::from(player) && view.view_origin.len() == 3 {
                let o = &view.view_origin;
                let a = &view.view_angles;
                writeln!(
                    out,
                    "{frame_index},{:.4},{:.1},{:.1},{:.1},{:.1},{:.1},view",
                    f.time,
                    o[0],
                    o[1],
                    o[2],
                    a.first().copied().unwrap_or(0.0),
                    a.get(1).copied().unwrap_or(0.0)
                )
                .expect("write");
            } else if let Some(o) = replay.get(player).and_then(PlayerPose::known_origin) {
                writeln!(
                    out,
                    "{frame_index},{:.4},{:.1},{:.1},{:.1},,,entity",
                    f.time, o[0], o[1], o[2]
                )
                .expect("write");
            }
        }
    }
}
