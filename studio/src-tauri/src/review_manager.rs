//! Review highlights (#623): hands the game a queue of highlights to play one
//! after another, and passes each answer it sends back to the frontend.
//!
//! The queue goes in a file (`review_queue.tsv` in the app data folder); the
//! game is told to read it with `dodstudio_review start "<file>"` over its
//! command pipe (#413). When no game is running, DoD Studio starts one as the
//! Launch Game button does and waits for its pipe; a running game started
//! with other launch settings is not reused (#666). The answers come back as
//! `[dod-studio] REVIEW` lines on the game's events pipe (#434): a thread
//! per game reads them for as long as that game runs and emits each one as a
//! `review_event`.

use std::io::{BufRead, BufReader};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use native::patch::launch_settings::{RunningGameCheck, check_running_game};
use native::review_queue::{ReviewHighlight, format_queue, parse_event};
use native::sys::game_remote::{self, Sent};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

const QUEUE_FILE: &str = "review_queue.tsv";
/// The event each review line becomes.
const EVENT: &str = "review_event";
/// How long a game DoD Studio just started gets to open its pipe.
const LAUNCH_WAIT: Duration = Duration::from_secs(120);
const RETRY: Duration = Duration::from_millis(500);

/// The game whose events pipe a thread is reading, if any.
static LISTENING: Mutex<Option<u32>> = Mutex::new(None);

#[derive(Debug, Serialize)]
pub struct ReviewStarted {
    pub pid: u32,
    /// DoD Studio started the game for this review.
    pub launched: bool,
    pub count: usize,
}

/// The console line that starts a review of `queue`.
fn start_line(queue: &std::path::Path) -> String {
    format!(
        "dodstudio_review start \"{}\"",
        queue.to_string_lossy().replace('\\', "/")
    )
}

/// Sends `line` to game `pid`. `Ok(None)` when it doesn't take commands.
fn try_send(pid: u32, line: &str) -> Result<Option<u32>, String> {
    match game_remote::send_console_commands(pid, &[line.to_string()])
        .map_err(crate::messages::failed_to_send_review)?
    {
        Sent::Delivered => Ok(Some(pid)),
        Sent::NotListening => Ok(None),
    }
}

/// Sends `line` to the running game `pid`, which must take commands: one
/// that doesn't wasn't started by DoD Studio.
fn send_to_game(pid: u32, line: &str) -> Result<Option<u32>, String> {
    try_send(pid, line)?
        .map(Some)
        .ok_or_else(|| crate::messages::REVIEW_GAME_NOT_FROM_STUDIO.to_string())
}

/// Sends `line` to the first running game that takes commands, and says
/// which. `Ok(None)` when none does. For the game DoD Studio just started.
fn send_to_any_game(line: &str) -> Result<Option<u32>, String> {
    for pid in native::sys::process::game_pids() {
        if let Some(pid) = try_send(pid, line)? {
            return Ok(Some(pid));
        }
    }
    Ok(None)
}

/// Starts reading `pid`'s answers, unless a thread already is.
fn ensure_listener(app: AppHandle, pid: u32) {
    {
        let mut listening = LISTENING.lock().unwrap_or_else(|e| e.into_inner());
        if *listening == Some(pid) {
            return;
        }
        *listening = Some(pid);
    }
    std::thread::spawn(move || {
        listen(&app, pid);
        let mut listening = LISTENING.lock().unwrap_or_else(|e| e.into_inner());
        if *listening == Some(pid) {
            *listening = None;
        }
    });
}

/// Reads `pid`'s events pipe until the game is gone, emitting each review
/// line. The pipe keeps lines written while nobody read it, so an answer
/// given during a reconnect still arrives.
fn listen(app: &AppHandle, pid: u32) {
    let name = game_remote::events_pipe_name(pid);
    loop {
        if !native::sys::process::game_pids().contains(&pid) {
            log::info!("[review] game {pid} has closed; no more answers to read");
            let _ = app.emit(
                EVENT,
                native::review_queue::ReviewEvent::Ended {
                    reason: "closed".to_string(),
                },
            );
            return;
        }
        let Ok(pipe) = std::fs::OpenOptions::new().read(true).open(&name) else {
            std::thread::sleep(RETRY);
            continue;
        };
        log::info!("[review] reading answers from game {pid}");
        for bytes in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&bytes);
            if let Some(event) = parse_event(&line) {
                log::info!("[review] {}", line.trim());
                let _ = app.emit(EVENT, event);
            }
        }
    }
}

/// Starts reviewing `highlights` in the running game, or in one DoD Studio
/// starts for it. `fast_forward_gap`: see [`format_queue`] (#665).
#[tauri::command]
pub async fn start_highlight_review(
    app: AppHandle,
    highlights: Vec<ReviewHighlight>,
    fast_forward_gap: Option<f64>,
) -> Result<ReviewStarted, String> {
    let count = highlights.len();
    let queue = native::shared::paths::get_appdata_dir().join(QUEUE_FILE);
    std::fs::write(&queue, format_queue(&highlights, fast_forward_gap))
        .map_err(crate::messages::could_not_write_review_queue)?;
    let line = start_line(&queue);

    let launch = crate::capture_manager::launch_config(
        &crate::capture_manager::saved_settings(&app),
        &crate::capture_manager::LaunchRequest::default(),
    );
    let sent = {
        let line = line.clone();
        crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
            // Only a game started with the settings Launch Game would use now
            // (#666). The frontend asks about any other before calling this,
            // so a mismatch here means it changed in between.
            match check_running_game(&launch.launch_settings()) {
                RunningGameCheck::None => Ok(None),
                RunningGameCheck::Match { pid } => send_to_game(pid, &line),
                RunningGameCheck::Mismatch { .. } => {
                    Err(crate::messages::REVIEW_GAME_LAUNCH_SETTINGS_DIFFER.to_string())
                }
            }
        }))
        .await?
    };
    let (pid, launched) = match sent {
        Some(pid) => (pid, false),
        None => {
            if !native::sys::process::game_pids().is_empty() {
                return Err(crate::messages::REVIEW_GAME_NOT_FROM_STUDIO.to_string());
            }
            crate::capture_manager::launch_standalone_game(app.clone()).await?;
            let pid =
                crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
                    let since = Instant::now();
                    while since.elapsed() < LAUNCH_WAIT {
                        if let Some(pid) = send_to_any_game(&line)? {
                            return Ok(pid);
                        }
                        std::thread::sleep(RETRY);
                    }
                    Err(crate::messages::REVIEW_GAME_DID_NOT_START.to_string())
                }))
                .await?;
            (pid, true)
        }
    };
    log::info!("[review] {count} highlight(s) sent to game {pid} (launched: {launched})");
    ensure_listener(app, pid);
    Ok(ReviewStarted {
        pid,
        launched,
        count,
    })
}

/// Ends the review in the game being read; its answers so far are already in.
#[tauri::command]
pub async fn stop_highlight_review() -> Result<(), String> {
    let pid = *LISTENING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(pid) = pid else { return Ok(()) };
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        game_remote::send_console_commands(pid, &["dodstudio_review stop".to_string()])
            .map(|_| ())
            .map_err(crate::messages::failed_to_send_review)
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_line_quotes_the_queue_with_forward_slashes() {
        let line = start_line(std::path::Path::new(
            r"C:\Users\me\AppData\Roaming\dod-studio\review_queue.tsv",
        ));
        assert_eq!(
            line,
            "dodstudio_review start \"C:/Users/me/AppData/Roaming/dod-studio/review_queue.tsv\""
        );
        assert!(game_remote::check_command(&line).is_ok());
    }
}
