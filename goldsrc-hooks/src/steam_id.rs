//! SteamIDs: reading them from the text a player types, from the engine's
//! player record, and printing them in the classic form (#1205).
//!
//! Shared by the kill feed's block list (`deathmsg`), the name rules
//! (`player_names`), the spray lookup (`spray_hltv`) and the Players table
//! (`player_table`), so none of them reaches into another feature for it.
//! Pure apart from [`from_record`], which reads the engine's memory.

use std::ffi::{CStr, c_char};

/// SteamID64 of account number 0 in the public universe; an individual
/// account's SteamID64 is this plus its account number.
pub(crate) const BASE: u64 = 76_561_197_960_265_728;

/// A SteamID in any form a user is likely to paste: the 17-digit SteamID64,
/// `STEAM_X:Y:Z` (= base + 2Z + Y; X, the universe, is 0 or 1 depending on
/// the engine that printed it and does not change the account), or the newer
/// SteamID3 `[U:1:N]` (= base + N), with or without its brackets.
pub(crate) fn parse(text: &str) -> Option<u64> {
    let bare = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    if bare.len() > 4 && bare[..4].eq_ignore_ascii_case("U:1:") {
        let n: u64 = bare[4..].parse().ok()?;
        return n.checked_add(BASE);
    }
    if text.len() >= 6 && text[..6].eq_ignore_ascii_case("STEAM_") {
        let mut parts = text[6..].split(':');
        let (Some(universe), Some(y), Some(z), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        universe.parse::<u8>().ok()?;
        let y: u64 = y.parse().ok().filter(|y| *y <= 1)?;
        let z: u64 = z.parse().ok()?;
        return z
            .checked_mul(2)
            .and_then(|z2| z2.checked_add(y))
            .and_then(|n| n.checked_add(BASE));
    }
    if text.len() == 17 && text.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse().ok();
    }
    None
}

/// A SteamID64 in the `STEAM_0:Y:Z` form GoldSrc's own `status` prints, the
/// one players and server admins recognise (#537). `None` for an id that
/// isn't a player's own account, such as the HLTV proxy's: that form only
/// exists for individual accounts, base + a 32-bit account number.
pub(crate) fn classic(id: u64) -> Option<String> {
    let account = id.checked_sub(BASE).filter(|&a| a <= u64::from(u32::MAX))?;
    Some(format!("STEAM_0:{}:{}", account % 2, account / 2))
}

/// Bytes of a player's userinfo string (`MAX_INFO_STRING`).
const USERINFO_LEN: usize = 256;

/// The SteamID in the userinfo stored just before a player's name.
///
/// The engine's player record (0x250 bytes, the same on both `hw.dll`
/// builds) is `userid`, then `userinfo[256]`, then `name[32]`, and
/// `pfnGetPlayerInfo` hands out a pointer to `name`. So the userinfo is the
/// 256 bytes before it. It's only trusted when it really is this player's:
/// its `name` key must equal the name. Otherwise 0.
///
/// Safety: `name` is the pointer `pfnGetPlayerInfo` returned, into the
/// engine's static player array, whose earlier bytes are always mapped.
pub(crate) unsafe fn from_record(name: *const c_char) -> u64 {
    if name.is_null() {
        return 0;
    }
    let userinfo =
        unsafe { std::slice::from_raw_parts((name as *const u8).sub(USERINFO_LEN), USERINFO_LEN) };
    let name = unsafe { CStr::from_ptr(name) }.to_bytes();
    sid_from_userinfo(userinfo, name).unwrap_or(0)
}

/// `*sid` from a `\key\value\...` userinfo buffer, if its `name` is `name`.
fn sid_from_userinfo(buffer: &[u8], name: &[u8]) -> Option<u64> {
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    let text = buffer[..end].strip_prefix(b"\\")?;
    let mut fields = text.split(|&b| b == b'\\');
    let mut sid = None;
    let mut name_ok = false;
    while let (Some(key), Some(value)) = (fields.next(), fields.next()) {
        match key {
            b"name" => name_ok = value == name,
            b"*sid" => sid = std::str::from_utf8(value).ok()?.parse::<u64>().ok(),
            _ => {}
        }
    }
    sid.filter(|&id| name_ok && id != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example from #468: STEAM_0:0:7654321.
    const ME: u64 = 76_561_197_975_574_370;

    #[test]
    fn only_player_accounts_get_the_classic_form() {
        assert_eq!(
            classic(76_561_197_962_734_863).as_deref(),
            Some("STEAM_0:1:1234567")
        );
        // The HLTV proxy's id from a real demo's `players` list: not an
        // individual account, so no STEAM_0 form (#539 live test).
        assert_eq!(classic(90_071_996_842_377_220), None);
        assert_eq!(classic(12), None);
    }

    #[test]
    fn the_steam_id_comes_from_the_matching_userinfo_only() {
        // As recorded in wsod25_grp5_h1_hltv.dem.
        let mut record = [0u8; USERINFO_LEN];
        let info = b"\\model\\us-inf\\name\\dicE[: :]m00cat :D\\*sid\\76561197962734863\\*hltv\\0";
        record[..info.len()].copy_from_slice(info);
        assert_eq!(
            sid_from_userinfo(&record, b"dicE[: :]m00cat :D"),
            Some(76_561_197_962_734_863)
        );
        assert_eq!(
            sid_from_userinfo(&record, b"someone else"),
            None,
            "not this player's"
        );
        assert_eq!(sid_from_userinfo(&[0u8; 16], b"x"), None, "empty");
        assert_eq!(
            sid_from_userinfo(b"\\name\\bot\\*sid\\0", b"bot"),
            None,
            "a bot's 0"
        );
        assert_eq!(
            sid_from_userinfo(b"name\\bot", b"bot"),
            None,
            "not userinfo"
        );
    }

    #[test]
    fn steam_ids_parse_in_both_forms() {
        assert_eq!(parse("76561197975574370"), Some(ME));
        assert_eq!(parse("STEAM_0:0:7654321"), Some(ME));
        assert_eq!(parse("steam_1:0:7654321"), Some(ME));
        assert_eq!(parse("STEAM_0:1:7654321"), Some(ME + 1));
        assert_eq!(parse("STEAM_0:2:7654321"), None);
        assert_eq!(parse("STEAM_0:0"), None);
        // SteamID3: account number 15308642 = 2 * 7654321.
        assert_eq!(parse("[U:1:15308642]"), Some(ME));
        assert_eq!(parse("U:1:15308642"), Some(ME));
        assert_eq!(parse("[u:1:15308642]"), Some(ME));
        assert_eq!(parse("[U:1:]"), None);
        assert_eq!(parse("[G:1:15308642]"), None, "a group, not a user");
        assert_eq!(parse("STEAM_0:0:1:2"), None);
        assert_eq!(parse("7656119797793012"), None, "16 digits");
        assert_eq!(parse("12"), None);
    }
}
