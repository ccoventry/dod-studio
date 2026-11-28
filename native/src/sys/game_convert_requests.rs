//! The game asks Studio for a protocol-48 copy of a protocol-47 demo (#1549).
//!
//! The game's `viewdemo` refuses a protocol-47 demo (`Core.dll` disconnects
//! at its server info), and the in-game Load Demo window and Demos tab start
//! every demo with it. The hook DLL notices one before `viewdemo` runs
//! (`goldsrc-hooks/src/old_protocol.rs`) and asks Studio over its game pipe
//! (`analysis_server`, which hands every `convert ` line here) for the copy
//! Studio's own paths play (`patch::protocol48_copies`), then plays that.
//! The conversion runs here, in Studio, off the game's thread, with the same
//! code and the same copies as a Launch Preview or a batch.
//!
//! ## Protocol
//!
//! One request per connection, one line each way:
//!
//! - the game: `convert <absolute path to the .dem>`;
//! - Studio: `copy <absolute path to the copy>`, or `failed <why>`.
//!
//! A Studio from before this module answers `failed not an analyze request`
//! (the pipe's default request), which the game shows as it shows any other
//! failure.

use std::path::{Path, PathBuf};

use crate::patch::protocol48_copies::{KEEP_COPIES, Playable, copies_root, prepare_playable};

/// Whether `line` is one of this module's requests, for the pipe's dispatch.
pub fn is_convert_request(line: &str) -> bool {
    line.starts_with("convert ")
}

/// The demo a request names: `convert <absolute path to an existing .dem>`.
pub fn parse_request(line: &str) -> Result<PathBuf, String> {
    let path = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("convert ")
        .ok_or("not a convert request")?;
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
    if !path.is_file() {
        return Err(format!("{} isn't there", path.display()));
    }
    Ok(super::game_review_requests::without_dots(path))
}

/// The answer to a request for `demo`, given what [`prepare_playable`] made
/// of it.
fn reply_line(played: Result<Playable, String>) -> String {
    let one_line = |text: &str| text.replace(['\r', '\n'], " ");
    match played {
        Ok(Playable::Copy { path, .. }) => format!("copy {}", path.display()),
        Ok(Playable::Original) => format!(
            "failed {}",
            crate::messages::protocol47::GAME_CONVERT_NOT_PROTOCOL_47
        ),
        Err(why) => format!("failed {}", one_line(&why)),
    }
}

/// Answers one request line: logs it, makes (or reuses) the copy, and writes
/// `copy <path>` or why not to `writer`. Writes the copy and its analyzer
/// cache entry, prunes the copies past `KEEP_COPIES` (the one just played is
/// the newest and stays), and logs each step to the activity log.
pub fn serve(line: &str, writer: &mut impl std::io::Write) {
    use crate::messages::protocol47 as m;
    let played = parse_request(line).and_then(|demo| {
        crate::log_markdown(&m::game_convert_request(demo.display()));
        prepare_playable(&demo, &mut |_| {}).map_err(|e| e.to_string())
    });
    match &played {
        Ok(Playable::Copy { .. }) => {
            crate::patch::copy_cache::prune(&copies_root(), KEEP_COPIES);
        }
        Ok(Playable::Original) => {}
        Err(why) => crate::log_markdown(&m::game_convert_failed(why)),
    }
    let _ = writer.write_all(format!("{}\n", reply_line(played)).as_bytes());
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_names_a_demo_that_is_there() {
        let dir = crate::test_support::Scratch::new("convert_request");
        let demo = dir.join("old.dem");
        std::fs::write(&demo, b"HLDEMO").unwrap();
        let line = format!("convert {}\n", demo.display());
        assert!(is_convert_request(&line));
        assert_eq!(parse_request(&line).unwrap(), demo);
    }

    #[test]
    fn a_bad_request_says_why() {
        for line in [
            "analyze C:/x.dem",
            "convert x.dem",
            "convert C:/x.txt",
            "convert C:/no/such/demo.dem",
        ] {
            assert!(parse_request(line).is_err(), "{line}");
        }
        assert!(!is_convert_request("analyze C:/x.dem"));
    }

    #[test]
    fn the_answer_is_the_copy_or_why_not() {
        let copy = PathBuf::from(r"C:\copies\v1-1-2\old_p48.dem");
        assert_eq!(
            reply_line(Ok(Playable::Copy {
                path: copy.clone(),
                made: true
            })),
            format!("copy {}", copy.display())
        );
        assert!(reply_line(Ok(Playable::Original)).starts_with("failed "));
        assert_eq!(
            reply_line(Err("no room\non C:".into())),
            "failed no room on C:"
        );
    }

    #[test]
    fn serving_a_protocol_48_demo_makes_no_copy() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-fixtures/ci_fixture.dem");
        let mut out = Vec::new();
        serve(&format!("convert {}\n", fixture.display()), &mut out);
        let answer = String::from_utf8(out).unwrap();
        assert!(answer.starts_with("failed "), "{answer}");
        assert!(answer.ends_with('\n'));
    }
}
