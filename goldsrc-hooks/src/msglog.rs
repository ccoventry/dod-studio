//! `dodstudio_msglog` — dump chosen DoD user messages and their payloads to
//! the log, forwarded to the game untouched.
//!
//! Every investigation of "what does the client actually receive, and when"
//! has so far been a demo-parsing exercise against the `dod` crate's own
//! parser -- a *reconstruction* of the message stream, not the stream. This
//! hook sees what the client sees, in the session, with the engine's own
//! framing. See `docs/goldsrc_client_dll_survey.md` §4.
//!
//! ## Mechanism
//!
//! The same one `deathmsg`'s `block`/`fake` already rely on:
//! `pfnHookUserMsg` **prepends** a fresh record rather than overwriting the
//! existing one, so the engine's by-name dispatcher stops at the newest
//! entry and the game's own handler stays reachable by calling its thunk
//! directly. **Nothing is patched.** See [`crate::engine::HookUserMsgFn`].
//!
//! One handler, [`hooked_msg`], is registered for every message DoD's
//! `client.dll` hooks -- the full 71-entry name/thunk table below, derived
//! from `goldsrc-hooks/tools/survey_client_dll.py messages` (the same method
//! that produces `docs/goldsrc_client_dll_survey.md` §4; its `DeathMsg` row
//! matches `deathmsg.rs`'s own hand-verified `THUNK_RVA` exactly, which is
//! the cross-check that this table is right). The handler decides whether to
//! actually log each call at call time, from the currently wanted set, so
//! registering a message once and later dropping it from the wanted set
//! doesn't need an unhook -- there isn't one to ask for.
//!
//! ## What's logged
//!
//! The **log file only** (`crate::debug::report`), never the game console:
//! `TextMsg` and `ScoreInfo` alone are frequent enough that echoing every hit
//! to the console would bury whatever the movie-maker was actually watching
//! for. A repeat of the exact same payload for the same message, back to
//! back, is suppressed -- most of these resend the same state every tick
//! while nothing changes, and a log that is one entry per actual change is
//! the one worth reading. The command's own replies (what's now being
//! watched) still go to the console, like every other `dodstudio_*` command.
//!
//! Analysis subject: `dod/cl_dlls/client.dll`, 977,816 bytes, byte-identical
//! across the stock, pre-Anniversary and post-Anniversary installs.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::engine;
use crate::names::console_name;

pub const COMMAND_NAMES: &[&str] = &[COMMAND];
const COMMAND: &str = console_name!("msglog");

/// Payload bytes kept per log line. DoD's user messages are all small,
/// fixed-shape structs (the biggest named ones here are a few dozen bytes),
/// so this is generous headroom, not a real limit -- it exists so a
/// message this table doesn't expect to be huge can't turn one hex dump
/// into the whole log.
const MAX_DUMP: usize = 256;

