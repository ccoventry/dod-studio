//! `ds_player_change_name` (#1021): rename a player everywhere their name
//! shows during playback -- the kill feed, the scoreboard, the aim-at-teammate
//! name, the voice icon, chat, and the join and team lines -- by SteamID, slot
//! or `self`, for one demo, a few, or all of them. The demo files stay as they
//! are.
//!
//! ```text
//! ds_player_change_name add id <slot | SteamID | self> [in <demo>[,<demo>...]] to <new name>
//! ds_player_change_name list
//! ds_player_change_name remove <n>
//! ds_player_change_name clear
//! ```
//!
//! ## Where the names come from
//!
//! Almost every name DoD draws goes through one engine function,
//! `pfnGetPlayerInfo`: `client.dll`'s copy of the engine table, slot 21, 19
//! call sites (the R&D on #1021). So that copy's slot is detoured: the detour
//! asks the engine, then swaps the name pointer for one this module owns when
//! the slot is renamed. `client.dll` keeps the pointer (the scoreboard stores
//! it in `g_PlayerInfoList`), so every name handed out is leaked and never
//! freed, and a `remove` or `clear` leaves it valid.
//!
//! The hook's own lookups (`deathmsg`'s SteamIDs, `spray_hltv`, this module's
//! own resolution) go through the engine's table ([`engine::engfuncs`]), not
//! `client.dll`'s copy, so they always see the real name and the real player
//! record behind it.
//!
//! Two places get the name another way:
//! - **SayText**: the server wrote the name into the text. The client colours
//!   it by finding `GetPlayerInfo`'s name inside the line (`client+0x4609a`),
//!   so the text is rewritten here and the detour supplies the same name, and
//!   the colour follows.
//! - **TextMsg** join and team lines (`#game_joined_game\0<name>`): an
//!   argument equal to a renamed player's real name is swapped, in the filter
//!   `clan_text` already keeps at the front of the engine's list.
//!
//! A name may be up to [`MAX_NAME_BYTES`] long: the kill feed's own buffers are
//! 64 bytes, but `MsgFunc_DeathMsg` copies at most 31, so a longer name lifts
//! that cap (`deathmsg::lift_name_cap`). Every other place a name reaches
//! formats it into a buffer of 128 bytes or more, or a bounded copy (checked
//! offline at each of the 19 call sites).
//!
//! ## Once per frame, then lock-free
//!
//! [`poll`] (from `HUD_Frame`) works out who each slot is -- its SteamID from
//! the userinfo, whether it's the recording player, the loaded demo's name --
//! and which rename wins, and publishes the result per slot in atomics: the
//! new name, and a hash of the real name it was resolved against. The detour
//! and the message filters only read those. A slot whose real name no longer
//! matches its hash (someone else took the slot, or the player renamed in the
//! demo) keeps its real name until the next frame resolves it again.
//!
//! Which rename wins for a player: one with `in` over one without, then the
//! one added last.

// Only the 32-bit build installs the detour; a host build still runs the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, RwLock};

use crate::engine::{self, GetPlayerInfoFn, HudPlayerInfo};
use crate::names::console_name;

pub const NAME: &str = console_name!("player_change_name");

/// The longest new name, in bytes: the kill feed's `szKiller[64]` and
/// `szVictim[64]` with their terminator.
pub const MAX_NAME_BYTES: usize = 63;

/// What the engine and the stock kill feed hold: `name[32]`, terminator
/// included.
const ENGINE_NAME_BYTES: usize = 31;

/// Player slots, 1-based.
const MAX_SLOTS: usize = 32;

/// `pfnGetPlayerInfo`'s slot in `cl_enginefunc_t`.
const ENGFUNCS_SLOT_GET_PLAYER_INFO: usize = 21;

const _: () = assert!(
    std::mem::offset_of!(engine::ClEngineFuncsPartial, pfn_get_player_info)
        == ENGFUNCS_SLOT_GET_PLAYER_INFO * std::mem::size_of::<usize>(),
    "pfnGetPlayerInfo is not slot 21 of ClEngineFuncsPartial"
);

const SELF_WARNING: &str =
    "self is a different player in each POV demo; add `in <demo>` or use a SteamID";

// ── The command, pure ────────────────────────────────────────────────────────

/// Who a rename is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Id {
    Slot(i32),
    /// SteamID64, matched against the slot's userinfo `*sid`.
    Steam(u64),
    /// The recording player of a POV demo; nobody in an HLTV demo.
    OwnPov,
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Id::Slot(slot) => write!(f, "{slot}"),
            Id::Steam(id) => match crate::deathmsg::classic_steam_id(*id) {
                Some(classic) => f.write_str(&classic),
                None => write!(f, "{id}"),
            },
            Id::OwnPov => f.write_str("self"),
        }
    }
}

/// One `add`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rule {
    id: Id,
    /// Demo file names or `*` patterns, lowercase, without `.dem`. Empty:
    /// every demo.
    demos: Vec<String>,
    name: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Add(Rule),
    List,
    /// 1-based, as `list` numbers them.
    Remove(usize),
    Clear,
}

/// A parsed command and the warnings it earns.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    command: Command,
    warnings: Vec<String>,
}

/// Puts back what GoldSrc's tokenizer split: it makes `:` and `,` tokens of
/// their own, so `STEAM_0:1:123` arrives as five pieces and `a,b` as three
/// (`goldsrc-misc-facts`: hw+0x2b18b). A `:` or `,` is glued to the token
/// before it and the one after. A quoted argument arrives whole, so a name in
/// quotes is never touched.
///
/// Beside each token: whether it was glued from several, so `add` can warn
/// that an unquoted name lost any spaces around its `:` or `,`.
fn rejoin(tokens: &[String]) -> (Vec<String>, Vec<bool>) {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut glued: Vec<bool> = Vec::with_capacity(tokens.len());
    let mut glue_next = false;
    for token in tokens {
        let separator = token == ":" || token == ",";
        match (out.last_mut(), glued.last_mut()) {
            (Some(last), Some(mark)) if separator => {
                last.push_str(token);
                *mark = true;
                glue_next = true;
                continue;
            }
            (Some(last), Some(_)) if glue_next => last.push_str(token),
            _ => {
                out.push(token.clone());
                glued.push(false);
            }
        }
        glue_next = false;
    }
    (out, glued)
}

