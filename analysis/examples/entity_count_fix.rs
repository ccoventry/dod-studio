//! Restates every packet's `entity_count` as the size of the snapshot the engine
//! will actually build from it (#15).
//!
//! The recovered `_v10` builds crash on the pre-Anniversary engine a few seconds
//! after their first join (measured live: `m1_h1`, `m1_h2`, `m3_h1-1`). The
//! injected snapshot at a join holds every entity the prefix ever saw (245 in
//! `m1_h1`), while the tail's delta packets declare the size the server's own
//! snapshot had (about 10). `hw.dll`'s `CL_ParsePacketEntities` allocates the new
//! frame from the declared count and copies every carried entity into it with no
//! bounds check (`rep movsd` of 0x154 bytes at `hw.dll+0x13453`), so the copy
//! runs off the end of the heap block: an access violation there, or heap
//! corruption that faults a few seconds later.
//!
//! Carrying the entity set forward (`shared/entity_carry.rs`) gives the size the
//! client really builds, and this writes that size into every packet whose
//! declaration differs. The extra entities stay in the client's table until the
//! stream removes them (it may never, since the server never knew the client had
//! them); the report says how far apart the two counts are and for how long.
//!
//! Writes a new file and never overwrites.
//!
//!     cargo run --release -p analysis --example entity_count_fix -- <in.dem> <out.dem> [--trace <from> <to>]

use dem::bit::BitSliceCast;
use dem::open_demo_from_bytes;
use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

#[path = "shared/entity_carry.rs"]
mod entity_carry;
use entity_carry::Carry;

/// One packet whose declared count disagreed with the carried set.
struct Fixed {
    time: f32,
    declared: usize,
    carried: usize,
}

fn carried_count(em: &EngineMessage, seq: i32, carry: &mut Carry) -> Option<(usize, usize)> {
    match em {
        EngineMessage::SvcPacketEntities(pe) => {
            let set = carry.full(seq, pe.entity_states.iter().map(|e| e.entity_index));
            Some((pe.entity_count.to_u32() as usize, set.len()))
        }
        EngineMessage::SvcDeltaPacketEntities(dpe) => {
            let entries: Vec<(u16, bool)> = dpe
                .entity_states
                .iter()
                .map(|e| (e.entity_index, e.remove_entity))
                .collect();
            // A packet the engine throws away keeps its count: it is never used.
            let set = carry.delta(seq, dpe.delta_sequence.to_u8(), &entries)?;
            Some((dpe.entity_count.to_u32() as usize, set.len()))
        }
        _ => None,
    }
}

fn set_count(em: &mut EngineMessage, count: usize) {
    match em {
        EngineMessage::SvcPacketEntities(pe) => pe.entity_count = dem::nbit_num!(count as u32, 16),
        EngineMessage::SvcDeltaPacketEntities(dpe) => {
            dpe.entity_count = dem::nbit_num!(count as u32, 16)
        }
        _ => {}
    }
}

fn is_packet_entities(m: &NetMessage) -> bool {
    matches!(m, NetMessage::EngineMessage(em) if matches!(&**em,
        EngineMessage::SvcPacketEntities(_) | EngineMessage::SvcDeltaPacketEntities(_)))
}

