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
//! command-list functions, no per-build address), and [`decide`] picks:
//!
//! - `connect <address>` and `listen <address>`: held back while
//!   [`server_query`](crate::server_query) asks the address what it is, off
//!   the game thread. An HLTV proxy that says VAC is off is joined: `poll`
//!   reissues the command, approved. Anything else -- a game server, a proxy
//!   that says VAC is on, no answer -- is refused. Watching a match through
//!   HLTV is the one online use the user wants with this DLL loaded;
//!   checking the proxy's own VAC byte as well keeps that safe if a proxy
//!   ever reports one. `connect local`, the engine's own listen server
//!   (`map`), which no one else can be on, goes straight through.
//! - `retry`: always handed on. It only queues `connect` or `listen` for the
//!   last address, which is then checked like any other.
//! - `reconnect`: handed on when the game is on its own map (a `changelevel`
//!   there sends it), on an HLTV proxy this let it join, or playing a demo,
//!   where it joins nothing. Refused otherwise.
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

use std::ffi::{CStr, CString, c_char};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use crate::engine::{self, ClEngineFuncsPartial, ConsoleCommandFn};
use crate::server_query::{self, QueryError, ServerInfo};

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

/// What the last `connect` or `listen` let through joined, as a [`Session`].
static SESSION: AtomicU8 = AtomicU8::new(Session::None as u8);

/// The server the game was last let join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Session {
    None = 0,
    /// `connect local`: the game's own listen server.
    Local = 1,
    /// An HLTV proxy that said VAC is off.
    Hltv = 2,
}

impl Session {
    fn current() -> Self {
        match SESSION.load(Ordering::Relaxed) {
            1 => Self::Local,
            2 => Self::Hltv,
            _ => Self::None,
        }
    }
}

/// How long a server gets to say what it is.
const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// A check `connect`/`listen` started and, once the query thread is done, its
/// answer. The guard and `poll` touch it on the game thread (`poll` only
/// `try_lock`s), the query thread once, at the end.
struct Check {
    command: Command,
    address: String,
    answer: Option<Result<ServerInfo, QueryError>>,
}

static CHECK: Mutex<Option<Check>> = Mutex::new(None);

/// The command `poll` reissued after a check passed, which the guard lets
/// through once. Game thread only.
static APPROVED: Mutex<Option<(Command, String)>> = Mutex::new(None);

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
    finish_check();
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

/// What the guard does with a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Hand it to the engine.
    Allow,
    /// Ask the server what it is first.
    Check,
    Refuse,
}

/// What to do with `command`. `argument` is its first argument, if any;
/// `approved` is whether `poll` reissued exactly this command after a check
/// passed; `playing_demo` is `pDemoAPI->IsPlayingback()`.
fn decide(
    command: Command,
    argument: Option<&str>,
    approved: bool,
    session: Session,
    playing_demo: bool,
) -> Action {
    match command {
        // A bare `connect` or `listen` only prints its usage.
        Command::Connect | Command::Listen => match argument {
            None => Action::Allow,
            Some(server) if command == Command::Connect && is_local(server) => Action::Allow,
            Some(_) if approved => Action::Allow,
            Some(_) => Action::Check,
        },
        // It only queues `connect`/`listen` for the last address, checked then.
        Command::Retry => Action::Allow,
        Command::Reconnect => {
            if session != Session::None || playing_demo {
                Action::Allow
            } else {
                Action::Refuse
            }
        }
    }
}

/// Whether a server's answer lets the game join it, or why not.
fn verdict(answer: &Result<ServerInfo, QueryError>) -> Result<(), &'static str> {
    match answer {
        Ok(info) if info.is_hltv() && !info.vac => Ok(()),
        Ok(info) if info.is_hltv() => Err("it is an HLTV proxy, but it says VAC is on"),
        Ok(_) => Err("it is a game server, not an HLTV proxy"),
        Err(QueryError::NoAnswer) => {
            Err("it didn't answer, so there's no telling whether it is an HLTV proxy")
        }
        Err(QueryError::Unreadable) => Err("its answer couldn't be read"),
        Err(QueryError::BadAddress(_)) => Err("that address couldn't be looked up"),
    }
}

/// An address `poll` can safely put back on a command line: nothing that
/// could carry a second command (quotes, `;`, spaces, line breaks).
fn is_plain_address(address: &str) -> bool {
    !address.is_empty()
        && address
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'_'))
}

/// `local` is the engine's own listen server, the one `map` joins.
fn is_local(server: &str) -> bool {
    server.trim().eq_ignore_ascii_case("local")
}

