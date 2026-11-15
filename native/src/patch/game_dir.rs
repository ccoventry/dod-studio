//! The game folder a patched demo says it was recorded in (#402).
//!
//! A demo names its game folder twice: in the file header (`game_directory`,
//! 260 bytes) and in every `svc_serverinfo` message. The engine refuses to
//! play a demo whose `svc_serverinfo` folder differs from its own `-game`
//! ("Server is running game dod.  Restart in that game to connect."), so a
//! copy of DoD such as `dod_movies` could play nothing. Every copy the
//! patcher writes gets the running game folder in both places. The hook DLL
//! skips the same check for original demos (`goldsrc-hooks/src/game_dir_check.rs`).

/// `[u8; 260]` at the header's `game_directory`: magic (8), demo protocol
/// (4), network protocol (4), map name (260).
const HEADER_GAME_DIR: std::ops::Range<usize> = 276..536;
/// The header's network protocol (48 for DoD 1.3).
const HEADER_NETWORK_PROTOCOL: std::ops::Range<usize> = 12..16;
const SVC_SERVERINFO: u8 = 11;
/// From the message id to the game folder string: protocol, spawn count,
/// map checksum (4 each), client.dll hash (16), max players, player index,
/// deathmatch (1 each). Mirrors `demo_split::TO_GAME_DIR`.
const TO_GAME_DIR: usize = 1 + 4 + 4 + 4 + 16 + 3;
/// A game folder is a plain folder name; the engine's own buffer is 260.
const MAX_GAME_DIR: usize = 64;

/// The demo's network protocol, from its header.
pub fn header_network_protocol(header: &[u8]) -> Option<i32> {
    let b = header.get(HEADER_NETWORK_PROTOCOL)?;
    Some(i32::from_le_bytes(b.try_into().ok()?))
}

/// The game folder the header names, NUL-trimmed.
pub fn header_game_dir(header: &[u8]) -> Option<&[u8]> {
    let field = header.get(HEADER_GAME_DIR)?;
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    Some(&field[..end])
}

/// Writes `folder` into the header's `game_directory`, NUL-padded. Returns
/// whether the header changed. A header too short to hold the field, or a
/// folder that would not fit, is left alone.
pub fn set_header_game_dir(header: &mut [u8], folder: &str) -> bool {
    let bytes = folder.as_bytes();
    if bytes.is_empty() || bytes.len() >= HEADER_GAME_DIR.len() {
        return false;
    }
    if header_game_dir(header) == Some(bytes) {
        return false;
    }
    let Some(field) = header.get_mut(HEADER_GAME_DIR) else {
        return false;
    };
    field.fill(0);
    field[..bytes.len()].copy_from_slice(bytes);
    true
}

fn printable(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(|&b| (0x20..0x7f).contains(&b))
}

/// Where each `svc_serverinfo` game folder string sits in `msg`: `(start,
/// end)` of the string without its NUL. Recognised like
/// `demo_split::quick_maps`: the id and the protocol, then a printable
/// folder name where one belongs.
fn server_info_game_dirs(msg: &[u8], network_protocol: i32) -> Vec<(usize, usize)> {
    let mut needle = vec![SVC_SERVERINFO];
    needle.extend_from_slice(&network_protocol.to_le_bytes());
    let mut found = Vec::new();
    let mut at = 0;
    while at + needle.len() <= msg.len() {
        let Some(i) = msg[at..].windows(needle.len()).position(|w| w == needle) else {
            break;
        };
        let start = at + i + TO_GAME_DIR;
        at = at + i + 1;
        let Some(rest) = msg.get(start..) else {
            continue;
        };
        let Some(len) = rest.iter().take(MAX_GAME_DIR + 1).position(|&b| b == 0) else {
            continue;
        };
        if !printable(&rest[..len]) {
            continue;
        }
        found.push((start, start + len));
        at = start + len;
    }
    found
}

