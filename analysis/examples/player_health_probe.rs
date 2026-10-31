//! What a demo tells the client about *other* players' health (#1130).
//!
//! `client.dll` keeps a per-player `health` in `g_PlayerExtraInfo` and fills
//! it from two places (both found by disassembly, see
//! `docs/goldsrc_player_info.md`):
//!
//! - `HUD_ProcessPlayerState` copies `entity_state_t::health` for every
//!   player in the frame's packet. DoD's `delta.lst` sends it for players
//!   (`health, DT_BYTE, 8`), so a demo has it only for the players the
//!   server put in the recorder's packet.
//! - `CHud::MsgFunc_HLTV` writes it from the `HLTV` user message, `BYTE
//!   player, BYTE health | 0x80`, which the server sends only to spectators
//!   (`CBasePlayer::TakeDamage`, `UpdateClientData`, `RestartRound`).
//!
//! Per demo this prints whether it is a POV or an HLTV recording (only HLTV
//! demos carry the `HLTV` message), how many player-entity deltas carried `health`
//! and for how many players, how many of those values were 1..=100, and how
//! many `HLTV` messages carried a health value.
//!
//!     cargo run --release -p analysis --example player_health_probe -- <demo>...

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeSet;

#[derive(Default)]
struct Tally {
    health_deltas: usize,
    health_alive_values: usize,
    players_with_health: BTreeSet<u16>,
    hltv_messages: usize,
    hltv_health: usize,
    hltv_players: BTreeSet<u8>,
}

fn is_health(key: &str) -> bool {
    key.trim_end_matches('\0') == "health"
}

impl Tally {
    fn note_entity(&mut self, index: u16, delta: &dem::types::Delta) {
        if !(1..=32).contains(&index) {
            return;
        }
        for (k, v) in delta {
            if is_health(k) {
                self.health_deltas += 1;
                self.players_with_health.insert(index);
                if v.first().is_some_and(|h| (1..=100).contains(h)) {
                    self.health_alive_values += 1;
                }
            }
        }
    }

    fn note_hltv(&mut self, data: &[u8]) {
        self.hltv_messages += 1;
        if let [player, value, ..] = data
            && value & 0x80 != 0
        {
            self.hltv_health += 1;
            self.hltv_players.insert(*player);
        }
    }
}

fn survey(bytes: &[u8]) -> Option<Tally> {
    let demo = open_demo_from_bytes(bytes).ok()?;
    let mut t = Tally::default();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(msgs) = &bt.1.messages else {
                continue;
            };
            for m in msgs {
                match m {
                    NetMessage::EngineMessage(em) => match &**em {
                        EngineMessage::SvcPacketEntities(pe) => {
                            for es in &pe.entity_states {
                                t.note_entity(es.entity_index, &es.delta);
                            }
                        }
                        EngineMessage::SvcDeltaPacketEntities(pe) => {
                            for es in &pe.entity_states {
                                if let Some(d) = &es.delta {
                                    t.note_entity(es.entity_index, d);
                                }
                            }
                        }
                        _ => {}
                    },
                    NetMessage::UserMessage(um) => {
                        if um.name.split(|&b| b == 0).next() == Some(b"HLTV") {
                            t.note_hltv(&um.data);
                        }
                    }
                }
            }
        }
    }
    Some(t)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: player_health_probe <demo>...");
        std::process::exit(2);
    }
    println!("demo\tkind\thealth_deltas\talive_values\tplayers\thltv_msgs\thltv_health\thltv_players");
    for path in &args {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let Some(t) = std::fs::read(path).ok().as_deref().and_then(survey) else {
            println!("{name}\tunreadable");
            continue;
        };
        // HLTV proxies record `svc_clientdata` too; the `HLTV` message,
        // sent to spectators only, is what tells the two apart.
        let kind = if t.hltv_messages > 0 { "HLTV" } else { "POV" };
        println!(
            "{name}\t{kind}\t{}\t{}\t{}\t{}\t{}\t{}",
            t.health_deltas,
            t.health_alive_values,
            t.players_with_health.len(),
            t.hltv_messages,
            t.hltv_health,
            t.hltv_players.len()
        );
    }
}
