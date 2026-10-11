//! `dodstudio_schedule`: runs console commands at demo-player times, from the
//! hook instead of from frames patched into the demo (issue #434, step 3).
//!
//! ## Why
//!
//! A capture batch injects its Initial and Scheduled Commands into a patched
//! copy of each demo as `ConsoleCommand` frames. That costs the 64-byte limit
//! per command, shifts every later frame ordinal, and puts every command
//! through the engine's demo command filter (#679: `bind`, `_set`, `exit`...
//! are dropped without a word). Run from the hook through `pfnClientCmd`, the
//! same commands need none of that. This module is the first piece: a
//! schedule the hook runs by the demo player's own clock, and a record of how
//! close to its time each command ran, to compare with injected frames before
//! anything is built on it.
//!
//! ## The schedule file
//!
//! [`HEADER`] on the first line, then one command per line: the time on the
//! demo player's clock (the one `dodstudio_seek_to` takes), a tab, the
//! command. Blank lines and lines starting with `//` are skipped.
//! `dodstudio_schedule load "<file>"` reads it; each command then runs once,
//! on the first frame at or past its time while a `viewdemo` demo plays. A
//! jump back (a seek) re-arms the commands after the new time, so a replayed
//! stretch runs them again; a jump forward runs the ones it passed, late.
//!
//! Every command run is logged with its due time and how late it ran. While a
//! schedule is loaded, every `[dod-studio]` marker `echo`ed (from the demo's
//! own frames or from the schedule) is logged on the same clock, which is how
//! the two routes are compared.
//!
//! Commands and [`poll`] run on the game's main thread.

// The demo player is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;

use crate::names::console_name;

pub const NAME: &str = console_name!("schedule");

/// The schedule file's first line.
pub(crate) const HEADER: &str = "dodstudio-schedule 1";

const USAGE: &str = "load \"<file>\" | clear (bare: what is loaded)";

/// A clock change between two frames bigger than this, backwards, is a seek.
const BACK_JUMP_SECONDS: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub at: f64,
    pub command: String,
}

/// Reads a schedule file's text: the entries sorted by time (a stable sort,
/// so commands at one time keep their order), or why it can't be used.
pub(crate) fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let mut lines = text.lines();
    match lines.next().map(str::trim) {
        Some(HEADER) => {}
        _ => {
            return Err(format!(
                "not a schedule file (the first line isn't \"{HEADER}\")"
            ));
        }
    }
    let mut entries = Vec::new();
    for (i, line) in lines.enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let number = i + 2;
        let Some((time, command)) = line.split_once('\t') else {
            return Err(format!(
                "line {number}: no tab between the time and the command"
            ));
        };
        let at: f64 = time.trim().parse().map_err(|_| {
            format!(
                "line {number}: \"{}\" is not a time in seconds",
                time.trim()
            )
        })?;
        if !at.is_finite() || at < 0.0 {
            return Err(format!("line {number}: the time must be 0 or more"));
        }
        let command = command.trim();
        if command.is_empty() {
            return Err(format!("line {number}: no command"));
        }
        if command.contains(['\r', '\n']) {
            return Err(format!("line {number}: a command can't hold a line break"));
        }
        entries.push(Entry {
            at,
            command: command.to_string(),
        });
    }
    entries.sort_by(|a, b| a.at.total_cmp(&b.at));
    Ok(entries)
}

/// A loaded schedule and where the cursor is.
#[derive(Debug)]
pub(crate) struct Schedule {
    pub entries: Vec<Entry>,
    /// The first entry not yet run.
    pub next: usize,
    /// The clock on the last frame looked at.
    pub last_clock: Option<f64>,
}

impl Schedule {
    pub fn new(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            next: 0,
            last_clock: None,
        }
    }

    /// The entries due at clock `now`, in order, as indices; moves the cursor
    /// past them. A jump back re-arms everything after `now`.
    pub fn due(&mut self, now: f64) -> std::ops::Range<usize> {
        if let Some(last) = self.last_clock
            && now < last - BACK_JUMP_SECONDS
        {
            self.next = self.entries.partition_point(|e| e.at <= now);
        }
        self.last_clock = Some(now);
        let start = self.next;
        while self.next < self.entries.len() && self.entries[self.next].at <= now {
            self.next += 1;
        }
        start..self.next
    }
}

static SCHEDULE: Mutex<Option<Schedule>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Schedule>> {
    SCHEDULE.lock().unwrap_or_else(|e| e.into_inner())
}

fn say(line: &str) {
    crate::commands::console_print(&format!("{NAME}: {line}\n"));
    unsafe { crate::debug::report(&format!("schedule: {line}")) };
}

fn trace(line: &str) {
    unsafe { crate::debug::report(&format!("schedule: {line}")) };
}

fn run(line: &str) -> bool {
    std::ffi::CString::new(line).is_ok_and(|l| crate::engine::client_cmd(&l))
}

