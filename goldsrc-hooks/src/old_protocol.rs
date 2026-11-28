//! A protocol-47 demo started with `viewdemo` plays DoD Studio's converted
//! copy (#1549).
//!
//! ## Why
//!
//! `viewdemo` plays a demo through the HLTV proxy's `Core.dll`, whose
//! server-info parser disconnects on any protocol but 48: a 2004-era
//! protocol-47 demo prints "WARNING! Server returned protocol version 47, not
//! 48", "Disconnected." and loads nothing, on both engine builds
//! (`docs/goldsrc_protocol_47.md`). The in-game Load Demo window and the DoD
//! Studio window's Demos tab start every demo with `viewdemo`, and so does a
//! user typing it. Studio's own paths (previews, Seek Between Clips, Review)
//! already send the game a converted copy, so what reaches here is a demo the
//! player picked in the game.
//!
//! ## What it does
//!
//! `demo_reload`'s `viewdemo` wrapper hands every named `viewdemo` to
//! [`take_over_viewdemo`] before the real command runs. It finds the file the
//! name opens (`demo_locate`) and reads its first 16 bytes; anything but a
//! protocol-47 demo goes on to the real command untouched. For a protocol-47
//! one the real command doesn't run (it would only disconnect, stopping the
//! demo that was playing): a thread of its own asks Studio over its game pipe
//! for the copy (`convert <path>`, answered by
//! `native::sys::game_convert_requests` with `copy <path>` or `failed <why>`),
//! and on a later frame [`poll`] plays the copy with `viewdemo`. The
//! conversion runs in Studio, so the game's thread never waits on it and the
//! hook carries no second copy of the conversion. When Studio isn't running,
//! or can't make the copy, the console and the screen say why and what to do.
//!
//! A copy that arrives after another demo was started (a later `viewdemo` or
//! `playdemo`) is dropped: the player has moved on.

// Only the 32-bit build wraps viewdemo; a host build still compiles it for
// the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The bytes of a demo's start that say its protocol: the magic, the demo
/// protocol, then the network protocol.
const HEADER_BYTES: usize = 16;
const MAGIC: &[u8; 8] = b"HLDEMO\0\0";
const OLDER_PROTOCOL: i32 = 47;

/// The console line while Studio makes the copy; `{name}` is the demo.
const ASKING: &str = "{name} is an older protocol-47 demo, which viewdemo can't play: \
                      asking DoD Studio for a converted copy...\n";
/// The console line once the copy plays; `{copy}` is its path.
const PLAYING: &str = "Playing DoD Studio's converted copy: {copy}\n";
/// The console line when Studio isn't running.
const NO_STUDIO: &str = "This older protocol-47 demo can't play with viewdemo, and DoD Studio \
                         isn't running to convert it. Open DoD Studio and load it again (it plays \
                         a converted copy), or start it with playdemo, which plays it with a warning.\n";
/// The console line when Studio couldn't make the copy; `{why}` is its answer.
const FAILED: &str = "DoD Studio couldn't convert this older protocol-47 demo: {why}\n";
/// The centre print for both failures.
const CENTRE_CANNOT_PLAY: &str =
    "This is an older protocol-47 demo, which viewdemo can't play.\nSee the console.";

/// Bumped by every named `viewdemo` and `playdemo`, so an answer for a demo
/// the player has since moved on from is dropped.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Set once [`ANSWER`] holds Studio's answer, for [`poll`]'s one atomic load.
static READY: AtomicBool = AtomicBool::new(false);
static ANSWER: Mutex<Option<Pending>> = Mutex::new(None);

/// Studio's answer for one request.
#[derive(Debug, Clone, PartialEq)]
enum Answer {
    Copy(PathBuf),
    Failed(String),
    NoStudio,
}

#[derive(Debug)]
struct Pending {
    generation: u64,
    answer: Answer,
}

/// Whether `head` (a file's first bytes) is a protocol-47 demo's header.
fn is_protocol_47_header(head: &[u8]) -> bool {
    head.len() >= HEADER_BYTES
        && head[..8] == MAGIC[..]
        && i32::from_le_bytes([head[12], head[13], head[14], head[15]]) == OLDER_PROTOCOL
}

