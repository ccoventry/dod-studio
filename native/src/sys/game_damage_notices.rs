//! The game tells Studio a demo it is playing is damaged (#1373).
//!
//! The hook DLL notices a demo that stops on a damaged frame under
//! `playdemo`, or that `viewdemo` loaded only up to one, and says so in the
//! game (`goldsrc-hooks/src/demo_damage.rs`). It also sends one request over
//! Studio's game pipe (`analysis_server`, which hands every `damaged ` line
//! here), since that pipe is served whenever Studio runs, while the game's
//! events pipe has a reader only during a capture batch or a review. Studio
//! shows a toast with a Repair button for it.
//!
//! ## Protocol
//!
//! One request per connection, one line each way:
//!
//! - the game: `damaged <seconds> <reason> <absolute path to the .dem>`,
//!   `seconds` on the demo's clock with two decimals, or `-` when the game
//!   couldn't read its clock; `reason` one word (`message_too_big`,
//!   `illegible_message`, `bad_frame`, `stopped_loading`, ...);
//! - Studio: `ok`, or `failed <why>`.
//!
//! The page does the rest: Tauri registers a [`set_damage_handler`] that
//! hands each notice to it as an event. Studio answers at once, so the game
//! never waits on the page.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One notice from the game.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DamageNotice {
    /// The demo, absolute, with any `..` worked out.
    pub demo: PathBuf,
    /// Where it stops, on the demo's clock; `None` when the game couldn't
    /// tell.
    pub seconds: Option<f64>,
    /// Why, as the hook names it.
    pub reason: String,
}

type Handler = Box<dyn Fn(DamageNotice) -> Result<(), String> + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Sets what a notice is handed to, once per process (Tauri's setup).
/// Until then every notice is answered "DoD Studio is still starting".
pub fn set_damage_handler(
    handler: impl Fn(DamageNotice) -> Result<(), String> + Send + Sync + 'static,
) {
    let _ = HANDLER.set(Box::new(handler));
}

/// Whether `line` is one of this module's requests, for the pipe's dispatch.
pub fn is_damage_notice(line: &str) -> bool {
    line.starts_with("damaged ")
}

/// Reads one request line into a notice, or says why not.
pub fn parse_notice(line: &str) -> Result<DamageNotice, String> {
    let rest = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("damaged ")
        .ok_or("not a damaged-demo notice")?;
    let mut fields = rest.splitn(3, ' ');
    let seconds = fields.next().ok_or("no time")?;
    let reason = fields.next().ok_or("no reason")?;
    let path = fields.next().ok_or("no demo")?;
    let seconds = match seconds {
        "-" => None,
        s => Some(
            s.parse::<f64>()
                .ok()
                .filter(|s| s.is_finite() && *s >= 0.0)
                .ok_or_else(|| format!("{s:?} is not a time"))?,
        ),
    };
    let well_formed = !reason.is_empty()
        && reason
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !well_formed {
        return Err(format!("{reason:?} is not a reason"));
    }
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
    Ok(DamageNotice {
        demo: super::game_review_requests::without_dots(path),
        seconds,
        reason: reason.to_string(),
    })
}

/// Answers one request line: logs it, hands it on, and writes `ok` or why
/// not to `writer`.
pub fn serve(line: &str, writer: &mut impl std::io::Write) {
    let answer = match parse_notice(line) {
        Ok(notice) => {
            crate::log_markdown(&crate::messages::game_damage_notice(
                notice.demo.display(),
                notice.seconds,
                &notice.reason,
            ));
            match HANDLER.get() {
                Some(handler) => handler(notice).map_err(|e| {
                    crate::log_markdown(&crate::messages::game_damage_not_handed_on(&e));
                    e
                }),
                None => Err(crate::messages::GAME_DAMAGE_STUDIO_STARTING.to_string()),
            }
        }
        Err(why) => {
            crate::log_markdown(&crate::messages::game_damage_bad_notice(&why));
            Err(why)
        }
    };
    let reply = match answer {
        Ok(()) => "ok".to_string(),
        Err(why) => format!("failed {}", why.replace(['\r', '\n'], " ")),
    };
    let _ = writer.write_all(format!("{reply}\n").as_bytes());
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_names_the_time_the_reason_and_the_demo() {
        let notice =
            parse_notice("damaged 507.75 message_too_big C:\\Games\\dod\\..\\demos\\a b.dem\n")
                .unwrap();
        assert_eq!(notice.seconds, Some(507.75));
        assert_eq!(notice.reason, "message_too_big");
        assert_eq!(notice.demo, PathBuf::from(r"C:\Games\demos\a b.dem"));
        let unknown = parse_notice(r"damaged - bad_frame \\server\share\x.dem").unwrap();
        assert_eq!(unknown.seconds, None);
        assert_eq!(unknown.demo, PathBuf::from(r"\\server\share\x.dem"));
    }

    #[test]
    fn a_bad_notice_says_why() {
        for line in [
            "analyze C:\\a.dem",
            "damaged",
            "damaged 5",
            "damaged 5 bad_frame",
            "damaged soon bad_frame C:\\a.dem",
            "damaged -1 bad_frame C:\\a.dem",
            "damaged 5 Bad-Frame C:\\a.dem",
            "damaged 5 bad_frame a.dem",
            "damaged 5 bad_frame C:\\a.txt",
        ] {
            assert!(parse_notice(line).is_err(), "{line}");
        }
        assert!(is_damage_notice("damaged 5 bad_frame C:\\a.dem"));
        assert!(!is_damage_notice("review start C:\\a.dem"));
    }

    #[test]
    fn studio_answers_ok_or_why_not_on_one_line() {
        let mut out = Vec::new();
        serve("damaged 5 bad_frame a.dem\n", &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("failed ") && text.ends_with('\n'), "{text}");
        assert_eq!(text.lines().count(), 1);
    }
}