/// `self`, a SteamID in any form `ds_hud_deathmsg block` takes, or a slot.
fn parse_id(text: &str) -> Result<Id, String> {
    if text.eq_ignore_ascii_case("self") {
        return Ok(Id::OwnPov);
    }
    if let Some(id) = crate::deathmsg::parse_steam_id(text) {
        return Ok(Id::Steam(id));
    }
    match text.parse::<i32>() {
        Ok(slot) if text.len() <= 2 && (1..=MAX_SLOTS as i32).contains(&slot) => Ok(Id::Slot(slot)),
        _ => Err(format!(
            "`{text}` is not a slot (1-32), a SteamID (STEAM_0:x:y, [U:1:n] or 7656119...) or self"
        )),
    }
}

/// A demo name as `in` and the loaded demo are compared: the file name only,
/// without `.dem`, lowercase.
fn demo_key(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let lower = file.to_ascii_lowercase();
    match lower.strip_suffix(".dem") {
        Some(stem) => stem.to_string(),
        None => lower,
    }
}

fn parse_demos(text: &str) -> Result<Vec<String>, String> {
    let demos: Vec<String> = text
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(demo_key)
        .filter(|d| !d.is_empty())
        .collect();
    if demos.is_empty() {
        return Err("`in` needs a demo name, a comma list of them, or a * pattern".to_string());
    }
    Ok(demos)
}

fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("`to` needs a name".to_string());
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(format!(
            "the name is {} bytes; the kill feed holds at most {MAX_NAME_BYTES}",
            name.len()
        ));
    }
    // The game passes names through printf-style formatting in places (old
    // servers strip `%` from player names for that reason).
    if name.contains('%') {
        return Err("a name can't contain %: the game can read it as a format code".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("a name can't contain control characters".to_string());
    }
    Ok(())
}

/// `add`'s named parameters, in any order. `glued` is [`rejoin`]'s mark for
/// each of `args`.
fn parse_add(args: &[String], glued: &[bool]) -> Result<Parsed, String> {
    let mut id = None;
    let mut demos = None;
    let mut name: Option<String> = None;
    let mut name_glued = false;
    let mut after_to = false;
    let mut i = 0;
    while i < args.len() {
        let word = args[i].to_ascii_lowercase();
        let value = args.get(i + 1);
        match word.as_str() {
            "id" | "in" | "to" => {
                let Some(value) = value else {
                    return Err(match word.as_str() {
                        "id" => "`id` needs a slot, a SteamID or self".to_string(),
                        "in" => "`in` needs a demo name, a comma list of them, or a * pattern"
                            .to_string(),
                        _ => "`to` needs a name".to_string(),
                    });
                };
                let given_twice = match word.as_str() {
                    "id" => id.replace(parse_id(value)?).is_some(),
                    "in" => demos.replace(parse_demos(value)?).is_some(),
                    _ => name.replace(value.clone()).is_some(),
                };
                if given_twice {
                    return Err(format!("`{word}` is given twice"));
                }
                after_to = word == "to";
                if after_to {
                    name_glued = glued.get(i + 1).copied().unwrap_or(false);
                }
                i += 2;
            }
            _ => {
                let hint = if after_to {
                    " -- a name with spaces, or with any of : , ( ) { } ', goes in quotes"
                } else {
                    ""
                };
                return Err(format!("unknown word `{}`{hint}", args[i]));
            }
        }
    }
    let (Some(id), Some(name)) = (id, name) else {
        return Err("add needs `id <slot | SteamID | self>` and `to <new name>`".to_string());
    };
    check_name(&name)?;
    let demos = demos.unwrap_or_default();
    let mut warnings = Vec::new();
    if demos.is_empty() {
        match id {
            Id::OwnPov => warnings.push(SELF_WARNING.to_string()),
            Id::Slot(slot) => warnings.push(format!(
                "slot {slot} is a different player in each demo; add `in <demo>` or use a SteamID"
            )),
            Id::Steam(_) => {}
        }
    }
    // The console splits an unquoted `dicE[: :]Alex` into `dicE[`, `:`, `:`
    // and `]Alex`, so the space is gone before the hook sees it.
    if name_glued {
        warnings.push(format!(
            "the name was typed without quotes, so the console dropped any spaces around its : or , and it reads \"{name}\"; put it in quotes to keep it as typed"
        ));
    }
    Ok(Parsed {
        command: Command::Add(Rule { id, demos, name }),
        warnings,
    })
}

fn no_more(verb: &str, rest: &[String]) -> Result<(), String> {
    match rest.first() {
        Some(extra) => Err(format!("unknown word `{extra}` after `{verb}`")),
        None => Ok(()),
    }
}

/// The arguments after the command name.
fn parse(raw: &[String]) -> Result<Parsed, String> {
    let (tokens, glued) = rejoin(raw);
    let Some((verb, rest)) = tokens.split_first() else {
        return Ok(Parsed {
            command: Command::List,
            warnings: Vec::new(),
        });
    };
    let plain = |command| {
        Ok(Parsed {
            command,
            warnings: Vec::new(),
        })
    };
    match verb.to_ascii_lowercase().as_str() {
        "add" => parse_add(rest, &glued[1..]),
        "list" => no_more("list", rest).and_then(|()| plain(Command::List)),
        "clear" => no_more("clear", rest).and_then(|()| plain(Command::Clear)),
        "remove" => {
            let Some((n, more)) = rest.split_first() else {
                return Err("remove needs the number `list` shows".to_string());
            };
            no_more("remove", more)?;
            match n.parse::<usize>() {
                Ok(n) if n >= 1 => plain(Command::Remove(n)),
                _ => Err(format!("`{n}` is not a number from `list`")),
            }
        }
        _ => Err(format!(
            "unknown word `{verb}` -- the subcommands are add, list, remove and clear"
        )),
    }
}

