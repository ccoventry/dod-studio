// batch_close.rs
// What Studio's close prompt needs to know about a running batch (#545).
//
// After Start, Studio launches the game, waits for the hook's pipe and sends
// the batch's start about 5 s later (`native::game_handoff`). Closed in that
// window, Studio would leave the game at its main menu with no batch. So on
// close, `batch_close_prompt.js` waits here, a bounded time, for the start to
// reach the game: then the batch runs without Studio and the next start's
// catch-up marks it. If it never gets there, the prompt says the batch has not
// started and closing cancels it.

use native::game_handoff::{BatchStart, StartState};
use std::time::{Duration, Instant};

use crate::capture_manager::CaptureManager;

/// How often the wait looks again: the user is waiting on a closing window.
const POLL: Duration = Duration::from_millis(100);
/// The longest a close waits, whatever the frontend asks for.
const MAX_WAIT: Duration = Duration::from_secs(60);

/// Waits while the start is still on its way to the game (`Pending`), up to
/// `timeout`, and returns where it stands then. Stops early once `running`
/// says the batch is over (it ended or was cancelled).
fn wait_for_start(
    start: &BatchStart,
    timeout: Duration,
    poll: Duration,
    running: impl Fn() -> bool,
) -> StartState {
    let began = Instant::now();
    loop {
        let state = start.get();
        if state != StartState::Pending || !running() || began.elapsed() >= timeout {
            return state;
        }
        std::thread::sleep(poll);
    }
}

/// The running batch's start, once it is no longer on its way or
/// `timeout_ms` has passed: "preparing", "pending", "sent", "not_needed" or
/// "failed", or "not_running" when no batch runs.
#[tauri::command]
pub async fn wait_for_batch_start(
    state: tauri::State<'_, CaptureManager>,
    timeout_ms: u64,
) -> Result<String, String> {
    if !state.is_running() {
        return Ok("not_running".to_string());
    }
    let start = std::sync::Arc::clone(&state.batch_start);
    let is_running = std::sync::Arc::clone(&state.is_running);
    let cancel = std::sync::Arc::clone(&state.cancel_token);
    let timeout = Duration::from_millis(timeout_ms).min(MAX_WAIT);
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let running = || {
            *is_running.lock().unwrap_or_else(|p| p.into_inner())
                && !cancel.load(std::sync::atomic::Ordering::Relaxed)
        };
        let state = wait_for_start(&start, timeout, POLL, running);
        if state == StartState::Pending {
            native::log_markdown(&format!(
                "[capture] Closing: the batch's start had not reached the game after {} s",
                timeout.as_secs()
            ));
        }
        Ok(state.as_str().to_string())
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const FAST: Duration = Duration::from_millis(5);

    #[test]
    fn a_start_already_sent_or_not_needed_answers_at_once() {
        for state in [
            StartState::Sent,
            StartState::NotNeeded,
            StartState::Preparing,
        ] {
            let start = BatchStart::default();
            start.set(state);
            let began = Instant::now();
            assert_eq!(
                wait_for_start(&start, Duration::from_secs(10), FAST, || true),
                state
            );
            assert!(began.elapsed() < Duration::from_secs(1));
        }
    }

    #[test]
    fn a_pending_start_is_waited_for_until_it_is_sent() {
        let start = Arc::new(BatchStart::default());
        start.set(StartState::Pending);
        let sender = Arc::clone(&start);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            sender.set(StartState::Sent);
        });
        assert_eq!(
            wait_for_start(&start, Duration::from_secs(10), FAST, || true),
            StartState::Sent
        );
        handle.join().unwrap();
    }

    #[test]
    fn a_start_that_never_comes_gives_up_at_the_timeout() {
        let start = BatchStart::default();
        start.set(StartState::Pending);
        let began = Instant::now();
        assert_eq!(
            wait_for_start(&start, Duration::from_millis(40), FAST, || true),
            StartState::Pending
        );
        assert!(began.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn a_batch_that_ends_stops_the_wait() {
        let start = BatchStart::default();
        start.set(StartState::Pending);
        let began = Instant::now();
        assert_eq!(
            wait_for_start(&start, Duration::from_secs(10), FAST, || false),
            StartState::Pending
        );
        assert!(began.elapsed() < Duration::from_secs(1));
    }
}
