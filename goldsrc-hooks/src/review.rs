//! `dodstudio_review`: plays every highlight Studio queued, one after another,
//! and sends what the user makes of each back to Studio (issue #623).
//!
//! ## The flow
//!
//! Studio writes the queue to a file and sends `dodstudio_review start
//! "<file>"` over the game's pipe (#413). For each highlight the review loads
//! its demo with `viewdemo` (unless it is the one already in the player),
//! waits for the whole file to be read, seeks to [`LEAD`] seconds before the
//! first kill, and plays at normal speed until [`TAIL`] seconds after the
//! last. There it pauses and opens the DoD Studio window on its Review tab.
//!
//! `yes` or `no` sends the answer -- with the kill range and note from the
//! Review tab's boxes -- to Studio as a `[dod-studio] REVIEW` line on the
//! events pipe (#434), then moves on. `replay` plays the highlight again,
//! `next` and `back` move without answering, `stop` ends the review.
//!
//! ## The queue file
//!
//! [`QUEUE_HEADER`] on the first line, then one highlight per line, tab
//! separated: the demo's full path, Studio's key for the row, the player,
//! each kill's time on the demo player's clock (comma separated), the kill
//! range's first and last kill (from 1), the answer already given (`yes`,
//! `no` or `-`), and the note. The review starts at the first highlight with
//! no answer yet.
//!
//! Commands and [`poll`] both run on the game's main thread.

// The demo player and the window are 32-bit only; a host build compiles the
// rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::names::console_name;

pub const NAME: &str = console_name!("review");

/// The queue file's first line.
pub(crate) const QUEUE_HEADER: &str = "dodstudio-review 1";

/// Seconds played before a highlight's first kill, and after its last.
const LEAD: f64 = 4.0;
const TAIL: f64 = 2.0;
/// How long a demo may take to load before the review gives up on it.
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Frames after `viewdemo` before the demo player's state counts: until the
/// command runs, the player still holds the previous demo.
const SETTLE_FRAMES: u32 = 30;
/// Frames after which a demo that was never seen loading counts as loaded:
/// a small one can be read between two looks.
const UNSEEN_LOAD_FRAMES: u32 = 300;
/// The longest note sent to Studio, in characters.
const NOTE_MAX: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Yes,
    No,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Verdict::Yes => "yes",
            Verdict::No => "no",
        }
    }
}

/// One highlight in the queue.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Highlight {
    /// The demo's full path.
    pub demo: String,
    /// Studio's key for the row; handed back as is.
    pub key: String,
    pub player: String,
    /// Each kill's time on the demo player's clock.
    pub kills: Vec<f64>,
    /// The kill range, from 1.
    pub from: usize,
    pub to: usize,
    pub verdict: Option<Verdict>,
    pub note: String,
}

impl Highlight {
    /// Where playback starts and stops.
    fn window(&self) -> (f64, f64) {
        let first = self.kills.first().copied().unwrap_or(0.0);
        let last = self.kills.last().copied().unwrap_or(first);
        ((first - LEAD).max(0.0), last + TAIL)
    }
}

/// Reads a queue file.
pub(crate) fn parse_queue(text: &str) -> Result<Vec<Highlight>, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(QUEUE_HEADER) {
        return Err(format!("not a review queue (no \"{QUEUE_HEADER}\" line)"));
    }
    let mut queue = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            return Err(format!("line {} has {} fields, not 8", n + 2, fields.len()));
        }
        let kills = fields[3]
            .split(',')
            .filter(|t| !t.trim().is_empty())
            .map(|t| t.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("line {}: kill times {:?}", n + 2, fields[3]))?;
        if kills.is_empty() {
            return Err(format!("line {}: no kill times", n + 2));
        }
        let count = kills.len();
        let from = fields[4]
            .trim()
            .parse::<usize>()
            .unwrap_or(1)
            .clamp(1, count);
        let to = fields[5]
            .trim()
            .parse::<usize>()
            .unwrap_or(count)
            .clamp(from, count);
        let verdict = match fields[6].trim() {
            "yes" => Some(Verdict::Yes),
            "no" => Some(Verdict::No),
            _ => None,
        };
        queue.push(Highlight {
            demo: fields[0].trim().to_string(),
            key: fields[1].trim().to_string(),
            player: fields[2].trim().to_string(),
            kills,
            from,
            to,
            verdict,
            note: fields
                .get(7)
                .map_or(String::new(), |n| n.trim().to_string()),
        });
    }
    if queue.is_empty() {
        return Err("the queue has no highlights".to_string());
    }
    Ok(queue)
}

