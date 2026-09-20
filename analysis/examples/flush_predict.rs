//! Predict, offline, every entity packet the engine will throw away (#15, #224).
//!
//! The recovery work had no automatable pass/fail for a stitched demo, because
//! `WARNING:  CL_FlushEntityPacket` only ever appears on screen -- and static
//! analysis of `hw.dll` says why: the function at `0x1d12240` prints through
//! `Con_NXPrintf` with a `{index, ttl, color}` block, which draws to the screen
//! and never touches `Con_Printf`, so nothing reaches `qconsole.log`.
//!
//! But the *condition* is in the binary and is exact. `CL_ParsePacketEntities`
//! (`0x1d12e30`) reads the delta sequence byte, then:
//!
//!     mov  eax, [0x2d59b20]   ; cls.netchan.incoming_sequence
//!     mov  ecx, [0x1e3afcc]   ; 63 -- CL_UPDATE_BACKUP - 1
//!     sub  eax, edi           ; - delta_sequence
//!     and  eax, 0xff
//!     cmp  eax, ecx
//!     jl   keep               ; else fall through to CL_FlushEntityPacket
//!
//! so a packet is discarded exactly when
//!
//!     ((incoming_sequence - delta_sequence) & 0xFF) >= 63
//!
//! Both terms are in the demo file: `incoming_sequence` sits in each network
//! message frame's own sequence header, and `delta_sequence` in the
//! `svc_deltapacketentities` message. So the warning is predictable without
//! running the game, which is what makes a stitch testable in a loop.
//!
//! A flush also sets `cl.validsequence = 0` and marks that frame invalid, so a
//! run of consecutive flushes is what shows up as missing world geometry.
//!
//!     cargo run --release -p analysis --example flush_predict -- <demo.dem>

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

/// `CL_UPDATE_BACKUP - 1`, read from `hw.dll` .data at 0x1e3afcc.
const FLUSH_LIMIT: i64 = 63;

fn main() {
    let path = std::env::args().nth(1).expect("usage: flush_predict <demo.dem>");
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");

    let mut packets = 0usize;
    let mut flushes: Vec<(f32, i32, u32, i64)> = Vec::new();
    let mut worst = 0i64;

    for entry in demo.directory.entries.iter() {
        for f in &entry.frames {
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let incoming = bt.1.sequence_info.incoming_sequence;
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs {
                let NetMessage::EngineMessage(em) = m else { continue };
                let EngineMessage::SvcDeltaPacketEntities(pe) = &**em else { continue };
                packets += 1;
                let delta = pe.delta_sequence.to_u32();
                let gap = (incoming as i64 - delta as i64) & 0xff;
                if gap > worst { worst = gap }
                if gap >= FLUSH_LIMIT {
                    flushes.push((f.time, incoming, delta, gap));
                }
            }
        }
    }

    println!("{path}");
    println!("  {packets} delta entity packets, worst sequence gap {worst} (flush at {FLUSH_LIMIT})");
    if flushes.is_empty() {
        println!("  no packet is discarded -- the engine will not print the flush warning");
        return;
    }
    println!("  {} packets WILL be discarded:", flushes.len());
    for (time, incoming, delta, gap) in flushes.iter().take(20) {
        println!("    t={time:.2}s incoming={incoming} delta_sequence={delta} gap={gap}");
    }
    if flushes.len() > 20 { println!("    ... and {} more", flushes.len() - 20) }
    let first = flushes.first().unwrap().0;
    let last = flushes.last().unwrap().0;
    println!("  discards run from t={first:.2}s to t={last:.2}s");
}
