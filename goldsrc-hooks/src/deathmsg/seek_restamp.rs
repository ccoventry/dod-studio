//! Kill notices a stepped seek replays, re-dated to the demo clock at the
//! landing (#1392).
//!
//! `MsgFunc_DeathMsg` stamps a notice `gHUD.m_flTime + hud_deathnotice_time`,
//! and `m_flTime` is the client's own clock, which runs on in real time
//! however far a seek moves the demo's. A stepped seek (`demo_seek`) replays
//! the last notice lifetime before its target in well under a second of real
//! time, so every kill of those 10 s was stamped as if it had just happened
//! and stayed up a full lifetime past the landing. Measured on PRE (rewinds to
//! 653.59 on `ktps8w9-stealth_gskill_rr2_h1`, 1229.92 on
//! `ktps8w5-db_thunder_thunder2_h2`, 978.61 on `solifotw-pb2`): every notice
//! up at the landing had 9.97-9.99 s left, where playing in leaves kills
//! 0.05-7.5 s.
//!
//! So from the seek's kill-feed clear ([`begin`]) each arriving notice is
//! noted with its `m_flTime` and the stretch of demo time its step covered
//! ([`step_to`]), and at the landing ([`land`]) each one still up is stamped
//! as playing in would have: its kill's demo time (the middle of that
//! stretch) plus the lifetime, counted on from the landing. A kill whose time
//! has run out by then goes at once. The steps after the clear are short
//! (`demo_seek::rewind::FEED_STEP_SECONDS`), so the middle is never far off.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::clear;

/// One notice that arrived while a seek was recording: the HUD clock it was
/// stamped at, and the demo time its kill is taken to be at.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Arrival {
    hud: f32,
    demo: f64,
}

struct Recording {
    /// The demo time the last step reached, where the next one's stretch
    /// starts.
    reached: f64,
    /// The middle of the stretch the latest step covered.
    kill_at: f64,
    arrivals: Vec<Arrival>,
}

/// More than a notice lifetime of kills; past this, notices keep the game's
/// own stamps rather than the list growing without end.
const MAX_ARRIVALS: usize = 256;

/// How far a stamp may sit from an arrival's and still be that notice's:
/// the game stamps in `f32`, and two frames are at least a millisecond apart.
const MATCH_SECONDS: f32 = 0.0005;

static ON: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<Recording>> = Mutex::new(None);

/// Whether a seek is noting notices: `deathmsg::poll` keeps its handler
/// installed meanwhile.
pub(super) fn recording() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The seek has just emptied the feed with its clock at `clock`: note every
/// notice from here.
pub(super) fn begin(clock: f64) {
    if let Ok(mut state) = STATE.lock() {
        *state = Some(Recording {
            reached: clock,
            kill_at: clock,
            arrivals: Vec::new(),
        });
        ON.store(true, Ordering::Relaxed);
    }
}

/// The seek moves the clock on to `to`: notices from now came from that step.
// Only the 32-bit demo player steps; a host build has none.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn step_to(to: f64) {
    if !recording() {
        return;
    }
    if let Ok(mut state) = STATE.lock()
        && let Some(r) = state.as_mut()
    {
        r.kill_at = (r.reached + to) / 2.0;
        r.reached = to;
    }
}

/// The game has just added a notice: note it, if a seek is recording.
pub(super) fn on_arrival() {
    if !recording() {
        return;
    }
    let Some(hud) = clear::hud_time() else {
        return;
    };
    if let Ok(mut state) = STATE.lock()
        && let Some(r) = state.as_mut()
        && r.arrivals.len() < MAX_ARRIVALS
    {
        let demo = r.kill_at;
        r.arrivals.push(Arrival { hud, demo });
    }
}

/// A seek that stopped without landing: stop noting.
pub(crate) fn cancel() {
    ON.store(false, Ordering::Relaxed);
    if let Ok(mut state) = STATE.lock() {
        *state = None;
    }
}

/// The seek has landed at `target`: re-dates every notice it noted, and
/// stops noting. Returns how many were re-dated.
pub(crate) fn land(target: f64) -> usize {
    if !recording() {
        return 0;
    }
    let recorded = STATE.lock().ok().and_then(|mut s| s.take());
    ON.store(false, Ordering::Relaxed);
    let (Some(r), Some(lifetime), Some(hud)) =
        (recorded, super::notice_lifetime(), clear::hud_time())
    else {
        return 0;
    };
    clear::restamp_lines(|stamp| redated(stamp, lifetime, hud, target, &r.arrivals))
}