/// Where a review picks up: the first highlight not answered yet, or the
/// first of all when every one has been.
fn first_unanswered(queue: &[Highlight]) -> usize {
    queue.iter().position(|h| h.verdict.is_none()).unwrap_or(0)
}

/// A note as one line Studio can split on tabs.
fn clean_note(note: &str) -> String {
    let flat: String = note
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.trim().chars().take(NOTE_MAX).collect()
}

/// A kill number typed in a box, or `None` when it isn't one of `1..=count`.
fn typed_kill(text: &str, count: usize) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|&n| (1..=count).contains(&n))
}

/// The marker Studio reads an answer from.
fn answer_label(h: &Highlight, verdict: Verdict) -> String {
    format!(
        "REVIEW\t{}\t{}\t{}\t{}\t{}\t{}",
        h.demo,
        h.key,
        verdict.word(),
        h.from,
        h.to,
        clean_note(&h.note)
    )
}

/// The `viewdemo` line for `demo`, a full path, from the game's `dod`
/// folder: `viewdemo` takes a path from there, `..` included.
pub(crate) fn viewdemo_line(dod_dir: &str, demo: &str) -> Result<String, String> {
    let split = |p: &str| -> Vec<String> {
        p.split(['\\', '/'])
            .filter(|s| !s.is_empty() && *s != ".")
            .map(str::to_string)
            .collect()
    };
    let base = split(dod_dir);
    let target = split(demo);
    let same = |a: &String, b: &String| a.eq_ignore_ascii_case(b);
    if base.is_empty() || target.is_empty() || !same(&base[0], &target[0]) {
        return Err(format!(
            "{demo} is on another drive than the game, and viewdemo opens only demos it can reach from the game's folder"
        ));
    }
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(a, b)| same(a, b))
        .count();
    let mut parts: Vec<String> = vec!["..".to_string(); base.len() - common];
    parts.extend(target[common..].iter().cloned());
    let path = parts.join("/");
    if path.contains('"') {
        return Err(format!("{demo} has a quote in its path"));
    }
    Ok(format!("viewdemo \"{path}\"\n"))
}

/// What the review is doing.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// `viewdemo` sent; waiting for the demo to be read.
    Loading {
        since: Instant,
        frames: u32,
        seen_loading: bool,
    },
    /// Playing the highlight; pauses at `until` on the world clock.
    Playing { until: f64 },
    /// Paused at the end, for an answer.
    Waiting,
    /// This one could not be played; next and back still work.
    Failed(String),
}

struct Review {
    queue: Vec<Highlight>,
    at: usize,
    phase: Phase,
    /// The demo the review last loaded.
    loaded: Option<String>,
}

static REVIEW: Mutex<Option<Review>> = Mutex::new(None);
/// Bumped whenever the highlight changes or its range or note is set from the
/// console, so the Review tab fills its boxes again.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// Which highlight the Review tab's boxes belong to. An atomic, not the
/// review itself: the tab is asked for its boxes while an answer holds that.
pub(crate) fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

fn lock() -> std::sync::MutexGuard<'static, Option<Review>> {
    REVIEW.lock().unwrap_or_else(|e| e.into_inner())
}

fn say(line: &str) {
    crate::commands::console_print(&format!("{NAME}: {line}\n"));
    unsafe { crate::debug::report(&format!("review: {line}")) };
}

fn run(line: &str) -> bool {
    std::ffi::CString::new(line).is_ok_and(|l| crate::engine::client_cmd(&l))
}

/// What the Review tab shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TabView {
    pub generation: u32,
    pub heading: String,
    pub detail: String,
    pub from: usize,
    pub to: usize,
    pub kills: usize,
    pub note: String,
}

/// The Review tab's text with no review running.
pub(crate) const IDLE_HEADING: &str = "No review running.";
pub(crate) const IDLE_DETAIL: &str =
    "In DoD Studio, tick demos in the Master Demo Queue and press Review highlights.";

/// What the Review tab shows now, or `None` with no review running.
pub(crate) fn tab_view() -> Option<TabView> {
    let guard = lock();
    let review = guard.as_ref()?;
    let h = review.queue.get(review.at)?;
    Some(TabView {
        generation: GENERATION.load(Ordering::Relaxed),
        heading: heading(&review.queue, review.at),
        detail: detail(h, &review.phase),
        from: h.from,
        to: h.to,
        kills: h.kills.len(),
        note: h.note.clone(),
    })
}

