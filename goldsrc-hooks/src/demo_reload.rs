//! `ds_demo_reload`: play the current demo again from the start (#330).
//!
//! ## Nothing in the engine keeps the name
//!
//! `playdemo`'s handler (pre-Anniversary `hw.dll+0x105fd`) copies `Cmd_Argv(1)`
//! into a 256-byte buffer on its own stack, adds `.dem`, prints "Playing demo
//! from %s." and opens the file. The only thing it keeps is the open file
//! handle and the demo's header; the name is gone when the function returns.
//! So the DLL remembers it: it wraps the engine's own `playdemo` and
//! `viewdemo` commands, notes the name each time one runs, and hands straight
//! on to the real handler. `ds_demo_reload` then runs the same command
//! with the same name.
//!
//! ## Wrapping an engine command without an offset
//!
//! Through the engine's own command list, with no per-build address; see
//! [`crate::cmd_list`].
//!
//! The first attempt is at [`crate::commands::install`], right after
//! `client.dll`'s `Initialize`. In case the engine has not registered
//! `playdemo` by then, [`poll`] retries every frame for a few seconds.
//!
//! ## A demo started from the command line
//!
//! Launch Preview and capture start the game with `+viewdemo <name>` or
//! `+playdemo <name>` on its command line, and the engine runs that before
//! the wrap is in: live, the demo was a quarter of a second in when the wrap
//! landed. So with no name noted, `ds_demo_reload` takes the last
//! `+playdemo`/`+viewdemo` from the game's own command line instead.
//!
//! ## `timedemo` forgets the name
//!
//! `timedemo` (the engine's old benchmark, which DoD Studio never runs)
//! starts its demo through `playdemo`'s code without running the `playdemo`
//! command, so the wrap never sees its name. It is wrapped too, only to
//! forget the remembered name: after it, nothing here names a demo, rather
//! than naming the one played before it (a damaged `timedemo` was reported
//! against that other demo, #1394). The `startdemos` cycle needs nothing:
//! `Host_NextDemo` queues `playdemo %s` as console text (pre-Anniversary
//! `hw.dll+0x5d035`, Anniversary `+0x1ce6a9`), which runs the wrapped command.

// Only the 32-bit build installs anything; a host check still compiles it.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::cmd_list::{self, call_real, wrap};
use crate::engine;
use crate::names::console_name;

pub const NAME: &str = console_name!("demo_reload");

/// The engine's own handlers, called by the wrappers.
static REAL_PLAYDEMO: AtomicUsize = AtomicUsize::new(0);
static REAL_VIEWDEMO: AtomicUsize = AtomicUsize::new(0);
static REAL_TIMEDEMO: AtomicUsize = AtomicUsize::new(0);
static WRAPPED: AtomicBool = AtomicBool::new(false);

/// How many more frames `poll` retries a wrap that found nothing.
static RETRIES_LEFT: AtomicU32 = AtomicU32::new(300);

/// What `ds_demo_reload` says after a `timedemo`.
const AFTER_TIMEDEMO: &str = "the last demo was started with timedemo, which does not keep its name -- start it with playdemo or viewdemo";

/// The last demo command this session.
#[derive(Debug, Clone, PartialEq)]
enum Last {
    /// None yet: the game's command line may name one.
    Unset,
    /// `playdemo`/`viewdemo` and the name it was given.
    Played(&'static str, String),
    /// A `timedemo`, whose name is not known (see the module doc).
    Forgotten,
}

thread_local! {
    /// Console commands run on the engine's main thread, like everything
    /// else here.
    static LAST: RefCell<Last> = const { RefCell::new(Last::Unset) };
}

/// Wraps `playdemo` and `viewdemo`. Called once engfuncs are captured.
pub fn install() {
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    }
}

/// Retries a wrap that has not found `playdemo` yet, for a few seconds of
/// frames, then says so once. A no-op after that, or once wrapped.
pub fn poll() {
    if RETRIES_LEFT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    } else if RETRIES_LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
        unsafe {
            crate::debug::report(&format!(
                "{NAME}: no playdemo command in the engine's command list -- {NAME} will not work"
            ))
        };
    }
}

fn try_wrap() -> bool {
    if WRAPPED.load(Ordering::Relaxed) {
        return true;
    }
    let mut found_playdemo = false;
    let listed = cmd_list::for_each(|name, entry| {
        if name.eq_ignore_ascii_case(b"playdemo") {
            found_playdemo |= wrap(entry, &REAL_PLAYDEMO, wrapped_playdemo);
        } else if name.eq_ignore_ascii_case(b"viewdemo") {
            wrap(entry, &REAL_VIEWDEMO, wrapped_viewdemo);
        } else if name.eq_ignore_ascii_case(b"timedemo") {
            wrap(entry, &REAL_TIMEDEMO, wrapped_timedemo);
        }
    });
    if !listed {
        return false;
    }
    if found_playdemo {
        WRAPPED.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "{NAME}: wrapped playdemo{} to remember the demo name",
                if REAL_VIEWDEMO.load(Ordering::Relaxed) != 0 {
                    " and viewdemo"
                } else {
                    ""
                }
            ))
        };
    }
    found_playdemo
}

