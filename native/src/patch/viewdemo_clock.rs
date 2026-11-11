//! A demo's ticks placed on the `viewdemo` clock: the server time each kill's
//! `viewdemo_times` entry is in, which the hook DLL runs a batch on
//! (`runtime_batch`, #434) and so the clock a Seek Between Clips take is
//! recorded on.
//!
//! The scanner's `frame_times` only reach the end of a demo's loading
//! segment (a few dozen frames), so for a clip's ticks this is the only clock
//! that says how many seconds lie between them: a tick is placed by the
//! nearest kill of the same demo, that kill's `viewdemo` time plus the
//! playback time between the two frames (the demo's average frame rate where
//! `frame_times` don't reach). The take plan (#1084) reads its seconds here
//! too, so a plan says what the batch actually scheduled.

use std::path::Path;

use crate::patch::types::CaptureStreak;

/// One kill whose frame and `viewdemo` time are both known.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    tick: i32,
    viewdemo: f64,
}

/// How many of the nearest kills place a tick (see `Clock::seconds`).
const NEAREST_ANCHORS: usize = 7;
/// How far, in playback seconds, a kill may be from the tick to count.
const NEAR_SECONDS: f64 = 30.0;

/// Places ticks of one demo on the `viewdemo` clock.
pub(crate) struct Clock<'a> {
    anchors: Vec<Anchor>,
    frame_times: &'a [f32],
    fps: f64,
}

impl Clock<'_> {
    fn playback(&self, tick: i32) -> Option<f64> {
        usize::try_from(tick)
            .ok()
            .and_then(|t| self.frame_times.get(t))
            .map(|&t| f64::from(t))
    }

    /// Where `tick` falls on the `viewdemo` clock, in seconds.
    pub(crate) fn seconds(&self, tick: i32) -> f64 {
        // With frame times, the demo clock is playback time plus an offset,
        // and each kill gives one reading of it. One kill's reading is off by
        // up to a network frame (the kill's server time is its packet's), so
        // the median of the nearest few is used.
        if let Some(at) = self.playback(tick) {
            let mut near: Vec<(i64, f64)> = self
                .anchors
                .iter()
                .filter_map(|a| {
                    let played = self.playback(a.tick)?;
                    // Only kills close by: the offset can step at a pause.
                    ((played - at).abs() <= NEAR_SECONDS).then(|| {
                        let distance = (i64::from(a.tick) - i64::from(tick)).abs();
                        (distance, a.viewdemo - played)
                    })
                })
                .collect();
            if !near.is_empty() {
                near.sort_by_key(|&(distance, _)| distance);
                near.truncate(NEAREST_ANCHORS);
                let mut offsets: Vec<f64> = near.iter().map(|&(_, o)| o).collect();
                offsets.sort_by(f64::total_cmp);
                return (at + offsets[offsets.len() / 2]).max(0.0);
            }
        }
        let anchor = self
            .anchors
            .iter()
            .min_by_key(|a| (i64::from(a.tick) - i64::from(tick)).abs())
            .copied()
            .expect("a clock has anchors");
        let between = match (self.playback(tick), self.playback(anchor.tick)) {
            (Some(t), Some(a)) => t - a,
            _ => f64::from(tick - anchor.tick) / self.fps,
        };
        (anchor.viewdemo + between).max(0.0)
    }
}

/// The clock of `demo`, from the kills of its highlights in `streaks`.
/// `Err` when none of them has a `viewdemo` time (a scan older than them).
pub(crate) fn clock_for<'a>(demo: &str, streaks: &'a [CaptureStreak]) -> Result<Clock<'a>, String> {
    let mine: Vec<&CaptureStreak> = streaks.iter().filter(|s| s.source_demo == demo).collect();
    let anchors: Vec<Anchor> = mine
        .iter()
        .flat_map(|s| {
            s.kills
                .iter()
                .zip(&s.viewdemo_times)
                .filter(|(_, vd)| vd.is_finite() && **vd > 0.0)
                .map(|(kill, vd)| Anchor {
                    tick: kill.0,
                    viewdemo: f64::from(*vd),
                })
        })
        .collect();
    if anchors.is_empty() {
        return Err(format!(
            "{} has no demo-player times for its kills (scan it again)",
            Path::new(demo).display()
        ));
    }
    let frame_times = mine
        .iter()
        .map(|s| s.frame_times.as_slice())
        .max_by_key(|f| f.len())
        .unwrap_or(&[]);
    let fps = mine
        .iter()
        .map(|s| s.demo_fps)
        .find(|&f| f > 0.0)
        .unwrap_or(30.0);
    Ok(Clock {
        anchors,
        frame_times,
        fps: f64::from(fps),
    })
}