/// "Demo 2 of 5 | highlight 3 of 9 | 4 left to answer".
fn heading(queue: &[Highlight], at: usize) -> String {
    let mut demos: Vec<&str> = Vec::new();
    for h in queue {
        if demos.last() != Some(&h.demo.as_str()) {
            demos.push(&h.demo);
        }
    }
    let demo_at = queue[..=at]
        .iter()
        .fold((0, ""), |(n, last), h| {
            if h.demo != last {
                (n + 1, h.demo.as_str())
            } else {
                (n, last)
            }
        })
        .0;
    let left = queue.iter().filter(|h| h.verdict.is_none()).count();
    format!(
        "Demo {demo_at} of {} | highlight {} of {} | {left} left to answer",
        demos.len(),
        at + 1,
        queue.len()
    )
}

fn detail(h: &Highlight, phase: &Phase) -> String {
    let who = if h.player.is_empty() {
        String::new()
    } else {
        format!("{}, ", h.player)
    };
    let kills = match h.kills.len() {
        1 => "1 kill".to_string(),
        n => format!("{n} kills"),
    };
    let answered = match h.verdict {
        Some(v) => format!(" (answered {})", v.word()),
        None => String::new(),
    };
    let what = match phase {
        Phase::Loading { .. } => "loading the demo...".to_string(),
        Phase::Playing { .. } => "playing...".to_string(),
        Phase::Waiting => "Yes or No?".to_string(),
        Phase::Failed(why) => format!("can't play this one: {why}"),
    };
    format!("{who}{kills}{answered} -- {what}")
}

/// Starts playing the current highlight from its start.
fn play(review: &mut Review) {
    let h = &review.queue[review.at];
    let (from, until) = h.window();
    let started = crate::demo_seek::seek_to_seconds(from)
        .and_then(|_| crate::demo_seek::set_time_scale(1.0))
        .and_then(|()| crate::demo_seek::set_paused(false));
    match started {
        Ok(()) => {
            review.phase = Phase::Playing { until };
            // Out of the way while it plays.
            run(&format!("{} 0\n", crate::studio_panel::NAME));
        }
        Err(why) => fail(review, why),
    }
}

fn fail(review: &mut Review, why: String) {
    say(&format!(
        "highlight {} of {}: {why} -- {NAME} next skips it",
        review.at + 1,
        review.queue.len()
    ));
    review.phase = Phase::Failed(why);
    open_tab();
}

fn open_tab() {
    run(&format!("{} review\n", crate::studio_panel::NAME));
}

/// Moves to highlight `at` and starts it: loads its demo, or plays it at
/// once when that demo is the one in the player.
fn go_to(review: &mut Review, at: usize) {
    review.at = at;
    GENERATION.fetch_add(1, Ordering::Relaxed);
    let demo = review.queue[at].demo.clone();
    let in_player = review.loaded.as_deref() == Some(demo.as_str())
        && crate::demo_seek::clock().is_some_and(|c| c.active && !c.loading);
    if in_player {
        play(review);
        return;
    }
    let dod = crate::texture_hires::game_dir();
    match viewdemo_line(&dod.to_string_lossy(), &demo) {
        Ok(line) if run(&line) => {
            review.loaded = Some(demo);
            review.phase = Phase::Loading {
                since: Instant::now(),
                frames: 0,
                seen_loading: false,
            };
        }
        Ok(_) => fail(
            review,
            "the engine's command buffer is not available".into(),
        ),
        Err(why) => fail(review, why),
    }
}

/// Moves on from the current highlight, or ends the review after the last.
fn advance(guard: &mut Option<Review>) {
    let Some(review) = guard.as_mut() else { return };
    if review.at + 1 < review.queue.len() {
        let next = review.at + 1;
        go_to(review, next);
    } else {
        let answered = review.queue.iter().filter(|h| h.verdict.is_some()).count();
        let total = review.queue.len();
        say(&format!(
            "that was the last highlight -- {answered} of {total} answered"
        ));
        crate::events::send("REVIEW_END\tdone");
        *guard = None;
        GENERATION.fetch_add(1, Ordering::Relaxed);
        open_tab();
    }
}

