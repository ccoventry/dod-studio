//! The `viewdemo` catch (#1373): a damaged demo under `viewdemo` never
//! reaches `Host_EndGame` or `Host_Error`.
//!
//! `viewdemo` hands the file to `DemoPlayer`'s loader (`Core.dll`), which
//! reads it whole into its world before playback can reach the end. At the
//! first frame it can't read (`WARNING! DemoFile::ReadDemoPacket: ...`) or
//! message it can't parse (`WARNING! Server::ParseBad: ...`) it stops
//! reading and says it has loaded; the demo then plays to there and ends
//! like a healthy one (measured on both builds, #1394).
//!
//! So the catch compares what the player holds once it has loaded with what
//! the file holds, both on the server's clock (`svc_time`, the player's
//! world clock, #866):
//!
//! - a background thread walks every frame header of the file
//!   ([`crate::demo_file::walk`]), as the loading strip already does for its
//!   percent: the span the player should hold, and whether a frame no reader
//!   can make sense of cut the walk short;
//! - once the player's `IsLoading` clears, its buffered span
//!   (`GetEndTime - GetStartTime`) is held against that ([`verdict`]), once
//!   its start matches the file's first `svc_time`.
//!
//! Only a load that stopped where the walk found a bad frame is reported.
//! A player holding less of a file the walk read whole is logged only
//! ([`Verdict::Unconfirmed`]): no real damaged demo has ended that way yet,
//! and it is the one verdict a healthy demo could get.
//!
//! The verdict is dropped if another demo was started before it came in.
//!
//! No new engine address: the player's slots are `demo_seek`'s, checked on
//! both builds by `tools/verify_demo_seek_offsets.py`.
//!
//! It watches only from a named `viewdemo` until that demo has loaded, so
//! nothing runs per frame otherwise but one atomic load.

use lock_recover::lock_or_recover;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use super::{Cause, Damage};
use crate::demo_file::Walk;

/// How far the player's span may fall short of the file's, or run past it
/// on a bad frame, and still be the same place: the two clocks agree to a
/// frame on a healthy demo (#866), and a damaged demo stops minutes early.
const SAME_PLACE: f64 = 2.0;

/// How long a `viewdemo` may take to start loading before the watch gives
/// up (a demo that didn't open never does).
const START_WAIT: Duration = Duration::from_secs(30);

/// How long the walk may take after the player has loaded: it reads every
/// frame header, which a file on a slow drive can make long.
const WALK_WAIT: Duration = Duration::from_secs(120);

/// Set while a `viewdemo` is watched, so [`poll`] costs one load otherwise.
static ARMED: AtomicBool = AtomicBool::new(false);
static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

/// One `viewdemo`, from its command until its verdict.
struct Watch {
    /// The demo as it was played, relative to the game folder.
    name: String,
    started: Instant,
    walk: WalkState,
    /// The player has been seen loading this demo.
    seen_loading: bool,
    /// Once loaded: what the player holds.
    loaded: Option<Loaded>,
}

/// The player's world once it has loaded, on its clock, and when.
#[derive(Clone, Copy)]
struct Loaded {
    start: f64,
    span: f64,
    end: f64,
    at: Instant,
}

enum WalkState {
    Running(Receiver<Option<Walk>>),
    Done(Option<Walk>),
}

/// Starts watching the `viewdemo` of `name` that has just run, and walks its
/// file (at `path`) on a thread of its own. A later `viewdemo` replaces the
/// watch.
pub(super) fn started(name: String, path: std::path::PathBuf) {
    let (send, receive) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("demo-damage-walk".into())
        .spawn(move || {
            let _ = send.send(crate::demo_file::walk(&path));
        });
    if let Err(e) = spawned {
        unsafe {
            crate::debug::report(&format!(
                "demo_damage: viewdemo {name}: could not start reading the file ({e}), not watched"
            ))
        };
        return;
    }
    let mut watch = lock_or_recover(&WATCH);
    *watch = Some(Watch {
        name,
        started: Instant::now(),
        walk: WalkState::Running(receive),
        seen_loading: false,
        loaded: None,
    });
    ARMED.store(true, Ordering::Release);
}

