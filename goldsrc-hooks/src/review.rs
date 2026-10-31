//! `ds_demo_review`: plays every highlight Studio queued, one after another,
//! and sends what the user makes of each back to Studio (issue #623).
//!
//! ## The flow
//!
//! Studio writes the queue to a file and sends `ds_demo_review start
//! "<file>"` over the game's pipe (#413). For each highlight the review loads
//! its demo with `viewdemo` (unless it is the one already in the player),
//! waits for the whole file to be read, seeks to [`LEAD`] seconds before the
//! first kill, and plays at normal speed until [`TAIL`] seconds after the
//! last. There it pauses and waits for an answer: on the game screen, from
//! the answer keys (`review_keys`, #660), or with `ds_ui_review_keys 0` on
//! the DoD Studio window's Review tab, which it opens. The keys answer while
//! the highlight plays too, ending it early (#1141), but not for [`LOCKOUT`]
//! after it starts.
//!
//! `yes`, `maybe` (#661) or `no` sends the answer -- with the kill range and note from the
//! Review tab's boxes -- to Studio as a `[dod-studio] REVIEW` line on the
//! events pipe (#434), then moves on. `replay` plays the highlight again,
//! `next` and `back` move without answering, `stop` ends the review.
//!
//! The kill range and note can also be set without the menu: E (or `form`)
//! opens a small form over the demo (`review_form`, #1139), whose Enter puts
//! them on the highlight and sends them at once as a `REVIEW_EDIT` line,
//! which sets the row's range and note and leaves its mark. The answer after
//! it carries the same values.
//!
//! ## The queue file
//!
//! [`QUEUE_HEADER`] on the first line, optionally followed by tab-separated
//! `key=value` options ([`Options`]), then one highlight per line, tab
//! separated: the demo's full path, Studio's key for the row, the player,
//! each kill's time on the demo player's clock (comma separated), the kill
//! range's first and last kill (from 1), the answer already given (`yes`,
//! `maybe`, `no` or `-`), the note, and, for an HLTV demo, the player's
//! number (#822; `-` or missing for a POV demo), and each kill's victim and
//! weapon ([`KILL_SEP`] between kills, [`PART_SEP`] between a kill's two;
//! #1140; missing from an older Studio). The review starts at the
//! first highlight: Studio's Review box decides what the queue holds, so
//! picking up where a review left off is its Unmarked choice. A header's
//! `from=<seconds>` starts it at the first highlight whose first kill is at
//! or after that time instead (#1142's From here); Back still reaches the
//! ones before.
//!
//! ## Starting one from the game
//!
//! With no review running, the Review tab's From the start and From here
//! (`ask start`, `ask here`) ask DoD Studio to review the demo playing
//! (`review_ask`, #1142); Studio sends the queue back as above.
//!
//! ## HLTV demos
//!
//! An HLTV demo's camera follows no one by itself, so a highlight with a
//! player number puts the camera on them in first person (`spec_mode 4` and
//! `ds_spec_target`) once each seek has landed, the way the
//! Highlights tab's Go does: the player may only exist after the seek. For
//! the whole review it holds `ds_spec_lock`'s lock on, so a death
//! before a highlight ends can't move the camera to someone else. The hold
//! adds to the cvar (the lock is on if either asks), never writes it, so a
//! user who keeps the cvar at 1 still has the lock after the review ends.
//! While the hold is on, the viewer's own spectator buttons (a click, jump
//! or duck) do nothing either (`spectator_buttons`, #826), and the demo
//! controls (VCR bar, Load Demo, the Playback / Demos / Highlights tabs) are
//! greyed out (`review_lock`, #826). The console refuses the commands that
//! move the demo or the camera too (#855), all but the review's own lines,
//! which `run` and the seek's follow-up name to the lock first.
//!
//! Commands and [`poll`] both run on the game's main thread.

// The demo player and the window are 32-bit only; a host build compiles the
// rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::names::console_name;
use crate::review_keys::KeyMode;

pub const NAME: &str = console_name!("demo_review");

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
/// How long the answer keys do nothing once a highlight starts playing
/// (#1141): long enough that a double press on one answer, or a key held a
/// beat long, can't answer the next highlight unseen; short enough that a
/// clip the user already knows is a No can be answered at once.
pub(crate) const LOCKOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Yes,
    /// Undecided: look again later (#661). Counts as answered, so the next
    /// review passes over it unless Studio queues only Maybe rows.
    Maybe,
    No,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Verdict::Yes => "yes",
            Verdict::Maybe => "maybe",
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
    /// HLTV: the player the camera goes on, by number (#822).
    pub spectate: Option<i32>,
    /// Each kill's victim and weapon, one per kill, or none from an older
    /// Studio (#1140).
    pub details: Vec<KillDetail>,
}

/// One kill's victim and weapon, as Studio's Highlight Details lists them.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct KillDetail {
    pub victim: String,
    pub weapon: String,
}

/// Between two kills in the queue's details field, and between a kill's
/// victim and weapon: control characters, which Studio keeps out of every
/// name. Must match `native::review_queue::KILL_SEP` and `PART_SEP`.
pub(crate) const KILL_SEP: char = '\u{1f}';
pub(crate) const PART_SEP: char = '\u{1e}';

/// The details field, or none when it is missing or doesn't hold one entry
/// per kill.
fn parse_details(field: Option<&str>, count: usize) -> Vec<KillDetail> {
    let Some(field) = field.filter(|f| !f.trim().is_empty()) else {
        return Vec::new();
    };
    let details: Vec<KillDetail> = field
        .split(KILL_SEP)
        .map(|kill| {
            let (victim, weapon) = kill.split_once(PART_SEP).unwrap_or((kill, ""));
            KillDetail {
                victim: victim.trim().to_string(),
                weapon: weapon.trim().to_string(),
            }
        })
        .collect();
    if details.len() == count {
        details
    } else {
        Vec::new()
    }
}

impl Highlight {
    /// What runs once the seek has landed: the camera onto the player, in
    /// first person, for an HLTV highlight.
    fn camera_line(&self) -> Option<String> {
        self.spectate
            .map(|n| format!("spec_mode 4; {} {n}", crate::spectator_follow::TARGET_NAME))
    }

    /// Where playback starts and stops.
    fn window(&self) -> (f64, f64) {
        let first = self.kills.first().copied().unwrap_or(0.0);
        let last = self.kills.last().copied().unwrap_or(first);
        ((first - LEAD).max(0.0), last + TAIL)
    }
}

/// The default fast-forward speed when the queue names none.
const FAST_SPEED: f32 = 4.0;

/// How the queue asks the review to play (#665).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Options {
    /// A gap between two kills longer than this many seconds is fast-forwarded;
    /// 0 plays every gap at normal speed.
    pub gap: f64,
    /// The speed a gap plays at.
    pub speed: f32,
    /// Start at the first highlight at or after this demo time (#1142).
    pub from: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            gap: 0.0,
            speed: FAST_SPEED,
            from: None,
        }
    }
}

