//! What can bring up the weapon-selection list during POV playback (#1032).
//!
//! A POV demo records the player's `slotN`, `invnext` and `invprev` presses as
//! console-command frames (`lastinv` is a server command and never appears).
//! Per demo this counts them, and for each one says how the recorder's weapon
//! switch (`CurWeapon`) followed: at once (no list on their screen), or only
//! after a later `+attack` (the list was open, and playback opens it too). It
//! also counts `slotN` presses whose bucket held two or more carried weapons
//! (`clientdata_t.weapons` against the `WeaponList` slots): DoD's `HasAmmo`
//! always says yes, so those open the list even with `hud_fastswitch 1`.
//!
//!     cargo run --release -p analysis --example weapon_list_probe -- [--all FROM TO] demo.dem...
//!
//! With `--all`, prints every console command, weapon message and change of
//! carried weapons between FROM and TO seconds (the viewdemo clock, the last
//! `SvcTime`).

use dem::open_demo_from_bytes;
use dem::types::{EngineMessage, FrameData, MessageData, NetMessage};
use std::collections::BTreeMap;

fn clock(t: f32) -> String {
    let cs = (t.max(0.0) * 100.0) as u32;
    format!(
        "{:7.2} ({}:{:02}.{:02})",
        t,
        cs / 6000,
        cs / 100 % 60,
        cs % 100
    )
}

fn ids(bits: u64) -> String {
    let v: Vec<String> = (0..64)
        .filter(|i| bits & (1u64 << i) != 0)
        .map(|i| i.to_string())
        .collect();
    format!("[{}]", v.join(","))
}

