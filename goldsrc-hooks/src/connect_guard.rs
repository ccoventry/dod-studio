//! Refuses to join a server while the DLL is loaded (#451).
//!
//! This DLL patches `hw.dll` and `client.dll` in memory, which is what VAC
//! looks for, so joining a VAC-secured server with it loaded is a ban risk
//! (`docs/vac_safety.md`). HLAE asks before `connect`; this refuses outright,
//! which also covers a session HLAE isn't in (`inject.exe`) and one where
//! HLAE's own hook failed to install.
//!
//! ## Every route goes through `connect`
//!
//! Read from both `hw.dll`s (pre-Anniversary offsets): `retry` (`+0x17aa0`)
//! does nothing itself but queue `connect <last server>` (or `listen`) on the
//! command buffer. So do a server's redirect (`+0x169f2`) and a Steam join
//! request (`+0xa507d`), and the server browser's Join button
//! (`ServerBrowser.dll`'s `connect %s`). `reconnect` (`+0x59e20`) re-signs on
//! to the server the game is already connected to -- what a server sends at a
//! map change -- or queues `listen <address>`. `map` joins its own listen
//! server with `connect local`, run through the same command table.
//!
//! So the DLL wraps the four command nodes -- `connect`, `listen`, `retry`,
//! `reconnect` -- the way `demo_reload` wraps `playdemo` (the SDK's
//! command-list functions, no per-build address), and [`refuses`] decides:
//!
//! - `connect`: refused, except `connect local`, the engine's own listen
//!   server (`map`), which no one else can be on.
//! - `listen` (an HLTV proxy's broadcast): refused.
//! - `retry`: refused, unless the last connect was `connect local`.
//! - `reconnect`: refused, unless the last connect was `connect local` (a
//!   `changelevel` on your own map sends it) or a demo is playing, where it
//!   joins nothing and is left as the engine has it.
//!
//! A refused command prints why to the console and writes a line to the hook
//! log, so a "why can't I connect" report explains itself.
//!
//! ## What it can't cover
//!
//! A `+connect` on the game's launch line runs before the wrap is in (the
//! same timing `demo_reload`'s module doc measured for `+viewdemo`). DoD
//! Studio never puts one there. And injecting into a game that is already on
//! a server is too late to help.
//!
//! `GOLDSRC_HOOKS_ALLOW_CONNECT=1` leaves all four commands as the engine has
//! them, for someone who knowingly tests on their own server.

// Only the 32-bit build installs anything; a host check still compiles it.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, ClEngineFuncsPartial, ConsoleCommandFn};

/// `GOLDSRC_HOOKS_ALLOW_CONNECT=1` sets this false, and nothing is wrapped.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// The environment variable that turns the guard off.
pub const ALLOW_ENV: &str = "GOLDSRC_HOOKS_ALLOW_CONNECT";

/// `cl_enginefunc_t` slot, from the public SDK's `APIProxy.h`.
const SLOT_GET_FIRST_CMD_FUNCTION_HANDLE: usize = 102;

/// A node of the engine's command list, as `Cmd_AddCommand` builds it.
#[repr(C)]
struct CmdFunction {
    next: *mut CmdFunction,
    name: *const c_char,
    function: Option<ConsoleCommandFn>,
    flags: i32,
}

type GetFirstCmdFn = unsafe extern "C" fn() -> *mut CmdFunction;

/// The engine's own handlers, called when a command is allowed.
static REAL_CONNECT: AtomicUsize = AtomicUsize::new(0);
static REAL_LISTEN: AtomicUsize = AtomicUsize::new(0);
static REAL_RETRY: AtomicUsize = AtomicUsize::new(0);
static REAL_RECONNECT: AtomicUsize = AtomicUsize::new(0);
static WRAPPED: AtomicBool = AtomicBool::new(false);

/// Whether the last `connect` let through was `connect local`.
static LOCAL_SESSION: AtomicBool = AtomicBool::new(false);

/// How many more frames `poll` retries a wrap that found nothing.
static RETRIES_LEFT: AtomicU32 = AtomicU32::new(300);

/// The list is a few hundred nodes; this only stops a corrupted one looping.
const MAX_NODES: usize = 8192;

/// Wraps the four commands. Called once engfuncs are captured.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "connect_guard: off ({ALLOW_ENV}=1) -- connect, retry, reconnect and listen are stock; \
                 joining a VAC-secured server with this DLL loaded is a ban risk (#451)"
            ))
        };
        return;
    }
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    }
}

/// Retries a wrap that has not found `connect` yet, for a few seconds of
/// frames, then says so once, in the log and the console. A no-op after that,
/// or once wrapped.
pub fn poll() {
    if RETRIES_LEFT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    } else if RETRIES_LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
        let why = "connect_guard: no connect command in the engine's command list -- \
                   joining a server is NOT blocked, so don't";
        unsafe { crate::debug::report(why) };
        crate::commands::console_print(&format!("{why}\n"));
    }
}