/// Runs every frame.
pub fn poll() {
    let mut guard = lock();
    let Some(review) = guard.as_mut() else { return };
    match review.phase.clone() {
        Phase::Loading {
            since,
            frames,
            seen_loading,
        } => {
            let frames = frames + 1;
            let clock = crate::demo_seek::clock();
            let seen_loading = seen_loading || clock.is_some_and(|c| c.loading);
            if since.elapsed() > LOAD_TIMEOUT {
                fail(review, "the demo did not load in three minutes".into());
                return;
            }
            let ready = frames >= SETTLE_FRAMES
                && (seen_loading || frames >= UNSEEN_LOAD_FRAMES)
                && clock.is_some_and(|c| c.active && !c.loading);
            if !ready {
                review.phase = Phase::Loading {
                    since,
                    frames,
                    seen_loading,
                };
                return;
            }
            let end = clock.map_or(0.0, |c| c.end);
            let last = review.queue[review.at].kills.last().copied().unwrap_or(0.0);
            if end < last {
                fail(
                    review,
                    format!("the demo ends at {end:.0} s, before the last kill at {last:.0} s"),
                );
                return;
            }
            play(review);
        }
        Phase::Playing { until } => match crate::demo_seek::clock() {
            Some(c) if c.active && c.now < until => {}
            Some(c) if c.active => {
                let _ = crate::demo_seek::set_paused(true);
                review.phase = Phase::Waiting;
                open_tab();
            }
            _ => fail(review, "the demo stopped".into()),
        },
        Phase::Waiting | Phase::Failed(_) => {}
    }
}

/// The kill range and note to answer with: the Review tab's boxes when it
/// has them, else what the highlight holds.
fn take_inputs(h: &mut Highlight) {
    let Some((from, to, note)) = crate::studio_panel::review_inputs() else {
        return;
    };
    let count = h.kills.len();
    let from = typed_kill(&from, count).unwrap_or(h.from);
    let to = typed_kill(&to, count).unwrap_or(h.to);
    (h.from, h.to) = (from.min(to), from.max(to));
    h.note = clean_note(&note);
}

fn answer(verdict: Verdict) {
    let mut guard = lock();
    let Some(review) = guard.as_mut() else {
        say("no review running");
        return;
    };
    let at = review.at;
    let h = &mut review.queue[at];
    take_inputs(h);
    h.verdict = Some(verdict);
    let label = answer_label(h, verdict);
    say(&format!(
        "highlight {} {}: kills {}-{}",
        at + 1,
        verdict.word(),
        h.from,
        h.to
    ));
    crate::events::send(&label);
    advance(&mut guard);
}