/// `(name, thunk RVA)` for every message DoD's `client.dll` hooks with
/// `pfnHookUserMsg`, sorted by name. Regenerate with:
///
/// ```text
/// python goldsrc-hooks/tools/survey_client_dll.py messages
/// ```
///
/// and re-derive this table from its output (`name  thunk +0xRVA  ...`) if
/// it ever needs to change -- it is data, not something to hand-edit a
/// single entry of. `tools/verify_msglog_table.py` checks it against a real
/// `client.dll` name for name and RVA for RVA, the same standard every other
/// patched-site table in this crate is held to.
static MESSAGES: &[(&str, usize)] = &[
    ("AllowSpec", 0x20e30),
    ("AmmoPickup", 0x271a0),
    ("AmmoShort", 0x27180),
    ("AmmoX", 0x27160),
    ("BloodPuff", 0x20cd0),
    ("CameraView", 0x2c4c0),
    ("CancelProg", 0x2f6e0),
    ("CapMsg", 0x4c050),
    ("ClCorpse", 0x2d610),
    ("ClanTimer", 0x2cb50),
    ("ClientAreas", 0x2d5f0),
    ("CurMarker", 0x210d0),
    ("CurWeapon", 0x27120),
    ("DeathMsg", 0x2ad70),
    ("Frags", 0x20fe0),
    ("GameRules", 0x2c4a0),
    ("GameTitle", 0x24560),
    ("HLTV", 0x20c50),
    ("HandSignal", 0x20ce0),
    ("Health", 0x2d5b0),
    ("HideWeapon", 0x271e0),
    ("HudText", 0x24540),
    ("InitHUD", 0x20c00),
    ("InitObj", 0x2f640),
    ("ItemPickup", 0x27200),
    ("Logo", 0x20ba0),
    ("MOTD", 0x20d10),
    ("MapMarker", 0x20ef0),
    ("ObjScore", 0x21010),
    ("Object", 0x2d5d0),
    ("PClass", 0x21070),
    ("PShoot", 0x3ed10),
    ("PStatus", 0x21040),
    ("PTeam", 0x210a0),
    ("PlayersIn", 0x2f720),
    ("ProgUpdate", 0x2f6c0),
    ("RandomPC", 0x20d40),
    ("ReloadDone", 0x27220),
    ("ReqState", 0x51ab0),
    ("ResetHUD", 0x20bc0),
    ("ResetSens", 0x2c4e0),
    ("RoundState", 0x20c90),
    ("SayText", 0x45810),
    ("Scope", 0x46350),
    ("ScoreInfo", 0x20e60),
    ("ScoreInfoLong", 0x20e90),
    ("ScoreShort", 0x20fb0),
    ("ServerName", 0x20d70),
    ("SetFOV", 0x20c30),
    ("SetObj", 0x2f660),
    ("ShowMenu", 0x3de50),
    ("Spectator", 0x20e00),
    ("StartProg", 0x2f680),
    ("StartProgF", 0x2f6a0),
    ("StatusIcon", 0x47140),
    ("StatusValue", 0x47400),
    ("TeamNames", 0x20da0),
    ("TeamScore", 0x20ec0),
    ("TextMsg", 0x4c030),
    ("TimeLeft", 0x20cb0),
    ("TimerStatus", 0x2f700),
    ("Train", 0x4c6c0),
    ("UseSound", 0x20c70),
    ("VGUIMenu", 0x20dd0),
    ("VoiceMask", 0x51a90),
    ("WaveStatus", 0x20f50),
    ("WaveTime", 0x20f20),
    ("WeapPickup", 0x271c0),
    ("WeaponList", 0x27140),
    ("WideScreen", 0x20f80),
    ("YouDied", 0x20be0),
];

fn find_thunk(name: &str) -> Option<usize> {
    MESSAGES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, rva)| rva)
}

/// What `dodstudio_msglog` is currently set to watch.
enum Wanted {
    None,
    All,
    Names(Vec<String>),
}

static WANTED: Mutex<Wanted> = Mutex::new(Wanted::None);

/// True whenever [`WANTED`] is not [`Wanted::None`] -- lets [`poll`] skip the
/// lock on the overwhelmingly common case (logging off) without changing
/// behaviour, the same shape `commands.rs`' other `poll_*` helpers use.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The last payload logged per message name, for the identical-repeat
/// throttle described in the module doc.
static LAST_LOGGED: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());

fn is_wanted(name: &str) -> bool {
    match WANTED.lock() {
        Ok(w) => match &*w {
            Wanted::None => false,
            Wanted::All => true,
            Wanted::Names(names) => names.iter().any(|n| n.eq_ignore_ascii_case(name)),
        },
        Err(_) => false,
    }
}

fn hex_dump(bytes: &[u8]) -> String {
    let shown = &bytes[..bytes.len().min(MAX_DUMP)];
    let mut out = shown
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    if bytes.len() > MAX_DUMP {
        out.push_str(&format!(" … ({} more byte(s))", bytes.len() - MAX_DUMP));
    }
    out
}

/// `client.dll`'s own handler for `name`, which we forward to.
fn original_thunk(name: &str) -> Option<engine::UserMsgHookFn> {
    let base = engine::client_module_base()?;
    let rva = find_thunk(name)?;
    // Safety: rva is a code offset into the module taken from `MESSAGES`,
    // and the signature is the engine's own pfnUserMsgHook.
    Some(unsafe { std::mem::transmute::<usize, engine::UserMsgHookFn>(base + rva) })
}

/// Logs `name`'s payload (if wanted and not an exact repeat of the last one
/// logged for it), then forwards to the game's own handler untouched --
/// mirrors `deathmsg::hooked_death_msg`'s "same buffer, unmodified" shape.
unsafe extern "C" fn hooked_msg(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if !name.is_null() {
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        if is_wanted(&name) {
            let bytes: &[u8] = if size > 0 && !buf.is_null() {
                unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
            } else {
                &[]
            };
            let repeat = LAST_LOGGED
                .lock()
                .map(|log| log.get(name.as_ref()).is_some_and(|last| last == bytes))
                .unwrap_or(false);
            if !repeat {
                unsafe {
                    crate::debug::report(&format!(
                        "msglog: {name} ({size} byte(s)) {}",
                        hex_dump(bytes)
                    ))
                };
                if let Ok(mut log) = LAST_LOGGED.lock() {
                    log.insert(name.into_owned(), bytes.to_vec());
                }
            }
        }
    }
    let name_str = if name.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(name) }.to_string_lossy())
    };
    match name_str.as_deref().and_then(original_thunk) {
        Some(original) => unsafe { original(name, size, buf) },
        // No thunk on record for this name (should not happen -- we only
        // ever register names out of MESSAGES) -- 1 is what the engine's
        // own dispatcher treats as handled, the safe side to fail on.
        None => 1,
    }
}