fn load(path: &str) {
    if path.is_empty() {
        say(&format!("usage: {NAME} {USAGE}"));
        return;
    }
    let entries = match std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {path}: {e}"))
        .and_then(|text| parse(&text))
    {
        Ok(entries) => entries,
        Err(why) => {
            say(&why);
            return;
        }
    };
    let count = entries.len();
    let span = match (entries.first(), entries.last()) {
        (Some(first), Some(last)) => format!(", {:.2} to {:.2} s", first.at, last.at),
        _ => String::new(),
    };
    *lock() = Some(Schedule::new(entries));
    say(&format!("loaded {count} command(s){span} from {path}"));
}

/// The console command.
pub unsafe extern "C" fn command() {
    let args = crate::cmd_list::args();
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        None => match lock().as_ref() {
            Some(s) => say(&format!(
                "{} of {} command(s) run; usage: {NAME} {USAGE}",
                s.next,
                s.entries.len()
            )),
            None => say(&format!("nothing loaded; usage: {NAME} {USAGE}")),
        },
        Some("load") => load(args[1..].join(" ").trim().trim_matches('"')),
        Some("clear") => {
            *lock() = None;
            say("cleared");
        }
        Some(_) => say(&format!("usage: {NAME} {USAGE}")),
    }
}

/// Runs every frame: the commands due on the demo player's clock.
pub fn poll() {
    // Nothing loaded: don't touch the demo player at all. Reading its clock
    // every frame from start-up crashed demoplayer.dll before it was ready
    // (demoplayer.dll+0x2fe6 on PRE, +0x3086 on POST, 2026-10-10).
    if lock().is_none() {
        return;
    }
    let Some(clock) = crate::demo_seek::clock() else {
        return;
    };
    if !clock.active || clock.loading {
        return;
    }
    // Taken out of the lock before running: a command may be this one.
    let due: Vec<(usize, Entry)> = {
        let mut guard = lock();
        let Some(schedule) = guard.as_mut() else {
            return;
        };
        let range = schedule.due(clock.now);
        range.map(|i| (i, schedule.entries[i].clone())).collect()
    };
    for (i, entry) in due {
        let ok = run(&entry.command);
        trace(&format!(
            "ran #{} at {:.3} (due {:.3}, {:.0} ms late){}: {}",
            i + 1,
            clock.now,
            entry.at,
            (clock.now - entry.at) * 1000.0,
            if ok { "" } else { " -- the engine refused it" },
            entry.command
        ));
    }
}

/// While a schedule is loaded, logs a `[dod-studio]` marker line with the
/// demo player's clock, whichever route `echo`ed it (`events.rs`).
pub(crate) fn note_marker(line: &str) {
    if lock().is_none() {
        return;
    }
    if let Some(clock) = crate::demo_seek::clock() {
        trace(&format!("marker at {:.3}: {line}", clock.now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schedule_parses_sorted_and_skips_comments() {
        let text = format!("{HEADER}\n// a comment\n\n12.5\techo b\n3\techo a\n12.5\techo c\n");
        let entries = parse(&text).unwrap();
        let got: Vec<_> = entries.iter().map(|e| (e.at, e.command.as_str())).collect();
        assert_eq!(got, [(3.0, "echo a"), (12.5, "echo b"), (12.5, "echo c")]);
    }

    #[test]
    fn a_bad_file_says_which_line() {
        assert!(
            parse("hello\n1\techo")
                .unwrap_err()
                .contains("not a schedule file")
        );
        let e = parse(&format!("{HEADER}\n1 echo")).unwrap_err();
        assert!(e.contains("line 2") && e.contains("no tab"), "{e}");
        let e = parse(&format!("{HEADER}\nsoon\techo")).unwrap_err();
        assert!(e.contains("not a time"), "{e}");
        let e = parse(&format!("{HEADER}\n-1\techo")).unwrap_err();
        assert!(e.contains("0 or more"), "{e}");
        let e = parse(&format!("{HEADER}\n1\t   ")).unwrap_err();
        assert!(e.contains("no command"), "{e}");
    }

    #[test]
    fn commands_run_once_in_order_as_the_clock_passes_them() {
        let mut s = Schedule::new(parse(&format!("{HEADER}\n1\ta\n2\tb\n2\tc\n5\td")).unwrap());
        assert_eq!(s.due(0.5), 0..0);
        assert_eq!(s.due(1.0), 0..1);
        assert_eq!(s.due(1.5), 1..1);
        // A frame past two at once runs both, in file order.
        assert_eq!(s.due(2.01), 1..3);
        // A seek forward runs what it passed, late.
        assert_eq!(s.due(9.0), 3..4);
        assert_eq!(s.due(10.0), 4..4);
    }

    #[test]
    fn a_seek_back_rearms_what_comes_after_it() {
        let mut s = Schedule::new(parse(&format!("{HEADER}\n1\ta\n2\tb\n5\tc")).unwrap());
        assert_eq!(s.due(6.0), 0..3);
        // Back to 1.5: b and c run again when the clock reaches them.
        assert_eq!(s.due(1.5), 1..1);
        assert_eq!(s.due(2.0), 1..2);
        assert_eq!(s.due(5.0), 2..3);
        // A small step back (frame jitter) is not a seek.
        assert_eq!(s.due(4.8), 3..3);
    }
}