/// `*` matches any run of characters, everything else itself. Both sides are
/// already lowercase.
fn glob(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

impl Rule {
    /// Whether this rule is in force for the demo playing (its key, or
    /// `None` when no demo plays).
    fn applies_to(&self, demo: Option<&str>) -> bool {
        if self.demos.is_empty() {
            return true;
        }
        demo.is_some_and(|demo| self.demos.iter().any(|p| glob(p, demo)))
    }

    fn describe(&self) -> String {
        let demos = if self.demos.is_empty() {
            String::new()
        } else {
            format!(" in {}", self.demos.join(","))
        };
        format!("id {}{demos} to \"{}\"", self.id, self.name)
    }
}

/// Who is in a slot right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Who {
    slot: i32,
    /// 0 when unknown (a bot, or no `*sid`).
    steam_id: u64,
    own_pov: bool,
}

impl Id {
    fn matches(self, who: &Who) -> bool {
        match self {
            Id::Slot(slot) => slot == who.slot,
            Id::Steam(id) => id != 0 && id == who.steam_id,
            Id::OwnPov => who.own_pov,
        }
    }
}

/// The index of the rename that wins for `who`: one with `in` over one
/// without, then the one added last.
fn pick(rules: &[&Rule], who: &Who, demo: Option<&str>) -> Option<usize> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.id.matches(who) && r.applies_to(demo))
        .max_by_key(|(i, r)| (!r.demos.is_empty(), *i))
        .map(|(i, _)| i)
}

/// `text` with its first `old` swapped for `new`, or `None` when `old` isn't
/// in it.
fn replace_first(text: &[u8], old: &[u8], new: &[u8]) -> Option<Vec<u8>> {
    if old.is_empty() || old.len() > text.len() {
        return None;
    }
    let at = text.windows(old.len()).position(|w| w == old)?;
    let mut out = Vec::with_capacity(text.len() - old.len() + new.len());
    out.extend_from_slice(&text[..at]);
    out.extend_from_slice(new);
    out.extend_from_slice(&text[at + old.len()..]);
    Some(out)
}

/// A `SayText` payload (sender slot, then the line) with the sender's real
/// name swapped for `new` in the line. `None` when the name isn't in it.
fn rewrite_say_text(payload: &[u8], old: &[u8], new: &[u8]) -> Option<Vec<u8>> {
    let (&sender, rest) = payload.split_first()?;
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    let line = replace_first(&rest[..end], old, new)?;
    let mut out = Vec::with_capacity(1 + line.len() + rest.len() - end + 1);
    out.push(sender);
    out.extend_from_slice(&line);
    if end < rest.len() {
        out.extend_from_slice(&rest[end..]);
    } else {
        out.push(0);
    }
    Some(out)
}

/// A `TextMsg` payload (destination, the text or key, then its string
/// arguments) with every argument equal to one of `renames`' real names
/// swapped for its new one. `None` when nothing changed. The text itself is
/// left alone: the server's own words, not a name.
fn rewrite_text_msg(payload: &[u8], renames: &[(&[u8], &[u8])]) -> Option<Vec<u8>> {
    let (&dest, rest) = payload.split_first()?;
    let mut parts: Vec<&[u8]> = rest.split(|&b| b == 0).collect();
    // The last piece is what follows the final terminator: empty for a
    // well-formed message.
    let trailing = parts.pop().unwrap_or_default();
    let mut changed = false;
    let mut out = vec![dest];
    for (i, part) in parts.iter().enumerate() {
        let swap = (i > 0)
            .then(|| {
                renames
                    .iter()
                    .find(|(old, _)| !old.is_empty() && old == part)
            })
            .flatten();
        match swap {
            Some((_, new)) => {
                out.extend_from_slice(new);
                changed = true;
            }
            None => out.extend_from_slice(part),
        }
        out.push(0);
    }
    out.extend_from_slice(trailing);
    changed.then_some(out)
}

/// FNV-1a: the real name a slot was resolved against.
fn name_hash(name: &[u8]) -> u32 {
    name.iter().fold(0x811c_9dc5_u32, |h, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

// ── State ────────────────────────────────────────────────────────────────────

/// One rename, with its name as the C string the detour hands out.
struct Entry {
    rule: Rule,
    name: &'static CStr,
}

/// Written by the command, read once per frame by [`poll`] (a `try_read`, so
/// the frame never waits).
static RULES: RwLock<Vec<Entry>> = RwLock::new(Vec::new());
/// Whether [`RULES`] holds anything: lets [`poll`] skip the lock.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether any slot has a name published, so a `clear` is followed by one
/// frame that takes them all back.
static PUBLISHED: AtomicBool = AtomicBool::new(false);

/// Every new name ever handed out, so a name added again reuses its string.
/// Only the command touches it. Never freed: `client.dll` keeps the pointers.
static NAME_POOL: Mutex<Vec<&'static CStr>> = Mutex::new(Vec::new());

/// Per slot (index 0 unused): the new name, or null.
static NEW_NAME: [AtomicPtr<c_char>; MAX_SLOTS + 1] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; MAX_SLOTS + 1];
/// Per slot: [`name_hash`] of the real name [`NEW_NAME`] was resolved
/// against.
static REAL_HASH: [AtomicU32; MAX_SLOTS + 1] = [const { AtomicU32::new(0) }; MAX_SLOTS + 1];

/// `client.dll`'s `pfnGetPlayerInfo` before the detour: the engine's.
static REAL_GET_PLAYER_INFO: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static DETOUR_INSTALLED: AtomicBool = AtomicBool::new(false);
static DETOUR_FAILED: AtomicBool = AtomicBool::new(false);

fn intern(name: &str) -> Option<&'static CStr> {
    let c = CString::new(name).ok()?;
    let mut pool = NAME_POOL.lock().ok()?;
    if let Some(found) = pool.iter().find(|p| p.to_bytes() == c.as_bytes()) {
        return Some(found);
    }
    let leaked: &'static CStr = Box::leak(c.into_boxed_c_str());
    pool.push(leaked);
    Some(leaked)
}