/// Whether the file at `path` is a protocol-47 demo, from its first bytes.
fn is_protocol_47_file(path: &Path) -> bool {
    let mut head = [0u8; HEADER_BYTES];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok_and(|()| is_protocol_47_header(&head))
}

/// A named `playdemo` is starting: an answer still on its way is for a demo
/// the player no longer wants.
pub(crate) fn demo_command_started() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// Called by `demo_reload`'s wrapper with a named `viewdemo`'s name, before
/// the real command runs, on the engine's thread. Returns `true` when the
/// name opens a protocol-47 demo: the real command must not run, and the
/// copy plays once Studio has made it. One file open and a 16-byte read.
pub(crate) fn take_over_viewdemo(name: &str) -> bool {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let Ok(opened) = crate::demo_locate::locate(name) else {
        // The engine says it can't open it, as it always has.
        return false;
    };
    if !is_protocol_47_file(&opened) {
        return false;
    }
    let demo = crate::review_ask::demo_path(&opened);
    crate::commands::console_print(&ASKING.replace("{name}", name));
    log(&format!(
        "{} is protocol 47: asking Studio for a copy",
        demo.display()
    ));
    let spawned = std::thread::Builder::new()
        .name("old-protocol-convert".into())
        .spawn(move || {
            let answer = ask_studio(&demo);
            log(&format!(
                "Studio's answer for {}: {answer:?}",
                demo.display()
            ));
            store(Pending { generation, answer });
        });
    if let Err(e) = spawned {
        store(Pending {
            generation,
            answer: Answer::Failed(format!("no thread to ask it on ({e})")),
        });
    }
    true
}

/// Keeps `pending` for [`poll`], unless an answer for a later request is
/// already waiting: two protocol-47 demos started in quick succession can
/// answer in either order, and only the later one is still wanted.
fn store(pending: Pending) {
    let mut slot = lock_recover::lock_or_recover(&ANSWER);
    if slot
        .as_ref()
        .is_some_and(|held| held.generation > pending.generation)
    {
        return;
    }
    *slot = Some(pending);
    drop(slot);
    READY.store(true, Ordering::Release);
}

/// Asks Studio for `demo`'s copy and waits for its answer, on the request's
/// own thread.
fn ask_studio(demo: &Path) -> Answer {
    let Ok(mut pipe) = crate::studio_pipe::connect() else {
        return Answer::NoStudio;
    };
    if let Err(e) = pipe.write_all(request_line(demo).as_bytes()) {
        return Answer::Failed(format!("the request didn't reach it ({e})"));
    }
    let mut line = String::new();
    match BufReader::new(pipe).read_line(&mut line) {
        Ok(_) => parse_answer(&line),
        Err(e) => Answer::Failed(format!("no answer ({e})")),
    }
}

/// The request for `demo`'s copy.
fn request_line(demo: &Path) -> String {
    format!("convert {}\n", demo.display())
}

/// Studio's answer line: `copy <path>` or `failed <why>`. Anything else (a
/// Studio from before #1549 answers `failed not an analyze request`) is a
/// failure that says what came back.
fn parse_answer(line: &str) -> Answer {
    let line = line.trim();
    if let Some(path) = line.strip_prefix("copy ") {
        return Answer::Copy(PathBuf::from(path.trim()));
    }
    let why = line.strip_prefix("failed ").unwrap_or(line);
    Answer::Failed(if why.is_empty() {
        "it gave no answer".to_string()
    } else {
        why.to_string()
    })
}

/// Plays a copy Studio has made, or says why there is none. Runs every frame
/// from `commands::poll`, on the engine's thread; one atomic load until an
/// answer is in.
pub fn poll() {
    if !READY.swap(false, Ordering::AcqRel) {
        return;
    }
    let Some(pending) = ANSWER.try_lock().ok().and_then(|mut a| a.take()) else {
        // The request's thread is storing it: next frame.
        READY.store(true, Ordering::Release);
        return;
    };
    if pending.generation != GENERATION.load(Ordering::Acquire) {
        log("answer dropped: another demo was started since");
        return;
    }
    match pending.answer {
        Answer::Copy(copy) => play_copy(&copy),
        Answer::Failed(why) => cannot_play(&FAILED.replace("{why}", &why)),
        Answer::NoStudio => cannot_play(NO_STUDIO),
    }
}