/// Walks every packet-entities message; restates the counts when `fix` is set.
fn walk(
    demo: &mut Demo,
    fix: bool,
    trace: Option<std::ops::Range<f32>>,
) -> (Vec<Fixed>, usize, usize, usize) {
    let mut carry = Carry::default();
    let mut fixed = Vec::new();
    let mut packets = 0;
    for entry in demo.directory.entries.iter_mut() {
        for frame in entry.frames.iter_mut() {
            let time = frame.time;
            let FrameData::NetworkMessage(bt) = &mut frame.frame_data else {
                continue;
            };
            let seq = bt.1.sequence_info.incoming_sequence;
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            if !msgs.iter().any(is_packet_entities) {
                continue;
            }
            let mut wrong = Vec::new();
            for (n, m) in msgs.iter().enumerate() {
                if let NetMessage::EngineMessage(em) = m
                    && let Some((declared, carried)) = carried_count(em, seq, &mut carry)
                {
                    packets += 1;
                    if trace.as_ref().is_some_and(|r| r.contains(&time)) {
                        let kind = match &**em {
                            EngineMessage::SvcDeltaPacketEntities(d) => {
                                format!(
                                    "delta from {:>3} lists {:>3}",
                                    d.delta_sequence.to_u8(),
                                    d.entity_states.len()
                                )
                            }
                            EngineMessage::SvcPacketEntities(p) => {
                                format!("FULL          lists {:>3}", p.entity_states.len())
                            }
                            _ => String::new(),
                        };
                        println!(
                            "    t={time:>9.3} in_seq {seq:>7} (&ff {:>3}) {kind} declared {declared:>3} carried {carried:>3}",
                            seq & 0xFF
                        );
                    }
                    if declared != carried {
                        fixed.push(Fixed {
                            time,
                            declared,
                            carried,
                        });
                        wrong.push((n, carried));
                    }
                }
            }
            if fix && !wrong.is_empty() {
                let MessageData::Parsed(msgs) = bt.1.messages_mut() else {
                    continue;
                };
                for (n, carried) in wrong {
                    if let NetMessage::EngineMessage(em) = &mut msgs[n] {
                        set_count(em, carried);
                    }
                }
            }
        }
    }
    (fixed, packets, carry.missing_base, carry.flushed)
}

/// Prints the disagreements as runs: where each starts, how long it lasts and
/// how far apart the counts get.
fn report(fixed: &[Fixed]) {
    let mut start = 0;
    while start < fixed.len() {
        let mut end = start;
        while end + 1 < fixed.len() && fixed[end + 1].time - fixed[end].time < 2.0 {
            end += 1;
        }
        let run = &fixed[start..=end];
        let over = run.iter().map(|f| f.carried as i64 - f.declared as i64);
        let (lo, hi) = over.fold((i64::MAX, i64::MIN), |(lo, hi), d| (lo.min(d), hi.max(d)));
        println!(
            "  t={:>8.2}s .. {:>8.2}s  {:>6} packets  carried - declared {lo:+} .. {hi:+}  (first: declared {}, carried {})",
            run[0].time,
            run[run.len() - 1].time,
            run.len(),
            run[0].declared,
            run[0].carried
        );
        start = end + 1;
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: entity_count_fix <in.dem> <out.dem> [--trace <from> <to>]";
    let input = args.next().expect(usage);
    let output = args.next().expect(usage);
    // `--trace <from> <to>` prints every packet in that stretch of frame time.
    let trace = (args.next().as_deref() == Some("--trace")).then(|| {
        let mut t = || {
            args.next()
                .and_then(|v| v.parse::<f32>().ok())
                .expect("--trace <from> <to>")
        };
        t()..t()
    });
    if std::path::Path::new(&output).exists() {
        eprintln!("{output} already exists; this never overwrites");
        std::process::exit(2);
    }
    let bytes = std::fs::read(&input).expect("read demo");
    let mut demo = open_demo_from_bytes(&bytes).expect("parse demo");
    let (fixed, packets, missing, flushed) = walk(&mut demo, true, trace);
    println!(
        "{input}: {packets} packet-entity messages, {} with a count the carried set disagrees with, {missing} with no base, {flushed} the engine throws away",
        fixed.len()
    );
    report(&fixed);
    let out = demo.write_to_bytes();
    std::fs::write(&output, &out).expect("write output");
    let mut again = open_demo_from_bytes(&out).expect("output re-parses");
    let (left, packets, _, _) = walk(&mut again, false, None);
    println!(
        "wrote {output}; check: {packets} packets, {} still disagree",
        left.len()
    );
    std::process::exit(if left.is_empty() { 0 } else { 1 });
}