fn try_wrap() -> bool {
    if WRAPPED.load(Ordering::Relaxed) {
        return true;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    // Safety: slot 102 of the engine's own table, checked in both builds by
    // tools/verify_cmd_list_slots.py.
    let first = unsafe {
        let slot = *(engfuncs as *const ClEngineFuncsPartial as *const usize)
            .add(SLOT_GET_FIRST_CMD_FUNCTION_HANDLE);
        if slot == 0 {
            return false;
        }
        let get_first: GetFirstCmdFn = std::mem::transmute(slot);
        get_first()
    };

    let mut wrapped = Vec::new();
    let mut node = first;
    let mut seen = 0;
    while !node.is_null() && seen < MAX_NODES {
        seen += 1;
        // Safety: a live node of the engine's list; see demo_reload's module doc.
        let entry = unsafe { &mut *node };
        if !entry.name.is_null() {
            let name = unsafe { CStr::from_ptr(entry.name) }.to_bytes();
            let target = match Command::from_name(name) {
                Some(Command::Connect) => {
                    Some((&REAL_CONNECT, wrapped_connect as ConsoleCommandFn))
                }
                Some(Command::Listen) => Some((&REAL_LISTEN, wrapped_listen as ConsoleCommandFn)),
                Some(Command::Retry) => Some((&REAL_RETRY, wrapped_retry as ConsoleCommandFn)),
                Some(Command::Reconnect) => {
                    Some((&REAL_RECONNECT, wrapped_reconnect as ConsoleCommandFn))
                }
                None => None,
            };
            if let Some((real, wrapper)) = target
                && wrap(entry, real, wrapper)
            {
                wrapped.push(String::from_utf8_lossy(name).into_owned());
            }
        }
        node = entry.next;
    }
    // `connect` is the one every route ends in; the rest are wrapped as found.
    let found = REAL_CONNECT.load(Ordering::Relaxed) != 0;
    if found {
        WRAPPED.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "connect_guard: wrapped {} -- joining a server is refused while this DLL is loaded \
                 ({ALLOW_ENV}=1 turns it off)",
                wrapped.join(", ")
            ))
        };
    }
    found
}

/// Points `entry` at `wrapper`, keeping its handler in `real`.
fn wrap(entry: &mut CmdFunction, real: &AtomicUsize, wrapper: ConsoleCommandFn) -> bool {
    let Some(current) = entry.function else {
        return false;
    };
    if current as usize == wrapper as usize {
        return true;
    }
    real.store(current as usize, Ordering::Relaxed);
    entry.function = Some(wrapper);
    true
}

/// The commands the guard wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Connect,
    Listen,
    Retry,
    Reconnect,
}

impl Command {
    fn from_name(name: &[u8]) -> Option<Self> {
        [
            (b"connect".as_slice(), Self::Connect),
            (b"listen", Self::Listen),
            (b"retry", Self::Retry),
            (b"reconnect", Self::Reconnect),
        ]
        .into_iter()
        .find(|(known, _)| name.eq_ignore_ascii_case(known))
        .map(|(_, command)| command)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Listen => "listen",
            Self::Retry => "retry",
            Self::Reconnect => "reconnect",
        }
    }
}

/// Whether `command` would be refused. `argument` is its first argument, if
/// any; `local_session` is whether the last connect let through was
/// `connect local`; `playing_demo` is `pDemoAPI->IsPlayingback()`.
fn refuses(
    command: Command,
    argument: Option<&str>,
    local_session: bool,
    playing_demo: bool,
) -> bool {
    match command {
        // A bare `connect` only prints its usage.
        Command::Connect => argument.is_some_and(|server| !is_local(server)),
        Command::Listen => argument.is_some(),
        Command::Retry => !local_session,
        Command::Reconnect => !(local_session || playing_demo),
    }
}

/// `local` is the engine's own listen server, the one `map` joins.
fn is_local(server: &str) -> bool {
    server.trim().eq_ignore_ascii_case("local")
}

/// Wraps `real`: refuses, or remembers and hands on.
unsafe fn guard(command: Command, real: &AtomicUsize) {
    let argument = first_argument();
    let playing_demo = playing_demo();
    if refuses(
        command,
        argument.as_deref(),
        LOCAL_SESSION.load(Ordering::Relaxed),
        playing_demo,
    ) {
        refuse(command, argument.as_deref());
        return;
    }
    if let (Command::Connect, Some(server)) = (command, argument.as_deref()) {
        LOCAL_SESSION.store(is_local(server), Ordering::Relaxed);
    }
    let address = real.load(Ordering::Relaxed);
    if address != 0 {
        let real: ConsoleCommandFn = unsafe { std::mem::transmute(address) };
        unsafe { real() };
    }
}

fn refuse(command: Command, argument: Option<&str>) {
    let line = match argument {
        Some(argument) => format!("{} {argument}", command.name()),
        None => command.name().to_string(),
    };
    crate::commands::console_print(&refusal_message(&line));
    unsafe { crate::debug::report(&format!("connect_guard: refused `{line}`")) };
}