/// The damaged-demo notice for the watched `viewdemo`, once its verdict is
/// in. Runs every frame from `demo_damage::poll`, on the engine's thread.
pub(super) fn poll() -> Option<Damage> {
    if !ARMED.load(Ordering::Acquire) {
        return None;
    }
    let mut slot = WATCH.try_lock().ok()?;
    let watch = slot.as_mut()?;
    match step(watch) {
        Step::Wait => None,
        Step::Done(damage) => {
            *slot = None;
            ARMED.store(false, Ordering::Release);
            damage
        }
    }
}

enum Step {
    Wait,
    /// The watch is over, with a notice or without.
    Done(Option<Damage>),
}

fn step(watch: &mut Watch) -> Step {
    if let WalkState::Running(receive) = &watch.walk {
        match receive.try_recv() {
            Ok(walk) => watch.walk = WalkState::Done(walk),
            Err(TryRecvError::Disconnected) => watch.walk = WalkState::Done(None),
            Err(TryRecvError::Empty) => {}
        }
    }
    let Some(loaded) = watch.loaded else {
        return watch_loading(watch);
    };
    let walk = match &watch.walk {
        WalkState::Done(walk) => *walk,
        WalkState::Running(_) if loaded.at.elapsed() < WALK_WAIT => return Step::Wait,
        WalkState::Running(_) => {
            log(&watch.name, "the file took too long to read, no verdict");
            return Step::Done(None);
        }
    };
    let Some(walk) = walk else {
        log(&watch.name, "the file's directory doesn't read, no verdict");
        return Step::Done(None);
    };
    // The demo may have changed while the walk ran (a playdemo, a timedemo,
    // another viewdemo's name): the verdict is only about the watched one.
    if crate::demo_reload::current_demo().as_deref() != Some(watch.name.as_str()) {
        log(
            &watch.name,
            "another demo was started before the verdict, none given",
        );
        return Step::Done(None);
    }
    let verdict = verdict(loaded.start, loaded.span, &walk);
    log(
        &watch.name,
        &format!(
            "loaded {:.2} s ({:.2} to {:.2}), the file {:?} from {:?}{}: {verdict:?}",
            loaded.span,
            loaded.start,
            loaded.end,
            walk.span,
            walk.first,
            if walk.bad_frame {
                " up to a bad frame"
            } else {
                ""
            },
        ),
    );
    let Verdict::Damaged(reason) = verdict else {
        return Step::Done(None);
    };
    Step::Done(Some(Damage {
        cause: Cause::DamagedAtLoad,
        reason,
        message: format!(
            "viewdemo loaded {:.2} s of the file's {:?}",
            loaded.span, walk.span
        ),
        demo: Some(watch.name.clone()),
        at: Some(loaded.end),
        clock: "demo player",
    }))
}

/// Watches the player until it has loaded the demo: it must be seen loading
/// first, since until then it may still report the last demo, loaded.
fn watch_loading(watch: &mut Watch) -> Step {
    let clock = crate::demo_seek::clock();
    match clock {
        Some(c) if c.active && c.loading => {
            watch.seen_loading = true;
            Step::Wait
        }
        Some(c) if c.active && watch.seen_loading => {
            watch.loaded = Some(Loaded {
                start: c.start,
                span: c.end - c.start,
                end: c.end,
                at: Instant::now(),
            });
            Step::Wait
        }
        _ if watch.seen_loading => {
            log(&watch.name, "the demo stopped before it had loaded");
            Step::Done(None)
        }
        _ if watch.started.elapsed() > START_WAIT => {
            log(&watch.name, "the demo player never started loading it");
            Step::Done(None)
        }
        _ => Step::Wait,
    }
}