/// Wraps `real`: refuses, starts a check, or remembers and hands on.
unsafe fn guard(command: Command, real: &AtomicUsize) {
    let argument = command_argument();
    let approved = take_approval(command, argument.as_deref());
    match decide(
        command,
        argument.as_deref(),
        approved,
        Session::current(),
        playing_demo(),
    ) {
        Action::Refuse => {
            refuse(
                command,
                argument.as_deref(),
                "the game isn't on its own map or an HLTV proxy",
            );
            return;
        }
        Action::Check => {
            start_check(command, argument.unwrap_or_default());
            return;
        }
        Action::Allow => {}
    }
    match (command, argument.as_deref()) {
        (Command::Connect, Some(server)) if is_local(server) => {
            SESSION.store(Session::Local as u8, Ordering::Relaxed)
        }
        (Command::Connect | Command::Listen, Some(_)) => {
            SESSION.store(Session::Hltv as u8, Ordering::Relaxed)
        }
        _ => {}
    }
    let address = real.load(Ordering::Relaxed);
    if address != 0 {
        let real: ConsoleCommandFn = unsafe { std::mem::transmute(address) };
        unsafe { real() };
    }
}

/// Whether `poll` reissued exactly this command; used up either way.
fn take_approval(command: Command, argument: Option<&str>) -> bool {
    let Ok(mut approved) = APPROVED.lock() else {
        return false;
    };
    match (approved.take(), argument) {
        (Some((c, address)), Some(argument)) => c == command && address == argument,
        _ => false,
    }
}

/// Asks `address` what it is on another thread; `poll` acts on the answer.
fn start_check(command: Command, address: String) {
    if !is_plain_address(&address) {
        refuse(
            command,
            Some(&address),
            "that isn't a plain host:port address",
        );
        return;
    }
    let line = format!("{} {address}", command.name());
    let Ok(mut check) = CHECK.lock() else {
        return;
    };
    if check.is_some() {
        crate::commands::console_print(&format!(
            "DoD Studio is still checking the last server; try `{line}` again in a moment.\n"
        ));
        return;
    }
    *check = Some(Check {
        command,
        address: address.clone(),
        answer: None,
    });
    drop(check);
    crate::commands::console_print(&format!(
        "DoD Studio is checking whether {address} is an HLTV proxy before joining...\n"
    ));
    unsafe { crate::debug::report(&format!("connect_guard: checking `{line}`")) };
    std::thread::spawn(move || {
        let answer = server_query::query(&address, QUERY_TIMEOUT);
        if let Ok(mut check) = CHECK.lock()
            && let Some(check) = check.as_mut()
        {
            check.answer = Some(answer);
        }
    });
}

/// Acts on a finished check: reissues the command, approved, or refuses it.
fn finish_check() {
    let Ok(mut slot) = CHECK.try_lock() else {
        return;
    };
    if slot.as_ref().is_none_or(|check| check.answer.is_none()) {
        return;
    }
    let Some(Check {
        command,
        address,
        answer: Some(answer),
    }) = slot.take()
    else {
        return;
    };
    drop(slot);
    let line = format!("{} {address}", command.name());
    match verdict(&answer) {
        Ok(()) => {
            let Ok(cmd) = CString::new(format!("{line}\n")) else {
                return;
            };
            if let Ok(mut approved) = APPROVED.lock() {
                *approved = Some((command, address.clone()));
            }
            crate::commands::console_print(&format!(
                "DoD Studio: {address} is an HLTV proxy with VAC off. Joining.\n"
            ));
            unsafe {
                crate::debug::report(&format!(
                    "connect_guard: allowed `{line}` (HLTV proxy, VAC off)"
                ))
            };
            engine::client_cmd(&cmd);
        }
        Err(why) => refuse(command, Some(&address), why),
    }
}

fn refuse(command: Command, argument: Option<&str>, why: &str) {
    let line = match argument {
        Some(argument) => format!("{} {argument}", command.name()),
        None => command.name().to_string(),
    };
    crate::commands::console_print(&refusal_message(&line, why));
    unsafe { crate::debug::report(&format!("connect_guard: refused `{line}`: {why}")) };
}

/// What the console shows for a refused `line`, and `why`. It leaves out
/// `GOLDSRC_HOOKS_ALLOW_CONNECT`: an environment variable set before the game
/// starts is a developer's escape hatch, documented in `docs/vac_safety.md`,
/// not something to offer every player who hits this.
fn refusal_message(line: &str, why: &str) -> String {
    format!(
        "DoD Studio refused `{line}`: {why}. With DoD Studio's hook DLL loaded, the game only \
         joins HLTV proxies that say VAC is off; joining a game server risks a VAC ban. To play \
         online, close the game and start Day of Defeat from Steam, without DoD Studio.\n"
    )
}

/// The command's argument, as the engine's own `connect` reads it: the rest
/// of the line (`Cmd_Args`), which `cl_enginefunc_t` doesn't offer. GoldSrc's
/// tokenizer makes `:` a token of its own, so `connect 1.2.3.4:27020` arrives
/// as `1.2.3.4`, `:`, `27020`; [`join_tokens`] puts them back together. Taking
/// only the first token loses the port (found live, 2026-10-05).
fn command_argument() -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    let tokens: Vec<String> = unsafe {
        (1..(engfuncs.cmd_argc)())
            .filter_map(|i| {
                let arg = (engfuncs.cmd_argv)(i);
                (!arg.is_null()).then(|| CStr::from_ptr(arg).to_string_lossy().into_owned())
            })
            .collect()
    };
    join_tokens(&tokens)
}