fn install_hook(name: &str) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(c_name) = CString::new(name) else {
        return;
    };
    unsafe { (engfuncs.pfn_hook_user_msg)(c_name.as_ptr(), hooked_msg) };
}

/// Called once per frame from [`crate::commands::poll`]. Re-prepends
/// whichever messages are currently wanted -- cheap and idempotent the same
/// way `deathmsg::poll` is (`pfnHookUserMsg` returns early when the first
/// record already carries this exact handler), and it's how a hook survives
/// the engine freeing the whole message list on disconnect.
pub fn poll() {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let Ok(w) = WANTED.lock() else { return };
    match &*w {
        Wanted::None => {}
        Wanted::All => {
            for (name, _) in MESSAGES {
                install_hook(name);
            }
        }
        Wanted::Names(names) => {
            for name in names {
                install_hook(name);
            }
        }
    }
}

/// The command's own reply to a bare `dodstudio_msglog` -- always something,
/// including "logging nothing", since a query should never come back empty.
fn status() -> String {
    match WANTED.lock() {
        Ok(w) => match &*w {
            Wanted::None => format!("{COMMAND} = logging nothing\n"),
            Wanted::All => format!("{COMMAND} = logging all {} message(s)\n", MESSAGES.len()),
            Wanted::Names(names) => format!("{COMMAND} = logging {}\n", names.join(", ")),
        },
        Err(_) => format!("{COMMAND} = (lock poisoned)\n"),
    }
}

/// Folded into `dodstudio_debug_status`. `None` while logging is off, so that
/// command's gate on the two other opt-in diagnostics (`anim_fix`,
/// `sound_fix`) can treat this the same way -- see `commands.rs`.
pub(crate) fn status_line() -> Option<String> {
    if !ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    Some(status().trim_end().to_string())
}

fn usage() -> String {
    format!(
        "usage:\n\
         \x20 {COMMAND}                    what is being logged\n\
         \x20 {COMMAND} <name>...          log these, with a hex dump of each payload, to the log file\n\
         \x20 {COMMAND} all                everything (noisy; expect a large log)\n\
         \x20 {COMMAND} clear              stop logging\n"
    )
}

