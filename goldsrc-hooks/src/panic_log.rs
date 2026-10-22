//! Says *why* the game died when the cause was a panic in this DLL.
//!
//! The release profile builds with `panic = "abort"` (workspace `Cargo.toml`),
//! so a panic in any hook ends `hl.exe` on the spot. `catch_unwind` can't
//! stop that (nothing unwinds), and the abort is a fail-fast that skips the
//! vectored handler in `crash.rs`, so before this a panic left nothing in the
//! log at all: the game simply closed.
//!
//! The panic hook runs before the abort. It writes one `PANIC:` line with the
//! message and the source location, which `tools/crash_report.py` lists with
//! the other crashes. It only logs; the abort still happens.

use std::panic::PanicHookInfo;

/// Installs the hook. Called once, first thing at load, next to
/// `crash::install`.
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let line = describe(info);
        // Safety: `report` opens the log file per call and takes no lock, so
        // it can't deadlock on state the panicking code was holding.
        unsafe { crate::debug::report(&line) };
    }));
}

/// `PANIC: <message> at <file>:<line>:<column> (thread <name>)`.
fn describe(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)");
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "an unknown location".to_string());
    let thread = std::thread::current();
    format!(
        "PANIC: {message} at {location} (thread {})",
        thread.name().unwrap_or("unnamed")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// What `describe` makes of a real panic, caught on a test thread.
    fn described(f: impl FnOnce() + Send + 'static) -> String {
        let seen = Arc::new(Mutex::new(String::new()));
        let keep = Arc::clone(&seen);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            *keep.lock().unwrap() = describe(info);
        }));
        let _ = std::thread::Builder::new()
            .name("probe".into())
            .spawn(f)
            .unwrap()
            .join();
        std::panic::set_hook(previous);
        seen.lock().unwrap().clone()
    }

    /// One test: the panic hook is process-wide, so two tests swapping it in
    /// parallel would read each other's panics.
    #[test]
    fn a_panic_names_its_message_location_and_thread() {
        let line = described(|| panic!("index {} out of range", 7));
        assert!(
            line.starts_with("PANIC: index 7 out of range at "),
            "{line}"
        );
        assert!(line.contains("panic_log.rs:"), "{line}");
        assert!(line.ends_with("(thread probe)"), "{line}");

        let line = described(|| panic!("plain"));
        assert!(line.starts_with("PANIC: plain at "), "{line}");
    }
}