/// The stamp a notice stamped `stamp` gets at a landing at `target`, the HUD
/// clock at `hud`: its kill's demo time plus `lifetime`, counted from the
/// landing; `None` for a notice none of `arrivals` stamped. A stamp at or
/// before `hud` is one `Draw` takes out at once.
fn redated(stamp: f32, lifetime: f64, hud: f32, target: f64, arrivals: &[Arrival]) -> Option<f32> {
    let lifetime_f32 = lifetime as f32;
    let arrival = arrivals
        .iter()
        .map(|a| (a, (a.hud + lifetime_f32 - stamp).abs()))
        .filter(|&(_, off)| off <= MATCH_SECONDS)
        .min_by(|x, y| x.1.total_cmp(&y.1))?
        .0;
    let left = (arrival.demo + lifetime - target).min(lifetime);
    Some(hud + left as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The PRE thunder rewind to 1229.92: four kills (1219.97, 1220.44,
    /// 1221.96, 1227.42) all stamped within a frame of the landing.
    #[test]
    fn a_replayed_kill_keeps_only_what_playing_in_would_leave_of_it() {
        let hud = 25.717_f32;
        let arrivals = [
            Arrival {
                hud: 25.69,
                demo: 1220.17,
            },
            Arrival {
                hud: 25.70,
                demo: 1227.67,
            },
        ];
        let early = redated(35.69, 10.0, hud, 1229.92, &arrivals).unwrap();
        assert!((early - (hud + 0.25)).abs() < 1e-3, "{early}");
        let late = redated(35.70, 10.0, hud, 1229.92, &arrivals).unwrap();
        assert!((late - (hud + 7.75)).abs() < 1e-3, "{late}");
    }

    #[test]
    fn a_kill_whose_time_ran_out_before_the_landing_goes_at_once() {
        let arrivals = [Arrival {
            hud: 25.0,
            demo: 1218.0,
        }];
        let stamp = redated(35.0, 10.0, 25.7, 1229.92, &arrivals).unwrap();
        assert!(stamp <= 25.7);
    }

    /// A notice the seek did not replay (none of its arrivals stamped it)
    /// keeps the game's own stamp.
    #[test]
    fn a_notice_the_seek_did_not_note_is_left_alone() {
        let arrivals = [Arrival {
            hud: 25.0,
            demo: 1225.0,
        }];
        assert_eq!(redated(31.0, 10.0, 25.7, 1229.92, &arrivals), None);
        assert_eq!(redated(35.0, 10.0, 25.7, 1229.92, &[]), None);
    }

    /// Two notices a frame apart are told apart by their stamps.
    #[test]
    fn the_nearest_arrival_names_a_notice() {
        let arrivals = [
            Arrival {
                hud: 1000.000,
                demo: 500.0,
            },
            Arrival {
                hud: 1000.004,
                demo: 504.0,
            },
        ];
        let a = redated(1010.004, 10.0, 1000.01, 505.0, &arrivals).unwrap();
        assert!((a - (1000.01 + 9.0)).abs() < 1e-2, "{a}");
    }

    /// Never longer than a lifetime from the landing, even with a target
    /// before the stretch a kill was noted in (a seek that replaced another).
    #[test]
    fn no_notice_outlives_one_lifetime_from_the_landing() {
        let arrivals = [Arrival {
            hud: 20.0,
            demo: 1300.0,
        }];
        let stamp = redated(30.0, 10.0, 20.5, 1229.92, &arrivals).unwrap();
        assert!((stamp - 30.5).abs() < 1e-4, "{stamp}");
    }

    /// The stretch a notice is put in: the middle of its step.
    #[test]
    fn a_notice_is_put_in_the_middle_of_its_step() {
        begin(968.61);
        step_to(969.11);
        let kill_at = STATE.lock().unwrap().as_ref().unwrap().kill_at;
        assert!((kill_at - 968.86).abs() < 1e-9);
        cancel();
        assert!(!recording());
    }
}