/// The tokens after the command name, joined with nothing between them, as
/// an address is written. None when there are none.
fn join_tokens<S: AsRef<str>>(tokens: &[S]) -> Option<String> {
    let joined: String = tokens.iter().map(AsRef::as_ref).collect();
    (!joined.is_empty()).then_some(joined)
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

    const NONE: Session = Session::None;

    #[test]
    fn connecting_to_an_address_asks_it_first() {
        for server in ["1.2.3.4:27015", "dod.example.com", "localhost", "127.0.0.1"] {
            assert_eq!(
                decide(Command::Connect, Some(server), false, NONE, false),
                Action::Check
            );
            assert_eq!(
                decide(Command::Connect, Some(server), false, Session::Hltv, true),
                Action::Check
            );
            assert_eq!(
                decide(Command::Listen, Some(server), false, NONE, false),
                Action::Check
            );
        }
    }

    #[test]
    fn the_reissued_command_goes_through() {
        assert_eq!(
            decide(Command::Connect, Some("1.2.3.4:27020"), true, NONE, false),
            Action::Allow
        );
        assert_eq!(
            decide(Command::Listen, Some("1.2.3.4:27020"), true, NONE, false),
            Action::Allow
        );
    }

    #[test]
    fn the_games_own_listen_server_is_allowed() {
        assert_eq!(
            decide(Command::Connect, Some("local"), false, NONE, false),
            Action::Allow
        );
        assert_eq!(
            decide(Command::Connect, Some(" LOCAL "), false, NONE, false),
            Action::Allow
        );
    }

    #[test]
    fn a_bare_connect_or_listen_only_prints_its_usage() {
        assert_eq!(
            decide(Command::Connect, None, false, NONE, false),
            Action::Allow
        );
        assert_eq!(
            decide(Command::Listen, None, false, NONE, false),
            Action::Allow
        );
    }

    #[test]
    fn retry_is_handed_on_since_what_it_queues_is_checked() {
        assert_eq!(
            decide(Command::Retry, None, false, NONE, false),
            Action::Allow
        );
    }

    #[test]
    fn reconnect_only_on_your_own_map_an_hltv_proxy_or_in_a_demo() {
        assert_eq!(
            decide(Command::Reconnect, None, false, NONE, false),
            Action::Refuse
        );
        assert_eq!(
            decide(Command::Reconnect, None, false, Session::Local, false),
            Action::Allow
        );
        assert_eq!(
            decide(Command::Reconnect, None, false, Session::Hltv, false),
            Action::Allow
        );
        assert_eq!(
            decide(Command::Reconnect, None, false, NONE, true),
            Action::Allow
        );
    }

    #[test]
    fn only_an_hltv_proxy_with_vac_off_is_joined() {
        let info = |server_type, vac| Ok(ServerInfo { server_type, vac });
        assert_eq!(verdict(&info(b'p', false)), Ok(()));
        assert!(
            verdict(&info(b'p', true))
                .unwrap_err()
                .contains("VAC is on")
        );
        assert!(
            verdict(&info(b'd', false))
                .unwrap_err()
                .contains("game server")
        );
        assert!(verdict(&info(b'l', false)).is_err());
        assert!(verdict(&Err(QueryError::NoAnswer)).is_err());
        assert!(verdict(&Err(QueryError::Unreadable)).is_err());
        assert!(verdict(&Err(QueryError::BadAddress("x".into()))).is_err());
    }

    #[test]
    fn an_address_split_at_its_colon_is_put_back_together() {
        assert_eq!(
            join_tokens(&["74.91.112.242", ":", "27020"]).as_deref(),
            Some("74.91.112.242:27020")
        );
        assert_eq!(join_tokens(&["local"]).as_deref(), Some("local"));
        assert_eq!(join_tokens::<&str>(&[]), None);
    }

    #[test]
    fn only_a_plain_address_is_put_back_on_a_command_line() {
        for ok in ["1.2.3.4:27020", "hltv.example.com", "my-proxy_1:27020"] {
            assert!(is_plain_address(ok), "{ok}");
        }
        for bad in ["", "1.2.3.4;quit", "a b", "\"a\"", "1.2.3.4\nquit"] {
            assert!(!is_plain_address(bad), "{bad:?}");
        }
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
    fn the_refusal_names_the_command_the_reason_and_the_way_out() {
        let message = refusal_message("connect 1.2.3.4:27015", "it is a game server");
        assert!(message.contains("`connect 1.2.3.4:27015`: it is a game server."));
        assert!(message.contains("HLTV"));
        assert!(message.contains("VAC"));
        assert!(message.contains("from Steam"));
        // The developer's env var stays out of a player's console.
        assert!(!message.contains(ALLOW_ENV));
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
