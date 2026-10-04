//! `dodstudio_reload_demo`: play the current demo again from the start (#330).
//!
//! ## Nothing in the engine keeps the name
//!
//! `playdemo`'s handler (pre-Anniversary `hw.dll+0x105fd`) copies `Cmd_Argv(1)`
//! into a 256-byte buffer on its own stack, adds `.dem`, prints "Playing demo
//! from %s." and opens the file. The only thing it keeps is the open file
//! handle and the demo's header; the name is gone when the function returns.
//! So the DLL remembers it: it wraps the engine's own `playdemo` and
//! `viewdemo` commands, notes the name each time one runs, and hands straight
//! on to the real handler. `dodstudio_reload_demo` then runs the same command
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
//! landed. So with no name noted, `dodstudio_reload_demo` takes the last
//! `+playdemo`/`+viewdemo` from the game's own command line instead.

// Only the 32-bit build installs anything; a host check still compiles it.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::cmd_list::{self, call_real, wrap};
use crate::engine;
use crate::names::console_name;

pub const NAME: &str = console_name!("reload_demo");

/// The engine's own handlers, called by the wrappers.
static REAL_PLAYDEMO: AtomicUsize = AtomicUsize::new(0);
static REAL_VIEWDEMO: AtomicUsize = AtomicUsize::new(0);
static WRAPPED: AtomicBool = AtomicBool::new(false);

/// How many more frames `poll` retries a wrap that found nothing.
static RETRIES_LEFT: AtomicU32 = AtomicU32::new(300);

thread_local! {
    /// The last `playdemo`/`viewdemo` and the name it was given. Console
    /// commands run on the engine's main thread, like everything else here.
    static LAST: RefCell<Option<(&'static str, String)>> = const { RefCell::new(None) };
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
    remember("playdemo");
    unsafe { call_real(&REAL_PLAYDEMO) };
}

unsafe extern "C" fn wrapped_viewdemo() {
    // A bare viewdemo brings a closed VCR bar back without restarting the
    // demo (or prints its usage with none loaded). With
    // dodstudio_viewdemo_in_panel it does that, so the bar can lend its slider
    // again, and then opens the DoD Studio window on Playback.
    if crate::cmd_list::args().is_empty() && crate::studio_panel::viewdemo_in_panel() {
        unsafe { call_real(&REAL_VIEWDEMO) };
        crate::studio_panel::bare_viewdemo();
        return;
    }
    remember("viewdemo");
    unsafe { call_real(&REAL_VIEWDEMO) };
    // dodstudio_viewdemo_in_panel: the DoD Studio window stands in for the bar.
    crate::studio_panel::after_viewdemo();
}

/// Notes the name the command was given. A bare `playdemo` (it prints its
/// usage) changes nothing.
fn remember(command: &'static str) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let name = unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return;
        }
        let arg = (engfuncs.cmd_argv)(1);
        if arg.is_null() {
            return;
        }
        CStr::from_ptr(arg).to_string_lossy().trim().to_string()
    };
    if replay_line(command, &name).is_some() {
        LAST.with(|last| *last.borrow_mut() = Some((command, name)));
    }
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
/// name.
fn from_command_line(args: impl IntoIterator<Item = String>) -> Option<(&'static str, String)> {
    let mut found = None;
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        let command = if arg.eq_ignore_ascii_case("+playdemo") {
            "playdemo"
        } else if arg.eq_ignore_ascii_case("+viewdemo") {
            "viewdemo"
        } else {
            continue;
        };
        if let Some(name) = args.next_if(|next| !next.starts_with(['+', '-'])) {
            let name = name.trim().to_string();
            if replay_line(command, &name).is_some() {
                found = Some((command, name));
            }
        }
    }
    found
}

/// The last `playdemo`/`viewdemo` and its demo name, from this session or
/// the game's command line. Main thread only (where commands run).
fn last_played() -> Option<(&'static str, String)> {
    LAST.with(|last| last.borrow().clone())
        // `args_os`, not `args`: `std::env::args()` panics on any argument
        // that isn't valid Unicode, and a panic in a console command handler
        // is a game crash under `panic = "abort"`. A lossy name just fails to
        // match and falls through to the "no demo" message.
        .or_else(|| {
            from_command_line(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()))
        })
}

/// The name the demo last played was given (relative to the game folder,
/// maybe without `.dem`), for the DoD Studio window's Highlights tab.
pub fn current_demo() -> Option<String> {
    last_played().map(|(_, name)| name)
}

/// `dodstudio_reload_demo`: runs the last `playdemo`/`viewdemo` again.
pub unsafe extern "C" fn command() {
    let last = last_played();
    let Some((command, name)) = last else {
        let why = if WRAPPED.load(Ordering::Relaxed) {
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
        let line = "hl.exe -game dod -insecure -demoedit -windowed -w 1280 -h 720 -gl -32bpp \
                    -afxRenderMode standard -afxForceAlpha8 1 -condebug \
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
    fn a_command_line_name_that_would_break_the_line_is_not_used() {
        assert_eq!(
            from_command_line(vec!["+playdemo".to_string(), "a;quit".to_string()]),
            None
        );
    }
}
