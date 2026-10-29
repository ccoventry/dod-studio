//! The game asks Studio to review the demo it has loaded (#1142).
//!
//! The in-game DoD Studio window's Review tab has **From the start** and
//! **From here** buttons while no review runs. They send a request over
//! Studio's game pipe (`analysis_server`, which hands every `review ` line
//! here): Studio reviews that demo as if its Review highlights button had
//! been pressed for it alone, scanning it first when it isn't in the Master
//! Demo Queue, and tells the game how it is going until the review has
//! started or failed.
//!
//! ## Protocol
//!
//! One request per connection, text lines:
//!
//! - the game: `review start <absolute path to the .dem>`, or
//!   `review at <seconds> <absolute path>` to start at the first highlight
//!   whose first kill is at or after `seconds` on the demo player's clock;
//! - Studio: any number of `progress <text>` lines (what it is doing, for
//!   the game to show), then `done` once the review has been sent to the
//!   game, or `failed <why>`.
//!
//! The work is the frontend's: Tauri registers a [`set_review_handler`] that
//! hands each request to the page, which answers through [`report`] with the
//! request's id. A request nobody answers within [`ANSWER_TIMEOUT`] fails.
//!
//! The hook DLL's side is `goldsrc-hooks/src/review_ask.rs`.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Where a review the game asked for starts.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "seconds", rename_all = "snake_case")]
pub enum ReviewStart {
    /// The demo's first highlight.
    First,
    /// The first highlight at or after this time on the demo player's clock.
    At(f64),
}

/// One request from the game.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReviewRequest {
    /// Answers to this request go to [`report`] with this id.
    pub id: u64,
    /// The demo, absolute, with any `..` worked out.
    pub demo: PathBuf,
    pub start: ReviewStart,
}

/// What the frontend tells the game about a request.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewUpdate {
    /// A step, in words the game shows ("Scanning the demo...").
    Progress(String),
    /// The review was sent to the game.
    Done,
    Failed(String),
}

/// How long a request waits for the frontend's answer: a scan of one demo
/// takes seconds, a game DoD Studio has to start up to two minutes.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(600);

type Handler = Box<dyn Fn(ReviewRequest) + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();
static PENDING: Mutex<Option<HashMap<u64, Sender<ReviewUpdate>>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Sets what a request is handed to, once per process (Tauri's setup).
/// Until then every request fails with "DoD Studio is still starting".
pub fn set_review_handler(handler: impl Fn(ReviewRequest) + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(handler));
}

/// Passes the frontend's word on request `id` to the game. A request that
/// has already ended (answered, timed out, or the game hung up) takes no
/// more, which is not an error: the game no longer listens.
pub fn report(id: u64, update: ReviewUpdate) {
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(sender) = pending.as_ref().and_then(|map| map.get(&id)) {
        let _ = sender.send(update);
    }
}

/// Whether `line` is one of this module's requests, for the pipe's dispatch.
pub fn is_review_request(line: &str) -> bool {
    line.starts_with("review ")
}

/// Reads one request line into a demo and a start, or says why not.
pub fn parse_request(line: &str) -> Result<(PathBuf, ReviewStart), String> {
    let rest = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("review ")
        .ok_or("not a review request")?;
    let (start, path) = if let Some(path) = rest.strip_prefix("start ") {
        (ReviewStart::First, path)
    } else if let Some(rest) = rest.strip_prefix("at ") {
        let (seconds, path) = rest.split_once(' ').ok_or("no demo after the time")?;
        let seconds: f64 = seconds
            .parse()
            .ok()
            .filter(|s: &f64| s.is_finite() && *s >= 0.0)
            .ok_or_else(|| format!("{seconds:?} is not a time"))?;
        (ReviewStart::At(seconds), path)
    } else {
        return Err("not \"review start\" or \"review at\"".to_string());
    };
    let path = Path::new(path.trim());
    if !path.is_absolute() {
        return Err(format!("{} is not a full path", path.display()));
    }
    let is_demo = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dem"));
    if !is_demo {
        return Err(format!("{} is not a .dem file", path.display()));
    }
    Ok((without_dots(path), start))
}

