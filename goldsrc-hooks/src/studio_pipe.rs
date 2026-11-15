//! Which DoD Studio the game asks for analyses and reviews (#1235).
//!
//! Each Studio serves its game pipe under a name of its own and puts that
//! name on the command line of every game it starts, as
//! `-dodstudio_pipe dodstudio-analyzer-<Studio's pid>`
//! (`native::sys::studio_pipe`). The game asks that pipe, so with two
//! Studios open each game asks the one that started it.
//!
//! Without the flag (a game started outside Studio, or by a Studio from
//! before #1235), or once the Studio that named it has closed, the game asks
//! the plain [`PLAIN_PIPE`], which the first Studio open serves.

use std::sync::OnceLock;

/// The pipe every Studio's first instance serves; must match
/// `native::sys::analysis_server::PIPE_NAME` to the character.
pub(crate) const PLAIN_PIPE: &str = r"\\.\pipe\dodstudio-analyzer";

/// The command-line flag naming the Studio's own pipe; must match
/// `native::sys::studio_pipe::PIPE_ARG`.
const PIPE_FLAG: &str = "-dodstudio_pipe";

/// A Studio's own pipe name is this and its pid; must match
/// `native::sys::studio_pipe::INSTANCE_PREFIX`.
const INSTANCE_PREFIX: &str = "dodstudio-analyzer-";

const PIPE_ROOT: &str = r"\\.\pipe\";

/// The pipe `args` (the game's command line, program first) names after
/// [`PIPE_FLAG`], as a full pipe path. Only a Studio pipe name is taken
/// (the prefix and a pid), so the flag can't point the game at any other
/// pipe.
fn named_pipe<S: AsRef<str>>(args: &[S]) -> Option<String> {
    let at = args
        .iter()
        .skip(1)
        .position(|a| a.as_ref().eq_ignore_ascii_case(PIPE_FLAG))?;
    let value = args.get(at + 2)?.as_ref();
    let pid = value.strip_prefix(INSTANCE_PREFIX)?;
    let is_pid = !pid.is_empty() && pid.len() <= 10 && pid.bytes().all(|b| b.is_ascii_digit());
    is_pid.then(|| format!("{PIPE_ROOT}{value}"))
}

/// The pipe the launch line names, read once.
fn launch_pipe() -> Option<&'static str> {
    static NAMED: OnceLock<Option<String>> = OnceLock::new();
    NAMED
        .get_or_init(|| {
            // `args_os`, not `args`: `std::env::args()` panics on an
            // argument that isn't valid Unicode, and a panic here is a game
            // crash under `panic = "abort"`. A lossy value just isn't a
            // Studio pipe name.
            let args: Vec<String> = std::env::args_os()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let named = named_pipe(&args);
            let line = match &named {
                Some(name) => format!("studio pipe: asking {name} ({PIPE_FLAG})"),
                None => format!("studio pipe: no {PIPE_FLAG}, asking {PLAIN_PIPE}"),
            };
            // Safety: `report` only appends to the log file.
            unsafe { crate::debug::report(&line) };
            named
        })
        .as_deref()
}

/// The pipes to try, in order: the launch line's, then the plain one.
fn pipes() -> impl Iterator<Item = &'static str> {
    launch_pipe().into_iter().chain([PLAIN_PIPE])
}

/// Opens Studio's game pipe for one request: the launch line's pipe, or the
/// plain one when the launch line names none or nobody serves the one it
/// names (its Studio has closed). A named pipe that exists but fails to open
/// (busy) is that Studio's answer, so it is not passed over for another's.
pub(crate) fn connect() -> std::io::Result<std::fs::File> {
    let mut last = None;
    for name in pipes() {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
        {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => last = Some(e),
            result => return result,
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::NotFound.into()))
}

/// Whether a Studio serves one of [`pipes`] now. `WaitNamedPipeW` with a
/// 1 ms wait returns at once whether the pipe exists or not (Studio always
/// keeps an instance listening).
#[cfg(windows)]
pub(crate) fn studio_serves() -> bool {
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    pipes().any(|name| {
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // Safety: a NUL-terminated name; a 1 ms wait at most.
        unsafe { WaitNamedPipeW(wide.as_ptr(), 1) != 0 }
    })
}

#[cfg(not(windows))]
pub(crate) fn studio_serves() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<&str> {
        line.split(' ').collect()
    }

    #[test]
    fn the_launch_line_names_a_studios_own_pipe() {
        assert_eq!(
            named_pipe(&args(
                "hl.exe -game dod -dodstudio_pipe dodstudio-analyzer-4242 +viewdemo x"
            )),
            Some(r"\\.\pipe\dodstudio-analyzer-4242".to_string())
        );
        assert_eq!(
            named_pipe(&args("hl.exe -DODSTUDIO_PIPE dodstudio-analyzer-7")),
            Some(r"\\.\pipe\dodstudio-analyzer-7".to_string())
        );
    }

    #[test]
    fn without_the_flag_or_its_value_no_pipe_is_named() {
        assert_eq!(named_pipe(&args("hl.exe -game dod")), None);
        assert_eq!(named_pipe(&args("hl.exe -dodstudio_pipe")), None);
        // The program name is never the flag.
        assert_eq!(
            named_pipe(&args("-dodstudio_pipe dodstudio-analyzer-1")),
            None
        );
    }

    #[test]
    fn only_a_studio_pipe_name_is_taken() {
        for value in [
            "other-pipe",
            "dodstudio-analyzer-",
            "dodstudio-analyzer-12a",
            r"dodstudio-analyzer-1\..\x",
            "dodstudio-analyzer-12345678901",
            "+quit",
        ] {
            assert_eq!(
                named_pipe(&["hl.exe", "-dodstudio_pipe", value]),
                None,
                "{value}"
            );
        }
    }

    #[test]
    fn the_names_match_studios() {
        let server = include_str!("../../native/src/sys/analysis_server.rs");
        assert!(server.contains(&format!("pub const PIPE_NAME: &str = r\"{PLAIN_PIPE}\";")));
        let names = include_str!("../../native/src/sys/studio_pipe.rs");
        assert!(names.contains(&format!("pub const PIPE_ARG: &str = \"{PIPE_FLAG}\";")));
        assert!(names.contains(&format!(
            "pub const INSTANCE_PREFIX: &str = \"{INSTANCE_PREFIX}\";"
        )));
    }
}
