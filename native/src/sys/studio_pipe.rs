//! Which DoD Studio a game asks for analyses and reviews (#1235).
//!
//! Every Studio serves its game pipe (`analysis_server`) under a name of its
//! own, `\\.\pipe\dodstudio-analyzer-<Studio's pid>`, and puts that name on
//! the command line of every game it starts ([`launch_arg`]). The hook DLL
//! reads it back (`goldsrc-hooks/src/studio_pipe.rs`), so with two Studios
//! open each game asks the Studio that started it.
//!
//! The first Studio to start also serves the plain name,
//! [`analysis_server::PIPE_NAME`](crate::sys::analysis_server::PIPE_NAME), for
//! a game nothing named a pipe for: one started outside Studio, by a Studio
//! from before #1235, or whose Studio has closed since. A hook DLL from before
//! #1235 ignores the flag and asks the plain name too.
//!
//! The hook's side must match [`PIPE_ARG`] and [`INSTANCE_PREFIX`] to the
//! character; its tests check.

/// The game's command-line flag naming this Studio's pipe; the value is
/// [`instance_name`].
pub const PIPE_ARG: &str = "-dodstudio_pipe";

/// Every Studio's own pipe name starts with this; the rest is its pid.
pub const INSTANCE_PREFIX: &str = "dodstudio-analyzer-";

/// Where Windows keeps named pipes.
const PIPE_ROOT: &str = r"\\.\pipe\";

/// This Studio's own pipe name, without the `\\.\pipe\` root: what goes on
/// the game's command line.
pub fn instance_name() -> String {
    format!("{INSTANCE_PREFIX}{}", std::process::id())
}

/// This Studio's own pipe, as `analysis_server` serves it.
pub fn instance_pipe() -> String {
    format!("{PIPE_ROOT}{}", instance_name())
}

/// What `build_hlae_process` adds to every game's command line.
pub fn launch_arg() -> String {
    format!("{PIPE_ARG} {}", instance_name())
}

/// The pipe name a game's command line (one argument per entry) carries
/// after [`PIPE_ARG`], if any.
pub fn pipe_on_command_line(cmd: &[String]) -> Option<&str> {
    let at = cmd.iter().position(|a| a.eq_ignore_ascii_case(PIPE_ARG))?;
    cmd.get(at + 1).map(String::as_str)
}

/// What decides which Studio a running game asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GamePipeFacts<'a> {
    /// The name on the game's command line ([`pipe_on_command_line`]).
    pub named: Option<&'a str>,
    /// Some Studio serves that name now.
    pub named_served: bool,
    /// This Studio serves the plain name now.
    pub we_serve_plain: bool,
}

/// Whether a game asks this Studio (named `ours`): its own name when it was
/// given one that is still served, else the plain name, which only one
/// Studio serves. A game that asks another Studio is not borrowed (#1235).
pub fn game_asks(facts: &GamePipeFacts, ours: &str) -> bool {
    match facts.named {
        Some(name) if name.eq_ignore_ascii_case(ours) => true,
        Some(_) if facts.named_served => false,
        _ => facts.we_serve_plain,
    }
}

/// Whether running game `pid` asks this Studio ([`game_asks`]). A game that
/// isn't running any more, or whose command line can't be read, counts as
/// carrying no name.
pub fn game_asks_this_studio(pid: u32) -> bool {
    let games = crate::sys::process::running_games();
    let cmd = games
        .iter()
        .find(|g| g.pid == pid)
        .map(|g| g.cmd.as_slice())
        .unwrap_or_default();
    let named = pipe_on_command_line(cmd);
    let facts = GamePipeFacts {
        named,
        named_served: named.is_some_and(|n| is_served(&format!("{PIPE_ROOT}{n}"))),
        we_serve_plain: crate::sys::analysis_server::serves_plain_name(),
    };
    game_asks(&facts, &instance_name())
}

/// Whether some process serves pipe `name`. `WaitNamedPipeW` with a 1 ms
/// wait returns at once; it times out rather than failing "not found" when
/// every instance is busy, which still means it is served.
fn is_served(name: &str) -> bool {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // Safety: a NUL-terminated name; a 1 ms wait at most.
    if unsafe { WaitNamedPipeW(wide.as_ptr(), 1) } != 0 {
        return true;
    }
    // Safety: read straight after the call above, on the same thread.
    unsafe { GetLastError() != ERROR_FILE_NOT_FOUND }
}

const ERROR_FILE_NOT_FOUND: u32 = 2;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WaitNamedPipeW(name: *const u16, timeout_ms: u32) -> i32;
    fn GetLastError() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn the_launch_arg_names_this_studios_own_pipe() {
        assert_eq!(launch_arg(), format!("-dodstudio_pipe {}", instance_name()));
        assert_eq!(
            instance_pipe(),
            format!(r"\\.\pipe\dodstudio-analyzer-{}", std::process::id())
        );
    }

    #[test]
    fn the_pipe_name_is_read_back_off_a_game_command_line() {
        assert_eq!(
            pipe_on_command_line(&args(
                "hl.exe -game dod -dodstudio_pipe dodstudio-analyzer-42 +viewdemo x"
            )),
            Some("dodstudio-analyzer-42")
        );
        assert_eq!(
            pipe_on_command_line(&args("hl.exe -DODSTUDIO_PIPE dodstudio-analyzer-7")),
            Some("dodstudio-analyzer-7")
        );
        assert_eq!(pipe_on_command_line(&args("hl.exe -game dod")), None);
        assert_eq!(pipe_on_command_line(&args("hl.exe -dodstudio_pipe")), None);
        assert_eq!(pipe_on_command_line(&[]), None);
    }

    const OURS: &str = "dodstudio-analyzer-10";

    fn facts(named: Option<&str>, named_served: bool, we_serve_plain: bool) -> GamePipeFacts<'_> {
        GamePipeFacts {
            named,
            named_served,
            we_serve_plain,
        }
    }

    #[test]
    fn a_game_this_studio_named_asks_it() {
        assert!(game_asks(&facts(Some(OURS), true, false), OURS));
    }

    #[test]
    fn a_game_another_running_studio_named_does_not() {
        assert!(!game_asks(
            &facts(Some("dodstudio-analyzer-20"), true, true),
            OURS
        ));
    }

    #[test]
    fn a_game_with_no_name_or_a_gone_studios_asks_whoever_serves_the_plain_name() {
        for named in [None, Some("dodstudio-analyzer-20")] {
            assert!(game_asks(&facts(named, false, true), OURS));
            assert!(!game_asks(&facts(named, false, false), OURS));
        }
    }

    #[test]
    fn a_pipe_nobody_serves_is_not_served() {
        assert!(!is_served(
            r"\\.\pipe\dodstudio-analyzer-nobody-serves-this"
        ));
    }
}