/// `msg` with every `svc_serverinfo` game folder replaced by `folder`, or
/// `None` when nothing in it needs changing. A changed message is a
/// different length, so the caller rewrites the frame's length and shifts
/// the directory by the difference.
pub fn rewrite_server_info_game_dir(
    msg: &[u8],
    network_protocol: i32,
    folder: &str,
) -> Option<Vec<u8>> {
    let bytes = folder.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_GAME_DIR {
        return None;
    }
    let spots: Vec<(usize, usize)> = server_info_game_dirs(msg, network_protocol)
        .into_iter()
        .filter(|&(s, e)| &msg[s..e] != bytes)
        .collect();
    if spots.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(msg.len() + spots.len() * bytes.len());
    let mut from = 0;
    for (s, e) in spots {
        out.extend_from_slice(&msg[from..s]);
        out.extend_from_slice(bytes);
        from = e;
    }
    out.extend_from_slice(&msg[from..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_with(folder: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; 544];
        h[..8].copy_from_slice(b"HLDEMO\0\0");
        h[12..16].copy_from_slice(&48i32.to_le_bytes());
        h[276..276 + folder.len()].copy_from_slice(folder);
        h
    }

    fn server_info(folder: &[u8]) -> Vec<u8> {
        let mut m = vec![SVC_SERVERINFO];
        m.extend_from_slice(&48i32.to_le_bytes());
        m.extend_from_slice(&7i32.to_le_bytes());
        m.extend_from_slice(&0x1234i32.to_le_bytes());
        m.extend_from_slice(&[0xAB; 16]);
        m.extend_from_slice(&[32, 1, 1]);
        m.extend_from_slice(folder);
        m.push(0);
        m.extend_from_slice(b"KTP server\0maps/dod_anzio.bsp\0mapcycle.txt\0");
        m.push(0);
        m
    }

    #[test]
    fn header_folder_is_read_and_replaced() {
        let mut h = header_with(b"dod");
        assert_eq!(header_network_protocol(&h), Some(48));
        assert_eq!(header_game_dir(&h), Some(&b"dod"[..]));
        assert!(set_header_game_dir(&mut h, "dod_movies"));
        assert_eq!(header_game_dir(&h), Some(&b"dod_movies"[..]));
        assert_eq!(h.len(), 544);
        // A second write of the same name changes nothing.
        assert!(!set_header_game_dir(&mut h, "dod_movies"));
        // Shorter names leave no tail of the old one behind.
        assert!(set_header_game_dir(&mut h, "dod"));
        assert_eq!(header_game_dir(&h), Some(&b"dod"[..]));
        assert_eq!(&h[279..536], &[0u8; 257][..]);
    }

    #[test]
    fn header_too_short_or_name_too_long_is_left_alone() {
        let mut short = vec![0u8; 100];
        assert!(!set_header_game_dir(&mut short, "dod_movies"));
        let mut h = header_with(b"dod");
        assert!(!set_header_game_dir(&mut h, &"x".repeat(260)));
        assert!(!set_header_game_dir(&mut h, ""));
        assert_eq!(header_game_dir(&h), Some(&b"dod"[..]));
    }

    #[test]
    fn server_info_folder_is_replaced_in_place() {
        let mut msg = b"\x08\x00hello\0".to_vec(); // an svc_print before it
        msg.extend_from_slice(&server_info(b"dod"));
        msg.extend_from_slice(&[0x0C, 1, 2, 3]); // something after it
        let out = rewrite_server_info_game_dir(&msg, 48, "dod_movies").expect("changed");
        assert_eq!(out.len(), msg.len() + "dod_movies".len() - 3);
        let mut want = b"\x08\x00hello\0".to_vec();
        want.extend_from_slice(&server_info(b"dod_movies"));
        want.extend_from_slice(&[0x0C, 1, 2, 3]);
        assert_eq!(out, want);
        // Already right: nothing to do.
        assert!(rewrite_server_info_game_dir(&out, 48, "dod_movies").is_none());
        // Back to dod works too.
        assert_eq!(
            rewrite_server_info_game_dir(&out, 48, "dod").as_deref(),
            Some(&msg[..])
        );
    }

    #[test]
    fn two_server_infos_in_one_message_both_change() {
        let mut msg = server_info(b"dod");
        msg.extend_from_slice(&server_info(b"dod"));
        let out = rewrite_server_info_game_dir(&msg, 48, "dod2").expect("changed");
        let mut want = server_info(b"dod2");
        want.extend_from_slice(&server_info(b"dod2"));
        assert_eq!(out, want);
    }

    /// A real recording: the header says `dod`, and the rename finds its
    /// svc_serverinfo and leaves the map where `demo_split` looks for it.
    #[test]
    fn ci_fixture_is_renamed_and_still_scans() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).expect("test-fixtures/ci_fixture.dem");
        assert_eq!(header_game_dir(&bytes), Some(&b"dod"[..]));
        let protocol = header_network_protocol(&bytes).expect("protocol");
        let mut renamed = rewrite_server_info_game_dir(&bytes, protocol, "dod_movies")
            .expect("the fixture's svc_serverinfo says dod");
        assert_eq!(renamed.len(), bytes.len() + "dod_movies".len() - 3);
        assert!(set_header_game_dir(&mut renamed, "dod_movies"));
        assert_eq!(
            crate::demo_split::quick_maps(&renamed, protocol),
            crate::demo_split::quick_maps(&bytes, protocol)
        );
    }

    #[test]
    fn a_message_without_server_info_is_untouched() {
        let msg = b"\x08\x00hello\0\x0B\x30\x00\x00".to_vec(); // id + a cut-off protocol
        assert!(rewrite_server_info_game_dir(&msg, 48, "dod_movies").is_none());
        assert!(rewrite_server_info_game_dir(&[], 48, "dod_movies").is_none());
        // A stray id + protocol followed by junk, not a printable folder.
        let mut stray = vec![SVC_SERVERINFO];
        stray.extend_from_slice(&48i32.to_le_bytes());
        stray.extend_from_slice(&[0xFF; 40]);
        assert!(rewrite_server_info_game_dir(&stray, 48, "dod_movies").is_none());
        // The other protocol does not match.
        assert!(rewrite_server_info_game_dir(&server_info(b"dod"), 47, "dod_movies").is_none());
    }
}
