//! `dodstudio_batch`: runs a whole capture batch from the hook, on the original
//! demos, with seeks instead of patched demos and fast-forward (issue #434,
//! step 3).
//!
//! ## The flow
//!
//! Studio plans the batch as before (`native::patch::build_batch_queue`), then
//! writes the plan to a file instead of patching a copy of each demo, and sends
//! `dodstudio_batch start "<file>"` over the game's pipe (#413). For each demo
//! the batch loads it with `viewdemo`, waits for the whole file to be read,
//! runs the demo's init commands, then runs its timed commands on the demo
//! player's clock. Where the patched demo would fast-forward to the next clip,
//! the batch seeks there (`dodstudio_seek_to`'s stepped seek, #601). At the
//! demo's `next` time it moves on to the next demo, and after the last one it
//! runs the `end` commands.
//!
//! The game then runs the batch on its own: Studio only watches the markers
//! the commands `echo` and can end the game.
//!
//! ## The batch file
//!
//! [`HEADER`] on the first line, then one tab-separated line each:
//!
//! - `demo <full path>`: starts the next demo's section;
//! - `init <command>`: run once the demo has loaded, before anything else;
//! - `at <seconds> <command>`: run on the first frame at or past that time;
//! - `seek <seconds> <to>`: at that time, jump forward to `to`;
//! - `next <seconds>`: at that time, the demo is done;
//! - `end <command>`: after the last demo (outside any section).
//!
//! Times are the demo player's clock, the one `dodstudio_seek_to` takes. Lines
//! starting with `//` and blank lines are skipped.
//!
//! Commands and [`poll`] run on the game's main thread.

// The demo player is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::names::console_name;

pub const NAME: &str = console_name!("batch");

/// The batch file's first line.
pub(crate) const HEADER: &str = "dodstudio-batch 1";

const USAGE: &str = "start \"<batch file>\" | stop (bare: where it is)";

/// How long a demo may take to load before the batch skips it.
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Frames after `viewdemo` before the demo player's state counts: until the
/// command runs, the player still holds the previous demo.
const SETTLE_FRAMES: u32 = 30;
/// Frames after which a demo never seen loading counts as loaded: a small one
/// can be read between two looks.
const UNSEEN_LOAD_FRAMES: u32 = 300;
/// A seek shorter than this just plays on.
const MIN_SEEK_SECONDS: f64 = 0.5;
/// How often the log hears where the batch is.
const HEARTBEAT: Duration = Duration::from_secs(10);

/// What a timed line does.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Action {
    Run(String),
    Seek(f64),
    Next,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub at: f64,
    pub action: Action,
}

/// One demo's section of the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct DemoPlan {
    pub demo: String,
    pub init: Vec<String>,
    /// Sorted by time; lines at one time keep the file's order.
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Plan {
    pub demos: Vec<DemoPlan>,
    pub end: Vec<String>,
}

fn time(raw: &str, number: usize) -> Result<f64, String> {
    let value: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("line {number}: \"{}\" is not a time in seconds", raw.trim()))?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!("line {number}: the time must be 0 or more"));
    }
    Ok(value)
}

fn command_text(raw: &str, number: usize) -> Result<String, String> {
    let command = raw.trim();
    if command.is_empty() {
        return Err(format!("line {number}: no command"));
    }
    Ok(command.to_string())
}

