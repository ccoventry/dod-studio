//! Sends a capture batch's first console lines to the game once it takes
//! commands over the hook DLL's pipe (#413), polled from the capture loop.
//!
//! A batch used to start its first demo from the launch line (`+playdemo`).
//! The engine runs `config.cfg` and `movie.cfg` only after the launch line,
//! so that demo loaded its map before the user's `brightness`, `gamma` and
//! `lightgamma` were set, and the map's textures and lighting were built with
//! the defaults. Every clip of the batch came out darker than the game looks
//! when played: mean frame brightness 51 against 72 for the same demo started
//! after the game was up (PRE, 2026-10-10). The cvars read the same either way
//! afterwards; only when the map loads matters. Started over the pipe, the
//! demo loads after the configs have run.

use std::time::{Duration, Instant};

use crate::sys::game_remote::{Sent, send_console_commands};

/// How long a launched game has to open its command pipe.
const PIPE_WAIT: Duration = Duration::from_secs(120);
/// After the pipe answers, how long before the lines are sent: the pipe opens
/// while the engine is still starting (the same wait Launch Preview makes).
const SETTLE: Duration = Duration::from_secs(5);

/// One look's outcome.
#[derive(Debug, PartialEq)]
pub enum Handoff {
    Waiting,
    /// Sent on this look, to this game.
    JustSent(u32),
    Sent,
    Failed(String),
}

pub struct GameHandoff {
    pub lines: Vec<String>,
    started: Instant,
    answered: Option<(u32, Instant)>,
    sent: bool,
}

impl GameHandoff {
    pub fn new(lines: Vec<String>) -> Self {
        Self {
            lines,
            started: Instant::now(),
            answered: None,
            sent: false,
        }
    }

    /// One look, from the capture loop; never blocks longer than one pipe write.
    pub fn poll(&mut self, game_pids: &[u32]) -> Handoff {
        if self.sent {
            return Handoff::Sent;
        }
        if let Some((pid, at)) = self.answered {
            if at.elapsed() < SETTLE {
                return Handoff::Waiting;
            }
            return match send_console_commands(pid, &self.lines) {
                Ok(Sent::Delivered) => {
                    self.sent = true;
                    Handoff::JustSent(pid)
                }
                Ok(Sent::NotListening) => Handoff::Failed(format!(
                    "the game (PID {pid}) stopped taking commands before the batch could start"
                )),
                Err(e) => Handoff::Failed(format!("could not start the batch in the game: {e}")),
            };
        }
        for &pid in game_pids {
            // An empty message checks the pipe without running anything.
            if matches!(send_console_commands(pid, &[]), Ok(Sent::Delivered)) {
                self.answered = Some((pid, Instant::now()));
                return Handoff::Waiting;
            }
        }
        if self.started.elapsed() > PIPE_WAIT {
            return Handoff::Failed(format!(
                "the game never opened its command pipe in {} s, so the batch could not start. \
                 Is the DoD Studio hook DLL turned on?",
                PIPE_WAIT.as_secs()
            ));
        }
        Handoff::Waiting
    }
}

/// How a batch's first demo starts: the launch-line arguments, and the lines
/// sent once the game is up (empty when everything is on the launch line).
/// With the hook DLL the demo is sent over its pipe; without it there is no
/// pipe, so the launch line keeps it.
pub fn first_demo_start(first_demo: &str, hook_dll: bool) -> (String, Vec<String>) {
    let exec = "+exec dodstudio_helper.cfg".to_string();
    if hook_dll {
        (exec, vec![format!("playdemo {first_demo}")])
    } else {
        (format!("{exec} +playdemo {first_demo}"), Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_the_hook_the_first_demo_waits_for_the_configs() {
        let (args, lines) = first_demo_start("dodstudio_primer", true);
        assert_eq!(args, "+exec dodstudio_helper.cfg");
        assert_eq!(lines, ["playdemo dodstudio_primer"]);
    }

    #[test]
    fn without_the_hook_it_stays_on_the_launch_line() {
        let (args, lines) = first_demo_start("dodstudio_primer", false);
        assert_eq!(
            args,
            "+exec dodstudio_helper.cfg +playdemo dodstudio_primer"
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn a_game_that_never_answers_waits_then_fails() {
        let mut handoff = GameHandoff::new(vec!["playdemo x".into()]);
        assert_eq!(handoff.poll(&[]), Handoff::Waiting);
        handoff.started -= PIPE_WAIT + Duration::from_secs(1);
        assert!(matches!(handoff.poll(&[]), Handoff::Failed(why) if why.contains("command pipe")));
    }
}