fn args() -> Vec<String> {
    let Some(engfuncs) = engine::engfuncs() else {
        return Vec::new();
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    (0..argc)
        .filter_map(|i| {
            let ptr = unsafe { (engfuncs.cmd_argv)(i) };
            if ptr.is_null() {
                return None;
            }
            Some(
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

/// Resolves a list of typed tokens against [`MESSAGES`], case-insensitively.
/// Pure -- touches no global state -- so the "reject the whole list on any
/// unknown name" rule is unit-testable without the shared-static problem
/// every other subcommand here has (see the test module).
fn resolve_names(rest: &[String]) -> Result<Vec<String>, Vec<String>> {
    let mut unknown = Vec::new();
    let mut canonical = Vec::new();
    for token in rest {
        match MESSAGES.iter().find(|(n, _)| n.eq_ignore_ascii_case(token)) {
            Some((n, _)) => canonical.push((*n).to_string()),
            None => unknown.push(token.clone()),
        }
    }
    if unknown.is_empty() {
        Ok(canonical)
    } else {
        Err(unknown)
    }
}

fn dispatch(argv: &[String]) -> String {
    // argv[0] is the command name itself, so a bare invocation (or an empty
    // argv, which args() returns when engfuncs isn't resolved yet) is a
    // query -- not `&argv[1..]`, which panics on an empty slice and aborts
    // the whole process under this DLL's release `panic = "abort"` profile.
    let rest = if argv.len() > 1 { &argv[1..] } else { &[] };
    if rest.is_empty() {
        return format!("{}{}", status(), usage());
    }
    if rest.len() == 1 && rest[0].eq_ignore_ascii_case("clear") {
        if let Ok(mut w) = WANTED.lock() {
            *w = Wanted::None;
        }
        ACTIVE.store(false, Ordering::Relaxed);
        if let Ok(mut log) = LAST_LOGGED.lock() {
            log.clear();
        }
        return format!("{COMMAND}: logging nothing\n");
    }
    if rest.len() == 1 && rest[0].eq_ignore_ascii_case("all") {
        if let Ok(mut w) = WANTED.lock() {
            *w = Wanted::All;
        }
        ACTIVE.store(true, Ordering::Relaxed);
        return format!("{COMMAND}: logging all {} message(s)\n", MESSAGES.len());
    }
    // Anything else is a list of message names, replacing whatever was
    // wanted before -- the same "each call restates the whole set" shape
    // `dodstudio_deathmsg block <id>...` uses.
    let canonical = match resolve_names(rest) {
        Ok(names) => names,
        Err(unknown) => {
            return format!(
                "{COMMAND}: no message(s) called {}\n{}",
                unknown.join(", "),
                usage()
            );
        }
    };
    let reply = format!("{COMMAND}: logging {}\n", canonical.join(", "));
    if let Ok(mut w) = WANTED.lock() {
        *w = Wanted::Names(canonical);
    }
    ACTIVE.store(true, Ordering::Relaxed);
    reply
}

pub unsafe extern "C" fn command() {
    let argv = args();
    let reply = dispatch(&argv);
    crate::commands::console_print(&reply);
    unsafe { crate::debug::report(&format!("msglog: {} -> {}", argv.join(" "), reply.trim())) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_name_has_a_distinct_lowercase_form() {
        // is_wanted/find_thunk match case-insensitively, so two entries that
        // only differ by case would make one permanently unreachable.
        let mut lower: Vec<String> = MESSAGES
            .iter()
            .map(|(n, _)| n.to_ascii_lowercase())
            .collect();
        let before = lower.len();
        lower.sort();
        lower.dedup();
        assert_eq!(
            lower.len(),
            before,
            "two MESSAGES entries collide case-insensitively"
        );
    }

    #[test]
    fn the_deathmsg_thunk_agrees_with_deathmsg_rs_own_constant() {
        // deathmsg.rs's THUNK_RVA was hand-verified independently; agreeing
        // with it here is the cross-check that this table -- derived by a
        // different method, survey_client_dll.py's hooked_messages() -- is
        // reading the right thing.
        assert_eq!(find_thunk("DeathMsg"), Some(0x2_ad70));
    }

    #[test]
    fn name_matching_is_case_insensitive() {
        assert_eq!(find_thunk("deathmsg"), find_thunk("DeathMsg"));
        assert_eq!(find_thunk("DEATHMSG"), find_thunk("DeathMsg"));
    }

    // dispatch() itself is not exercised here beyond the empty/usage path:
    // every other branch mutates the WANTED/ACTIVE/LAST_LOGGED statics, and
    // cargo test runs a crate's tests in parallel by default with no
    // isolation between them -- deathmsg.rs's own tests avoid touching its
    // equivalent BLOCK static for the same reason. resolve_names() carries
    // the actual validation logic and is pure, so it's what's tested here.

    #[test]
    fn an_unknown_name_is_named_rather_than_silently_accepted() {
        let err = resolve_names(&["NotARealMessage".to_string()]).unwrap_err();
        assert_eq!(err, vec!["NotARealMessage".to_string()]);
    }

    #[test]
    fn a_mixed_known_and_unknown_list_is_rejected_wholesale() {
        // Silently dropping just the bad name and logging the rest would
        // hide a typo instead of naming it -- the same reasoning
        // dodstudio_hide_hudelement's own fix (commit 3e44cd7) applied.
        let err = resolve_names(&["SayText".to_string(), "Nope".to_string()]).unwrap_err();
        assert_eq!(err, vec!["Nope".to_string()]);
    }

    #[test]
    fn resolved_names_are_canonicalised_to_the_table_spelling() {
        let names = resolve_names(&["saytext".to_string()]).unwrap();
        assert_eq!(names, vec!["SayText".to_string()]);
    }

    #[test]
    fn bare_invocation_is_a_status_query_not_a_mutation() {
        // The one dispatch() path with nothing to race on.
        let reply = dispatch(&["msglog".to_string()]);
        assert!(reply.contains("usage"), "{reply}");
    }

    #[test]
    fn a_genuinely_empty_argv_is_also_a_status_query() {
        // args() returns Vec::new() whenever engine::engfuncs() isn't
        // resolved yet, not just a one-element argv0-only vec -- `&argv[1..]`
        // panics on that, and this DLL ships with `panic = "abort"`.
        let reply = dispatch(&[]);
        assert!(reply.contains("usage"), "{reply}");
    }

    #[test]
    fn hex_dump_truncates_past_the_cap_and_says_so() {
        let bytes = vec![0xabu8; MAX_DUMP + 10];
        let dump = hex_dump(&bytes);
        assert!(dump.contains("10 more byte"), "{dump}");
    }

    #[test]
    fn hex_dump_of_an_empty_payload_is_empty_not_an_error() {
        assert_eq!(hex_dump(&[]), "");
    }
}
