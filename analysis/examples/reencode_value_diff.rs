//! Do a full round-trip's *values* survive, not just message counts, for the
//! real untouched tail content in a window? Matches frames between the
//! pre-injection splice (raw bytes, multi_bridge's own output -- ground truth)
//! and the post-injection file (fully re-encoded by snapshot_inject) using
//! `incoming_sequence`, which is untouched for real frames in either file, and
//! diffs their Debug-formatted message content.
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

/// `Delta` is a `HashMap`, whose Debug order is randomised per-process --
/// comparing raw Debug strings flags every frame as "different" even when
/// every field and value is identical. Canonicalise by sorting keys first.
fn canon_delta(d: &Delta) -> String {
    let mut keys: Vec<&String> = d.keys().collect();
    keys.sort();
    keys.iter().map(|k| format!("{k}={:?}", d[*k])).collect::<Vec<_>>().join(",")
}

fn canon_msg(m: &NetMessage) -> String {
    match m {
        NetMessage::EngineMessage(em) => match &**em {
            EngineMessage::SvcTime(t) => format!("time({})", t.time),
            EngineMessage::SvcClientData(cd) => format!(
                "clientdata(mask={:?},data={},weapons={:?})",
                cd.delta_update_mask.as_ref().map(|b| b.to_u32()),
                canon_delta(&cd.client_data),
                cd.weapon_data.as_ref().map(|ws| ws.iter()
                    .map(|w| format!("{}:{}", w.weapon_index.to_u32(), canon_delta(&w.weapon_data)))
                    .collect::<Vec<_>>()),
            ),
            EngineMessage::SvcEvent(ev) => {
                let events: Vec<String> = ev.events.iter().map(|e| format!(
                    "idx={}:pkt={:?}:delta={}", e.event_index.to_u32(),
                    e.packet_index.as_ref().map(|b| b.to_u32()),
                    e.delta.as_ref().map(canon_delta).unwrap_or_default(),
                )).collect();
                format!("event(count={},events={events:?})", ev.event_count.to_u32())
            }
            EngineMessage::SvcDeltaPacketEntities(pe) => {
                let mut ents: Vec<String> = pe.entity_states.iter().map(|es| format!(
                    "[{}:remove={}:{}]", es.entity_index, es.remove_entity,
                    es.delta.as_ref().map(canon_delta).unwrap_or_default(),
                )).collect();
                ents.sort();
                format!("deltapacketentities(count={},dseq={},ents={:?})",
                    pe.entity_count.to_u32(), pe.delta_sequence.to_u32(), ents)
            }
            _ => format!("{m:?}"),
        },
        NetMessage::UserMessage(um) => format!("usermsg({:?},{:?})", um.name, um.data),
    }
}

/// `BitVec`'s own Debug output embeds its heap allocation's `addr:` -- a raw
/// pointer, always different between two separate parses regardless of the
/// bits it holds. Every message type not hand-canonicalised above (SvcSound
/// here) falls through to `{m:?}` and inherits that false-positive source.
/// Strip it rather than hand-canonicalise every message type in the protocol.
fn strip_addrs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("addr: 0x") {
        out.push_str(&rest[..i]);
        rest = &rest[i + 8..];
        let end = rest.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(rest.len());
        rest = &rest[end..];
        // Skip the trailing ", " left after the address itself.
        rest = rest.trim_start_matches(", ");
    }
    out.push_str(rest);
    out
}

fn collect(path: &str, t_lo: f32, t_hi: f32) -> BTreeMap<i32, (f32, String)> {
    let bytes = std::fs::read(path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut out = BTreeMap::new();
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            if f.time < t_lo || f.time > t_hi { continue }
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            let seq = bt.1.sequence_info.incoming_sequence;
            let canon: Vec<String> = msgs.iter().map(|m| strip_addrs(&canon_msg(m))).collect();
            out.insert(seq, (f.time, canon.join(" | ")));
        }
    }
    out
}

fn main() {
    let mut a = std::env::args().skip(1);
    let before_path = a.next().expect("usage: reencode_value_diff <before.dem> <after.dem> <t_lo> <t_hi>");
    let after_path = a.next().expect("after.dem");
    let t_lo: f32 = a.next().expect("t_lo").parse().unwrap();
    let t_hi: f32 = a.next().expect("t_hi").parse().unwrap();

    let before = collect(&before_path, t_lo, t_hi);
    let after = collect(&after_path, t_lo, t_hi);
    println!("before: {} frames in window, after: {} frames in window", before.len(), after.len());

    let mut matched = 0usize;
    let mut identical = 0usize;
    let mut diffs = 0usize;
    for (seq, (t, b_content)) in &before {
        let Some((_, a_content)) = after.get(seq) else { continue };
        matched += 1;
        if a_content == b_content {
            identical += 1;
        } else {
            diffs += 1;
            println!("DIFF #{diffs} at seq={seq} t={t:.2}s");
        }
    }
    println!("\n{matched} sequence numbers matched between the two files, {identical} identical, {diffs} differ");
}