/// What the console shows for a refused `line`.
fn refusal_message(line: &str) -> String {
    format!(
        "DoD Studio refused `{line}`: joining a server with DoD Studio's hook DLL loaded risks \
         a VAC ban. Play online from a separate copy of Half-Life that DoD Studio never starts. \
         To test on your own server anyway, start the game with {ALLOW_ENV}=1.\n"
    )
}

fn first_argument() -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return None;
        }
        let arg = (engfuncs.cmd_argv)(1);
        if arg.is_null() {
            return None;
        }
        Some(CStr::from_ptr(arg).to_string_lossy().into_owned())
    }
}

fn playing_demo() -> bool {
    engine::engfuncs().is_some_and(|engfuncs| {
        let api = engfuncs.p_demo_api;
        !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
    })
}

unsafe extern "C" fn wrapped_connect() {
    unsafe { guard(Command::Connect, &REAL_CONNECT) };
}

unsafe extern "C" fn wrapped_listen() {
    unsafe { guard(Command::Listen, &REAL_LISTEN) };
}

unsafe extern "C" fn wrapped_retry() {
    unsafe { guard(Command::Retry, &REAL_RETRY) };
}

unsafe extern "C" fn wrapped_reconnect() {
    unsafe { guard(Command::Reconnect, &REAL_RECONNECT) };
}

/// Reads `GOLDSRC_HOOKS_ALLOW_CONNECT`: only an explicit `1` turns the guard
/// off, so a typo or an empty value leaves it on.
pub fn allowed_by_env(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.trim() == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connecting_to_a_server_is_refused() {
        for server in ["1.2.3.4:27015", "dod.example.com", "localhost", "127.0.0.1"] {
            assert!(
                refuses(Command::Connect, Some(server), false, false),
                "{server}"
            );
            assert!(
                refuses(Command::Connect, Some(server), true, true),
                "{server}"
            );
        }
    }

    #[test]
    fn the_games_own_listen_server_is_allowed() {
        assert!(!refuses(Command::Connect, Some("local"), false, false));
        assert!(!refuses(Command::Connect, Some(" LOCAL "), false, false));
    }

    #[test]
    fn a_bare_connect_or_listen_only_prints_its_usage() {
        assert!(!refuses(Command::Connect, None, false, false));
        assert!(!refuses(Command::Listen, None, false, false));
    }

    #[test]
    fn listen_is_refused() {
        assert!(refuses(Command::Listen, Some("1.2.3.4:27020"), true, true));
    }

    #[test]
    fn retry_is_refused_unless_the_last_connect_was_local() {
        assert!(refuses(Command::Retry, None, false, false));
        assert!(refuses(Command::Retry, None, false, true));
        assert!(!refuses(Command::Retry, None, true, false));
    }

    #[test]
    fn reconnect_is_left_alone_on_your_own_map_and_in_a_demo() {
        assert!(refuses(Command::Reconnect, None, false, false));
        assert!(!refuses(Command::Reconnect, None, true, false));
        assert!(!refuses(Command::Reconnect, None, false, true));
    }

    #[test]
    fn commands_are_matched_by_name_ignoring_case() {
        assert_eq!(Command::from_name(b"connect"), Some(Command::Connect));
        assert_eq!(Command::from_name(b"Connect"), Some(Command::Connect));
        assert_eq!(Command::from_name(b"RECONNECT"), Some(Command::Reconnect));
        assert_eq!(Command::from_name(b"retry"), Some(Command::Retry));
        assert_eq!(Command::from_name(b"listen"), Some(Command::Listen));
        assert_eq!(Command::from_name(b"disconnect"), None);
        assert_eq!(Command::from_name(b"connect_x"), None);
        for command in [
            Command::Connect,
            Command::Listen,
            Command::Retry,
            Command::Reconnect,
        ] {
            assert_eq!(Command::from_name(command.name().as_bytes()), Some(command));
        }
    }

    #[test]
    fn only_an_explicit_1_turns_the_guard_off() {
        assert!(allowed_by_env(Some("1")));
        assert!(allowed_by_env(Some(" 1 ")));
        assert!(!allowed_by_env(None));
        assert!(!allowed_by_env(Some("")));
        assert!(!allowed_by_env(Some("0")));
        assert!(!allowed_by_env(Some("true")));
    }

    #[test]
    fn the_refusal_names_the_command_and_the_way_out() {
        let message = refusal_message("connect 1.2.3.4:27015");
        assert!(message.contains("`connect 1.2.3.4:27015`"));
        assert!(message.contains("VAC"));
        assert!(message.contains(ALLOW_ENV));
        assert!(message.ends_with('\n'));
    }

    #[test]
    fn a_node_is_laid_out_as_cmd_add_command_builds_it() {
        assert_eq!(
            std::mem::size_of::<CmdFunction>(),
            4 * std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::offset_of!(CmdFunction, function),
            2 * std::mem::size_of::<usize>()
        );
    }
}