/// Sets what the detour hands out for `slot`. The name goes null first and
/// is set last, so a reader never pairs a new name with an old hash.
fn publish(slot: usize, desired: Option<(u32, *const c_char)>) {
    let current = NEW_NAME[slot].load(Ordering::Acquire);
    match desired {
        None => {
            if !current.is_null() {
                NEW_NAME[slot].store(std::ptr::null_mut(), Ordering::Release);
            }
        }
        Some((hash, name)) => {
            if std::ptr::eq(current, name) && REAL_HASH[slot].load(Ordering::Acquire) == hash {
                return;
            }
            NEW_NAME[slot].store(std::ptr::null_mut(), Ordering::Release);
            REAL_HASH[slot].store(hash, Ordering::Release);
            NEW_NAME[slot].store(name as *mut c_char, Ordering::Release);
        }
    }
}

/// The new name for `slot` whose real name is `real`, if it's renamed.
fn new_name_for(slot: i32, real: &[u8]) -> Option<&'static CStr> {
    let index = usize::try_from(slot)
        .ok()
        .filter(|s| (1..=MAX_SLOTS).contains(s))?;
    let name = NEW_NAME[index].load(Ordering::Acquire);
    if name.is_null() || REAL_HASH[index].load(Ordering::Acquire) != name_hash(real) {
        return None;
    }
    // Safety: every published pointer is a leaked `CStr` from `intern`.
    Some(unsafe { CStr::from_ptr(name) })
}

// ── Engine side ──────────────────────────────────────────────────────────────

/// The real name and identity of a slot, through the engine's own table.
fn real_player(slot: i32, hltv: bool, want_steam_id: bool) -> Option<(&'static [u8], Who)> {
    let engfuncs = engine::engfuncs()?;
    let mut info = HudPlayerInfo::empty();
    // Safety: the engine's own function, on its thread, into a struct of the
    // layout it writes.
    unsafe { (engfuncs.pfn_get_player_info)(slot, &mut info) };
    if info.name.is_null() {
        return None;
    }
    // Safety: a pointer into the engine's static player array.
    let name = unsafe { CStr::from_ptr(info.name) }.to_bytes();
    if name.is_empty() {
        return None;
    }
    let steam_id = if want_steam_id {
        // Safety: `info.name` is the engine's own record pointer.
        unsafe { crate::deathmsg::steam_id_from_record(info.name) }
    } else {
        0
    };
    Some((
        name,
        Who {
            slot,
            steam_id,
            own_pov: !hltv && info.thisplayer != 0,
        },
    ))
}

fn is_hltv() -> bool {
    engine::engfuncs().is_some_and(|e| unsafe { (e.is_spectate_only)() } != 0)
}

/// The demo playing, as [`demo_key`] has it; `None` while none plays.
fn playing_demo() -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    let api = engfuncs.p_demo_api;
    // Safety: the engine's demo API, valid for the session.
    if api.is_null() || unsafe { ((*api).is_playingback)() } == 0 {
        return None;
    }
    crate::demo_reload::current_demo().map(|name| demo_key(&name))
}

/// The detour on `client.dll`'s copy of `pfnGetPlayerInfo`.
unsafe extern "C" fn hooked_get_player_info(slot: i32, info: *mut HudPlayerInfo) {
    let real = REAL_GET_PLAYER_INFO.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    // Safety: the pointer that was in the slot, of the slot's signature.
    let real: GetPlayerInfoFn = unsafe { std::mem::transmute(real) };
    unsafe { real(slot, info) };
    if info.is_null() {
        return;
    }
    // Safety: the caller's struct, just filled by the engine.
    let name = unsafe { (*info).name };
    if name.is_null() {
        return;
    }
    let real_name = unsafe { CStr::from_ptr(name) }.to_bytes();
    if let Some(new) = new_name_for(slot, real_name) {
        unsafe { (*info).name = new.as_ptr() };
    }
}

/// Points `client.dll`'s `pfnGetPlayerInfo` at the detour, once.
fn install_detour() -> Result<(), String> {
    if DETOUR_INSTALLED.load(Ordering::Acquire) {
        return Ok(());
    }
    let gengfuncs = engine::client_gengfuncs()
        .ok_or_else(|| "client.dll's copy of the engine functions isn't found yet".to_string())?;
    let hook = hooked_get_player_info as *mut c_void;
    // Safety: the live copy, slot 21 has GetPlayerInfo's signature.
    let current = unsafe { *gengfuncs.add(ENGFUNCS_SLOT_GET_PLAYER_INFO) };
    if current != hook {
        let was = unsafe {
            engine::patch_gengfuncs_slot(gengfuncs, ENGFUNCS_SLOT_GET_PLAYER_INFO, hook)
        }?;
        REAL_GET_PLAYER_INFO.store(was, Ordering::Release);
    }
    DETOUR_INSTALLED.store(true, Ordering::Release);
    unsafe {
        crate::debug::report(&format!(
            "{NAME}: client.dll's pfnGetPlayerInfo (gEngfuncs at {gengfuncs:p}, slot 21) now goes through the rename detour"
        ))
    };
    Ok(())
}

/// Hands `payload` to `client.dll`'s own handler for `message`.
fn deliver(message: &str, payload: &mut [u8]) -> i32 {
    let (Some(original), Ok(c_name)) = (
        crate::msglog::original_thunk(message),
        CString::new(message),
    ) else {
        return 1;
    };
    let size = i32::try_from(payload.len()).unwrap_or(0);
    // Safety: the game's own pfnUserMsgHook, with a buffer we own for the call.
    unsafe { original(c_name.as_ptr(), size, payload.as_mut_ptr().cast()) }
}

fn payload<'a>(size: i32, buf: *mut c_void) -> &'a [u8] {
    if size > 0 && !buf.is_null() {
        // Safety: the engine's message buffer, `size` bytes, for this call.
        unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
    } else {
        &[]
    }
}

