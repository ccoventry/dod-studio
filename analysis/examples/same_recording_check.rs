//! Is `monday-wsod25_r07_m3_h2.dem` the same underlying recording as
//! `wsod25_ply2_m3_h2_warchyld.dem`, just corrupted? Cross-check the healthy
//! prefix of one against the corresponding window of the other (a fixed time
//! offset established from matching kill events) using the same canonical,
//! address-stripped, HashMap-order-independent comparison already proven for
//! the reencode fidelity check.
use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Delta, EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

fn canon_delta(d: &Delta) -> String {
    let mut keys: Vec<&String> = d.keys().collect();
    keys.sort();
    keys.iter().map(|k| format!("{k}={:?}", d[*k])).collect::<Vec<_>>().join(",")
}

fn canon_msg(m: &NetMessage) -> String {
    match m {
        NetMessage::EngineMessage(em) => match &**em {
            EngineMessage::SvcTime(t) => format!("time({})", t.time),
            EngineMessage::SvcDeltaPacketEntities(pe) => {
                let mut ents: Vec<String> = pe.entity_states.iter().map(|es| format!(
                    "[{}:remove={}:{}]", es.entity_index, es.remove_entity,
                    es.delta.as_ref().map(canon_delta).unwrap_or_default(),
                )).collect();
                ents.sort();
                format!("deltapacketentities(count={},ents={:?})", pe.entity_count.to_u32(), ents)
            }
            _ => format!("{m:?}"),
        },
        NetMessage::UserMessage(um) => format!("usermsg({:?},{:?})", um.name, um.data),
    }
}

fn strip_addrs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("addr: 0x") {
        out.push_str(&rest[..i]);
        rest = &rest[i + 8..];
        let end = rest.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(rest.len());
        rest = &rest[end..];
        rest = rest.trim_start_matches(", ");
    }
    out.push_str(rest);
    out
}

fn collect(path: &str, t_lo: f32, t_hi: f32) -> BTreeMap<i64, (f32, Vec<String>)> {
    let bytes = std::fs::read(path).expect("read");
    let demo = open_demo_from_bytes(&bytes).expect("parse");
    let mut out = BTreeMap::new();
    for entry in demo.directory.entries.iter().skip(1) {
        for f in &entry.frames {
            if f.time < t_lo || f.time > t_hi { continue }
            let FrameData::NetworkMessage(bt) = &f.frame_data else { continue };
            let MessageData::Parsed(msgs) = &bt.1.messages else { continue };
            let canon: Vec<String> = msgs.iter()
                .filter(|m| !matches!(m, NetMessage::EngineMessage(em) if matches!(**em, EngineMessage::SvcClientData(_))))
                .map(|m| strip_addrs(&canon_msg(m))).collect();
            // Key on rounded time (x100) since we're aligning two independently
            // recorded files, not comparing a file against its own re-encode.
            out.insert((f.time * 100.0).round() as i64, (f.time, canon));
        }
    }
    out
}

fn main() {
    let mut a = std::env::args().skip(1);
    let path_a = a.next().expect("usage: same_recording_check <a.dem> <b.dem> <offset_b_to_a> <t_lo_a> <t_hi_a>");
    let path_b = a.next().expect("b.dem");
    let offset: f32 = a.next().expect("offset").parse().unwrap();
    let t_lo: f32 = a.next().expect("t_lo").parse().unwrap();
    let t_hi: f32 = a.next().expect("t_hi").parse().unwrap();

    let a_data = collect(&path_a, t_lo, t_hi);
    let b_data = collect(&path_b, t_lo - offset, t_hi - offset);
    // Re-key b onto a's time frame.
    let b_shifted: BTreeMap<i64, (f32, Vec<String>)> = b_data.into_iter()
        .map(|(_, (t, c))| (((t + offset) * 100.0).round() as i64, (t + offset, c)))
        .collect();

    println!("{} frames in {path_a} window, {} frames in shifted {path_b} window", a_data.len(), b_shifted.len());
    let mut matched = 0usize;
    let mut identical = 0usize;
    let mut diffs = 0usize;
    for (key, (t, ca)) in &a_data {
        // Allow a +/-1 key (0.01s) tolerance for float rounding at the boundary.
        let hit = b_shifted.get(key).or_else(|| b_shifted.get(&(key+1))).or_else(|| b_shifted.get(&(key-1)));
        let Some((_, cb)) = hit else { continue };
        matched += 1;
        if ca == cb { identical += 1 } else {
            diffs += 1;
            if diffs <= 3 {
                println!("\nDIFF at t={t:.2}s:\n  a: {ca:?}\n  b: {cb:?}");
            }
        }
    }
    println!("\n{matched} frames time-matched, {identical} identical, {diffs} differ");
}
