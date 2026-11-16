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
//! stretch) plus the lifetime, counted on from the landing in the HUD's
//! seconds (the demo's divided by the player's time scale). A kill whose time
//! has run out by then goes at once. The steps after the clear are short
//! (`demo_seek::rewind::FEED_STEP_SECONDS`), so the middle is never far off.
//!
//! A notice is found again by its stamp, and `m_flTime` only moves while
//! `CHud::Redraw` runs, which it does not while the console or the game menu
//! is up (`restamp_display_time` in `deathmsg.rs`). A seek typed at the
//! console stamps every kill it replays alike, so equal stamps are told apart
//! by order: the game appends each notice after the last and drops the
//! oldest when full, so the last line with a stamp is the last arrival with
//! it ([`redate_all`]). For the same reason the landing counts from the
//! engine's client clock, which `m_flTime` catches up to on the next
//! `Redraw`, not from `m_flTime` itself.
//!
//! A plain forward stepped seek shorter than a notice lifetime keeps the
//! notices already up instead ([`adopt`]): playing in left them, so each is
//! noted with the demo time its kill had and is re-dated with the rest.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use super::clear;
use crate::engine;

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

/// Where and when a seek lands, for [`redate_all`].
#[derive(Clone, Copy, Debug)]
struct Landing {
    /// The demo time it lands at.
    target: f64,
    /// The engine's client clock at the landing, in the HUD's seconds.
    now: f32,
    lifetime: f64,
    /// Demo seconds per HUD second (`dem_speed`).
    scale: f64,
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
    start(clock, Vec::new());
}

/// A plain forward seek starts with its clock at `clock`, keeping the
/// notices up: note them, with the demo time each one's kill had, and every
/// notice from here.
pub(super) fn adopt(clock: f64) {
    let mut stamps = Vec::new();
    clear::for_each_line(|item| stamps.push(clear::stamp(item)));
    let (Some(lifetime), Some(now)) = (super::notice_lifetime(), engine::engine_client_time())
    else {
        // Nothing to date them by: they keep the game's stamps.
        start(clock, Vec::new());
        return;
    };
    start(clock, adopted(&stamps, now, lifetime, clock, time_scale()));
}

fn start(clock: f64, arrivals: Vec<Arrival>) {
    if let Ok(mut state) = STATE.lock() {
        *state = Some(Recording {
            reached: clock,
            kill_at: clock,
            arrivals,
        });
        ON.store(true, Ordering::Relaxed);
    }
}

/// The player's time scale, or 1 when it has none worth dividing by.
fn time_scale() -> f64 {
    crate::demo_seek::clock()
        .map(|c| f64::from(c.time_scale))
        .filter(|s| s.is_finite() && *s > 0.01)
        .unwrap_or(1.0)
}

/// The notices stamped `stamps` up at demo time `clock`, the client clock at
/// `now`, as arrivals: a notice with `r` HUD seconds left came from a kill
/// `(lifetime - r) * scale` demo seconds back. One already out of time (an
/// expired line `Draw` has not taken out yet) is left to go.
fn adopted(stamps: &[f32], now: f32, lifetime: f64, clock: f64, scale: f64) -> Vec<Arrival> {
    let lifetime_f32 = lifetime as f32;
    stamps
        .iter()
        .filter(|&&stamp| stamp > now)
        .map(|&stamp| Arrival {
            hud: stamp - lifetime_f32,
            demo: clock - (lifetime - f64::from(stamp - now)) * scale,
        })
        .collect()
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
    // What the game stamped it by, even while the console holds it still.
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
    let (Some(r), Some(lifetime), Some(now)) = (
        recorded,
        super::notice_lifetime(),
        engine::engine_client_time(),
    ) else {
        return 0;
    };
    let landing = Landing {
        target,
        now,
        lifetime,
        scale: time_scale(),
    };
    clear::restamp_lines(|stamps| redate_all(stamps, &r.arrivals, landing))
}