/// `SayText` with a renamed sender's name swapped in the line, handed to the
/// game; `None` to forward the original. Also called by `msglog`'s handler
/// while it watches `SayText`.
pub(crate) fn say_text(size: i32, buf: *mut c_void) -> Option<i32> {
    if !PUBLISHED.load(Ordering::Relaxed) {
        return None;
    }
    let bytes = payload(size, buf);
    let sender = i32::from(*bytes.first()?);
    let (old, _) = real_player(sender, false, false)?;
    let new = new_name_for(sender, old)?;
    let mut rewritten = rewrite_say_text(bytes, old, new.to_bytes())?;
    Some(deliver(SAY_TEXT, &mut rewritten))
}

/// `TextMsg` with each argument equal to a renamed player's real name
/// swapped, handed to the game; `None` to forward the original. Called from
/// `clan_text`'s filter, after its hides.
pub(crate) fn text_msg(bytes: &[u8]) -> Option<i32> {
    if !PUBLISHED.load(Ordering::Relaxed) {
        return None;
    }
    let mut renames: Vec<(&[u8], &[u8])> = Vec::new();
    for slot in 1..=MAX_SLOTS as i32 {
        if let Some((old, _)) = real_player(slot, false, false)
            && let Some(new) = new_name_for(slot, old)
        {
            renames.push((old, new.to_bytes()));
        }
    }
    if renames.is_empty() {
        return None;
    }
    let mut rewritten = rewrite_text_msg(bytes, &renames)?;
    Some(deliver(TEXT_MSG, &mut rewritten))
}

const SAY_TEXT: &str = "SayText";
const TEXT_MSG: &str = "TextMsg";

unsafe extern "C" fn hooked_say_text(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if let Some(result) = say_text(size, buf) {
        return result;
    }
    match crate::msglog::original_thunk(SAY_TEXT) {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

fn install_say_text_hook() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Safety: the engine's own pfnHookUserMsg, with a handler of its type.
    unsafe { (engfuncs.pfn_hook_user_msg)(c"SayText".as_ptr(), hooked_say_text) };
}

/// Once per frame, from `commands::poll`: works out each slot's name and
/// keeps the detour and the `SayText` handler in.
pub fn poll() {
    if !ACTIVE.load(Ordering::Relaxed) {
        if PUBLISHED.swap(false, Ordering::Relaxed) {
            for slot in 1..=MAX_SLOTS {
                publish(slot, None);
            }
        }
        return;
    }
    if !DETOUR_INSTALLED.load(Ordering::Relaxed)
        && let Err(why) = install_detour()
        && !DETOUR_FAILED.swap(true, Ordering::Relaxed)
    {
        unsafe { crate::debug::report(&format!("{NAME}: no detour yet -- {why}")) };
    }
    if crate::map_text::level_name().is_some() && !crate::msglog::watching(SAY_TEXT) {
        install_say_text_hook();
    }
    let Ok(entries) = RULES.try_read() else {
        return;
    };
    let rules: Vec<&Rule> = entries.iter().map(|e| &e.rule).collect();
    let demo = if rules.iter().any(|r| !r.demos.is_empty()) {
        playing_demo()
    } else {
        None
    };
    let hltv = is_hltv();
    let want_steam_id = rules.iter().any(|r| matches!(r.id, Id::Steam(_)));
    let mut any = false;
    for slot in 1..=MAX_SLOTS {
        let desired = real_player(slot as i32, hltv, want_steam_id).and_then(|(real, who)| {
            pick(&rules, &who, demo.as_deref())
                .map(|winner| (name_hash(real), entries[winner].name.as_ptr()))
        });
        any |= desired.is_some();
        publish(slot, desired);
    }
    PUBLISHED.store(any, Ordering::Relaxed);
}

// ── Console ──────────────────────────────────────────────────────────────────

fn usage() -> String {
    format!(
        "usage:\n\
         \x20 {NAME} add id <slot | SteamID | self> [in <demo>[,<demo>...]] to <new name>\n\
         \x20     rename a player everywhere their name shows: kill feed, scoreboard,\n\
         \x20     chat, join lines. Named parameters, any order. A name with spaces\n\
         \x20     goes in quotes (no \" or ;), up to {MAX_NAME_BYTES} characters.\n\
         \x20     in: demo file names or * patterns (wsod25_r03_*); none = every demo.\n\
         \x20     SteamID: STEAM_0:x:y, [U:1:n] or 7656119...; ds_hud_deathmsg players lists them.\n\
         \x20 {NAME} list         the renames, numbered\n\
         \x20 {NAME} remove <n>   drop rename n\n\
         \x20 {NAME} clear        drop them all\n"
    )
}

/// Which slots each rename wins right now, for `list`.
fn now_text(rules: &[&Rule]) -> Vec<Vec<String>> {
    let mut won = vec![Vec::new(); rules.len()];
    let demo = playing_demo();
    let hltv = is_hltv();
    for slot in 1..=MAX_SLOTS as i32 {
        if let Some((real, who)) = real_player(slot, hltv, true)
            && let Some(winner) = pick(rules, &who, demo.as_deref())
        {
            won[winner].push(format!("slot {slot} ({})", String::from_utf8_lossy(real)));
        }
    }
    won
}

fn list_text(entries: &[Entry]) -> String {
    if entries.is_empty() {
        return format!("{NAME}: no renames\n");
    }
    let rules: Vec<&Rule> = entries.iter().map(|e| &e.rule).collect();
    let won = now_text(&rules);
    let mut out = format!("{NAME}: {} rename(s)\n", entries.len());
    for (i, rule) in rules.iter().enumerate() {
        let now = if won[i].is_empty() {
            "nobody here".to_string()
        } else {
            won[i].join(", ")
        };
        out.push_str(&format!(
            "  {}  {}  -- now: {now}\n",
            i + 1,
            rule.describe()
        ));
    }
    out
}

/// Applies one parsed command to the list; the reply.
fn apply(entries: &mut Vec<Entry>, command: Command) -> Result<String, String> {
    match command {
        Command::List => Ok(String::new()),
        Command::Clear => {
            entries.clear();
            Ok(format!("{NAME}: cleared every rename\n"))
        }
        Command::Remove(n) => {
            if n > entries.len() {
                return Err(if entries.is_empty() {
                    "there are no renames to remove".to_string()
                } else {
                    format!(
                        "there is no rename {n}; `list` shows 1 to {}",
                        entries.len()
                    )
                });
            }
            let gone = entries.remove(n - 1);
            Ok(format!("{NAME}: removed {}\n", gone.rule.describe()))
        }
        Command::Add(rule) => {
            let name = intern(&rule.name).ok_or("the name can't be stored")?;
            // The same player and demos again replaces the name rather than
            // adding a second line, so a batch that runs its commands for
            // every demo doesn't grow the list.
            if let Some(same) = entries
                .iter_mut()
                .find(|e| e.rule.id == rule.id && e.rule.demos == rule.demos)
            {
                if same.rule.name == rule.name {
                    return Ok(format!("{NAME}: already renaming {}\n", rule.describe()));
                }
                same.rule.name = rule.name;
                same.name = name;
                return Ok(format!("{NAME}: now {}\n", same.rule.describe()));
            }
            let reply = format!("{NAME}: added {}\n", rule.describe());
            entries.push(Entry { rule, name });
            Ok(reply)
        }
    }
}

/// Lifts the kill feed's 31-byte cap for a long name; the warning when it
/// can't.
fn ensure_long_names(name: &str) -> Option<String> {
    if name.len() <= ENGINE_NAME_BYTES {
        return None;
    }
    crate::deathmsg::lift_name_cap().err().map(|why| {
        format!("the kill feed will cut this name at {ENGINE_NAME_BYTES} characters: {why}")
    })
}

fn run(args: &[String]) -> String {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(why) => return format!("{NAME}: refused: {why}\n{}", usage()),
    };
    let mut reply = String::new();
    for warning in &parsed.warnings {
        reply.push_str(&format!("{NAME}: warning: {warning}\n"));
    }
    let long_name = match &parsed.command {
        Command::Add(rule) => Some(rule.name.clone()),
        _ => None,
    };
    let bare = args.is_empty();
    let Ok(mut entries) = RULES.write() else {
        return format!("{NAME}: the rename list is unusable (lock poisoned)\n");
    };
    match apply(&mut entries, parsed.command) {
        Ok(text) => reply.push_str(&text),
        Err(why) => return format!("{reply}{NAME}: refused: {why}\n"),
    }
    ACTIVE.store(!entries.is_empty(), Ordering::Relaxed);
    if let Some(warning) = long_name.as_deref().and_then(ensure_long_names) {
        reply.push_str(&format!("{NAME}: warning: {warning}\n"));
    }
    if !entries.is_empty()
        && let Err(why) = install_detour()
    {
        reply.push_str(&format!(
            "{NAME}: warning: not renaming yet -- {why}; it retries every frame\n"
        ));
    }
    drop(entries);
    // The new names show from this frame on, not the next.
    poll();
    let entries = RULES.read().map(|e| list_text(&e)).unwrap_or_default();
    reply.push_str(&entries);
    if bare {
        reply.push_str(&usage());
    }
    reply
}

