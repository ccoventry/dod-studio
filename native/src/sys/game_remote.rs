//! Sending console commands to a game DoD Studio launched (issue #413).
//!
//! The hook DLL (`goldsrc-hooks/src/remote.rs`) serves a named pipe in every
//! game Studio starts, named after that `hl.exe`'s process id. Writing lines
//! to it runs them as console commands on the game's next frame. A game that
//! wasn't started by Studio has no pipe, and that is how the caller tells the
//! two apart.

use std::io::Write;
use std::time::Duration;

/// The pipe of the game running as `pid`. Must match `goldsrc-hooks`'
/// `remote::pipe_name` to the character.
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\dodstudio-hl-{pid}")
}

/// Why a command can't be sent as it is.
pub fn check_command(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("empty console command".to_string());
    }
    if command.len() > 1024 {
        return Err(format!(
            "console command is {} bytes, over the 1024 the game accepts",
            command.len()
        ));
    }
    if command.chars().any(|c| c.is_control() && c != '\t') {
        return Err(format!(
            "console command {command:?} contains a line break or control character"
        ));
    }
    Ok(())
}

/// What happened to a send.
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    /// The game took the commands; it runs them on its next frame.
    Delivered,
    /// No game with that process id is listening: it wasn't started by
    /// Studio, or its hooks are off.
    NotListening,
}

/// The pipe serves one client at a time; a second one gets "busy" and tries
/// again this many times, this far apart.
const BUSY_RETRIES: u32 = 20;
const BUSY_WAIT: Duration = Duration::from_millis(25);
/// `ERROR_PIPE_BUSY`.
const ERROR_PIPE_BUSY: i32 = 231;

/// Sends `commands` to the game running as `pid`, one per line.
pub fn send_console_commands(pid: u32, commands: &[String]) -> Result<Sent, String> {
    for command in commands {
        check_command(command)?;
    }
    let name = pipe_name(pid);
    let mut attempt = 0;
    let mut pipe = loop {
        match std::fs::OpenOptions::new().write(true).open(&name) {
            Ok(pipe) => break pipe,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Sent::NotListening),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempt < BUSY_RETRIES => {
                attempt += 1;
                std::thread::sleep(BUSY_WAIT);
            }
            Err(e) => return Err(format!("could not open {name}: {e}")),
        }
    };
    let mut message = commands.join("\n");
    message.push('\n');
    pipe.write_all(message.as_bytes())
        .and_then(|()| pipe.flush())
        .map_err(|e| format!("could not write to {name}: {e}"))?;
    Ok(Sent::Delivered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipe_is_named_after_the_game_process() {
        // goldsrc-hooks' remote.rs pins the same string.
        assert_eq!(pipe_name(4242), r"\\.\pipe\dodstudio-hl-4242");
    }

    #[test]
    fn a_command_must_be_one_short_line() {
        assert!(check_command("viewdemo \"temp demos/a_preview\"").is_ok());
        assert!(check_command("echo a;echo b").is_ok());
        assert!(check_command("   ").is_err());
        assert!(check_command("echo a\necho b").is_err());
        assert!(check_command(&"x".repeat(1025)).is_err());
    }

    #[test]
    fn a_game_with_no_pipe_is_reported_as_not_listening() {
        // No process has this id's pipe; the send says so rather than failing.
        assert_eq!(
            send_console_commands(u32::MAX - 7, &["echo hi".to_string()]),
            Ok(Sent::NotListening)
        );
    }

    #[test]
    fn a_bad_command_is_refused_before_anything_is_opened() {
        assert!(send_console_commands(u32::MAX - 7, &["a\nb".to_string()]).is_err());
    }
}
