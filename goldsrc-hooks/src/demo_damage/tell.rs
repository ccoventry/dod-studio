//! Where a damaged-demo notice goes besides the console, the hook log and
//! the events pipe (#1373): a line in the in-game DoD Studio window, a toast
//! in Studio with Repair, and a message on screen only when nothing is being
//! captured.
//!
//! - **The window**: the loading strip along the top of its tabs shows
//!   [`strip_line`] while the damaged demo is still the current one.
//! - **Studio**: one `damaged <seconds> <reason> <path>` request over
//!   Studio's game pipe (`native::sys::game_damage_notices`), on a thread of
//!   its own. That pipe is served while Studio runs, whatever it is doing; the
//!   events pipe has a reader only during a batch or a review.
//! - **On screen**: the engine's own centre print (`pfnCenterPrint`), unless
//!   a capture batch runs or `host_framerate` is set (HLAE sets it every
//!   frame while it records; a fast-forward sets it too).

// Only the 32-bit build has the window that reads the strip line.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;

/// The window's line, the demo it is about (as played), and the line.
static STRIP: Mutex<Option<(String, String)>> = Mutex::new(None);

/// The window's line for a damaged demo; `{at}` is the demo time.
const STRIP_DAMAGED: &str = "This demo is damaged at {at}: it plays only up to there. \
                             DoD Studio can repair it.";

/// The centre print's text. `SCR_CenterPrint` copies it as it is (up to
/// 1023 bytes) and breaks lines at each newline.
const CENTRE_DAMAGED: &str = "This demo is damaged at {at}.\nDoD Studio can repair it.";

/// One engine build's `pfnCenterPrint` wrapper, by offset in `hw.dll`
/// (`tools/verify_center_print_offsets.py`).
const CENTRE_PRINT_AT: [(&str, usize); 2] =
    [("pre-Anniversary", 0xb0e0), ("25th Anniversary", 0x195480)];

/// Remembers the window's line for `demo` (as played).
pub(super) fn remember_for_window(demo: &str, at_text: &str) {
    let line = STRIP_DAMAGED.replace("{at}", at_text);
    if let Ok(mut strip) = STRIP.lock() {
        *strip = Some((demo.to_string(), line));
    }
}

/// The window's line, while the demo it is about is still the current one.
/// `try_lock`: the window asks every frame.
pub fn strip_line() -> Option<String> {
    let current = crate::demo_reload::current_demo()?;
    let strip = STRIP.try_lock().ok()?;
    strip
        .as_ref()
        .filter(|(demo, _)| *demo == current)
        .map(|(_, line)| line.clone())
}

/// Whether something is being captured: a capture batch runs, or
/// `host_framerate` is set. Also asked by `old_protocol` (#1549).
pub(crate) fn capturing() -> bool {
    if crate::batch::running() {
        return true;
    }
    crate::engine::engfuncs().is_some_and(|engfuncs| {
        // Safety: the engine's accessor, on its thread.
        let rate = unsafe { (engfuncs.pfn_get_cvar_float)(c"host_framerate".as_ptr()) };
        rate != 0.0
    })
}

/// Shows the on-screen message, unless something is being captured. Says
/// in the hook log what it did.
pub(super) fn on_screen(at_text: &str) {
    if capturing() {
        log("not on screen: a capture runs");
        return;
    }
    match center_print(&CENTRE_DAMAGED.replace("{at}", at_text)) {
        Ok(build) => log(&format!("on screen ({build} centre print)")),
        Err(why) => log(&format!("not on screen: {why}")),
    }
}

/// `text` through the engine's `pfnCenterPrint`, when the slot points at
/// the wrapper a known build has. Also used by `old_protocol` (#1549).
pub(crate) fn center_print(text: &str) -> Result<&'static str, String> {
    let engfuncs = crate::engine::engfuncs().ok_or("no engine function table yet")?;
    let base = crate::engine::engine_module_base().ok_or("hw.dll is not loaded")?;
    let at = (engfuncs.pfn_center_print as usize).wrapping_sub(base);
    let build = CENTRE_PRINT_AT
        .iter()
        .find(|(_, offset)| *offset == at)
        .map(|(name, _)| *name)
        .ok_or_else(|| {
            format!("pfnCenterPrint is at +{at:#x}, not a wrapper this was checked against")
        })?;
    let text = std::ffi::CString::new(text).map_err(|e| e.to_string())?;
    // Safety: the engine's own wrapper (checked above), on its thread; it
    // hands the string to `SCR_CenterPrint` unformatted.
    unsafe { (engfuncs.pfn_center_print)(text.as_ptr()) };
    Ok(build)
}

/// Studio's request line for a damaged demo at `path`, `seconds` into it.
pub(super) fn studio_line(seconds: Option<f64>, reason: &str, path: &str) -> String {
    let at = seconds.map_or_else(|| "-".to_string(), |s| format!("{s:.2}"));
    format!("damaged {at} {reason} {path}\n")
}

/// Tells Studio, on a thread of its own: the engine thread never waits on a
/// pipe. Nothing to tell without a path, since Studio can't repair it.
pub(super) fn tell_studio(seconds: Option<f64>, reason: &'static str, path: Option<String>) {
    let Some(path) = path else {
        log("Studio not told: the demo's name is not known");
        return;
    };
    let line = studio_line(seconds, reason, &path);
    let spawned = std::thread::Builder::new()
        .name("demo-damage-studio".into())
        .spawn(move || log(&ask(&line)));
    if let Err(e) = spawned {
        log(&format!("Studio not told: no thread ({e})"));
    }
}

/// Sends `line` and reads Studio's one-line answer: what to log.
fn ask(line: &str) -> String {
    use std::io::{BufRead, BufReader, Write};
    let mut pipe = match crate::studio_pipe::connect() {
        Ok(pipe) => pipe,
        Err(e) => return format!("Studio not told: DoD Studio isn't running ({e})"),
    };
    if let Err(e) = pipe.write_all(line.as_bytes()) {
        return format!("Studio not told: {e}");
    }
    let mut answer = String::new();
    match BufReader::new(pipe).read_line(&mut answer) {
        Ok(_) if answer.trim() == "ok" => "Studio told".to_string(),
        Ok(_) => format!("Studio answered {:?}", answer.trim()),
        Err(e) => format!("Studio told, no answer ({e})"),
    }
}

fn log(what: &str) {
    unsafe { crate::debug::report(&format!("demo_damage: {what} (#1373)")) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_gets_the_time_the_reason_and_the_path() {
        assert_eq!(
            studio_line(Some(507.754), "message_too_big", r"C:\Games\dod\a b.dem"),
            "damaged 507.75 message_too_big C:\\Games\\dod\\a b.dem\n"
        );
        assert_eq!(
            studio_line(None, "bad_frame", r"C:\x.dem"),
            "damaged - bad_frame C:\\x.dem\n"
        );
    }

    #[test]
    fn the_texts_name_the_time() {
        for text in [STRIP_DAMAGED, CENTRE_DAMAGED] {
            assert!(text.contains("{at}"));
        }
        assert!(!STRIP_DAMAGED.contains('\n'));
    }

    #[test]
    fn the_window_line_is_for_the_demo_it_names() {
        remember_for_window("monday-wsod25_r07_m1_h1", "8:27.75");
        let strip = STRIP.lock().unwrap().clone();
        assert_eq!(
            strip,
            Some((
                "monday-wsod25_r07_m1_h1".to_string(),
                STRIP_DAMAGED.replace("{at}", "8:27.75")
            ))
        );
    }
}
