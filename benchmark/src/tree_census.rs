//! Counts the decoded tree's delta tables and their fields, so the decode's
//! heap-block count can be set against them (#37: as a
//! `HashMap<String, Vec<u8>>`, each table cost a block, plus a key and a
//! value per field; stored as `dem::types::Delta` now, a table costs one).

use dem::types::{Delta, Demo, EngineMessage, FrameData, MessageData, NetMessage};

#[derive(Default)]
pub struct Census {
    /// Delta tables that hold at least one field (an empty one allocates nothing).
    pub delta_tables: usize,
    /// Fields across every delta table.
    pub delta_fields: usize,
}

pub fn take(demo: &Demo) -> Census {
    let mut c = Census::default();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                if let NetMessage::EngineMessage(e) = m {
                    count_engine(e, &mut c);
                }
            }
        }
    }
    c
}

fn count_engine(e: &EngineMessage, c: &mut Census) {
    match e {
        EngineMessage::SvcDeltaPacketEntities(m) => {
            for s in &m.entity_states {
                if let Some(d) = &s.delta {
                    add(d, c);
                }
            }
        }
        EngineMessage::SvcPacketEntities(m) => {
            for s in &m.entity_states {
                add(&s.delta, c);
            }
        }
        EngineMessage::SvcClientData(m) => {
            add(&m.client_data, c);
            for w in m.weapon_data.iter().flatten() {
                add(&w.weapon_data, c);
            }
        }
        EngineMessage::SvcEvent(m) => {
            for ev in &m.events {
                if let Some(d) = &ev.delta {
                    add(d, c);
                }
            }
        }
        EngineMessage::SvcEventReliable(m) => add(&m.event_args, c),
        EngineMessage::SvcSpawnBaseline(m) => {
            for ent in &m.entities {
                add(&ent.delta, c);
            }
            for d in &m.extra_data {
                add(d, c);
            }
        }
        _ => {}
    }
}

fn add(d: &Delta, c: &mut Census) {
    if !d.is_empty() {
        c.delta_tables += 1;
        c.delta_fields += d.len();
    }
}