/// `path` with every `.` dropped and every `..` taken back a folder, as the
/// game names a demo from its own folder (`dod/../../demos/a.dem`) and the
/// Master Demo Queue lists it plainly. Lexical: nothing is read from disk.
fn without_dots(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The line the game reads for one update.
fn reply_line(update: &ReviewUpdate) -> String {
    let one_line = |text: &str| text.replace(['\r', '\n'], " ");
    match update {
        ReviewUpdate::Progress(text) => format!("progress {}\n", one_line(text)),
        ReviewUpdate::Done => "done\n".to_string(),
        ReviewUpdate::Failed(why) => format!("failed {}\n", one_line(why)),
    }
}

/// Answers one request on `writer`: hands it to the frontend and writes its
/// updates until the last, the timeout, or the game hanging up.
pub fn serve(line: &str, writer: &mut impl Write) {
    let (demo, start) = match parse_request(line) {
        Ok(parsed) => parsed,
        Err(why) => {
            let _ = writer.write_all(reply_line(&ReviewUpdate::Failed(why)).as_bytes());
            return;
        }
    };
    let Some(handler) = HANDLER.get() else {
        let why = "DoD Studio is still starting: try again in a moment".to_string();
        let _ = writer.write_all(reply_line(&ReviewUpdate::Failed(why)).as_bytes());
        return;
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = channel();
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(id, sender);
    crate::log_markdown(&format!(
        "Review from the game: {} ({start:?})",
        demo.display()
    ));
    handler(ReviewRequest { id, demo, start });
    forward(&receiver, writer, ANSWER_TIMEOUT);
    if let Some(map) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(&id);
    }
}

/// Writes each update from `receiver` until the last one, `timeout`, or a
/// failed write.
fn forward(receiver: &Receiver<ReviewUpdate>, writer: &mut impl Write, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let update = match receiver.recv_timeout(left) {
            Ok(update) => update,
            Err(RecvTimeoutError::Timeout) => {
                ReviewUpdate::Failed("DoD Studio took too long to start the review".to_string())
            }
            Err(RecvTimeoutError::Disconnected) => {
                ReviewUpdate::Failed("DoD Studio stopped answering".to_string())
            }
        };
        let last = !matches!(update, ReviewUpdate::Progress(_));
        if writer.write_all(reply_line(&update).as_bytes()).is_err() || last {
            let _ = writer.flush();
            return;
        }
        let _ = writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_names_where_to_start_and_the_demo() {
        assert_eq!(
            parse_request("review start C:/dod/a b.dem\n"),
            Ok((PathBuf::from("C:/dod/a b.dem"), ReviewStart::First))
        );
        assert_eq!(
            parse_request("review at 356.5 C:/dod/a.dem\r\n"),
            Ok((PathBuf::from("C:/dod/a.dem"), ReviewStart::At(356.5)))
        );
    }

    #[test]
    fn a_demo_named_from_the_game_folder_loses_its_dots() {
        let (demo, _) =
            parse_request("review start C:/Steam/Half-Life/dod/../../demos/./x.DEM").unwrap();
        assert_eq!(demo, PathBuf::from("C:/Steam/demos/x.DEM"));
    }

    #[test]
    fn a_bad_request_says_why() {
        assert!(parse_request("analyze C:/a.dem").is_err());
        assert!(parse_request("review now C:/a.dem").is_err());
        assert!(parse_request("review at soon C:/a.dem").is_err());
        assert!(parse_request("review at -3 C:/a.dem").is_err());
        assert!(parse_request("review at NaN C:/a.dem").is_err());
        assert!(parse_request("review start a.dem").is_err());
        assert!(parse_request("review start C:/a.txt").is_err());
    }

    #[test]
    fn updates_go_to_the_game_until_the_last() {
        let (sender, receiver) = channel();
        sender
            .send(ReviewUpdate::Progress("Scanning\nthe demo".into()))
            .unwrap();
        sender.send(ReviewUpdate::Done).unwrap();
        sender
            .send(ReviewUpdate::Progress("never sent".into()))
            .unwrap();
        let mut out = Vec::new();
        forward(&receiver, &mut out, Duration::from_secs(5));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "progress Scanning the demo\ndone\n"
        );
    }

    #[test]
    fn an_unanswered_request_fails_when_its_time_is_up() {
        let (_sender, receiver) = channel::<ReviewUpdate>();
        let mut out = Vec::new();
        forward(&receiver, &mut out, Duration::from_millis(10));
        assert!(String::from_utf8(out).unwrap().starts_with("failed "));
    }

    #[test]
    fn a_report_for_a_finished_request_goes_nowhere() {
        report(u64::MAX, ReviewUpdate::Done);
    }
}