pub unsafe extern "C" fn command() {
    let args = crate::cmd_list::args();
    let reply = run(&args);
    crate::commands::console_print(&reply);
    unsafe {
        crate::debug::report(&format!(
            "{NAME} {} -> {}",
            args.join(" "),
            reply.lines().next().unwrap_or_default()
        ))
    };
}

/// The `ds_debug_status` line, or `None` while nothing was ever renamed.
pub fn status_line() -> Option<String> {
    let count = RULES.read().map(|e| e.len()).unwrap_or(0);
    if count == 0 && !DETOUR_INSTALLED.load(Ordering::Relaxed) {
        return None;
    }
    let renamed = (1..=MAX_SLOTS)
        .filter(|&s| !NEW_NAME[s].load(Ordering::Relaxed).is_null())
        .count();
    Some(format!(
        "{NAME}: {count} rename(s), {renamed} player(s) renamed now; GetPlayerInfo detour {}; kill feed names up to {} characters",
        if DETOUR_INSTALLED.load(Ordering::Relaxed) {
            "on"
        } else {
            "off"
        },
        if crate::deathmsg::name_cap_lifted() {
            MAX_NAME_BYTES
        } else {
            ENGINE_NAME_BYTES
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn rule(parsed: Result<Parsed, String>) -> Rule {
        match parsed.expect("parses").command {
            Command::Add(rule) => rule,
            other => panic!("not an add: {other:?}"),
        }
    }

    const STEAM: u64 = 76_561_197_960_265_728 + 2 * 5_550_002 + 1;

    #[test]
    fn add_takes_its_parameters_in_any_order() {
        let want = Rule {
            id: Id::Steam(STEAM),
            demos: vec!["greetz1".to_string()],
            name: "dicE Alex".to_string(),
        };
        // As the console splits STEAM_0:1:5550002 unquoted.
        let id = ["id", "STEAM_0", ":", "1", ":", "5550002"];
        for order in [
            [&id[..], &["in", "greetz1", "to", "dicE Alex"][..]].concat(),
            [&["to", "dicE Alex", "in", "greetz1"][..], &id[..]].concat(),
            [&["in", "greetz1"][..], &id[..], &["to", "dicE Alex"][..]].concat(),
        ] {
            let parsed = parse(&s(&[&["add"][..], &order[..]].concat()));
            assert_eq!(rule(parsed), want, "{order:?}");
        }
    }

    #[test]
    fn a_quoted_name_arrives_whole_and_keeps_its_colons() {
        let parsed = parse(&s(&[
            "add",
            "id",
            "self",
            "in",
            "x",
            "to",
            "dicE[: :]Gorilla[bc",
        ]));
        assert_eq!(rule(parsed).name, "dicE[: :]Gorilla[bc");
    }

    #[test]
    fn an_unquoted_name_with_a_colon_is_glued_back() {
        let parsed = parse(&s(&[
            "add", "id", "3", "in", "x", "to", "dicE[", ":", "]Alex",
        ]));
        assert_eq!(rule(parsed).name, "dicE[:]Alex");
    }

    #[test]
    fn an_unquoted_name_with_a_colon_warns_that_spaces_are_gone() {
        // `to dicE[: :]Alex` typed without quotes.
        let parsed = parse(&s(&[
            "add", "id", "3", "in", "x", "to", "dicE[", ":", ":", "]Alex",
        ]))
        .unwrap();
        assert!(
            parsed
                .warnings
                .iter()
                .any(|w| w.contains("without quotes") && w.contains("\"dicE[::]Alex\"")),
            "{:?}",
            parsed.warnings
        );
        // Quoted, or a glued SteamID or demo list: no such warning.
        for quiet in [
            &["add", "id", "3", "in", "x", "to", "dicE[: :]Alex"][..],
            &[
                "add", "id", "STEAM_0", ":", "1", ":", "5", "in", "a", ",", "b", "to", "x",
            ][..],
        ] {
            let parsed = parse(&s(quiet)).unwrap();
            assert!(
                parsed.warnings.is_empty(),
                "{quiet:?}: {:?}",
                parsed.warnings
            );
        }
    }

    #[test]
    fn in_takes_a_comma_list_and_patterns() {
        // `,` is a token of its own too.
        let parsed = parse(&s(&[
            "add",
            "id",
            "76561197960265729",
            "in",
            "WSOD25_r03_m00cat",
            ",",
            "dod/wsod25_r03_alex.dem",
            ",",
            "wsod25_r04_*",
            "to",
            "x",
        ]));
        let rule = rule(parsed);
        assert_eq!(
            rule.demos,
            ["wsod25_r03_m00cat", "wsod25_r03_alex", "wsod25_r04_*"]
        );
        assert!(rule.applies_to(Some("wsod25_r03_alex")));
        assert!(rule.applies_to(Some("wsod25_r04_bob")));
        assert!(!rule.applies_to(Some("wsod25_r03_bob")));
        assert!(!rule.applies_to(None), "no demo playing");
    }

    #[test]
    fn no_in_means_every_demo() {
        let rule = rule(parse(&s(&["add", "id", "76561197960265729", "to", "x"])));
        assert!(rule.demos.is_empty());
        assert!(rule.applies_to(Some("anything")));
        assert!(rule.applies_to(None));
    }

    #[test]
    fn self_or_a_slot_without_in_is_warned_not_refused() {
        let parsed = parse(&s(&["add", "id", "self", "to", "x"])).unwrap();
        assert_eq!(parsed.warnings, [SELF_WARNING]);
        assert!(matches!(parsed.command, Command::Add(_)));
        let parsed = parse(&s(&["add", "id", "8", "to", "x"])).unwrap();
        assert!(parsed.warnings[0].contains("slot 8 is a different player"));
        for quiet in [
            &["add", "id", "self", "in", "a", "to", "x"][..],
            &["add", "id", "76561197960265729", "to", "x"][..],
        ] {
            assert!(parse(&s(quiet)).unwrap().warnings.is_empty(), "{quiet:?}");
        }
    }

    #[test]
    fn add_without_id_or_to_is_refused() {
        for args in [
            &["add", "to", "x"][..],
            &["add", "id", "self"][..],
            &["add"][..],
        ] {
            let why = parse(&s(args)).unwrap_err();
            assert!(why.contains("add needs `id"), "{args:?}: {why}");
        }
    }

    #[test]
    fn an_unknown_word_is_named() {
        let why = parse(&s(&["add", "id", "self", "vicitm", "3", "to", "x"])).unwrap_err();
        assert!(why.contains("unknown word `vicitm`"), "{why}");
        let why = parse(&s(&["rename", "x"])).unwrap_err();
        assert!(why.contains("unknown word `rename`"), "{why}");
        let why = parse(&s(&["list", "all"])).unwrap_err();
        assert!(why.contains("unknown word `all`"), "{why}");
        // A name with a space typed without quotes: the hint says so.
        let why = parse(&s(&["add", "id", "self", "in", "a", "to", "dicE", "Alex"])).unwrap_err();
        assert!(
            why.contains("unknown word `Alex`") && why.contains("quotes"),
            "{why}"
        );
    }

    #[test]
    fn bad_values_are_refused_with_the_reason() {
        let cases: &[(&[&str], &str)] = &[
            (&["add", "id", "40", "to", "x"], "is not a slot"),
            (&["add", "id", "0", "to", "x"], "is not a slot"),
            (&["add", "id", "bob", "to", "x"], "is not a slot"),
            (
                &["add", "id", "self", "id", "3", "to", "x"],
                "`id` is given twice",
            ),
            (&["add", "id", "self", "to"], "`to` needs a name"),
            (&["add", "id", "self", "to", "50%"], "can't contain %"),
            // Quoted, so the tokenizer leaves it whole.
            (&["add", "id", "self", "in", " , ", "to", "x"], "`in` needs"),
            (&["remove"], "remove needs"),
            (&["remove", "0"], "not a number"),
            (&["remove", "two"], "not a number"),
        ];
        for (args, want) in cases {
            let why = parse(&s(args)).unwrap_err();
            assert!(why.contains(want), "{args:?}: {why}");
        }
        let long = "x".repeat(MAX_NAME_BYTES + 1);
        let why = parse(&s(&["add", "id", "self", "to", &long])).unwrap_err();
        assert!(why.contains("at most 63"), "{why}");
        let longest = "x".repeat(MAX_NAME_BYTES);
        assert!(parse(&s(&["add", "id", "self", "in", "a", "to", &longest])).is_ok());
    }

    #[test]
    fn the_other_subcommands_parse() {
        assert_eq!(parse(&[]).unwrap().command, Command::List);
        assert_eq!(parse(&s(&["LIST"])).unwrap().command, Command::List);
        assert_eq!(parse(&s(&["clear"])).unwrap().command, Command::Clear);
        assert_eq!(
            parse(&s(&["remove", "2"])).unwrap().command,
            Command::Remove(2)
        );
    }

    #[test]
    fn demo_names_compare_by_file_name() {
        assert_eq!(
            demo_key("../../Half-Life/dod/Solifotw-Greetz1.DEM"),
            "solifotw-greetz1"
        );
        assert_eq!(demo_key("folder\\match"), "match");
        assert!(glob("solifotw-*", "solifotw-greetz1"));
        assert!(glob("*greetz*", "solifotw-greetz1"));
        assert!(glob("a*b*c", "axxbyyc"));
        assert!(!glob("a*b*c", "axxbyy"));
        assert!(glob("greetz1", "greetz1"));
        assert!(!glob("greetz1", "greetz12"), "no * means the whole name");
        assert!(glob("*", ""));
    }

    fn who(slot: i32, steam_id: u64, own_pov: bool) -> Who {
        Who {
            slot,
            steam_id,
            own_pov,
        }
    }

    fn r(id: Id, demos: &[&str], name: &str) -> Rule {
        Rule {
            id,
            demos: demos.iter().map(|d| d.to_string()).collect(),
            name: name.to_string(),
        }
    }

    #[test]
    fn ids_match_by_slot_steam_id_or_self() {
        let me = who(8, STEAM, true);
        assert!(Id::Slot(8).matches(&me));
        assert!(!Id::Slot(9).matches(&me));
        assert!(Id::Steam(STEAM).matches(&me));
        assert!(!Id::Steam(STEAM + 1).matches(&me));
        assert!(Id::OwnPov.matches(&me));
        assert!(!Id::OwnPov.matches(&who(8, STEAM, false)));
        // A bot has no SteamID, and 0 matches nothing.
        assert!(!Id::Steam(0).matches(&who(3, 0, false)));
    }

    #[test]
    fn a_rename_with_in_beats_one_without_then_the_last_wins() {
        let rules = [
            r(Id::Steam(STEAM), &["greetz1"], "only here"),
            r(Id::Steam(STEAM), &[], "everywhere"),
            r(Id::OwnPov, &[], "self later"),
        ];
        let refs: Vec<&Rule> = rules.iter().collect();
        let me = who(8, STEAM, true);
        assert_eq!(pick(&refs, &me, Some("greetz1")), Some(0));
        assert_eq!(pick(&refs, &me, Some("other")), Some(2));
        assert_eq!(pick(&refs, &who(8, STEAM, false), Some("other")), Some(1));
        assert_eq!(pick(&refs, &who(9, 1, false), Some("greetz1")), None);
    }

    #[test]
    fn adding_the_same_player_and_demos_again_changes_the_name() {
        let mut entries = Vec::new();
        let add = |name: &str| Command::Add(r(Id::OwnPov, &["a"], name));
        apply(&mut entries, add("one")).unwrap();
        let reply = apply(&mut entries, add("one")).unwrap();
        assert!(reply.contains("already"), "{reply}");
        apply(&mut entries, add("two")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rule.name, "two");
        assert_eq!(entries[0].name.to_bytes(), b"two");
        apply(&mut entries, Command::Add(r(Id::OwnPov, &[], "three"))).unwrap();
        assert_eq!(entries.len(), 2, "other demos are another rename");
        assert!(
            apply(&mut entries, Command::Remove(3))
                .unwrap_err()
                .contains("1 to 2")
        );
        apply(&mut entries, Command::Remove(1)).unwrap();
        assert_eq!(entries[0].rule.name, "three");
        apply(&mut entries, Command::Clear).unwrap();
        assert!(entries.is_empty());
        assert!(
            apply(&mut entries, Command::Remove(1))
                .unwrap_err()
                .contains("no renames")
        );
    }

    #[test]
    fn a_name_added_twice_is_one_string() {
        let a = intern("same-name-for-the-pool").unwrap();
        let b = intern("same-name-for-the-pool").unwrap();
        assert_eq!(a.as_ptr(), b.as_ptr());
    }

    #[test]
    fn say_text_swaps_the_senders_name_in_the_line() {
        let payload = b"\x08\x02(TEAM) gskiLL  motiv :  njk\n\0";
        let out = rewrite_say_text(payload, b"gskiLL  motiv", b"dicE[: :]Gorilla[bc").unwrap();
        assert_eq!(out, b"\x08\x02(TEAM) dicE[: :]Gorilla[bc :  njk\n\0");
        // Only the first: the message itself may say the name again.
        let out = rewrite_say_text(b"\x01bob : bob\0", b"bob", b"Robert").unwrap();
        assert_eq!(out, b"\x01Robert : bob\0");
        // Not in the line, or no terminator.
        assert_eq!(rewrite_say_text(b"\x01someone : hi\0", b"bob", b"x"), None);
        assert_eq!(
            rewrite_say_text(b"\x01bob : hi", b"bob", b"x").unwrap(),
            b"\x01x : hi\0"
        );
        assert_eq!(rewrite_say_text(b"", b"bob", b"x"), None);
        assert_eq!(rewrite_say_text(b"\x01bob\0", b"", b"x"), None);
    }

    #[test]
    fn text_msg_swaps_arguments_equal_to_a_real_name() {
        let renames: [(&[u8], &[u8]); 1] = [(b"motiv", b"m")];
        let joined = b"\x02#game_joined_game\0motiv\0";
        assert_eq!(
            rewrite_text_msg(joined, &renames).unwrap(),
            b"\x02#game_joined_game\0m\0"
        );
        // The key itself is never swapped, and a partial match isn't one.
        assert_eq!(rewrite_text_msg(b"\x02motiv\0", &renames), None);
        assert_eq!(rewrite_text_msg(b"\x02#k\0motivated\0", &renames), None);
        // Several arguments, the second one renamed.
        assert_eq!(
            rewrite_text_msg(b"\x02#k\0a\0motiv\0", &renames).unwrap(),
            b"\x02#k\0a\0m\0"
        );
        assert_eq!(rewrite_text_msg(b"", &renames), None);
    }

    #[test]
    fn the_hash_ties_a_name_to_the_player_it_was_resolved_for() {
        assert_eq!(name_hash(b"abc"), name_hash(b"abc"));
        assert_ne!(name_hash(b"abc"), name_hash(b"abd"));
    }

    #[test]
    fn the_command_has_its_name() {
        assert_eq!(NAME, "ds_player_change_name");
    }
}