/// The header line's `key=value` options. Unknown keys and bad values are
/// ignored, so an older or newer Studio still gets a review.
fn parse_options<'a>(fields: impl Iterator<Item = &'a str>) -> Options {
    let mut options = Options::default();
    for field in fields {
        match field.trim().split_once('=') {
            Some(("gap", v)) => {
                if let Ok(gap) = v.trim().parse::<f64>()
                    && gap.is_finite()
                {
                    options.gap = gap.clamp(0.0, 600.0);
                }
            }
            Some(("speed", v)) => {
                if let Ok(speed) = v.trim().parse::<f32>()
                    && speed.is_finite()
                {
                    options.speed = speed.clamp(1.0, 16.0);
                }
            }
            Some(("from", v)) => {
                options.from = v
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|from| from.is_finite() && *from >= 0.0);
            }
            _ => {}
        }
    }
    options
}

/// The stretches of a highlight to fast-forward: between two kills more than
/// `gap` seconds apart, from [`TAIL`] after the first to [`LEAD`] before the
/// second, the same margins a highlight starts and ends with.
fn fast_spans(kills: &[f64], gap: f64) -> Vec<(f64, f64)> {
    if gap <= 0.0 {
        return Vec::new();
    }
    kills
        .windows(2)
        .filter(|pair| pair[1] - pair[0] > gap)
        .map(|pair| (pair[0] + TAIL, pair[1] - LEAD))
        .filter(|(start, end)| end > start)
        .collect()
}

/// The log's note on what will be fast-forwarded, or nothing.
fn describe_spans(spans: &[(f64, f64)], speed: f32) -> String {
    if spans.is_empty() {
        return String::new();
    }
    let list: Vec<String> = spans
        .iter()
        .map(|(s, e)| format!("{s:.1}..{e:.1}"))
        .collect();
    format!("; at {speed}x: {}", list.join(", "))
}

/// Reads a queue file.
pub(crate) fn parse_queue(text: &str) -> Result<(Vec<Highlight>, Options), String> {
    let mut lines = text.lines();
    let mut header = lines.next().unwrap_or("").split('\t');
    if header.next().map(str::trim) != Some(QUEUE_HEADER) {
        return Err(format!("not a review queue (no \"{QUEUE_HEADER}\" line)"));
    }
    let options = parse_options(header);
    let mut queue = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 7 {
            return Err(format!(
                "line {} has {} fields, not 8 or 9",
                n + 2,
                fields.len()
            ));
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
            "maybe" => Some(Verdict::Maybe),
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
            spectate: fields
                .get(8)
                .and_then(|n| n.trim().parse::<i32>().ok())
                .filter(|n| (1..=32).contains(n)),
            details: parse_details(fields.get(9).copied(), count),
        });
    }
    if queue.is_empty() {
        return Err("the queue has no highlights".to_string());
    }
    Ok((queue, options))
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