/// Reads a batch file's text.
pub(crate) fn parse(text: &str) -> Result<Plan, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(HEADER) {
        return Err(format!(
            "not a batch file (the first line isn't \"{HEADER}\")"
        ));
    }
    let mut plan = Plan::default();
    for (i, line) in lines.enumerate() {
        let number = i + 2;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let fields: Vec<&str> = line.splitn(3, '\t').collect();
        let need = |n: usize| -> Result<(), String> {
            if fields.len() < n {
                Err(format!(
                    "line {number}: \"{}\" needs {} tab-separated field(s)",
                    fields[0].trim(),
                    n - 1
                ))
            } else {
                Ok(())
            }
        };
        let in_demo = |plan: &mut Plan| -> Result<(), String> {
            if plan.demos.is_empty() {
                Err(format!(
                    "line {number}: \"{}\" before any \"demo\" line",
                    fields[0].trim()
                ))
            } else {
                Ok(())
            }
        };
        match fields[0].trim() {
            "demo" => {
                need(2)?;
                let demo = fields[1..].join("\t").trim().to_string();
                if demo.is_empty() {
                    return Err(format!("line {number}: no demo path"));
                }
                plan.demos.push(DemoPlan {
                    demo,
                    ..DemoPlan::default()
                });
            }
            "init" => {
                need(2)?;
                in_demo(&mut plan)?;
                let line = command_text(&fields[1..].join("\t"), number)?;
                plan.demos.last_mut().unwrap().init.push(line);
            }
            "at" => {
                need(3)?;
                in_demo(&mut plan)?;
                let at = time(fields[1], number)?;
                let action = Action::Run(command_text(fields[2], number)?);
                plan.demos
                    .last_mut()
                    .unwrap()
                    .entries
                    .push(Entry { at, action });
            }
            "seek" => {
                need(3)?;
                in_demo(&mut plan)?;
                let at = time(fields[1], number)?;
                let to = time(fields[2], number)?;
                plan.demos.last_mut().unwrap().entries.push(Entry {
                    at,
                    action: Action::Seek(to),
                });
            }
            "next" => {
                need(2)?;
                in_demo(&mut plan)?;
                let at = time(fields[1], number)?;
                plan.demos.last_mut().unwrap().entries.push(Entry {
                    at,
                    action: Action::Next,
                });
            }
            "end" => {
                need(2)?;
                plan.end
                    .push(command_text(&fields[1..].join("\t"), number)?);
            }
            other => return Err(format!("line {number}: unknown line \"{other}\"")),
        }
    }
    if plan.demos.is_empty() {
        return Err("the batch has no demos".to_string());
    }
    for demo in &mut plan.demos {
        demo.entries.sort_by(|a, b| a.at.total_cmp(&b.at));
    }
    Ok(plan)
}

/// What one frame of a demo's schedule asks for, after its due commands ran.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Then {
    Play,
    Seek(f64),
    Next,
}

/// The cursor through one demo's entries.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cursor {
    pub next: usize,
}