/// Starts `copy` with `viewdemo`, as Review starts a demo by its full path.
fn play_copy(copy: &Path) {
    let dod = crate::texture_hires::game_dir();
    let line = match crate::review::viewdemo_line(&dod.to_string_lossy(), &copy.to_string_lossy()) {
        Ok(line) => line,
        Err(why) => {
            cannot_play(&FAILED.replace("{why}", &why));
            return;
        }
    };
    crate::commands::console_print(&PLAYING.replace("{copy}", &copy.display().to_string()));
    let sent = std::ffi::CString::new(line).is_ok_and(|l| crate::engine::client_cmd(&l));
    log(&format!(
        "playing {}: {}",
        copy.display(),
        if sent {
            "sent"
        } else {
            "the engine refused the line"
        }
    ));
}

/// Says why the demo didn't play: the console line, and the centre print
/// unless something is being captured.
fn cannot_play(console_line: &str) {
    crate::commands::console_print(console_line);
    if crate::demo_damage::tell::capturing() {
        return;
    }
    if let Err(why) = crate::demo_damage::tell::center_print(CENTRE_CANNOT_PLAY) {
        log(&format!("not on screen: {why}"));
    }
}

fn log(what: &str) {
    // Safety: `report` only appends to the hook's log file.
    unsafe { crate::debug::report(&format!("old_protocol: {what} (#1549)")) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(protocol: i32) -> Vec<u8> {
        let mut head = MAGIC.to_vec();
        head.extend_from_slice(&5i32.to_le_bytes());
        head.extend_from_slice(&protocol.to_le_bytes());
        head
    }

    #[test]
    fn only_a_protocol_47_header_is_taken_over() {
        assert!(is_protocol_47_header(&header(47)));
        assert!(!is_protocol_47_header(&header(48)));
        assert!(!is_protocol_47_header(&header(47)[..12]));
        let mut not_a_demo = header(47);
        not_a_demo[0] = b'X';
        assert!(!is_protocol_47_header(&not_a_demo));
    }

    #[test]
    fn a_file_is_read_by_its_first_bytes() {
        let dir = std::env::temp_dir().join(format!("old_protocol_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.dem");
        let new = dir.join("new.dem");
        std::fs::write(&old, [header(47), vec![0; 600]].concat()).unwrap();
        std::fs::write(&new, [header(48), vec![0; 600]].concat()).unwrap();
        assert!(is_protocol_47_file(&old));
        assert!(!is_protocol_47_file(&new));
        assert!(!is_protocol_47_file(&dir.join("missing.dem")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn studio_answers_with_the_copy_or_why_not() {
        assert_eq!(
            parse_answer(
                "copy C:\\Users\\me\\AppData\\Local\\dod-studio\\protocol48_demos\\v1-1-2\\a_p48.dem\n"
            ),
            Answer::Copy(PathBuf::from(
                "C:\\Users\\me\\AppData\\Local\\dod-studio\\protocol48_demos\\v1-1-2\\a_p48.dem"
            ))
        );
        assert_eq!(
            parse_answer("failed no room on C:\n"),
            Answer::Failed("no room on C:".into())
        );
        // A Studio from before #1549 takes it for an analyze request.
        assert_eq!(
            parse_answer("failed not an analyze request\n"),
            Answer::Failed("not an analyze request".into())
        );
        assert_eq!(parse_answer(""), Answer::Failed("it gave no answer".into()));
    }

    #[test]
    fn the_request_is_studios_convert_line() {
        assert_eq!(
            request_line(Path::new("C:\\demos\\a b.dem")),
            "convert C:\\demos\\a b.dem\n"
        );
    }

    #[test]
    fn an_answer_for_a_demo_the_player_moved_on_from_is_dropped() {
        let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        demo_command_started();
        store(Pending {
            generation,
            answer: Answer::NoStudio,
        });
        // Not the current generation: poll takes it and does nothing else
        // (no engine on the host to print to).
        poll();
        assert!(ANSWER.lock().unwrap().is_none());
        assert!(!READY.load(Ordering::Acquire));
    }

    #[test]
    fn the_texts_name_what_they_fill_in() {
        assert!(ASKING.contains("{name}"));
        assert!(PLAYING.contains("{copy}"));
        assert!(FAILED.contains("{why}"));
        for line in [ASKING, PLAYING, NO_STUDIO, FAILED] {
            assert!(line.ends_with('\n'));
        }
    }
}