/// The marker Studio reads a kill range and note saved without an answer
/// from (#1139's form): the highlight's mark stays as it is.
fn edit_label(h: &Highlight) -> String {
    format!(
        "REVIEW_EDIT\t{}\t{}\t{}\t{}\t{}",
        h.demo,
        h.key,
        h.from,
        h.to,
        clean_note(&h.note)
    )
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
/// folder: `viewdemo` takes a path from there, `..` included. A demo on
/// another drive goes by its full path, `D:/folder/demo.dem`, when
/// `demo_other_drives` can open it (#776, #779).
pub(crate) fn viewdemo_line(dod_dir: &str, demo: &str) -> Result<String, String> {
    viewdemo_line_with(dod_dir, demo, crate::demo_other_drives::ready())
}

/// The longest `viewdemo` argument: the engine copies it into a 256-byte
/// buffer. Studio checks the path without `.dem` against 240
/// (`native::preview_in_place::MAX_DEMO_ARG`); this one has it.
const MAX_DEMO_ARG: usize = 244;

/// [`viewdemo_line`], with `other_drives` whether a full path with a drive
/// letter opens now (`Err` says why not).
fn viewdemo_line_with(
    dod_dir: &str,
    demo: &str,
    other_drives: Result<(), String>,
) -> Result<String, String> {
    let split = |p: &str| -> Vec<String> {
        p.split(['\\', '/'])
            .filter(|s| !s.is_empty() && *s != ".")
            .map(str::to_string)
            .collect()
    };
    let base = split(dod_dir);
    let target = split(demo);
    let same = |a: &String, b: &String| a.eq_ignore_ascii_case(b);
    if base.is_empty() || target.is_empty() {
        return Err(format!("{demo} is not a full path"));
    }
    let path = if same(&base[0], &target[0]) {
        let common = base
            .iter()
            .zip(&target)
            .take_while(|(a, b)| same(a, b))
            .count();
        let mut parts: Vec<String> = vec!["..".to_string(); base.len() - common];
        parts.extend(target[common..].iter().cloned());
        parts.join("/")
    } else {
        let has_letter =
            matches!(target[0].as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic());
        if !has_letter || target.len() < 2 {
            return Err(format!(
                "{demo} is on another drive than the game, and viewdemo opens only demos it can reach from the game's folder or by a drive letter"
            ));
        }
        if let Err(why) = other_drives {
            return Err(format!(
                "{demo} is on another drive than the game, and {why}"
            ));
        }
        target.join("/")
    };
    if path.contains('"') {
        return Err(format!("{demo} has a quote in its path"));
    }
    // `viewdemo` lower-cases the name byte by byte before opening it, which
    // breaks every non-ASCII character (#779, measured on POST).
    if !path.is_ascii() {
        return Err(format!(
            "{demo} has a character viewdemo can't open (letters, digits and punctuation without accents only)"
        ));
    }
    if path.len() > MAX_DEMO_ARG {
        return Err(format!(
            "{demo} has too long a path for viewdemo ({} characters, {MAX_DEMO_ARG} at most)",
            path.len()
        ));
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
    /// Playing the highlight; pauses at `until` on the world clock. `last` is
    /// the clock a frame ago, to log a jump, and `beat` when the log last
    /// heard where it is. `spans` are the gaps to fast-forward ([`fast_spans`])
    /// and `fast` whether one is playing fast now.
    Playing {
        until: f64,
        last: f64,
        beat: Instant,
        spans: Vec<(f64, f64)>,
        fast: bool,
    },
    /// Paused at the end, for an answer.
    Waiting,
    /// This one could not be played; next and back still work.
    Failed(String),
}

struct Review {
    queue: Vec<Highlight>,
    options: Options,
    at: usize,
    phase: Phase,
    /// The demo the review last loaded.
    loaded: Option<String>,
    /// When the answer keys come on for the highlight playing ([`LOCKOUT`]).
    keys_from: Instant,
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

/// A clock change between two frames bigger than this is logged as a jump.
const JUMP_SECONDS: f64 = 2.0;
/// How often the log hears where a playing highlight is.
const HEARTBEAT: Duration = Duration::from_secs(10);

/// To the hook's log only, not the console.
fn trace(line: &str) {
    unsafe { crate::debug::report(&format!("review: {line}")) };
}

fn run(line: &str) -> bool {
    // The review's own `viewdemo`: not the user's, so the lock lets it run.
    crate::review_lock::let_through(line);
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

/// The Review tab's two buttons that start a review of the demo playing
/// (#1142), in `ui/Review.res`.
pub(crate) const START_BUTTONS: [&str; 2] = ["FromStartButton", "FromHereButton"];

/// What decides whether a review can start from the game, and what the idle
/// tab says.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct IdleFacts {
    /// DoD Studio serves its game pipe.
    pub studio: bool,
    /// A demo is loaded and readable.
    pub demo: bool,
    pub ask: crate::review_ask::Status,
}

impl IdleFacts {
    /// As they stand now. Only for the tab: it looks up the pipe at most once
    /// a second.
    pub(crate) fn now() -> IdleFacts {
        IdleFacts {
            studio: crate::review_ask::studio_running(),
            demo: loaded_demo().is_some(),
            ask: crate::review_ask::status(),
        }
    }

    /// Whether From the start and From here may be pressed.
    pub(crate) fn can_ask(&self) -> bool {
        use crate::review_ask::Status;
        self.studio && self.demo && matches!(self.ask, Status::Idle | Status::Failed(_))
    }

    /// The tab's second line with no review running.
    pub(crate) fn detail(&self) -> String {
        use crate::review_ask::Status;
        match &self.ask {
            Status::Asking(text) => return text.clone(),
            Status::Sent(_) => return "DoD Studio sent the review: it starts in a moment.".into(),
            Status::Failed(why) if self.studio => {
                return format!("DoD Studio couldn't start it: {why}");
            }
            Status::Failed(_) | Status::Idle => {}
        }
        // One line that fits the default window's 520-pixel label (about 85
        // characters); the buttons' tooltips say the rest.
        if !self.studio {
            "Start DoD Studio to review the demo playing from here.".to_string()
        } else if !self.demo {
            "Play a demo to review it from here, or use Review highlights in DoD Studio."
                .to_string()
        } else {
            "Review the demo playing: From the start, or From here (the next highlight from now on)."
                .to_string()
        }
    }
}

/// The loaded demo's name and the demo player's clock now, while a demo is
/// in the player and read.
fn loaded_demo() -> Option<(String, f64)> {
    let clock = crate::demo_seek::clock().filter(|c| c.active && !c.loading)?;
    crate::demo_reload::current_demo().map(|name| (name, clock.now))
}

/// The demo player as an ask sees it.
fn demo_now() -> crate::review_ask::DemoNow {
    use crate::review_ask::DemoNow;
    match crate::demo_seek::clock() {
        Some(c) if c.loading => DemoNow::Loading,
        Some(c) if c.active => DemoNow::Playing,
        _ => DemoNow::Absent,
    }
}

/// `ask start` / `ask here`: asks DoD Studio to review the demo playing, or
/// once the demo loading now plays.
fn ask(what: &str) {
    use crate::review_ask::AskFrom;
    if lock().is_some() {
        say("a review is running already");
        return;
    }
    let from = match what {
        "start" => AskFrom::Start,
        "here" => AskFrom::Here,
        _ => {
            say(&format!("usage: {NAME} ask start | ask here"));
            return;
        }
    };
    if demo_now() == crate::review_ask::DemoNow::Loading {
        crate::review_ask::wait_for_demo(from);
        say("the demo is still loading: DoD Studio is asked once it plays");
        return;
    }
    send_ask(from);
}

/// An ask waiting for its demo to load, every frame: sent once the demo
/// plays, dropped when a review starts meanwhile or the demo goes.
fn poll_waiting_ask() {
    use crate::review_ask::WaitStep;
    let Some((from, waited)) = crate::review_ask::waiting() else {
        return;
    };
    if lock().is_some() {
        crate::review_ask::stop_waiting(None);
        trace("ask: a review started while the demo loaded; not asking");
        return;
    }
    match crate::review_ask::wait_step(demo_now(), waited) {
        WaitStep::Wait => {}
        WaitStep::Ask => {
            crate::review_ask::stop_waiting(None);
            send_ask(from);
        }
        WaitStep::GiveUp(why) => {
            say(&format!("not asking DoD Studio: {why}"));
            crate::review_ask::stop_waiting(Some(why));
        }
    }
}

/// Sends the ask for the demo playing now.
fn send_ask(from: crate::review_ask::AskFrom) {
    let Some((name, now)) = loaded_demo() else {
        say("no demo is playing: play one with viewdemo first");
        return;
    };
    let start = match from {
        crate::review_ask::AskFrom::Start => crate::review_ask::Start::First,
        crate::review_ask::AskFrom::Here => crate::review_ask::Start::At(now),
    };
    let demo = crate::review_ask::demo_path(&crate::texture_hires::game_dir(), &name);
    match crate::review_ask::send(demo.clone(), start) {
        Ok(()) => say(&format!("asked DoD Studio to review {}", demo.display())),
        Err(why) => say(&why),
    }
}

/// Where a review starts: the first highlight whose first kill is at or
/// after `from`, or the first when `from` is none. `Err` with the first when
/// every highlight is before it.
fn start_index(queue: &[Highlight], from: Option<f64>) -> Result<usize, usize> {
    let Some(from) = from else { return Ok(0) };
    queue
        .iter()
        .position(|h| h.kills.first().is_some_and(|&t| t >= from))
        .ok_or(0)
}

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

/// What the kill range and note form shows for the highlight the review is
/// on (#1139), or `None` with no review running.
pub(crate) fn form_view() -> Option<crate::review_form::FormView> {
    let guard = lock();
    let review = guard.as_ref()?;
    let h = review.queue.get(review.at)?;
    Some(crate::review_form::FormView {
        key: h.key.clone(),
        title: panel_title(&review.queue, review.at),
        from: h.from,
        to: h.to,
        kills: h.kills.len(),
        note: h.note.clone(),
    })
}

/// Puts what the form saved on its highlight and sends it to Studio, which
/// sets the row's kill range and note and leaves its mark (#1139). The answer
/// after it carries the same values. A save for a highlight the review has
/// left is dropped: the form closes when the highlight changes, so only a
/// save racing that change gets here.
fn apply_saved(saved: crate::review_form::Saved) {
    let mut guard = lock();
    let Some(review) = guard.as_mut() else {
        trace("form: saved with no review running, dropped");
        return;
    };
    let at = review.at;
    let h = &mut review.queue[at];
    if h.key != saved.key {
        trace("form: saved for a highlight the review has left, dropped");
        return;
    }
    h.from = saved.from.clamp(1, h.kills.len());
    h.to = saved.to.clamp(h.from, h.kills.len());
    h.note = clean_note(&saved.note);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    crate::events::send(&edit_label(h));
    say(&format!(
        "highlight {}: kills {}-{} and the note saved",
        at + 1,
        h.from,
        h.to
    ));
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
        Phase::Waiting => "Yes, Maybe or No?".to_string(),
        Phase::Failed(why) => format!("can't play this one: {why}"),
    };
    format!("{who}{kills}{answered} -- {what}")
}

/// Starts playing the current highlight from its start.
fn play(review: &mut Review) {
    let h = &review.queue[review.at];
    let (from, until) = h.window();
    let spans = fast_spans(&h.kills, review.options.gap);
    let started = crate::demo_seek::seek_to_seconds_then(from, h.camera_line()).and_then(|seek| {
        crate::demo_seek::set_time_scale(1.0)?;
        crate::demo_seek::set_paused(false)?;
        Ok(seek)
    });
    match started {
        Ok(seek) => {
            let now = crate::demo_seek::clock().map_or(-1.0, |c| c.now);
            trace(&format!(
                "highlight {} of {}: playing {from:.1}..{until:.1} s, clock {now:.1} after the seek ({seek}){}",
                review.at + 1,
                review.queue.len(),
                describe_spans(&spans, review.options.speed)
            ));
            review.phase = Phase::Playing {
                until,
                last: now,
                beat: Instant::now(),
                spans,
                fast: false,
            };
            review.keys_from = Instant::now() + LOCKOUT;
            // Out of the way while it plays: the window and the menu.
            crate::studio_panel::close_for_playback();
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
        // Answered mid-highlight (#1141): the demo plays on at normal speed.
        if matches!(review.phase, Phase::Playing { fast: true, .. }) {
            let _ = crate::demo_seek::set_time_scale(1.0);
        }
        crate::events::send("REVIEW_END\tdone");
        *guard = None;
        crate::spectator_follow::hold_for_review(false);
        GENERATION.fetch_add(1, Ordering::Relaxed);
        open_tab();
    }
}

/// Runs every frame: an answer key pressed since the last frame, the
/// review's own step, then what the on-screen panel shows (#660).
pub fn poll() {
    if let Some(action) = crate::review_keys::take_pending() {
        key_action(action);
    }
    if let Some(saved) = crate::review_form::take_saved() {
        apply_saved(saved);
    }
    poll_waiting_ask();
    let mut guard = lock();
    step(&mut guard);
    let (panel, mode) = match guard.as_ref() {
        Some(review) => panel_view(
            review,
            Instant::now(),
            crate::demo_seek::clock()
                .filter(|c| c.active)
                .map(|c| c.now),
        ),
        None => (None, KeyMode::Off),
    };
    drop(guard);
    crate::review_keys::publish(panel, mode);
    watch_menu(mode);
}

/// "Highlight 3 of 12 | m00cat | 4 kills".
fn panel_title(queue: &[Highlight], at: usize) -> String {
    let h = &queue[at];
    let mut parts = vec![format!("Highlight {} of {}", at + 1, queue.len())];
    if !h.player.is_empty() {
        parts.push(h.player.clone());
    }
    parts.push(match h.kills.len() {
        1 => "1 kill".to_string(),
        n => format!("{n} kills"),
    });
    if let Some(v) = h.verdict {
        parts.push(format!("answered {}", v.word()));
    }
    parts.join(" | ")
}

/// What the answer keys do in the review as it stands at `now`, with
/// `ds_ui_review_keys` at `keys_on`: taken from the moment a highlight loads,
/// answering once [`LOCKOUT`] has passed since it started playing (#1141).
fn key_mode(review: &Review, keys_on: bool, now: Instant) -> KeyMode {
    if !keys_on {
        return KeyMode::Off;
    }
    match review.phase {
        Phase::Loading { .. } => KeyMode::Locked,
        Phase::Playing { .. } | Phase::Waiting if now < review.keys_from => KeyMode::Locked,
        Phase::Playing { .. } | Phase::Waiting => KeyMode::Live,
        // The Review tab is open for why this one failed.
        Phase::Failed(_) => KeyMode::Off,
    }
}

/// The keys while the kill range and note form shows (#1139): taken but
/// doing nothing, should one reach the game past the form's boxes, which
/// keep every key they're typed in.
fn under_form(mode: KeyMode, form_shown: bool) -> KeyMode {
    if form_shown && mode == KeyMode::Live {
        KeyMode::Locked
    } else {
        mode
    }
}

/// What the on-screen panel shows for the review as it stands at `now`,
/// with the demo player's clock at `clock`, and what the answer keys do.
fn panel_view(
    review: &Review,
    now: Instant,
    clock: Option<f64>,
) -> (Option<crate::review_keys::Panel>, KeyMode) {
    let keys = crate::review_keys::KEYS.on();
    let info = crate::review_keys::INFO.on();
    let Some(_) = review.queue.get(review.at) else {
        return (None, KeyMode::Off);
    };
    let mode = under_form(key_mode(review, keys, now), crate::review_form::is_shown());
    let title = panel_title(&review.queue, review.at);
    let panel = |status: &str| crate::review_keys::Panel {
        title: title.clone(),
        status: status.to_string(),
        keys,
        live: mode == KeyMode::Live,
        can_go_back: review.at > 0,
        kills: Vec::new(),
    };
    let h = &review.queue[review.at];
    let shown = match review.phase {
        // With the keys off, the Review tab is open for the answer.
        Phase::Waiting => keys.then(|| crate::review_keys::Panel {
            kills: kill_lines(h, KillClock::Ended),
            ..panel("")
        }),
        // The Review tab is open for why this one failed.
        Phase::Failed(_) => None,
        Phase::Loading { .. } => info.then(|| panel("Loading the demo...")),
        Phase::Playing { .. } => info.then(|| crate::review_keys::Panel {
            kills: kill_lines(h, clock.map_or(KillClock::Unknown, KillClock::At)),
            ..panel("")
        }),
    };
    (shown, mode)
}

/// Where the highlight is, for which of its kills is next.
#[derive(Debug, Clone, Copy, PartialEq)]
enum KillClock {
    /// The demo player's clock, while the highlight plays.
    At(f64),
    /// Played to its end.
    Ended,
    /// No clock to read.
    Unknown,
}

/// A time on the demo player's clock as its VCR bar shows it: `5:56`.
fn clock_text(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// The box's kill list (#1140): each kill's number, time, victim and weapon,
/// with the next one still to come stood out. A kill counts as come once
/// the clock reaches it.
fn kill_lines(h: &Highlight, clock: KillClock) -> Vec<crate::review_keys::KillLine> {
    use crate::review_keys::{KillLine, KillState};
    let next = match clock {
        KillClock::At(now) => h.kills.iter().position(|&t| t > now),
        KillClock::Ended | KillClock::Unknown => None,
    };
    h.kills
        .iter()
        .enumerate()
        .map(|(i, &t)| {
            let mut text = format!("{}  {}", i + 1, clock_text(t));
            if let Some(d) = h.details.get(i) {
                for part in [&d.victim, &d.weapon] {
                    if !part.is_empty() {
                        text.push_str("  ");
                        text.push_str(part);
                    }
                }
            }
            let state = match (clock, next) {
                (KillClock::Unknown, _) => KillState::Later,
                (_, Some(n)) if i == n => KillState::Next,
                (_, Some(n)) if i > n => KillState::Later,
                _ => KillState::Past,
            };
            KillLine { text, state }
        })
        .collect()
}

/// What an answer key asks for, if the keys still answer: the key may have
/// come in after the Review tab's buttons moved the review on.
fn key_action(action: crate::review_keys::Action) {
    use crate::review_keys::Action;
    let live = lock().as_ref().is_some_and(|r| {
        let mode = key_mode(r, crate::review_keys::KEYS.on(), Instant::now());
        under_form(mode, crate::review_form::is_shown()) == KeyMode::Live
    });
    if !live {
        return;
    }
    trace(&format!("key: {action:?}"));
    match action {
        Action::Form => crate::review_form::request_open(),
        Action::Yes => answer(Verdict::Yes),
        Action::Maybe => answer(Verdict::Maybe),
        Action::No => answer(Verdict::No),
        Action::Replay => replay(),
        Action::Back => back(),
        Action::Next => next(),
        Action::Stop => stop(),
    }
}

/// Whether the menu was up last frame, for [`menu_opens_tab`].
static MENU_WAS_UP: AtomicBool = AtomicBool::new(false);

/// Whether the menu just came up by itself, ESC's doing: up now and not a
/// frame ago, with neither our window nor the console window in it.
fn menu_opens_tab(was_up: bool, (menu, window, console): (bool, bool, bool)) -> bool {
    menu && !was_up && !window && !console
}

/// While the answer keys are the review's, ESC's menu comes up with the
/// Review tab in it, for the kill range and the note (#660): the box says
/// so whenever it lists the keys, while a highlight plays too (#1141).
fn watch_menu(mode: KeyMode) {
    let Some(state) = crate::studio_panel::menu_state() else {
        return;
    };
    let was_up = MENU_WAS_UP.swap(state.0, Ordering::Relaxed);
    // The form's Esc brings the menu up too; the form puts it away (#1139).
    if mode != KeyMode::Off
        && menu_opens_tab(was_up, state)
        && !crate::review_form::menu_is_from_esc()
    {
        trace("the menu came up during a highlight: opening the Review tab");
        open_tab();
    }
}

/// The review's step for this frame.
fn step(guard: &mut Option<Review>) {
    let Some(review) = guard.as_mut() else { return };
    // A stepped seek still landing: the clip hasn't started for the eye, so
    // neither has the lockout (#1141).
    if matches!(review.phase, Phase::Playing { .. }) && crate::demo_seek::seeking() {
        review.keys_from = Instant::now() + LOCKOUT;
    }
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
        Phase::Playing {
            until,
            last,
            beat,
            spans,
            fast,
        } => match crate::demo_seek::clock() {
            Some(c) if c.active && c.now < until => {
                // Into or out of a gap between kills (#665).
                let want_fast = spans.iter().any(|&(s, e)| c.now >= s && c.now < e);
                let fast = if want_fast != fast {
                    let speed = if want_fast { review.options.speed } else { 1.0 };
                    match crate::demo_seek::set_time_scale(speed) {
                        Ok(()) => {
                            trace(&format!(
                                "highlight {}: clock {:.1}, speed {speed}",
                                review.at + 1,
                                c.now
                            ));
                            want_fast
                        }
                        Err(why) => {
                            trace(&format!(
                                "highlight {}: speed {speed}: {why}",
                                review.at + 1
                            ));
                            fast
                        }
                    }
                } else {
                    fast
                };
                // A jump the review didn't make (#663's first ESC).
                if (c.now - last).abs() > JUMP_SECONDS {
                    trace(&format!(
                        "highlight {}: the clock jumped from {last:.1} to {:.1} s (playing until {until:.1})",
                        review.at + 1,
                        c.now
                    ));
                }
                // Where a long highlight is, so a stop that never comes shows.
                let beat = if beat.elapsed() >= HEARTBEAT {
                    trace(&format!(
                        "highlight {}: clock {:.1}, playing until {until:.1}{}",
                        review.at + 1,
                        c.now,
                        if c.paused { ", paused" } else { "" }
                    ));
                    Instant::now()
                } else {
                    beat
                };
                review.phase = Phase::Playing {
                    until,
                    last: c.now,
                    beat,
                    spans,
                    fast,
                };
            }
            Some(c) if c.active => {
                trace(&format!(
                    "highlight {}: clock {:.1} reached {until:.1} s, waiting for an answer",
                    review.at + 1,
                    c.now
                ));
                if fast {
                    let _ = crate::demo_seek::set_time_scale(1.0);
                }
                let _ = crate::demo_seek::set_paused(true);
                review.phase = Phase::Waiting;
                // With the answer keys on, the panel on screen asks instead
                // (#660); ESC still opens the tab.
                if !crate::review_keys::KEYS.on() {
                    open_tab();
                }
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

fn replay() {
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

fn back() {
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

/// The next highlight without answering this one.
fn next() {
    let mut guard = lock();
    if guard.is_none() {
        say("no review running");
    }
    advance(&mut guard);
}

/// Ends the review; every answer so far is already in Studio.
fn stop() {
    if let Some(review) = lock().take() {
        // Stopped inside a gap: the demo plays on at normal speed.
        if matches!(review.phase, Phase::Playing { fast: true, .. }) {
            let _ = crate::demo_seek::set_time_scale(1.0);
        }
        crate::events::send("REVIEW_END\tstopped");
        crate::spectator_follow::hold_for_review(false);
        GENERATION.fetch_add(1, Ordering::Relaxed);
        say("stopped");
    } else {
        say("no review running");
    }
}

const USAGE: &str = "start \"<queue file>\" | ask start | ask here | yes | maybe | no | replay | next | back | form | range <from> <to> | note <text> | stop";

/// `form`: the kill range and note form, as E opens it (#1139), for a bind
/// or a test that can't press keys.
fn open_form() {
    if lock().is_some() {
        crate::review_form::request_open();
    } else {
        say("no review running");
    }
}

/// `ds_demo_review <what>`.
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
        "ask" => ask(&words.next().unwrap_or("").to_ascii_lowercase()),
        "yes" => answer(Verdict::Yes),
        "maybe" => answer(Verdict::Maybe),
        "no" => answer(Verdict::No),
        "replay" => replay(),
        "next" => next(),
        "back" => back(),
        "form" => open_form(),
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
        "stop" => stop(),
        other => say(&format!("unknown \"{other}\" -- {NAME} {USAGE}")),
    }
}

fn start(path: &str) {
    if path.is_empty() {
        say(&format!("usage: {NAME} start \"<queue file>\""));
        return;
    }
    // A request from the Review tab ends here, started or not: a queue that
    // fails to read must not leave the tab waiting for it (#1142).
    crate::review_ask::review_started();
    let queue = match std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {path}: {e}"))
        .and_then(|text| parse_queue(&text))
    {
        Ok(parsed) => parsed,
        Err(why) => {
            say(&why);
            crate::events::send(&format!("REVIEW_END\tfailed: {}", clean_note(&why)));
            return;
        }
    };
    let (queue, options) = queue;
    let at = match start_index(&queue, options.from) {
        Ok(at) => at,
        Err(first) => {
            say("no highlight after where the demo was: starting from the first");
            first
        }
    };
    say(&format!("{} highlight(s)", queue.len()));
    crate::events::send(&format!("REVIEW_START\t{}", queue.len()));
    // Held for the whole review: it only matters on an HLTV highlight, and
    // it adds to the cvar rather than changing it (see the module docs).
    crate::spectator_follow::hold_for_review(true);
    let mut guard = lock();
    let review = guard.insert(Review {
        queue,
        options,
        at,
        phase: Phase::Waiting,
        loaded: None,
        keys_from: Instant::now(),
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
            spectate: None,
            details: Vec::new(),
        }
    }

    fn kill_detail(victim: &str, weapon: &str) -> KillDetail {
        KillDetail {
            victim: victim.to_string(),
            weapon: weapon.to_string(),
        }
    }

    #[test]
    fn a_queue_line_carries_each_kills_victim_and_weapon() {
        let line = format!(
            "p.dem\t1\t\t5,9\t1\t2\t-\t\t-\tgskiLL{PART_SEP}K98{KILL_SEP}{{$T}} Brain{PART_SEP}Colt"
        );
        let (queue, _) = parse_queue(&queue_text(&[&line])).unwrap();
        assert_eq!(
            queue[0].details,
            vec![
                kill_detail("gskiLL", "K98"),
                kill_detail("{$T} Brain", "Colt")
            ]
        );
        assert_eq!(queue[0].spectate, None);
    }

    #[test]
    fn details_that_are_not_one_per_kill_are_dropped() {
        assert_eq!(parse_details(Some("a"), 2), vec![]);
        assert_eq!(parse_details(None, 1), vec![]);
        assert_eq!(parse_details(Some(" "), 1), vec![]);
        // A kill with no weapon part keeps its victim.
        assert_eq!(parse_details(Some("a"), 1), vec![kill_detail("a", "")]);
    }

    #[test]
    fn a_kill_time_reads_as_the_vcr_bar_shows_it() {
        assert_eq!(clock_text(356.7), "5:56");
        assert_eq!(clock_text(5.0), "0:05");
        assert_eq!(clock_text(-1.0), "0:00");
    }

    #[test]
    fn the_kill_list_stands_out_the_next_kill() {
        use crate::review_keys::KillState::{Later, Next, Past};
        let mut h = highlight("a", None);
        h.kills = vec![356.0, 363.0, 370.0];
        h.details = vec![
            kill_detail("gskiLL", "K98"),
            kill_detail("Brain", ""),
            kill_detail("", ""),
        ];
        let states = |clock| {
            kill_lines(&h, clock)
                .into_iter()
                .map(|l| l.state)
                .collect::<Vec<_>>()
        };
        assert_eq!(states(KillClock::At(352.0)), vec![Next, Later, Later]);
        // A kill counts as come once the clock reaches it.
        assert_eq!(states(KillClock::At(356.0)), vec![Past, Next, Later]);
        assert_eq!(states(KillClock::At(371.0)), vec![Past, Past, Past]);
        assert_eq!(states(KillClock::Ended), vec![Past, Past, Past]);
        assert_eq!(states(KillClock::Unknown), vec![Later, Later, Later]);
        let texts: Vec<String> = kill_lines(&h, KillClock::Ended)
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(
            texts,
            vec!["1  5:56  gskiLL  K98", "2  6:03  Brain", "3  6:10"]
        );
    }

    #[test]
    fn the_box_lists_the_kills_while_playing_and_at_the_end() {
        use crate::review_keys::KillState::{Next, Past};
        let mut review = review_of(vec![highlight("a", None)], playing());
        let now = review.keys_from;
        let kills = |r: &Review, clock| panel_view(r, now, clock).0.expect("a panel").kills;
        assert_eq!(kills(&review, Some(8.0))[0].state, Next);
        review.phase = Phase::Waiting;
        assert_eq!(kills(&review, Some(8.0))[0].state, Past);
        review.phase = Phase::Loading {
            since: Instant::now(),
            frames: 0,
            seen_loading: false,
        };
        assert!(kills(&review, None).is_empty());
    }

    #[test]
    fn an_hltv_highlight_names_the_player_the_camera_goes_on() {
        let (queue, _) = parse_queue(&queue_text(&[
            "h.dem\t1\tgskiLL\t5\t1\t1\t-\t\t7",
            "h.dem\t2\tp\t9\t1\t1\t-\tnote\t-",
            "p.dem\t3\tp\t9\t1\t1\t-\tnote",
            "h.dem\t4\tp\t9\t1\t1\t-\t\t40",
        ]))
        .unwrap();
        assert_eq!(queue[0].spectate, Some(7));
        assert_eq!(
            queue[0].camera_line().as_deref(),
            Some("spec_mode 4; ds_spec_target 7")
        );
        // A POV demo's line has no number, or `-`; a number out of range is none.
        assert_eq!(queue[1].spectate, None);
        assert_eq!(queue[2].spectate, None);
        assert_eq!(queue[3].spectate, None);
        assert_eq!(queue[2].camera_line(), None);
    }

    #[test]
    fn a_queue_reads_every_field() {
        let text = queue_text(&[
            "C:/demos/a.dem\t3\tm00cat\t100.5,104,110.25\t2\t3\t-\tnice flick",
            "C:/demos/a.dem\t7\tm00cat\t200\t1\t1\tyes\t",
        ]);
        let (queue, options) = parse_queue(&text).unwrap();
        assert_eq!(options, Options::default());
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
        let (queue, _) = parse_queue(&queue_text(&["d.dem\t1\tp\t5\t1\t1\tno"])).unwrap();
        assert_eq!(queue[0].note, "");
        assert_eq!(queue[0].verdict, Some(Verdict::No));
    }

    #[test]
    fn a_range_outside_the_kills_is_pulled_in() {
        let (queue, _) = parse_queue(&queue_text(&["d.dem\t1\tp\t5,6\t0\t9\t-\t"])).unwrap();
        assert_eq!((queue[0].from, queue[0].to), (1, 2));
    }

    #[test]
    fn the_header_carries_the_fast_forward_options() {
        let text = format!("{QUEUE_HEADER}\tgap=8\tspeed=6\tnew=1\nd.dem\t1\tp\t5\t1\t1\t-\t");
        let (_, options) = parse_queue(&text).unwrap();
        assert_eq!(
            options,
            Options {
                gap: 8.0,
                speed: 6.0,
                from: None,
            }
        );
    }

    #[test]
    fn from_here_on_the_header_starts_at_the_first_highlight_after_it() {
        let text = format!("{QUEUE_HEADER}\tfrom=400.5\nd.dem\t1\tp\t5\t1\t1\t-\t");
        let (_, options) = parse_queue(&text).unwrap();
        assert_eq!(options.from, Some(400.5));
        assert_eq!(parse_options("from=-1\tfrom=x".split('\t')).from, None);
        let mut queue = vec![
            highlight("a", None),
            highlight("a", None),
            highlight("a", None),
        ];
        queue[0].kills = vec![356.0, 363.0];
        queue[1].kills = vec![402.9];
        queue[2].kills = vec![428.0];
        assert_eq!(start_index(&queue, None), Ok(0));
        // In the middle of the first highlight: the next one.
        assert_eq!(start_index(&queue, Some(360.0)), Ok(1));
        // At a first kill exactly: that one.
        assert_eq!(start_index(&queue, Some(428.0)), Ok(2));
        // Past every highlight: the first, said so.
        assert_eq!(start_index(&queue, Some(500.0)), Err(0));
    }

    #[test]
    fn the_start_buttons_need_studio_a_demo_and_no_request_on_its_way() {
        use crate::review_ask::Status;
        let facts = |studio, demo, ask| IdleFacts { studio, demo, ask };
        assert!(facts(true, true, Status::Idle).can_ask());
        assert!(facts(true, true, Status::Failed("x".into())).can_ask());
        assert!(!facts(false, true, Status::Idle).can_ask());
        assert!(!facts(true, false, Status::Idle).can_ask());
        assert!(!facts(true, true, Status::Asking("Scanning".into())).can_ask());
        assert!(!facts(true, true, Status::Sent(std::time::Instant::now())).can_ask());
    }

    #[test]
    fn the_idle_tab_says_why_it_cant_start_or_how_the_request_goes() {
        use crate::review_ask::Status;
        let facts = |studio, demo, ask| IdleFacts { studio, demo, ask };
        assert!(
            facts(false, true, Status::Idle)
                .detail()
                .starts_with("Start DoD Studio")
        );
        assert!(
            facts(true, false, Status::Idle)
                .detail()
                .starts_with("Play a demo")
        );
        assert!(
            facts(true, true, Status::Idle)
                .detail()
                .contains("From here")
        );
        assert_eq!(
            facts(true, true, Status::Asking("Scanning the demo...".into())).detail(),
            "Scanning the demo..."
        );
        assert_eq!(
            facts(true, true, Status::Failed("no highlights".into())).detail(),
            "DoD Studio couldn't start it: no highlights"
        );
        // A failure from before Studio closed gives way to why nothing works.
        assert!(
            facts(false, true, Status::Failed("x".into()))
                .detail()
                .starts_with("Start DoD Studio")
        );
        // Each fits the default window's line; a longer one was cut off.
        for studio in [false, true] {
            for demo in [false, true] {
                let line = facts(studio, demo, Status::Idle).detail();
                assert!(line.len() <= 88, "{line}");
            }
        }
    }

    #[test]
    fn the_start_buttons_are_in_the_review_tab_and_ask_studio() {
        let res = include_str!("../ui/Review.res");
        for (name, what) in START_BUTTONS.iter().zip(["start", "here"]) {
            let at = res
                .find(&format!("\"fieldName\"\t\t\"{name}\""))
                .expect(name);
            let block = &res[at..at + res[at..].find('}').unwrap()];
            assert!(
                block.contains(&format!("\"engine {NAME} ask {what}\"")),
                "{name}"
            );
        }
    }

    #[test]
    fn bad_fast_forward_options_fall_back_or_clamp() {
        let parse = |fields: &str| parse_options(fields.split('\t'));
        assert_eq!(parse("gap=x\tspeed=NaN"), Options::default());
        assert_eq!(
            parse("gap=-3\tspeed=99"),
            Options {
                gap: 0.0,
                speed: 16.0,
                from: None,
            }
        );
        assert_eq!(parse("gap=inf"), Options::default());
    }

    #[test]
    fn only_gaps_longer_than_the_setting_are_fast_forwarded() {
        // Kills at 217, 234, 247, 254: gaps of 17, 13 and 7 s.
        let kills = [217.0, 234.0, 247.0, 254.0];
        assert_eq!(
            fast_spans(&kills, 8.0),
            vec![(217.0 + TAIL, 234.0 - LEAD), (234.0 + TAIL, 247.0 - LEAD)]
        );
        assert_eq!(fast_spans(&kills, 0.0), vec![]);
        assert_eq!(fast_spans(&kills, 20.0), vec![]);
    }

    #[test]
    fn a_gap_too_short_for_its_margins_plays_at_normal_speed() {
        // 5 s apart, over a 3 s setting, but TAIL + LEAD is 6 s.
        assert_eq!(fast_spans(&[10.0, 15.0], 3.0), vec![]);
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
    fn maybe_is_read_and_sent_like_any_answer() {
        let (queue, _) = parse_queue(&queue_text(&[
            "d.dem\t1\tp\t5\t1\t1\tmaybe\t",
            "d.dem\t2\tp\t9\t1\t1\t-\t",
        ]))
        .unwrap();
        assert_eq!(queue[0].verdict, Some(Verdict::Maybe));
        assert_eq!(queue[1].verdict, None);
        assert_eq!(
            answer_label(&queue[0], Verdict::Maybe),
            "REVIEW\td.dem\t1\tmaybe\t1\t1\t"
        );
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
    }

    #[test]
    fn a_demo_on_another_drive_goes_by_its_full_path_when_the_hook_opens_it() {
        let dod = r"C:\Steam\common\Half-Life\dod";
        assert_eq!(
            viewdemo_line_with(dod, r"F:\WSOD 25\demos\m.dem", Ok(())).unwrap(),
            "viewdemo \"F:/WSOD 25/demos/m.dem\"\n"
        );
        assert!(
            viewdemo_line_with(dod, r"F:\Grüße\m.dem", Ok(()))
                .unwrap_err()
                .contains("can't open")
        );
        // A demo at the drive's root.
        assert_eq!(
            viewdemo_line_with(dod, "d:/m.dem", Ok(())).unwrap(),
            "viewdemo \"d:/m.dem\"\n"
        );
        let off = viewdemo_line_with(dod, r"F:\Downloads\m.dem", Err("it is off".into()));
        assert!(
            off.unwrap_err()
                .contains("another drive than the game, and it is off")
        );
        // No drive letter to open it by: a network path.
        assert!(
            viewdemo_line_with(dod, r"\\server\share\m.dem", Ok(()))
                .unwrap_err()
                .contains("another drive")
        );
        let long = format!(r"F:\{}\m.dem", "d".repeat(240));
        assert!(
            viewdemo_line_with(dod, &long, Ok(()))
                .unwrap_err()
                .contains("too long")
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
    fn the_panel_title_names_the_highlight_the_player_and_the_kills() {
        let mut queue = vec![highlight("a", None), highlight("a", None)];
        queue[1].player = "gorilla".to_string();
        queue[1].kills = vec![1.0, 2.0, 3.0, 4.0];
        assert_eq!(panel_title(&queue, 0), "Highlight 1 of 2 | 1 kill");
        assert_eq!(
            panel_title(&queue, 1),
            "Highlight 2 of 2 | gorilla | 4 kills"
        );
        queue[1].verdict = Some(Verdict::Maybe);
        assert_eq!(
            panel_title(&queue, 1),
            "Highlight 2 of 2 | gorilla | 4 kills | answered maybe"
        );
    }

    fn review_of(queue: Vec<Highlight>, phase: Phase) -> Review {
        Review {
            queue,
            options: Options::default(),
            at: 0,
            phase,
            loaded: Some("a".to_string()),
            keys_from: Instant::now(),
        }
    }

    fn playing() -> Phase {
        Phase::Playing {
            until: 20.0,
            last: 6.0,
            beat: Instant::now(),
            spans: Vec::new(),
            fast: false,
        }
    }

    #[test]
    fn the_keys_answer_while_a_highlight_plays_and_at_its_end() {
        // The settings' defaults (both on): no cvar in a host build.
        let mut review = review_of(vec![highlight("a", None)], Phase::Waiting);
        let now = review.keys_from;
        let (panel, mode) = panel_view(&review, now, None);
        assert_eq!(mode, KeyMode::Live);
        assert!(panel.is_some_and(|p| p.keys && p.live));
        review.phase = playing();
        let (panel, mode) = panel_view(&review, now, None);
        assert_eq!(mode, KeyMode::Live);
        assert!(panel.is_some_and(|p| p.keys && p.live && p.status.is_empty()));
    }

    #[test]
    fn the_keys_do_nothing_while_the_form_shows() {
        // Typing a Y in the note must not answer, should it reach the game.
        assert_eq!(under_form(KeyMode::Live, true), KeyMode::Locked);
        assert_eq!(under_form(KeyMode::Live, false), KeyMode::Live);
        // Off stays off: the form doesn't take keys the review doesn't.
        assert_eq!(under_form(KeyMode::Off, true), KeyMode::Off);
    }

    #[test]
    fn a_form_save_is_one_tab_separated_line_without_a_verdict() {
        let mut h = highlight("C:/d/a.dem", Some(Verdict::Yes));
        h.key = "4".to_string();
        h.kills = vec![1.0, 2.0, 3.0];
        (h.from, h.to) = (2, 3);
        h.note = "nice\tflick ".to_string();
        assert_eq!(
            edit_label(&h),
            "REVIEW_EDIT\tC:/d/a.dem\t4\t2\t3\tnice flick"
        );
    }

    #[test]
    fn the_keys_are_locked_while_loading_and_off_on_a_failed_highlight() {
        let mut review = review_of(
            vec![highlight("a", None)],
            Phase::Loading {
                since: Instant::now(),
                frames: 0,
                seen_loading: false,
            },
        );
        let now = review.keys_from;
        let (panel, mode) = panel_view(&review, now, None);
        assert_eq!(mode, KeyMode::Locked);
        let panel = panel.expect("a panel");
        assert_eq!(panel.status, "Loading the demo...");
        assert!(panel.keys && !panel.live);
        // A failed highlight has the Review tab open instead.
        review.phase = Phase::Failed("gone".into());
        assert_eq!(panel_view(&review, now, None), (None, KeyMode::Off));
    }

    #[test]
    fn the_keys_do_nothing_for_the_lockout_after_a_highlight_starts() {
        let mut review = review_of(vec![highlight("a", None)], playing());
        let started = Instant::now();
        review.keys_from = started + LOCKOUT;
        let (panel, mode) = panel_view(&review, started, None);
        assert_eq!(mode, KeyMode::Locked);
        assert!(panel.is_some_and(|p| p.keys && !p.live));
        let half = started + LOCKOUT / 2;
        assert_eq!(key_mode(&review, true, half), KeyMode::Locked);
        assert_eq!(key_mode(&review, true, started + LOCKOUT), KeyMode::Live);
    }

    #[test]
    fn with_the_keys_off_they_are_the_games_throughout() {
        let mut review = review_of(vec![highlight("a", None)], playing());
        let now = review.keys_from;
        assert_eq!(key_mode(&review, false, now), KeyMode::Off);
        review.phase = Phase::Waiting;
        assert_eq!(key_mode(&review, false, now), KeyMode::Off);
    }

    #[test]
    fn back_is_greyed_on_the_first_highlight_only() {
        use crate::review_keys::Action;
        let mut review = review_of(
            vec![highlight("a", None), highlight("a", None)],
            Phase::Waiting,
        );
        let now = review.keys_from;
        let first = panel_view(&review, now, None).0.expect("a panel");
        assert!(!first.key_live(Action::Back));
        assert!(first.key_live(Action::Next));
        review.at = 1;
        let second = panel_view(&review, now, None).0.expect("a panel");
        assert!(second.key_live(Action::Back));
    }

    #[test]
    fn only_a_menu_that_just_came_up_on_its_own_opens_the_review_tab() {
        // ESC: the menu up, nothing in it.
        assert!(menu_opens_tab(false, (true, false, false)));
        // Already up last frame, or brought up around our window or the
        // console window (the console key), or not up at all: left alone.
        assert!(!menu_opens_tab(true, (true, false, false)));
        assert!(!menu_opens_tab(false, (true, true, false)));
        assert!(!menu_opens_tab(false, (true, false, true)));
        assert!(!menu_opens_tab(false, (false, false, false)));
    }

    #[test]
    fn the_detail_says_what_to_do() {
        let mut h = highlight("a", None);
        h.player = "m00cat".to_string();
        h.kills = vec![1.0, 2.0];
        assert_eq!(
            detail(&h, &Phase::Waiting),
            "m00cat, 2 kills -- Yes, Maybe or No?"
        );
        h.verdict = Some(Verdict::No);
        assert_eq!(
            detail(&h, &Phase::Failed("gone".into())),
            "m00cat, 2 kills (answered no) -- can't play this one: gone"
        );
    }
}
