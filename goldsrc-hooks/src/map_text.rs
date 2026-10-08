//! `dodstudio_hide_map_text`: hide the text a map puts on screen itself --
//! the anzio mortar warning, the round result -- and nothing else (#287).
//!
//! ## What carries it
//!
//! Measured offline across the whole local demo library
//! (`analysis/examples/map_text_probe.rs`): every map-authored line arrives as
//! the **`HudText` user message**. `svc_temp_entity`/`TE_TEXTMESSAGE` and
//! `svc_centerprint` carry nothing at all. Three kinds were seen:
//!
//! - `dod_score_ent`'s `message` -- the round result (`MAP_ALLIED_VICTORY2`),
//!   at the frames the win music plays;
//! - `env_message`'s `message` -- a hint or warning (`MAP_SPAWN_WARNING`, the
//!   anzio mortar warning near a spawn exit);
//! - on custom maps that skip the token, the English sentence itself.
//!
//! `HudText` carries the *token*, not the resolved sentence: `client.dll` looks
//! it up in `titles.txt` afterwards. So the payload can be compared against
//! the map's own keyvalue as bytes, with no localisation on either side, and
//! a map that writes the sentence straight into `message` matches the same way.
//!
//! ## Only the map's own
//!
//! DoD's clan-match prompts (`#Clan_allies_ready`, `#Clan_axis_ready`) share
//! the `HudText` channel and appear in nearly every match demo, so the channel
//! is not blanked. A message is hidden only when its token is a `message` the
//! loaded map's entity lump declares on one of [`TEXT_CLASSES`]; everything
//! else goes to the game untouched. No list of DoD's own messages to keep in
//! step with the game.
//!
//! ## Mechanism
//!
//! The one `deathmsg` and `msglog` already use: `pfnHookUserMsg` **prepends**,
//! so the engine's dispatcher stops at our record and `client.dll`'s own
//! handler stays reachable by calling its thunk directly. Nothing is patched.
//! See [`crate::engine::HookUserMsgFn`]. The thunk comes from `msglog`'s table,
//! which `tools/verify_msglog_table.py` checks against a real `client.dll`
//! (byte-identical across the stock, pre-Anniversary and 25th Anniversary
//! installs).
//!
//! When `dodstudio_debug_msglog` is also watching `HudText`, this does not
//! install its own record: two modules re-prepending the same message every
//! frame would each push the other off the head and grow the engine's list
//! without end. `msglog`'s handler asks [`hide`] instead, so the text is still
//! hidden, and still logged first.
//!
//! ## Where the map's strings come from
//!
//! The map's BSP on disk, `dod/<level>` (or `dod_downloads/<level>`), read once
//! per level while the cvar is on: only the 12-byte header and the entity lump,
//! not the whole file. Read in the per-frame poll, so it lands during the map
//! load rather than in the middle of a message.
//!
//! The patch-time route in `native/src/patch/map_text.rs` stays as the
//! fallback; its classname rule is the same one used here.

use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;

pub const NAME: &str = console_name!("hide_map_text");

/// The user message DoD puts map text on.
pub(crate) const MESSAGE: &str = "HudText";

/// Entities whose `message` is on-screen text. `ambient_generic` uses the same
/// key for a `.wav` path, which is why the classname decides rather than the
/// key. `game_text` is here for completeness: no demo in the library carries
/// one on `HudText`, but a string it declares is still the map's own.
const TEXT_CLASSES: [&[u8]; 3] = [b"dod_score_ent", b"env_message", b"game_text"];

/// Largest entity lump read. The biggest in a 249-map install is ~440 KB
/// (`dod_lennon4`); anything past this is not an entity lump.
const MAX_ENTITY_LUMP: usize = 2 * 1024 * 1024;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The cvar's value as last seen: what the message handler reads.
static WANTED: AtomicBool = AtomicBool::new(false);
/// Hash of the level [`TEXT`] was read for, or 0 before the first read.
static LOADED_LEVEL: AtomicU32 = AtomicU32::new(0);
/// The loaded map's short name and its declared strings.
static TEXT: RwLock<MapText> = RwLock::new(MapText {
    map: String::new(),
    strings: BTreeSet::new(),
});
/// `HudText` messages hidden this session.
static HIDDEN: AtomicU32 = AtomicU32::new(0);

struct MapText {
    map: String,
    strings: BTreeSet<Vec<u8>>,
}

/// Called by `commands.rs` once `dodstudio_hide_map_text` is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

// ── Pure parsing ─────────────────────────────────────────────────────────────

/// Trimmed of the NULs and whitespace a keyvalue or a wire string may carry.
/// Case is *not* folded: the token is looked up verbatim in `titles.txt`.
fn trim(bytes: &[u8]) -> &[u8] {
    let keep = |b: &u8| *b != 0 && !b.is_ascii_whitespace();
    let Some(start) = bytes.iter().position(keep) else {
        return &[];
    };
    let end = bytes.iter().rposition(keep).map_or(start, |e| e + 1);
    &bytes[start..end]
}