/// Everything after the subcommand, joined.
fn rest_of_line(from: i32) -> String {
    let Some(engfuncs) = crate::engine::engfuncs() else {
        return String::new();
    };
    let mut words = Vec::new();
    // Safety: the engine's own argument accessors, during a command.
    unsafe {
        for i in from..(engfuncs.cmd_argc)() {
            let raw = (engfuncs.cmd_argv)(i);
            if !raw.is_null() {
                words.push(
                    std::ffi::CStr::from_ptr(raw as *const std::ffi::c_char)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    words.join(" ")
}

const USAGE: &str = "start \"<queue file>\" | yes | no | replay | next | back | range <from> <to> | note <text> | stop";

/// `dodstudio_review <what>`.
pub unsafe extern "C" fn command() {
    let sub = rest_of_line(1);
    let mut words = sub.split_whitespace();
    let what = words.next().unwrap_or("").to_ascii_lowercase();
    match what.as_str() {
        "" => {
            let line = match tab_view() {
                Some(v) => format!("{} -- {}", v.heading, v.detail),
                None => format!("{IDLE_HEADING} {NAME} {USAGE}"),
            };
            say(&line);
        }
        "start" => start(rest_of_line(2).trim().trim_matches('"')),
        "yes" => answer(Verdict::Yes),
        "no" => answer(Verdict::No),
        "replay" => {
            let mut guard = lock();
            match guard.as_mut() {
                Some(review) if review.loaded.is_some() => {
                    let at = review.at;
                    if matches!(review.phase, Phase::Loading { .. }) {
                        say("still loading the demo");
                    } else {
                        go_to(review, at);
                    }
                }
                Some(_) => say("nothing to replay yet"),
                None => say("no review running"),
            }
        }
        "next" => {
            let mut guard = lock();
            if guard.is_none() {
                say("no review running");
            }
            advance(&mut guard);
        }
        "back" => {
            let mut guard = lock();
            match guard.as_mut() {
                Some(review) if review.at > 0 => {
                    let at = review.at - 1;
                    go_to(review, at);
                }
                Some(_) => say("this is the first highlight"),
                None => say("no review running"),
            }
        }
        "range" => {
            let mut guard = lock();
            let Some(review) = guard.as_mut() else {
                say("no review running");
                return;
            };
            let at = review.at;
            let h = &mut review.queue[at];
            let count = h.kills.len();
            let from = words.next().and_then(|w| typed_kill(w, count));
            let to = words.next().and_then(|w| typed_kill(w, count));
            match (from, to) {
                (Some(a), Some(b)) => {
                    (h.from, h.to) = (a.min(b), a.max(b));
                    GENERATION.fetch_add(1, Ordering::Relaxed);
                    say(&format!("kills {}-{} of {count}", h.from, h.to));
                }
                _ => say(&format!(
                    "usage: {NAME} range <from> <to>, each 1 to {count}"
                )),
            }
        }
        "note" => {
            let mut guard = lock();
            let Some(review) = guard.as_mut() else {
                say("no review running");
                return;
            };
            let at = review.at;
            review.queue[at].note = clean_note(&rest_of_line(2));
            GENERATION.fetch_add(1, Ordering::Relaxed);
        }
        "stop" => {
            if lock().take().is_some() {
                crate::events::send("REVIEW_END\tstopped");
                GENERATION.fetch_add(1, Ordering::Relaxed);
                say("stopped");
            } else {
                say("no review running");
            }
        }
        other => say(&format!("unknown \"{other}\" -- {NAME} {USAGE}")),
    }
}

fn start(path: &str) {
    if path.is_empty() {
        say(&format!("usage: {NAME} start \"<queue file>\""));
        return;
    }
    let queue = match std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {path}: {e}"))
        .and_then(|text| parse_queue(&text))
    {
        Ok(queue) => queue,
        Err(why) => {
            say(&why);
            crate::events::send(&format!("REVIEW_END\tfailed: {}", clean_note(&why)));
            return;
        }
    };
    let at = first_unanswered(&queue);
    say(&format!(
        "{} highlight(s); starting at {}",
        queue.len(),
        at + 1
    ));
    crate::events::send(&format!("REVIEW_START\t{}", queue.len()));
    let mut guard = lock();
    let review = guard.insert(Review {
        queue,
        at,
        phase: Phase::Waiting,
        loaded: None,
    });
    go_to(review, at);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_text(lines: &[&str]) -> String {
        std::iter::once(QUEUE_HEADER)
            .chain(lines.iter().copied())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn highlight(demo: &str, verdict: Option<Verdict>) -> Highlight {
        Highlight {
            demo: demo.to_string(),
            key: "0".to_string(),
            player: String::new(),
            kills: vec![10.0],
            from: 1,
            to: 1,
            verdict,
            note: String::new(),
        }
    }

    #[test]
    fn a_queue_reads_every_field() {
        let text = queue_text(&[
            "C:/demos/a.dem\t3\tm00cat\t100.5,104,110.25\t2\t3\t-\tnice flick",
            "C:/demos/a.dem\t7\tm00cat\t200\t1\t1\tyes\t",
        ]);
        let queue = parse_queue(&text).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].demo, "C:/demos/a.dem");
        assert_eq!(queue[0].key, "3");
        assert_eq!(queue[0].player, "m00cat");
        assert_eq!(queue[0].kills, vec![100.5, 104.0, 110.25]);
        assert_eq!((queue[0].from, queue[0].to), (2, 3));
        assert_eq!(queue[0].verdict, None);
        assert_eq!(queue[0].note, "nice flick");
        assert_eq!(queue[1].verdict, Some(Verdict::Yes));
        assert_eq!(queue[1].note, "");
    }

    #[test]
    fn a_trailing_empty_note_may_be_missing() {
        let queue = parse_queue(&queue_text(&["d.dem\t1\tp\t5\t1\t1\tno"])).unwrap();
        assert_eq!(queue[0].note, "");
        assert_eq!(queue[0].verdict, Some(Verdict::No));
    }

    #[test]
    fn a_range_outside_the_kills_is_pulled_in() {
        let queue = parse_queue(&queue_text(&["d.dem\t1\tp\t5,6\t0\t9\t-\t"])).unwrap();
        assert_eq!((queue[0].from, queue[0].to), (1, 2));
    }

    #[test]
    fn a_bad_queue_says_why() {
        assert!(
            parse_queue("hello")
                .unwrap_err()
                .contains("not a review queue")
        );
        assert!(
            parse_queue(QUEUE_HEADER)
                .unwrap_err()
                .contains("no highlights")
        );
        assert!(
            parse_queue(&queue_text(&["d.dem\t1\tp\tx\t1\t1\t-\t"]))
                .unwrap_err()
                .contains("kill times")
        );
        assert!(
            parse_queue(&queue_text(&["d.dem\t1"]))
                .unwrap_err()
                .contains("fields")
        );
    }

    #[test]
    fn the_review_starts_at_the_first_unanswered_highlight() {
        let mut queue = vec![
            highlight("a", Some(Verdict::Yes)),
            highlight("a", None),
            highlight("b", None),
        ];
        assert_eq!(first_unanswered(&queue), 1);
        queue[1].verdict = Some(Verdict::No);
        queue[2].verdict = Some(Verdict::No);
        assert_eq!(first_unanswered(&queue), 0);
    }

    #[test]
    fn playback_runs_from_before_the_first_kill_to_after_the_last() {
        let mut h = highlight("a", None);
        h.kills = vec![100.0, 112.0];
        assert_eq!(h.window(), (100.0 - LEAD, 112.0 + TAIL));
        h.kills = vec![1.0];
        assert_eq!(h.window(), (0.0, 1.0 + TAIL));
    }

    #[test]
    fn an_answer_is_one_tab_separated_line() {
        let mut h = highlight("C:/d/a.dem", None);
        h.key = "4".to_string();
        h.kills = vec![1.0, 2.0, 3.0];
        (h.from, h.to) = (2, 3);
        h.note = "two\tlines\nhere ".to_string();
        assert_eq!(
            answer_label(&h, Verdict::Yes),
            "REVIEW\tC:/d/a.dem\t4\tyes\t2\t3\ttwo lines here"
        );
    }

    #[test]
    fn a_note_is_cut_to_its_limit() {
        assert_eq!(clean_note(&"x".repeat(NOTE_MAX + 50)).len(), NOTE_MAX);
    }

    #[test]
    fn a_typed_kill_must_be_one_of_the_kills() {
        assert_eq!(typed_kill(" 2 ", 3), Some(2));
        assert_eq!(typed_kill("0", 3), None);
        assert_eq!(typed_kill("4", 3), None);
        assert_eq!(typed_kill("two", 3), None);
    }

    #[test]
    fn viewdemo_reaches_a_demo_from_the_game_folder() {
        let dod = r"C:\Steam\common\Half-Life\dod";
        assert_eq!(
            viewdemo_line(dod, r"C:\Steam\common\Half-Life\dod\demos\a b.dem").unwrap(),
            "viewdemo \"demos/a b.dem\"\n"
        );
        assert_eq!(
            viewdemo_line(dod, r"c:/steam/common/Half-Life/dod/x.dem").unwrap(),
            "viewdemo \"x.dem\"\n"
        );
        assert_eq!(
            viewdemo_line(dod, r"C:\Users\me\Downloads\m.dem").unwrap(),
            "viewdemo \"../../../../Users/me/Downloads/m.dem\"\n"
        );
        assert!(
            viewdemo_line(dod, r"F:\Downloads\m.dem")
                .unwrap_err()
                .contains("another drive")
        );
    }

    #[test]
    fn the_heading_counts_demos_and_what_is_left() {
        let queue = vec![
            highlight("a", Some(Verdict::Yes)),
            highlight("a", None),
            highlight("b", None),
        ];
        assert_eq!(
            heading(&queue, 2),
            "Demo 2 of 2 | highlight 3 of 3 | 2 left to answer"
        );
        assert_eq!(
            heading(&queue, 0),
            "Demo 1 of 2 | highlight 1 of 3 | 2 left to answer"
        );
    }

    #[test]
    fn the_detail_says_what_to_do() {
        let mut h = highlight("a", None);
        h.player = "m00cat".to_string();
        h.kills = vec![1.0, 2.0];
        assert_eq!(detail(&h, &Phase::Waiting), "m00cat, 2 kills -- Yes or No?");
        h.verdict = Some(Verdict::No);
        assert_eq!(
            detail(&h, &Phase::Failed("gone".into())),
            "m00cat, 2 kills (answered no) -- can't play this one: gone"
        );
    }
}