fn is_selection(lower: &str) -> bool {
    lower.split(';').any(|c| {
        let c = c.trim();
        c.starts_with("invnext")
            || c.starts_with("invprev")
            || c.starts_with("slot")
            || c.starts_with("lastinv")
            || c.starts_with("hud_fastswitch")
            || c.starts_with("cancelselect")
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut window: Option<(f32, f32)> = None;
    if args.first().map(|a| a == "--all").unwrap_or(false) {
        let from = args[1].parse().unwrap();
        let to = args[2].parse().unwrap();
        window = Some((from, to));
        args.drain(0..3);
    }
    for path in args {
        let Ok(bytes) = std::fs::read(&path) else {
            println!("== {path}: cannot read");
            continue;
        };
        let Ok(demo) = open_demo_from_bytes(&bytes) else {
            println!("== {path}: does not parse");
            continue;
        };
        println!("== {path}");
        let mut svc_time = 0.0f32;
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut printed_keys = false;
        let mut weapon_bits: u64 = 0;
        let mut slot_of: std::collections::HashMap<u8, u8> = std::collections::HashMap::new();
        let mut pending: Option<(f32, String, Option<f32>)> = None;
        let mut active: u8 = 0;
        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                match &frame.frame_data {
                    FrameData::ConsoleCommand(c) => {
                        let text = String::from_utf8_lossy(c.command.as_slice())
                            .trim_end_matches('\0')
                            .to_string();
                        let lower = text.to_ascii_lowercase();
                        let in_window = window.map(|(a, b)| svc_time >= a && svc_time <= b);
                        let sel = is_selection(&lower);
                        if sel {
                            for part in lower.split(';') {
                                let p = part.trim();
                                if is_selection(p) {
                                    *counts.entry(p.to_string()).or_default() += 1;
                                }
                            }
                        }
                        // A slotN whose bucket holds 2+ carried weapons opens the list
                        // even with hud_fastswitch 1 (client.dll SelectSlot).
                        let mut multi = false;
                        for part in lower.split(';') {
                            let p = part.trim();
                            if let Some(n) =
                                p.strip_prefix("slot").and_then(|n| n.parse::<u8>().ok())
                            {
                                let in_bucket = (0..64u8)
                                    .filter(|&i| weapon_bits & (1u64 << i) != 0)
                                    .filter(|i| slot_of.get(i) == Some(&(n.wrapping_sub(1))))
                                    .count();
                                if in_bucket >= 2 {
                                    multi = true;
                                    *counts.entry(format!("{p} with 2+ in bucket")).or_default() +=
                                        1;
                                }
                            }
                        }
                        if in_window.unwrap_or_default() {
                            println!(
                                "  {} cmd {text:?}   carrying {}{}",
                                clock(svc_time),
                                ids(weapon_bits),
                                if multi { "  <- 2+ in that bucket" } else { "" }
                            );
                        }
                        if sel
                            && (lower.trim().starts_with("slot") || lower.trim().starts_with("inv"))
                        {
                            pending = Some((svc_time, text.clone(), None));
                        }
                        if lower.trim() == "+attack"
                            && let Some(p) = pending.as_mut()
                            && p.2.is_none()
                        {
                            p.2 = Some(svc_time);
                        }
                    }
                    FrameData::NetworkMessage(bt) => {
                        let MessageData::Parsed(msgs) = &bt.1.messages else {
                            continue;
                        };
                        for m in msgs {
                            match m {
                                NetMessage::EngineMessage(em) => {
                                    if let EngineMessage::SvcTime(t) = &**em {
                                        svc_time = t.time;
                                    }
                                    if let EngineMessage::SvcClientData(cd) = &**em {
                                        if !printed_keys && window.is_some() {
                                            let mut keys: Vec<String> = cd
                                                .client_data
                                                .keys()
                                                .map(|k| k.trim_end_matches('\0').to_string())
                                                .collect();
                                            keys.sort();
                                            println!("  clientdata fields seen first: {keys:?}");
                                            printed_keys = true;
                                        }
                                        if let Some(v) = cd
                                            .client_data
                                            .iter()
                                            .find(|(k, _)| k.trim_end_matches('\0') == "weapons")
                                            .map(|(_, v)| v)
                                        {
                                            let mut b = [0u8; 8];
                                            for (i, x) in v.iter().take(8).enumerate() {
                                                b[i] = *x;
                                            }
                                            let bits = u64::from_le_bytes(b);
                                            if bits != weapon_bits {
                                                weapon_bits = bits;
                                                if window
                                                    .map(|(a, z)| svc_time >= a && svc_time <= z)
                                                    == Some(true)
                                                {
                                                    println!(
                                                        "  {} weapons {}",
                                                        clock(svc_time),
                                                        ids(bits)
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                NetMessage::UserMessage(um) => {
                                    let name = String::from_utf8_lossy(&um.name)
                                        .trim_end_matches('\0')
                                        .to_string();
                                    if [
                                        "CurWeapon",
                                        "WeapPickup",
                                        "WeaponList",
                                        "AmmoPickup",
                                        "HideWeapon",
                                    ]
                                    .contains(&name.as_str())
                                    {
                                        *counts.entry(format!("msg {name}")).or_default() += 1;
                                        // DoD's WeaponList: ammo1, max1, ammo2, max2, slot, position, id, ...
                                        if name == "WeaponList" && um.data.len() >= 7 {
                                            slot_of.insert(um.data[6], um.data[4]);
                                        }
                                        // How did the recorder's switch land: at once after the
                                        // selection command (no list on the recorder's screen),
                                        // or only after a later +attack (the list was open).
                                        if name == "CurWeapon"
                                            && um.data.len() >= 2
                                            && um.data[0] == 1
                                            && um.data[1] != active
                                        {
                                            active = um.data[1];
                                            if let Some((t0, cmd, attack)) = pending.take() {
                                                let kind = if svc_time - t0 < 0.25 {
                                                    "switched at once"
                                                } else if attack.map(|a| svc_time - a < 0.25)
                                                    == Some(true)
                                                {
                                                    "switched after +attack (list was open)"
                                                } else {
                                                    "switched later, no +attack"
                                                };
                                                *counts
                                                    .entry(format!("{cmd}: {kind}"))
                                                    .or_default() += 1;
                                                if !kind.ends_with("at once") && window.is_none() {
                                                    println!(
                                                        "  {} {cmd:?} {kind}: switch at {:.2} to id {} (carrying {})",
                                                        clock(t0),
                                                        svc_time,
                                                        active,
                                                        ids(weapon_bits)
                                                    );
                                                }
                                            }
                                        }
                                        if name == "CurWeapon"
                                            && let Some((t0, ..)) = pending
                                            && svc_time - t0 > 5.0
                                        {
                                            pending = None;
                                        }
                                        let in_window =
                                            window.map(|(a, b)| svc_time >= a && svc_time <= b);
                                        if in_window == Some(true) {
                                            let d = &um.data;
                                            println!(
                                                "  {} {name} {:02x?}",
                                                clock(svc_time),
                                                &d[..d.len().min(8)]
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        println!("  counts: {counts:?}");
    }
}