/// The entity lump's `(offset, length)` from a BSP's first 12 bytes (version,
/// then lump 0's offset and length), or `None` if they are not plausible.
fn entity_lump_bounds(header: &[u8]) -> Option<(u64, usize)> {
    let int = |at: usize| -> Option<i32> {
        header
            .get(at..at + 4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let offset = u64::try_from(int(4)?).ok()?;
    let len = usize::try_from(int(8)?).ok()?;
    (len > 0 && len <= MAX_ENTITY_LUMP).then_some((offset, len))
}

/// Every entity in an entity lump, as its `"key" "value"` pairs in order.
/// Anything malformed just yields less: an unterminated quote ends the parse.
fn parse_entities(text: &[u8]) -> Vec<Vec<(&[u8], &[u8])>> {
    let mut entities = Vec::new();
    let mut current: Option<Vec<(&[u8], &[u8])>> = None;
    let mut key: Option<&[u8]> = None;
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            b'{' => {
                current = Some(Vec::new());
                key = None;
            }
            b'}' => {
                if let Some(pairs) = current.take() {
                    entities.push(pairs);
                }
                key = None;
            }
            b'"' => {
                let Some(close) = text[i + 1..].iter().position(|&b| b == b'"') else {
                    break;
                };
                let token = &text[i + 1..i + 1 + close];
                i += close + 1;
                if let Some(pairs) = current.as_mut() {
                    match key.take() {
                        None => key = Some(token),
                        Some(k) => pairs.push((k, token)),
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    entities
}

/// Every on-screen string an entity lump declares: the `message` of each
/// entity whose classname is in [`TEXT_CLASSES`].
fn declared_text(lump: &[u8]) -> BTreeSet<Vec<u8>> {
    let mut out = BTreeSet::new();
    for pairs in parse_entities(lump) {
        let get = |name: &[u8]| {
            pairs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|&(_, v)| trim(v))
        };
        let Some(class) = get(b"classname") else {
            continue;
        };
        if !TEXT_CLASSES.iter().any(|c| class.eq_ignore_ascii_case(c)) {
            continue;
        }
        if let Some(message) = get(b"message").filter(|m| !m.is_empty()) {
            out.insert(message.to_vec());
        }
    }
    out
}

/// The token a `HudText` payload carries: its first string, up to the NUL.
/// DoD follows it with one style byte, which is not part of the text.
fn hud_text_token(payload: &[u8]) -> &[u8] {
    let end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    trim(&payload[..end])
}

/// Whether `payload` is a `HudText` carrying one of `declared`.
fn is_declared(declared: &BTreeSet<Vec<u8>>, payload: &[u8]) -> bool {
    let token = hud_text_token(payload);
    !token.is_empty() && declared.contains(token)
}

/// `maps/dod_anzio.bsp` -> `dod_anzio`.
fn short_name(level: &str) -> &str {
    let base = level.rsplit(['/', '\\']).next().unwrap_or(level);
    base.strip_suffix(".bsp")
        .or_else(|| base.strip_suffix(".BSP"))
        .unwrap_or(base)
}

// ── Engine side ──────────────────────────────────────────────────────────────

/// The engine's level name (`maps/dod_anzio.bsp`), or `None` without one.
fn level_name() -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    // Safety: a pointer into the engine's client state, valid for the
    // session; checked for null before reading.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    if raw.is_null() {
        return None;
    }
    let name = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    (!name.is_empty()).then_some(name)
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let hash = bytes.iter().fold(0x811c_9dc5_u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    // 0 is "nothing loaded yet".
    hash.max(1)
}

/// The entity lump of the BSP at `path`, reading only the header and the lump.
fn read_entity_lump(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    let Some((offset, len)) = entity_lump_bounds(&header) else {
        return Err(std::io::Error::other("no plausible entity lump"));
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut lump = vec![0u8; len];
    file.read_exact(&mut lump)?;
    Ok(lump)
}

/// Reads what `level` declares into [`TEXT`], and says so in the log.
fn load(level: &str) {
    let game_dir = crate::texture_hires::game_dir();
    let candidates = [
        game_dir.join(level),
        game_dir.with_file_name("dod_downloads").join(level),
    ];
    let map = short_name(level).to_ascii_lowercase();
    let lump = candidates.iter().find_map(|p| read_entity_lump(p).ok());
    let strings = lump.as_deref().map(declared_text).unwrap_or_default();
    let report = match &lump {
        None => format!(
            "map_text: could not read the entity lump of {level} -- nothing will be hidden on {map}"
        ),
        Some(_) => format!(
            "map_text: {map} declares {} on-screen string(s){}",
            strings.len(),
            if strings.is_empty() {
                String::new()
            } else {
                format!(
                    ": {}",
                    strings
                        .iter()
                        .map(|s| String::from_utf8_lossy(s).into_owned())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ),
    };
    if let Ok(mut text) = TEXT.write() {
        text.map = map;
        text.strings = strings;
    }
    unsafe { crate::debug::report(&report) };
}

/// Whether a `HudText` with this payload should be kept from the game, and
/// counts it if so. Also called from `msglog`'s handler -- see the module doc.
pub(crate) fn hide(payload: &[u8]) -> bool {
    if !WANTED.load(Ordering::Relaxed) {
        return false;
    }
    let hidden = TEXT
        .read()
        .map(|text| is_declared(&text.strings, payload))
        .unwrap_or(false);
    if hidden {
        HIDDEN.fetch_add(1, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "map_text: hid HudText \"{}\"",
                String::from_utf8_lossy(hud_text_token(payload))
            ))
        };
    }
    hidden
}

/// Our `HudText` handler: drops the message when [`hide`] says so, and
/// otherwise hands the game the same buffer, untouched.
unsafe extern "C" fn hooked_hud_text(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    let payload: &[u8] = if size > 0 && !buf.is_null() {
        unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
    } else {
        &[]
    };
    if hide(payload) {
        // 1 is what the engine's own dispatcher treats as handled.
        return 1;
    }
    match crate::msglog::original_thunk(MESSAGE) {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

/// Safe to call every frame: `pfnHookUserMsg` returns early when the head
/// record already carries this handler, and re-prepending is how the hook
/// survives the engine freeing the message list on disconnect. Same shape as
/// `deathmsg::install_hook`.
fn install_hook() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(name) = CString::new(MESSAGE) else {
        return;
    };
    unsafe { (engfuncs.pfn_hook_user_msg)(name.as_ptr(), hooked_hud_text) };
}

/// Called every frame from `commands::poll`. Follows the cvar, reads the
/// map's strings once per level while it is on, and keeps the hook in place.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if wanted != WANTED.swap(wanted, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "map_text: {NAME} = {}",
                if wanted {
                    "1 (map text hidden)"
                } else {
                    "0 (normal)"
                }
            ))
        };
    }
    // Off leaves any installed record in place: it forwards everything while
    // WANTED is false, and there is no unhook to ask the engine for.
    if !wanted {
        return;
    }
    let Some(level) = level_name() else {
        return;
    };
    let hash = fnv1a(level.as_bytes());
    if LOADED_LEVEL.swap(hash, Ordering::Relaxed) != hash {
        load(&level);
    }
    if !crate::msglog::watching(MESSAGE) {
        install_hook();
    }
}