impl Cursor {
    /// Moves past every entry due at clock `now`. Returns the commands to run,
    /// in order, and what comes after them: the demo's end, or the furthest
    /// seek that is still ahead. Only ever forward: a batch never seeks back,
    /// so a clock that goes back (a seek by hand) re-runs nothing.
    pub fn due(&mut self, entries: &[Entry], now: f64) -> (Vec<String>, Then) {
        let mut run = Vec::new();
        let mut then = Then::Play;
        while self.next < entries.len() && entries[self.next].at <= now {
            match &entries[self.next].action {
                Action::Run(line) => run.push(line.clone()),
                Action::Seek(to) => {
                    if *to > now + MIN_SEEK_SECONDS {
                        then = match then {
                            Then::Seek(t) if t >= *to => Then::Seek(t),
                            _ => Then::Seek(*to),
                        };
                    }
                }
                Action::Next => {
                    self.next += 1;
                    return (run, Then::Next);
                }
            }
            self.next += 1;
        }
        (run, then)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// `viewdemo` sent; waiting for the demo to be read.
    Loading {
        since: Instant,
        frames: u32,
        seen_loading: bool,
    },
    /// Running the demo's schedule.
    Playing { cursor: Cursor, beat: Instant },
}

struct Batch {
    plan: Plan,
    at: usize,
    phase: Phase,
    started: Instant,
    /// Demos skipped, with why.
    skipped: Vec<String>,
}

static BATCH: Mutex<Option<Batch>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Batch>> {
    BATCH.lock().unwrap_or_else(|e| e.into_inner())
}

fn say(line: &str) {
    crate::commands::console_print(&format!("{NAME}: {line}\n"));
    unsafe { crate::debug::report(&format!("batch: {line}")) };
}

/// To the hook's log only, not the console.
fn trace(line: &str) {
    unsafe { crate::debug::report(&format!("batch: {line}")) };
}

fn run(line: &str) -> bool {
    let ok =
        std::ffi::CString::new(format!("{line}\n")).is_ok_and(|l| crate::engine::client_cmd(&l));
    if !ok {
        trace(&format!("the engine refused: {line}"));
    }
    ok
}

/// Loads demo `at`, or ends the batch after the last.
fn go_to(guard: &mut Option<Batch>, at: usize) {
    let Some(batch) = guard.as_mut() else { return };
    if at >= batch.plan.demos.len() {
        finish(guard);
        return;
    }
    batch.at = at;
    let demo = batch.plan.demos[at].demo.clone();
    let dod = crate::texture_hires::game_dir();
    match crate::review::viewdemo_line(&dod.to_string_lossy(), &demo) {
        Ok(line) if run(line.trim_end()) => {
            trace(&format!(
                "demo {} of {}: loading {demo}",
                at + 1,
                batch.plan.demos.len()
            ));
            batch.phase = Phase::Loading {
                since: Instant::now(),
                frames: 0,
                seen_loading: false,
            };
        }
        Ok(_) => skip(guard, "the engine's command buffer is not available".into()),
        Err(why) => skip(guard, why),
    }
}

/// Gives up on the current demo and moves to the next.
fn skip(guard: &mut Option<Batch>, why: String) {
    let Some(batch) = guard.as_mut() else { return };
    let line = format!(
        "demo {} of {} skipped: {why}",
        batch.at + 1,
        batch.plan.demos.len()
    );
    say(&line);
    crate::events::send(&format!("BATCH_PROBLEM\t{line}"));
    batch.skipped.push(line);
    // Whatever this demo was recording is not a clip.
    run("mirv_recordmovie_stop");
    let next = batch.at + 1;
    go_to(guard, next);
}

fn finish(guard: &mut Option<Batch>) {
    let Some(batch) = guard.take() else { return };
    say(&format!(
        "done: {} demo(s) in {:.0} s{}",
        batch.plan.demos.len(),
        batch.started.elapsed().as_secs_f64(),
        if batch.skipped.is_empty() {
            String::new()
        } else {
            format!(", {} skipped", batch.skipped.len())
        }
    ));
    for line in &batch.plan.end {
        run(line);
    }
}

/// Runs every frame.
pub fn poll() {
    // Never waits on the lock: a frame that finds it held tries again next frame.
    let Ok(mut guard) = BATCH.try_lock() else {
        return;
    };
    let Some(batch) = guard.as_mut() else { return };
    match batch.phase.clone() {
        Phase::Loading {
            since,
            frames,
            seen_loading,
        } => {
            let frames = frames + 1;
            let clock = crate::demo_seek::clock();
            let seen_loading = seen_loading || clock.is_some_and(|c| c.loading);
            if since.elapsed() > LOAD_TIMEOUT {
                skip(&mut guard, "the demo did not load in three minutes".into());
                return;
            }
            let ready = frames >= SETTLE_FRAMES
                && (seen_loading || frames >= UNSEEN_LOAD_FRAMES)
                && clock.is_some_and(|c| c.active && !c.loading);
            if !ready {
                batch.phase = Phase::Loading {
                    since,
                    frames,
                    seen_loading,
                };
                return;
            }
            let c = clock.expect("ready implies a clock");
            let plan = &batch.plan.demos[batch.at];
            trace(&format!(
                "demo {} of {}: loaded in {:.1} s, clock {:.1} ({:.1}..{:.1}); {} init command(s), {} timed",
                batch.at + 1,
                batch.plan.demos.len(),
                since.elapsed().as_secs_f64(),
                c.now,
                c.start,
                c.end,
                plan.init.len(),
                plan.entries.len()
            ));
            for line in plan.init.clone() {
                run(&line);
            }
            let _ = crate::demo_seek::set_time_scale(1.0);
            let _ = crate::demo_seek::set_paused(false);
            // The window and the menu would be in every frame recorded.
            crate::studio_panel::close_for_playback();
            batch.phase = Phase::Playing {
                cursor: Cursor { next: 0 },
                beat: Instant::now(),
            };
        }
        Phase::Playing { mut cursor, beat } => {
            if crate::demo_seek::seeking() {
                return;
            }
            let Some(c) = crate::demo_seek::clock().filter(|c| c.active && !c.loading) else {
                skip(&mut guard, "the demo stopped playing".into());
                return;
            };
            let entries = &batch.plan.demos[batch.at].entries;
            let (lines, then) = cursor.due(entries, c.now);
            let left = entries.len() - cursor.next;
            for line in &lines {
                run(line);
            }
            let beat = if beat.elapsed() >= HEARTBEAT {
                trace(&format!(
                    "demo {}: clock {:.1}, {left} timed line(s) left",
                    batch.at + 1,
                    c.now
                ));
                Instant::now()
            } else {
                beat
            };
            match then {
                Then::Next => {
                    let next = batch.at + 1;
                    go_to(&mut guard, next);
                }
                Then::Seek(to) => {
                    match crate::demo_seek::seek_to_seconds(to) {
                        Ok(how) => trace(&format!(
                            "demo {}: seeking from {:.1} to {to:.1} ({how})",
                            batch.at + 1,
                            c.now
                        )),
                        Err(why) => trace(&format!(
                            "demo {}: the seek to {to:.1} failed ({why}); playing on",
                            batch.at + 1
                        )),
                    }
                    batch.phase = Phase::Playing { cursor, beat };
                }
                Then::Play => {
                    // The last entry passed with no `next`: the demo is done.
                    if left == 0 && entries.last().is_some_and(|e| e.action != Action::Next) {
                        let next = batch.at + 1;
                        go_to(&mut guard, next);
                    } else if c.now >= c.end - 0.05 && left > 0 {
                        skip(
                            &mut guard,
                            format!(
                                "the demo ended at {:.1} s with {left} timed line(s) not run",
                                c.end
                            ),
                        );
                    } else {
                        batch.phase = Phase::Playing { cursor, beat };
                    }
                }
            }
        }
    }
}

fn start(path: &str) {
    if path.is_empty() {
        say(&format!("usage: {NAME} {USAGE}"));
        return;
    }
    let plan = match std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {path}: {e}"))
        .and_then(|text| parse(&text))
    {
        Ok(plan) => plan,
        Err(why) => {
            say(&why);
            crate::events::send(&format!("BATCH_PROBLEM\t{why}"));
            return;
        }
    };
    let timed: usize = plan.demos.iter().map(|d| d.entries.len()).sum();
    say(&format!(
        "{} demo(s), {timed} timed line(s), from {path}",
        plan.demos.len()
    ));
    // Running in the background: out of the user's way from the start.
    if crate::run_in_background::wanted() {
        crate::hlae_window::step_aside();
    }
    let mut guard = lock();
    *guard = Some(Batch {
        plan,
        at: 0,
        phase: Phase::Playing {
            cursor: Cursor { next: 0 },
            beat: Instant::now(),
        },
        started: Instant::now(),
        skipped: Vec::new(),
    });
    go_to(&mut guard, 0);
}

/// `dodstudio_batch <what>`.
pub unsafe extern "C" fn command() {
    let args = crate::cmd_list::args();
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        None => match lock().as_ref() {
            Some(b) => {
                let what = match &b.phase {
                    Phase::Loading { .. } => "loading".to_string(),
                    Phase::Playing { cursor, .. } => format!(
                        "{} of {} timed line(s) run",
                        cursor.next,
                        b.plan.demos[b.at].entries.len()
                    ),
                };
                say(&format!(
                    "demo {} of {}: {what}",
                    b.at + 1,
                    b.plan.demos.len()
                ));
            }
            None => say(&format!("no batch running; usage: {NAME} {USAGE}")),
        },
        Some("start") => start(args[1..].join(" ").trim().trim_matches('"')),
        Some("stop") => {
            if lock().take().is_some() {
                run("mirv_recordmovie_stop");
                say("stopped");
            } else {
                say("no batch running");
            }
        }
        Some(_) => say(&format!("usage: {NAME} {USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(lines: &[&str]) -> String {
        std::iter::once(HEADER)
            .chain(lines.iter().copied())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_batch_file_parses_into_sorted_demo_sections() {
        let plan = parse(&file(&[
            "// two demos",
            "demo\tC:/demos/a b.dem",
            "init\tmirv_movie_fps 60",
            "at\t310.5\techo late",
            "seek\t300\t305",
            "at\t300\techo first",
            "next\t320",
            "",
            "demo\tC:/demos/c.dem",
            "at\t10\techo c",
            "end\tsys_capture_done_path",
        ]))
        .unwrap();
        assert_eq!(plan.demos.len(), 2);
        let a = &plan.demos[0];
        assert_eq!(a.demo, "C:/demos/a b.dem");
        assert_eq!(a.init, ["mirv_movie_fps 60"]);
        let got: Vec<_> = a.entries.iter().map(|e| (e.at, e.action.clone())).collect();
        assert_eq!(
            got,
            [
                (300.0, Action::Seek(305.0)),
                (300.0, Action::Run("echo first".into())),
                (310.5, Action::Run("echo late".into())),
                (320.0, Action::Next),
            ]
        );
        assert_eq!(plan.end, ["sys_capture_done_path"]);
    }

    #[test]
    fn a_bad_batch_file_says_which_line() {
        assert!(parse("hello").unwrap_err().contains("not a batch file"));
        assert!(parse(HEADER).unwrap_err().contains("no demos"));
        let e = parse(&file(&["at\t1\techo"])).unwrap_err();
        assert!(e.contains("line 2") && e.contains("before any"), "{e}");
        let e = parse(&file(&["demo\tx.dem", "at\tsoon\techo"])).unwrap_err();
        assert!(e.contains("line 3") && e.contains("not a time"), "{e}");
        let e = parse(&file(&["demo\tx.dem", "at\t1"])).unwrap_err();
        assert!(e.contains("needs 2"), "{e}");
        let e = parse(&file(&["demo\tx.dem", "jump\t1"])).unwrap_err();
        assert!(e.contains("unknown line"), "{e}");
        let e = parse(&file(&["demo\tx.dem", "seek\t5\t-1"])).unwrap_err();
        assert!(e.contains("0 or more"), "{e}");
    }

    fn entries(text: &[&str]) -> Vec<Entry> {
        let mut lines = vec!["demo\tx.dem"];
        lines.extend_from_slice(text);
        parse(&file(&lines)).unwrap().demos.remove(0).entries
    }

    #[test]
    fn commands_run_once_in_order_and_a_seek_comes_after_them() {
        let e = entries(&[
            "at\t1\ta",
            "seek\t5\t20",
            "at\t5\tb",
            "at\t21\tc",
            "next\t30",
        ]);
        let mut cursor = Cursor { next: 0 };
        assert_eq!(cursor.due(&e, 0.5), (vec![], Then::Play));
        assert_eq!(cursor.due(&e, 1.0), (vec!["a".to_string()], Then::Play));
        assert_eq!(cursor.due(&e, 1.5), (vec![], Then::Play));
        // The command at the seek's own time still runs, before the seek.
        assert_eq!(
            cursor.due(&e, 5.0),
            (vec!["b".to_string()], Then::Seek(20.0))
        );
        assert_eq!(cursor.due(&e, 20.0), (vec![], Then::Play));
        assert_eq!(cursor.due(&e, 21.0), (vec!["c".to_string()], Then::Play));
        assert_eq!(cursor.due(&e, 31.0), (vec![], Then::Next));
    }

    #[test]
    fn a_seek_that_is_not_ahead_plays_on() {
        let e = entries(&["seek\t5\t5.2", "seek\t6\t4", "next\t9"]);
        let mut cursor = Cursor { next: 0 };
        assert_eq!(cursor.due(&e, 5.0), (vec![], Then::Play));
        assert_eq!(cursor.due(&e, 6.0), (vec![], Then::Play));
    }

    #[test]
    fn a_clock_that_goes_back_reruns_nothing() {
        let e = entries(&["at\t1\ta", "at\t2\tb", "next\t9"]);
        let mut cursor = Cursor { next: 0 };
        assert_eq!(cursor.due(&e, 2.0).0, ["a", "b"]);
        assert_eq!(cursor.due(&e, 0.5), (vec![], Then::Play));
        assert_eq!(cursor.due(&e, 2.0), (vec![], Then::Play));
    }

    #[test]
    fn next_stops_the_frame_at_the_demo_end() {
        let e = entries(&["at\t1\ta", "next\t2", "at\t3\tafter"]);
        let mut cursor = Cursor { next: 0 };
        assert_eq!(cursor.due(&e, 5.0), (vec!["a".to_string()], Then::Next));
    }
}
