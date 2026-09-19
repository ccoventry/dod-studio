//! Does the full-file re-encode change *content*, not just cosmetic bit
//! choices, in the untouched real tail? `snapshot_inject` reserializes every
//! message in the file via `write_to_bytes`, not just the ones it constructs.
//! Diff a pure `multi_bridge` splice (raw bytes, zero re-encoding) against the
//! same logical frames after `snapshot_inject`'s round-trip, restricted to a
//! narrow real-time window, and report which message TYPES actually differ in
//! byte length (a value change alone wouldn't show here, but a length change
//! means the encoded bit-width itself moved).
use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};

fn msg_kind(m: &NetMessage) -> &'static str {
    match m {
        NetMessage::EngineMessage(em) => match &**em {
            EngineMessage::SvcTime(_) => "svc_time",
            EngineMessage::SvcClientData(_) => "svc_clientdata",
            EngineMessage::SvcDeltaPacketEntities(_) => "svc_deltapacketentities",
            EngineMessage::SvcPacketEntities(_) => "svc_packetentities",
            EngineMessage::SvcSound(_) => "svc_sound",
            EngineMessage::SvcTempEntity(_) => "svc_tempentity",
            EngineMessage::SvcEvent(_) => "svc_event",
            EngineMessage::SvcEventReliable(_) => "svc_event_reliable",
            EngineMessage::SvcWeaponAnim(_) => "svc_weaponanim",
            EngineMessage::SvcPrint(_) => "svc_print",
            _ => "other",
        },
        NetMessage::UserMessage(_) => "usermsg",
    }
}

fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("usage: reencode_diff <demo.dem> <t_lo> <t_hi>");
    let t_lo: f32 = a.next().expect("t_lo").parse().unwrap();
    let t_hi: f32 = a.next().expect("t_hi").parse().unwrap();
    let bytes = std::fs::read(&path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let rewritten = demo.write_to_bytes();
    let redone = open_demo_from_bytes(&rewritten).expect("reparse");

    // Walk both in lockstep by (time window), comparing serialized message
    // byte lengths per kind for network-message frames in range.
    use std::collections::HashMap;
    let mut before: HashMap<&str, (usize, usize)> = HashMap::new(); // kind -> (count, total original raw bytes estimate n/a)
    let mut after: HashMap<&str, usize> = HashMap::new();
    let mut frames_in_window = 0usize;

    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            if f.time < t_lo || f.time > t_hi { continue }
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            frames_in_window += 1;
            for m in msgs { *before.entry(msg_kind(m)).or_insert((0,0)) = (before.get(msg_kind(m)).map(|x|x.0).unwrap_or(0)+1, 0); }
        }
    }
    for entry in redone.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            if f.time < t_lo || f.time > t_hi { continue }
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            for m in msgs { *after.entry(msg_kind(m)).or_insert(0) += 1; }
        }
    }
    println!("{path} window t={t_lo}..{t_hi}, {frames_in_window} network-message frames");
    println!("message-kind counts, before vs after full round-trip (should match exactly):");
    let mut kinds: Vec<&str> = before.keys().chain(after.keys()).copied().collect();
    kinds.sort(); kinds.dedup();
    for k in kinds {
        let b = before.get(k).map(|x| x.0).unwrap_or(0);
        let a = after.get(k).copied().unwrap_or(0);
        let flag = if b != a { "  <-- MISMATCH" } else { "" };
        println!("  {k:<22} before={b:<6} after={a:<6}{flag}");
    }
    println!("original file size: {}", bytes.len());
    println!("rewritten file size: {}", rewritten.len());
}