/// A `dodstudio_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return None;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !wanted && hidden == 0 {
        return None;
    }
    let (map, count) = TEXT
        .read()
        .map(|t| (t.map.clone(), t.strings.len()))
        .unwrap_or_default();
    let map = if map.is_empty() {
        "no map read yet".to_string()
    } else {
        format!("{map} declares {count} on-screen string(s)")
    };
    Some(format!(
        "{NAME} = {} -- {map}; {hidden} hidden this session",
        if wanted { "1" } else { "0" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anzio's text entities, trimmed to the keys that matter -- the same
    /// fixture `native/src/patch/map_text.rs` tests the patch route with.
    const ANZIO: &[u8] = br#"
{
"classname" "worldspawn"
"skyname" "anzio"
}
{
"classname" "dod_score_ent"
"targetname" "allies"
"message" "MAP_ALLIED_VICTORY2"
"team" "1"
}
{
"classname" "dod_score_ent"
"targetname" "axis"
"message" "MAP_AXIS_VICTORY2"
"team" "2"
}
{
"classname" "env_message"
"targetname" "msg_leave_axis"
"message" "MAP_SPAWN_WARNING"
"spawnflags" "16"
}
{
"classname" "ambient_generic"
"targetname" "allies"
"message" "ambience/uswin.wav"
}
"#;

    fn set(items: &[&str]) -> BTreeSet<Vec<u8>> {
        items.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    /// A `HudText` payload as the server writes it: the string, its NUL, and
    /// DoD's one style byte.
    fn hud_text(text: &str) -> Vec<u8> {
        let mut out = text.as_bytes().to_vec();
        out.push(0);
        out.push(1);
        out
    }

    #[test]
    fn parses_every_entity_and_its_pairs_in_order() {
        let entities = parse_entities(ANZIO);
        assert_eq!(entities.len(), 5);
        assert_eq!(
            entities[3],
            vec![
                (&b"classname"[..], &b"env_message"[..]),
                (b"targetname", b"msg_leave_axis"),
                (b"message", b"MAP_SPAWN_WARNING"),
                (b"spawnflags", b"16"),
            ]
        );
    }

    #[test]
    fn collects_round_results_and_hints() {
        assert_eq!(
            declared_text(ANZIO),
            set(&[
                "MAP_ALLIED_VICTORY2",
                "MAP_AXIS_VICTORY2",
                "MAP_SPAWN_WARNING"
            ])
        );
    }

    /// `ambient_generic` uses `message` for a wav path; keying on the classname
    /// keeps a sound out of the list.
    #[test]
    fn an_ambient_generics_message_is_not_on_screen_text() {
        assert!(!declared_text(ANZIO).contains(&b"ambience/uswin.wav"[..]));
    }

    /// A custom map that skips the token and writes the sentence straight
    /// into `message` -- `chain_01.dem` carries exactly this.
    #[test]
    fn a_literal_sentence_is_declared_the_same_way() {
        let lump =
            br#"{ "classname" "dod_score_ent" "message" "Allies take control over the village!" }"#;
        assert_eq!(
            declared_text(lump),
            set(&["Allies take control over the village!"])
        );
    }

    #[test]
    fn an_empty_message_is_not_a_string_to_hide() {
        assert!(declared_text(br#"{ "classname" "env_message" "message" "" }"#).is_empty());
    }

    #[test]
    fn a_truncated_lump_yields_what_came_before_it() {
        let lump = br#"{ "classname" "env_message" "message" "MAP_SPAWN_WARNING" } { "classname" "env_message" "mess"#;
        assert_eq!(declared_text(lump), set(&["MAP_SPAWN_WARNING"]));
    }

    #[test]
    fn a_hud_text_token_stops_at_its_nul_and_drops_the_style_byte() {
        assert_eq!(
            hud_text_token(&hud_text("MAP_SPAWN_WARNING")),
            b"MAP_SPAWN_WARNING"
        );
        assert_eq!(hud_text_token(b"MAP_SPAWN_WARNING"), b"MAP_SPAWN_WARNING");
        assert_eq!(hud_text_token(b""), b"");
    }

    #[test]
    fn the_maps_own_text_is_hidden() {
        let declared = declared_text(ANZIO);
        assert!(is_declared(&declared, &hud_text("MAP_SPAWN_WARNING")));
        assert!(is_declared(&declared, &hud_text("MAP_ALLIED_VICTORY2")));
    }

    /// DoD's own clan-match prompts share `HudText` and no map declares them.
    /// This is the test that says they get through on purpose.
    #[test]
    fn dods_own_prompts_pass_through() {
        let declared = declared_text(ANZIO);
        assert!(!is_declared(&declared, &hud_text("#Clan_allies_ready")));
        assert!(!is_declared(&declared, &hud_text("#Clan_axis_ready")));
    }

    /// The token is looked up verbatim in `titles.txt`, so a differently-cased
    /// one is a different message.
    #[test]
    fn matching_is_case_sensitive() {
        let declared = declared_text(ANZIO);
        assert!(!is_declared(&declared, &hud_text("map_spawn_warning")));
    }

    #[test]
    fn an_empty_payload_is_never_hidden() {
        assert!(!is_declared(&set(&[""]), b""));
        assert!(!is_declared(&declared_text(ANZIO), &[0, 1]));
    }

    #[test]
    fn entity_lump_bounds_reads_lump_zero() {
        let mut header = Vec::new();
        header.extend(30i32.to_le_bytes());
        header.extend(124i32.to_le_bytes());
        header.extend(5000i32.to_le_bytes());
        assert_eq!(entity_lump_bounds(&header), Some((124, 5000)));
    }

    #[test]
    fn implausible_entity_lump_bounds_are_refused() {
        let header = |offset: i32, len: i32| {
            let mut h = 30i32.to_le_bytes().to_vec();
            h.extend(offset.to_le_bytes());
            h.extend(len.to_le_bytes());
            h
        };
        assert_eq!(entity_lump_bounds(&header(-1, 10)), None);
        assert_eq!(entity_lump_bounds(&header(124, 0)), None);
        assert_eq!(entity_lump_bounds(&header(124, -5)), None);
        assert_eq!(
            entity_lump_bounds(&header(124, MAX_ENTITY_LUMP as i32 + 1)),
            None
        );
        assert_eq!(entity_lump_bounds(&[0; 8]), None);
    }

    #[test]
    fn short_name_trims_the_folder_and_extension() {
        assert_eq!(short_name("maps/dod_anzio.bsp"), "dod_anzio");
        assert_eq!(short_name("maps\\dod_anzio.BSP"), "dod_anzio");
        assert_eq!(short_name("dod_anzio"), "dod_anzio");
    }

    #[test]
    fn the_level_hash_is_never_the_unloaded_sentinel() {
        assert_ne!(fnv1a(b""), 0);
        assert_ne!(fnv1a(b"maps/dod_anzio.bsp"), 0);
    }
}
