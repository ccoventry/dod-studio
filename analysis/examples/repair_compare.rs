//! Compares two repaired builds of one demo (#15): Studio's repair against an
//! R&D build made with the old probes, say. Prints each side's playback
//! frames, full snapshots and their sizes, the engine checks, and which frames
//! only one side has (matched by type, time, ordinal and sequence, so a frame
//! moved by a sort still matches). The R&D builds' `echo [BC t]` breadcrumbs
//! are counted apart.
//!
//!     cargo run --release -p analysis --example repair_compare -- <a.dem> <b.dem>

use std::collections::BTreeMap;

use dem::open_demo_from_bytes;
use dem::types::{Demo, EngineMessage, Frame, FrameData, MessageData, NetMessage};

/// What identifies a frame across two builds.
type Key = (u8, u32, i32, i32, usize);

fn kind(fd: &FrameData) -> u8 {
    match fd {
        FrameData::NetworkMessage(_) => 1,
        FrameData::DemoStart => 2,
        FrameData::ConsoleCommand(_) => 3,
        FrameData::ClientData(_) => 4,
        FrameData::NextSection => 5,
        FrameData::Event(_) => 6,
        FrameData::WeaponAnimation(_) => 7,
        FrameData::Sound(_) => 8,
        FrameData::DemoBuffer(_) => 9,
    }
}

fn is_breadcrumb(f: &Frame) -> bool {
    matches!(&f.frame_data, FrameData::ConsoleCommand(c) if c.command.as_slice().starts_with(b"echo [BC "))
}

fn key(f: &Frame) -> Key {
    let (seq, msgs) = match &f.frame_data {
        FrameData::NetworkMessage(bt) => (
            bt.1.sequence_info.incoming_sequence,
            match &bt.1.messages {
                MessageData::Parsed(m) => m.len(),
                _ => 0,
            },
        ),
        _ => (0, 0),
    };
    (kind(&f.frame_data), f.time.to_bits(), f.frame, seq, msgs)
}

struct Summary {
    frames: usize,
    breadcrumbs: usize,
    snapshots: Vec<(f32, usize)>,
    keys: BTreeMap<Key, usize>,
    flushed: usize,
    count_disagreements: usize,
}

fn summarise(demo: &Demo) -> Summary {
    let mut s = Summary {
        frames: 0,
        breadcrumbs: 0,
        snapshots: Vec::new(),
        keys: BTreeMap::new(),
        flushed: 0,
        count_disagreements: 0,
    };
    for f in demo
        .directory
        .entries
        .iter()
        .skip(1)
        .flat_map(|e| e.frames.iter())
    {
        if is_breadcrumb(f) {
            s.breadcrumbs += 1;
            continue;
        }
        s.frames += 1;
        *s.keys.entry(key(f)).or_default() += 1;
        if let FrameData::NetworkMessage(bt) = &f.frame_data
            && let MessageData::Parsed(msgs) = &bt.1.messages
        {
            for m in msgs {
                if let NetMessage::EngineMessage(em) = m
                    && let EngineMessage::SvcPacketEntities(pe) = &**em
                {
                    s.snapshots.push((f.time, pe.entity_states.len()));
                }
            }
        }
    }
    let mut carry = analysis::repair::entity_carry::Carry::default();
    for f in demo.directory.entries.iter().flat_map(|e| e.frames.iter()) {
        let FrameData::NetworkMessage(bt) = &f.frame_data else {
            continue;
        };
        let seq = bt.1.sequence_info.incoming_sequence;
        let MessageData::Parsed(msgs) = &bt.1.messages else {
            continue;
        };
        for m in msgs {
            let NetMessage::EngineMessage(em) = m else {
                continue;
            };
            use dem::bit::BitSliceCast;
            let (declared, carried) = match &**em {
                EngineMessage::SvcPacketEntities(pe) => (
                    pe.entity_count.to_u32() as usize,
                    carry
                        .full(seq, pe.entity_states.iter().map(|e| e.entity_index))
                        .len(),
                ),
                EngineMessage::SvcDeltaPacketEntities(d) => {
                    let entries: Vec<(u16, bool)> = d
                        .entity_states
                        .iter()
                        .map(|e| (e.entity_index, e.remove_entity))
                        .collect();
                    match carry.delta(seq, d.delta_sequence.to_u8(), &entries) {
                        Some(set) => (d.entity_count.to_u32() as usize, set.len()),
                        None => continue,
                    }
                }
                _ => continue,
            };
            if declared != carried {
                s.count_disagreements += 1;
            }
        }
    }
    s.flushed = carry.flushed;
    s
}

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: repair_compare <a.dem> <b.dem>";
    let (a, b) = (args.next().expect(usage), args.next().expect(usage));
    let sa =
        summarise(&open_demo_from_bytes(&std::fs::read(&a).expect("read a")).expect("parse a"));
    let sb =
        summarise(&open_demo_from_bytes(&std::fs::read(&b).expect("read b")).expect("parse b"));
    for (name, s) in [(&a, &sa), (&b, &sb)] {
        println!("{name}");
        println!(
            "  {} playback frames (+{} breadcrumbs), {} discarded entity packets, {} count disagreements",
            s.frames, s.breadcrumbs, s.flushed, s.count_disagreements
        );
        let snaps: Vec<String> = s
            .snapshots
            .iter()
            .map(|(t, n)| format!("{t:.1}s:{n}"))
            .collect();
        println!("  full snapshots (time:entities): {}", snaps.join(" "));
    }
    let only = |x: &Summary, y: &Summary| -> Vec<(Key, usize)> {
        x.keys
            .iter()
            .filter_map(|(k, n)| {
                let m = y.keys.get(k).copied().unwrap_or(0);
                (n > &m).then_some((*k, n - m))
            })
            .collect()
    };
    for (label, diff) in [("only in a", only(&sa, &sb)), ("only in b", only(&sb, &sa))] {
        let total: usize = diff.iter().map(|(_, n)| n).sum();
        println!("{label}: {total} frames");
        for ((t, time, ord, seq, msgs), n) in diff.iter().take(12) {
            println!(
                "    type {t} t={:.3} ord={ord} seq={seq} msgs={msgs} x{n}",
                f32::from_bits(*time)
            );
        }
    }
}