/// What a load came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Damaged, for this reason: said everywhere.
    Damaged(&'static str),
    /// What would be said, in the hook log only: the `stopped_loading` case
    /// has not yet been seen on a real damaged demo, while it is the one way
    /// a healthy demo could be called damaged, so it waits for a survey.
    Unconfirmed(&'static str),
    NotDamaged,
    /// The player's world starts somewhere else than the file the walk read
    /// (another file of the same name, or a clock read wrong): no verdict.
    OtherStart,
}

/// What a load came to: `player_start` and `player_span` are what the player
/// holds once loaded, on its world clock, and `walk` what the file does.
///
/// Both must start at the same `svc_time` first, or the two are not the same
/// recording. Then:
/// - the walk stopped on a bad frame and the player stopped there too: the
///   loader read up to the same frame (`bad_frame`). If the player read well
///   past it, the walk misread the file, and nothing is said;
/// - otherwise the player holding less than the file means the loader
///   refused a message the frame walk doesn't parse (`stopped_loading`, only
///   logged for now: see [`Verdict::Unconfirmed`]).
pub(super) fn verdict(player_start: f64, player_span: f64, walk: &Walk) -> Verdict {
    let (Some(file_span), Some(first)) = (walk.span, walk.first) else {
        return Verdict::NotDamaged;
    };
    if !player_span.is_finite() || !player_start.is_finite() {
        return Verdict::NotDamaged;
    }
    if (player_start - f64::from(first)).abs() > SAME_PLACE {
        return Verdict::OtherStart;
    }
    let file_span = f64::from(file_span);
    if walk.bad_frame {
        if player_span <= file_span + SAME_PLACE {
            Verdict::Damaged("bad_frame")
        } else {
            Verdict::NotDamaged
        }
    } else if player_span + SAME_PLACE < file_span {
        Verdict::Unconfirmed("stopped_loading")
    } else {
        Verdict::NotDamaged
    }
}

fn log(name: &str, what: &str) {
    unsafe { crate::debug::report(&format!("demo_damage: viewdemo {name}: {what} (#1373)")) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(span: f32, bad_frame: bool) -> Walk {
        Walk {
            span: Some(span),
            first: Some(5.0),
            bad_frame,
        }
    }

    #[test]
    fn a_healthy_load_is_not_damaged() {
        // solifotw-bewton (#866): the player and the walk both hold 1952.1 s.
        assert_eq!(
            verdict(5.0, 1952.1, &walk(1952.1, false)),
            Verdict::NotDamaged
        );
        assert_eq!(
            verdict(5.0, 1951.0, &walk(1952.1, false)),
            Verdict::NotDamaged
        );
        // A whole-file length past a map restart is the walk's business: it
        // stops where the clock turns back, as the player does.
        assert_eq!(
            verdict(5.0, 1952.1, &walk(1952.6, false)),
            Verdict::NotDamaged
        );
    }

    #[test]
    fn a_bad_frame_where_the_player_stopped_is_damage() {
        // monday-wsod25_r07_m1_h1: both stop at the oversized message.
        assert_eq!(
            verdict(5.5, 507.7, &walk(507.7, true)),
            Verdict::Damaged("bad_frame")
        );
        assert_eq!(
            verdict(5.0, 500.0, &walk(507.7, true)),
            Verdict::Damaged("bad_frame")
        );
        // The player read well past the walk's bad frame: the walk misread.
        assert_eq!(verdict(5.0, 900.0, &walk(507.7, true)), Verdict::NotDamaged);
    }

    #[test]
    fn a_player_stopped_short_of_a_readable_file_is_only_logged() {
        assert_eq!(
            verdict(5.0, 397.0, &walk(1503.0, false)),
            Verdict::Unconfirmed("stopped_loading")
        );
    }

    #[test]
    fn a_player_starting_elsewhere_is_not_judged() {
        assert_eq!(
            verdict(400.0, 507.7, &walk(507.7, true)),
            Verdict::OtherStart
        );
        assert_eq!(
            verdict(400.0, 10.0, &walk(1503.0, false)),
            Verdict::OtherStart
        );
    }

    #[test]
    fn nothing_is_said_without_a_span() {
        let none = Walk {
            span: None,
            first: None,
            bad_frame: true,
        };
        assert_eq!(verdict(5.0, 10.0, &none), Verdict::NotDamaged);
        assert_eq!(
            verdict(5.0, f64::NAN, &walk(507.7, true)),
            Verdict::NotDamaged
        );
    }
}