/// The stamps for lines stamped `stamps` (in the feed's order) at `landing`:
/// each line's kill demo time plus the lifetime, counted from the landing;
/// `None` for a line none of `arrivals` stamped. Matched from the newest
/// back, so the last line with a stamp is the last arrival with it. A stamp
/// at or before `landing.now` is one `Draw` takes out at once.
fn redate_all(stamps: &[f32], arrivals: &[Arrival], landing: Landing) -> Vec<Option<f32>> {
    let lifetime_f32 = landing.lifetime as f32;
    let mut used = vec![false; arrivals.len()];
    let mut out = vec![None; stamps.len()];
    for (line, &stamp) in stamps.iter().enumerate().rev() {
        let Some(i) = (0..arrivals.len())
            .rev()
            .find(|&i| !used[i] && (arrivals[i].hud + lifetime_f32 - stamp).abs() <= MATCH_SECONDS)
        else {
            continue;
        };
        used[i] = true;
        let since = (landing.target - arrivals[i].demo) / landing.scale;
        let left = (landing.lifetime - since).min(landing.lifetime);
        out[line] = Some(landing.now + left as f32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn landing(target: f64, now: f32) -> Landing {
        Landing {
            target,
            now,
            lifetime: 10.0,
            scale: 1.0,
        }
    }

    /// The PRE thunder rewind to 1229.92: kills at 1219.97-1221.96 and
    /// 1227.42, all stamped within a frame of the landing.
    #[test]
    fn a_replayed_kill_keeps_only_what_playing_in_would_leave_of_it() {
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
        let got = redate_all(&[35.69, 35.70], &arrivals, landing(1229.92, 25.717));
        assert!((got[0].unwrap() - (25.717 + 0.25)).abs() < 1e-3, "{got:?}");
        assert!((got[1].unwrap() - (25.717 + 7.75)).abs() < 1e-3, "{got:?}");
    }

    #[test]
    fn a_kill_whose_time_ran_out_before_the_landing_goes_at_once() {
        let arrivals = [Arrival {
            hud: 25.0,
            demo: 1218.0,
        }];
        let got = redate_all(&[35.0], &arrivals, landing(1229.92, 25.7));
        assert!(got[0].unwrap() <= 25.7);
    }

    /// A notice the seek did not replay (none of its arrivals stamped it)
    /// keeps the game's own stamp.
    #[test]
    fn a_notice_the_seek_did_not_note_is_left_alone() {
        let arrivals = [Arrival {
            hud: 25.0,
            demo: 1225.0,
        }];
        assert_eq!(
            redate_all(&[31.0], &arrivals, landing(1229.92, 25.7)),
            [None]
        );
        assert_eq!(redate_all(&[35.0], &[], landing(1229.92, 25.7)), [None]);
    }

    /// The review's case: a seek typed at the console, where `m_flTime`
    /// stands still, so every replayed kill has the same stamp. Each line
    /// keeps its own kill's time, in order, counted from the client clock
    /// (here 4 s on from the frozen HUD clock).
    #[test]
    fn equal_stamps_are_told_apart_by_order() {
        let frozen = 20.0_f32;
        let arrivals = [
            Arrival {
                hud: frozen,
                demo: 1220.17,
            },
            Arrival {
                hud: frozen,
                demo: 1222.17,
            },
            Arrival {
                hud: frozen,
                demo: 1227.67,
            },
        ];
        let stamps = [frozen + 10.0; 3];
        let got = redate_all(&stamps, &arrivals, landing(1229.92, 24.0));
        let left: Vec<f32> = got.iter().map(|s| s.unwrap() - 24.0).collect();
        for (l, want) in left.iter().zip([0.25, 2.25, 7.75]) {
            assert!((l - want).abs() < 1e-3, "{left:?}");
        }
        // The feed held only the newest two (the game dropped the oldest):
        // they are the last two arrivals.
        let got = redate_all(&stamps[..2], &arrivals, landing(1229.92, 24.0));
        assert!((got[0].unwrap() - 26.25).abs() < 1e-3, "{got:?}");
        assert!((got[1].unwrap() - 31.75).abs() < 1e-3, "{got:?}");
    }

    /// Never longer than a lifetime from the landing, even with a target
    /// before the stretch a kill was noted in (a seek that replaced another).
    #[test]
    fn no_notice_outlives_one_lifetime_from_the_landing() {
        let arrivals = [Arrival {
            hud: 20.0,
            demo: 1300.0,
        }];
        let got = redate_all(&[30.0], &arrivals, landing(1229.92, 20.5));
        assert!((got[0].unwrap() - 30.5).abs() < 1e-4, "{got:?}");
    }

    /// At half speed the HUD's 10 s cover 5 s of demo: a kill 2 demo seconds
    /// before the landing has 6 HUD seconds left, as playing in leaves it.
    #[test]
    fn at_another_speed_the_time_left_is_in_the_hud_s_seconds() {
        let arrivals = [Arrival {
            hud: 20.0,
            demo: 1227.92,
        }];
        let mut at = landing(1229.92, 21.0);
        at.scale = 0.5;
        let got = redate_all(&[30.0], &arrivals, at);
        assert!((got[0].unwrap() - 27.0).abs() < 1e-3, "{got:?}");
        at.scale = 2.0;
        let got = redate_all(&[30.0], &arrivals, at);
        assert!((got[0].unwrap() - 30.0).abs() < 1e-3, "{got:?}");
    }

    /// A short forward seek from 1000 to 1007 keeps a kill that has 8 s left
    /// at 1000 (kill at 998): at the landing it has 1 s left, as playing in.
    #[test]
    fn a_kept_notice_runs_on_by_the_demo_time_the_seek_skipped() {
        let arrivals = adopted(&[58.0], 50.0, 10.0, 1000.0, 1.0);
        assert!((arrivals[0].demo - 998.0).abs() < 1e-3, "{arrivals:?}");
        let got = redate_all(&[58.0], &arrivals, landing(1007.0, 50.4));
        assert!((got[0].unwrap() - 51.4).abs() < 1e-3, "{got:?}");
        // One with 2 s left is gone by the landing.
        let arrivals = adopted(&[52.0], 50.0, 10.0, 1000.0, 1.0);
        let got = redate_all(&[52.0], &arrivals, landing(1007.0, 50.4));
        assert!(got[0].unwrap() <= 50.4);
        // A line already expired is not taken on.
        assert!(adopted(&[-1.0e9, 49.0], 50.0, 10.0, 1000.0, 1.0).is_empty());
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
