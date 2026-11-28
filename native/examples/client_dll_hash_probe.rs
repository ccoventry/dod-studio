//! Which `client.dll` build each demo's server was running (#1542).
//!
//! Every demo's first `svc_serverinfo` carries a 16-byte MD5 of the server's
//! `client.dll`. A modern 1.3 server sends zeros; an old one sends a real
//! hash, which names the DoD version once the hash is known
//! (`docs/dod_versions.md`).
//!
//! ```text
//! cargo run -p native --example client_dll_hash_probe -- <demo-or-folder>...
//! ```
//!
//! Prints one line per demo (file, network protocol, hash, version if known)
//! and a count per hash at the end. Reads the first 64 KB of each file and
//! finds the message by its shape, the way `native::maps::map_check` does for
//! the map checksum; once that reader is on `dev` this probe should call it.

#![allow(
    clippy::print_stdout,
    reason = "a dev-only probe: its output is the result"
)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const HEADER_LEN: usize = 544;
const READ_LEN: u64 = 64 * 1024;
const SVC_SERVER_INFO: u8 = 11;
const HASH_AT: usize = 13;
const MAX_PLAYERS_AT: usize = 29;
const PLAYER_INDEX_AT: usize = 30;
const GAME_DIR_AT: usize = 32;

/// Fingerprints with a version. Verified means the hash is of a real
/// `client.dll` file of that version; the rest are inferred from a demo's date
/// and wire format (`docs/dod_versions.md`).
const KNOWN: &[(&str, &str)] = &[
    (
        "2fd1fa826241265fde6417902a3d4790",
        "old wire format, probably 1.1c (unverified)",
    ),
    (
        "2046de1cb25d49529e2a3dfe65784d56",
        "1.3 wire format, probably 1.2 (unverified)",
    ),
];

/// The 16-byte hash and protocol of the first `svc_serverinfo`, if it reads as one.
fn server_info_hash(start: &[u8]) -> Option<(i32, [u8; 16])> {
    let body = start.get(HEADER_LEN..)?;
    for at in (0..body.len()).filter(|i| body[*i] == SVC_SERVER_INFO) {
        let m = &body[at..];
        let Some(protocol) = m
            .get(1..5)
            .and_then(|b| b.try_into().ok())
            .map(i32::from_le_bytes)
        else {
            continue;
        };
        let (Some(&max_players), Some(&slot)) = (m.get(MAX_PLAYERS_AT), m.get(PLAYER_INDEX_AT))
        else {
            continue;
        };
        if ![47, 48].contains(&protocol) || !(1..=32).contains(&max_players) || slot > max_players {
            continue;
        }
        let dir = m.get(GAME_DIR_AT..).unwrap_or(&[]);
        let dir = &dir[..dir.len().min(33)];
        let Some(len) = dir.iter().position(|b| *b == 0) else {
            continue;
        };
        if len == 0 || !dir[..len].iter().all(|b| b.is_ascii_graphic()) {
            continue;
        }
        if let Some(hash) = m.get(HASH_AT..HASH_AT + 16).and_then(|b| b.try_into().ok()) {
            return Some((protocol, hash));
        }
    }
    None
}

fn demos_in(target: &Path) -> Vec<PathBuf> {
    if !target.is_dir() {
        return vec![target.to_path_buf()];
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(target)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
        .collect();
    found.sort();
    found
}

fn main() {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for target in std::env::args().skip(1) {
        for demo in demos_in(Path::new(&target)) {
            let mut start = Vec::new();
            let read =
                std::fs::File::open(&demo).and_then(|f| f.take(READ_LEN).read_to_end(&mut start));
            let name = demo.file_name().unwrap_or_default().to_string_lossy();
            if let Err(e) = read {
                println!("{name}: {e}");
                continue;
            }
            let Some((protocol, hash)) = server_info_hash(&start) else {
                println!("{name}: no server info in the first 64 KB");
                continue;
            };
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            let label = if hash == [0; 16] {
                "zeros (modern server)"
            } else {
                KNOWN
                    .iter()
                    .find(|(h, _)| *h == hex)
                    .map_or("unknown", |(_, v)| v)
            };
            println!("{name}: protocol {protocol}, {hex}, {label}");
            *counts.entry(hex).or_default() += 1;
        }
    }
    println!();
    for (hex, n) in &counts {
        println!("{n:5}  {hex}");
    }
}