unsafe extern "C" fn wrapped_playdemo() {
    if let Some(name) = remember("playdemo") {
        crate::map_cfgs::before_demo(&name);
    }
    let named = !crate::cmd_list::args().is_empty();
    unsafe { call_real(&REAL_PLAYDEMO) };
    if named {
        crate::demo_seek::demo_started(crate::demo_seek::DemoCommand::Playdemo);
    }
}

unsafe extern "C" fn wrapped_viewdemo() {
    // A bare viewdemo brings a closed VCR bar back without restarting the
    // demo (or prints its usage with none loaded). With
    // ds_ui_viewdemo_in_panel it does that, so the bar can lend its slider
    // again, and then opens the DoD Studio window on Playback.
    if crate::cmd_list::args().is_empty() && crate::studio_panel::viewdemo_in_panel() {
        unsafe { call_real(&REAL_VIEWDEMO) };
        crate::studio_panel::bare_viewdemo();
        return;
    }
    if let Some(name) = remember("viewdemo") {
        crate::map_cfgs::before_demo(&name);
    }
    unsafe { call_real(&REAL_VIEWDEMO) };
    // A seek later on this command line waits for this demo to load (#928).
    crate::demo_seek::demo_started(crate::demo_seek::DemoCommand::Viewdemo);
    // ds_ui_viewdemo_in_panel: the DoD Studio window stands in for the bar.
    crate::studio_panel::after_viewdemo();
}

/// Forgets the remembered name before a named `timedemo` starts; a bare one
/// only prints its usage and changes nothing.
unsafe extern "C" fn wrapped_timedemo() {
    if !crate::cmd_list::args().is_empty() {
        LAST.with(|last| *last.borrow_mut() = Last::Forgotten);
    }
    unsafe { call_real(&REAL_TIMEDEMO) };
}

/// Notes the name the command was given and returns it. A bare `playdemo`
/// (it prints its usage), or a name that can't be replayed, changes nothing
/// and returns `None`.
fn remember(command: &'static str) -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    let name = unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return None;
        }
        let arg = (engfuncs.cmd_argv)(1);
        if arg.is_null() {
            return None;
        }
        CStr::from_ptr(arg).to_string_lossy().trim().to_string()
    };
    replay_line(command, &name)?;
    LAST.with(|last| *last.borrow_mut() = Last::Played(command, name.clone()));
    Some(name)
}

/// The console line that plays `name` again with `command`, or `None` for a
/// name that cannot be replayed safely: empty, or carrying a quote or a line
/// break that would end the argument early or start a second command.
fn replay_line(command: &str, name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['"', '\n', '\r', ';']) {
        return None;
    }
    Some(format!("{command} \"{name}\"\n"))
}

/// The last `+playdemo <name>` or `+viewdemo <name>` among the game's
/// command-line arguments, for a demo started at launch (see the module doc).
/// A `+` or `-` argument right after the command is the next option, not a
/// name. A named `+timedemo` after the last of them forgets it, as the
/// command does once the wrap is in.
fn from_command_line(args: impl IntoIterator<Item = String>) -> Option<(&'static str, String)> {
    let mut found = None;
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        let command = if arg.eq_ignore_ascii_case("+playdemo") {
            "playdemo"
        } else if arg.eq_ignore_ascii_case("+viewdemo") {
            "viewdemo"
        } else if arg.eq_ignore_ascii_case("+timedemo") {
            "timedemo"
        } else {
            continue;
        };
        if let Some(name) = args.next_if(|next| !next.starts_with(['+', '-'])) {
            let name = name.trim().to_string();
            if command == "timedemo" {
                found = None;
            } else if replay_line(command, &name).is_some() {
                found = Some((command, name));
            }
        }
    }
    found
}

/// The last `playdemo`/`viewdemo` and its demo name: this session's, or
/// with none yet, `command_line`'s. `None` after a `timedemo`.
fn resolve(
    last: Last,
    command_line: impl FnOnce() -> Option<(&'static str, String)>,
) -> Option<(&'static str, String)> {
    match last {
        Last::Played(command, name) => Some((command, name)),
        Last::Forgotten => None,
        Last::Unset => command_line(),
    }
}

/// [`resolve`] on this session. Main thread only (where commands run).
fn last_played() -> Option<(&'static str, String)> {
    resolve(LAST.with(|last| last.borrow().clone()), || {
        // `args_os`, not `args`: `std::env::args()` panics on any argument
        // that isn't valid Unicode, and a panic in a console command handler
        // is a game crash under `panic = "abort"`. A lossy name just fails to
        // match and falls through to the "no demo" message.
        from_command_line(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()))
    })
}

/// The name the demo last played was given (relative to the game folder,
/// maybe without `.dem`), for the DoD Studio window's Highlights tab and the
/// damaged-demo notice. `None` after a `timedemo`.
pub fn current_demo() -> Option<String> {
    last_played().map(|(_, name)| name)
}

/// `ds_demo_reload`: runs the last `playdemo`/`viewdemo` again.
pub unsafe extern "C" fn command() {
    let last = last_played();
    let Some((command, name)) = last else {
        let why = if LAST.with(|last| *last.borrow() == Last::Forgotten) {
            AFTER_TIMEDEMO
        } else if WRAPPED.load(Ordering::Relaxed) {
            "no demo has been played this session yet -- start one with playdemo or viewdemo first"
        } else {
            "the engine's playdemo command was not found, so the demo name cannot be tracked"
        };
        crate::commands::console_print(&format!("{NAME}: {why}\n"));
        return;
    };
    let Some(line) = replay_line(command, &name) else {
        return;
    };
    crate::commands::console_print(&format!("{NAME}: {}\n", line.trim_end()));
    if !client_cmd(&line) {
        crate::commands::console_print(&format!(
            "{NAME}: the engine's function table is not available\n"
        ));
    }
}

/// Runs `line` through the engine's `pfnClientCmd` (`engine::client_cmd`,
/// shared with the Studio pipe).
fn client_cmd(line: &str) -> bool {
    CString::new(line).is_ok_and(|line| engine::client_cmd(&line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_line_quotes_the_name() {
        assert_eq!(
            replay_line("playdemo", "test/my demo"),
            Some("playdemo \"test/my demo\"\n".to_string())
        );
        assert_eq!(
            replay_line("viewdemo", "../other/match.dem"),
            Some("viewdemo \"../other/match.dem\"\n".to_string())
        );
    }

    #[test]
    fn names_that_would_break_the_line_are_not_replayed() {
        assert_eq!(replay_line("playdemo", ""), None);
        assert_eq!(replay_line("playdemo", "a\"; quit"), None);
        assert_eq!(replay_line("playdemo", "a;quit"), None);
        assert_eq!(replay_line("playdemo", "a\nquit"), None);
    }

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn a_launch_preview_command_line_gives_its_demo() {
        let line = "hl.exe -game dod -insecure -demoedit -windowed -w 1280 -h 720 \
                    -afxForceAlpha8 1 -condebug \
                    +viewdemo k4_ktps9w1_gskill_allies_preview";
        assert_eq!(
            from_command_line(args(line)),
            Some(("viewdemo", "k4_ktps9w1_gskill_allies_preview".to_string()))
        );
    }

    #[test]
    fn a_capture_command_line_gives_its_demo() {
        let line = "hl.exe -game dod +exec dodstudio_helper.cfg +playdemo match1 -condebug";
        assert_eq!(
            from_command_line(args(line)),
            Some(("playdemo", "match1".to_string()))
        );
    }

    #[test]
    fn the_last_demo_on_the_command_line_wins_and_options_are_not_names() {
        assert_eq!(
            from_command_line(args("hl.exe +PlayDemo first +viewdemo second")),
            Some(("viewdemo", "second".to_string()))
        );
        assert_eq!(from_command_line(args("hl.exe +viewdemo -condebug")), None);
        assert_eq!(
            from_command_line(args("hl.exe +viewdemo +exec x.cfg")),
            None
        );
        assert_eq!(from_command_line(args("hl.exe +viewdemo")), None);
        assert_eq!(from_command_line(args("hl.exe -game dod")), None);
    }

    #[test]
    fn a_timedemo_on_the_command_line_forgets_the_demo_before_it() {
        assert_eq!(
            from_command_line(args("hl.exe +viewdemo healthy +timedemo damaged")),
            None
        );
        assert_eq!(
            from_command_line(args("hl.exe +timedemo bench +playdemo match1")),
            Some(("playdemo", "match1".to_string()))
        );
        // A bare +timedemo only prints its usage.
        assert_eq!(
            from_command_line(args("hl.exe +viewdemo healthy +timedemo -condebug")),
            Some(("viewdemo", "healthy".to_string()))
        );
    }

    #[test]
    fn a_timedemo_in_the_session_names_no_demo_even_with_one_on_the_command_line() {
        let command_line = || Some(("playdemo", "from_launch".to_string()));
        assert_eq!(resolve(Last::Forgotten, command_line), None);
        assert_eq!(
            resolve(Last::Unset, command_line),
            Some(("playdemo", "from_launch".to_string()))
        );
        assert_eq!(
            resolve(Last::Played("viewdemo", "typed".to_string()), command_line),
            Some(("viewdemo", "typed".to_string()))
        );
    }

    #[test]
    fn a_command_line_name_that_would_break_the_line_is_not_used() {
        assert_eq!(
            from_command_line(vec!["+playdemo".to_string(), "a;quit".to_string()]),
            None
        );
    }
}
