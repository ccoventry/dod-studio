//! Render while capturing (#861): when a running capture batch should stop at
//! a clip boundary so renders can catch up and their takes be deleted, and
//! when it may go on.
//!
//! A batch's raw takes normally all sit on disk until it ends. With "Render
//! while capturing" on, each take is rendered as soon as it is recorded and,
//! with "After a clip is finished" set to delete (#644), deleted once its
//! render verifies. Disk use then stays near the takes not yet rendered, as
//! long as renders keep up. When they don't, the batch holds between two
//! clips until enough space is back for the next one.
//!
//! Everything here is pure: the caller measures the drive and lists the
//! takes, this decides. Sizes are bytes on the drive the next clip records
//! to; takes and render output on other drives free or cost nothing there.

use std::time::{Duration, Instant};

/// One take this batch recorded that is still on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingTake {
    /// What the take holds on disk.
    pub raw_bytes: u64,
    /// Its clip file's size once its render finished, `None` before.
    pub output_bytes: Option<u64>,
    /// Whether the render writes its clip to the same drive as the take, so
    /// the clip eats into what deleting the take frees.
    pub output_on_same_drive: bool,
    /// Whether the take goes once its render verifies: "After a clip is
    /// finished" deletes it for good. Keep and the Recycle Bin free nothing.
    pub deleted_after_render: bool,
    /// Whether a render of it is still coming or done ([`render_coming`]).
    /// A take whose render failed, was cancelled or was never queued is never
    /// deleted from here, so it frees nothing: counting on it held a batch
    /// for space that never came (#1438).
    pub render_coming: bool,
}

impl PendingTake {
    /// What deleting this take after its render frees on the drive, with
    /// `output_ratio` (clip bytes per take byte) standing in for a clip not
    /// written yet. `None` when that can't be known yet.
    fn frees(&self, output_ratio: Option<f64>) -> Option<u64> {
        if !self.deleted_after_render || !self.render_coming {
            return Some(0);
        }
        if !self.output_on_same_drive {
            return Some(self.raw_bytes);
        }
        let output = match self.output_bytes {
            Some(bytes) => bytes,
            None => (self.raw_bytes as f64 * output_ratio?).ceil() as u64,
        };
        Some(self.raw_bytes.saturating_sub(output))
    }
}

/// How long after a take is handed to the Render tab a render of it may
/// still be missing from the list: the frontend queues it at once, retrying
/// for about 10 s while a scan the user started holds the queue.
pub const QUEUE_GRACE: Duration = Duration::from_secs(120);

/// How long a take whose renders all finished still counts on being
/// deleted: take cleanup deletes it at once, or, with "Only when space is
/// needed", the hold's own `make_room` does within a second or two. One
/// still there after this was declined (rendered from outside the
/// Destinations folders, a render file missing, no room plan that frees
/// enough), so it never goes from here (#1438).
pub const FINISHED_GRACE: Duration = Duration::from_secs(60);

/// What is known about one take's render, for [`render_coming`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TakeRenderState<'a> {
    /// The statuses of its rows in the Render tab.
    pub statuses: &'a [&'a str],
    /// How long ago it was handed to the Render tab; `None`: never.
    pub since_handover: Option<Duration>,
    /// How long ago its rows were first seen all finished; `None`: not yet,
    /// or just now.
    pub since_finished: Option<Duration>,
    /// Take cleanup is deleting it now. Its rows leave the list a moment
    /// before its files go (seconds, for a frame sequence).
    pub deleting: bool,
}

/// Whether a take's render will still free its space (#1438). Being deleted,
/// queued or rendering: coming. All finished: for [`FINISHED_GRACE`]. No row
/// past [`QUEUE_GRACE`] after the hand-over, or every row ended without all
/// finishing (an error, a cancel): nothing will ever delete it, until the
/// user retries it, which queues it again.
pub fn render_coming(take: &TakeRenderState) -> bool {
    if take.deleting {
        return true;
    }
    let statuses = take.statuses;
    if statuses.is_empty() {
        return take.since_handover.is_some_and(|d| d < QUEUE_GRACE);
    }
    if statuses.iter().any(|s| *s == "Queued" || *s == "Rendering") {
        return true;
    }
    statuses.iter().all(|s| *s == "Finished")
        && take.since_finished.is_none_or(|d| d < FINISHED_GRACE)
}

/// How long a held batch must stay unable to fit, with its drive's free
/// space not rising, before it stops: a take between losing its rows and
/// losing its files, or a render that errored, can make one look say "can
/// never fit" for a moment (#1438).
pub const STOP_DEBOUNCE: Duration = Duration::from_secs(20);

/// A rise in free space this large restarts [`STOP_DEBOUNCE`]: something is
/// still freeing the drive. Smaller ones are other programs' writes coming
/// and going.
pub const FREEING_BYTES: u64 = 256 * 1024 * 1024;

/// Whether a held batch that can't fit has stayed that way long enough to
/// stop.
#[derive(Debug, Clone, Default)]
pub struct StopDebounce {
    /// When the current run of "can't fit" began, and the free space then.
    since: Option<(Instant, u64)>,
}

impl StopDebounce {
    /// One look: `cannot_fit` from [`decide`], `free` the drive's free bytes.
    /// True once it has been unable to fit for [`STOP_DEBOUNCE`] with free
    /// space not rising by [`FREEING_BYTES`].
    pub fn should_stop(&mut self, cannot_fit: bool, free: u64, now: Instant) -> bool {
        if !cannot_fit {
            self.since = None;
            return false;
        }
        match self.since {
            Some((at, base)) if free <= base.saturating_add(FREEING_BYTES) => {
                now.saturating_duration_since(at) >= STOP_DEBOUNCE
            }
            _ => {
                self.since = Some((now, free));
                false
            }
        }
    }
}

/// Where the batch stands at a clip boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomInputs<'a> {
    /// Free space on the next clip's drive now.
    pub free_bytes: u64,
    /// What the capture leaves free on every drive it writes to
    /// (`MIN_DRIVE_HEADROOM_BYTES`).
    pub headroom_bytes: u64,
    /// What the clip being recorded now still has to write; 0 between clips.
    pub writing_bytes: u64,
    /// What the next clip is expected to need.
    pub next_clip_bytes: u64,
    /// The takes on that drive still on disk, recorded earlier in the batch.
    pub pending: &'a [PendingTake],
    /// Clip bytes per take byte, learned from this batch's finished renders.
    pub output_ratio: Option<f64>,
}

impl RoomInputs<'_> {
    /// What the drive must have free for the next clip to start.
    pub fn needed_bytes(&self) -> u64 {
        self.writing_bytes
            .saturating_add(self.next_clip_bytes)
            .saturating_add(self.headroom_bytes)
    }

    /// What the pending takes free once rendered and deleted, and whether any
    /// of them can't be priced yet (no render finished to learn from).
    fn reclaimable(&self) -> (u64, bool) {
        let mut total = 0u64;
        let mut unknown = false;
        for take in self.pending {
            match take.frees(self.output_ratio) {
                Some(bytes) => total = total.saturating_add(bytes),
                None => unknown = true,
            }
        }
        (total, unknown)
    }
}

/// What the batch does at the next clip boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum RoomDecision {
    /// The next clip fits: go on.
    Continue,
    /// Hold before the next clip until renders free `short_bytes`;
    /// `reclaimable_bytes` is what the pending takes are expected to free.
    Hold {
        short_bytes: u64,
        reclaimable_bytes: u64,
    },
    /// Even every pending take rendered and deleted leaves the next clip
    /// `short_bytes` short: holding would wait forever.
    CannotFit { short_bytes: u64 },
}

/// Whether the next clip fits, can be made to fit by waiting for renders, or
/// can't fit at all.
///
/// A take whose clip size can't be priced yet (nothing has rendered in this
/// batch to learn the ratio from, and the clip goes to the same drive) is
/// waited for rather than written off: its render finishing teaches the
/// ratio, and the next decision knows.
pub fn decide(inputs: &RoomInputs) -> RoomDecision {
    let needed = inputs.needed_bytes();
    if inputs.free_bytes >= needed {
        return RoomDecision::Continue;
    }
    let short_bytes = needed - inputs.free_bytes;
    let (reclaimable_bytes, unknown) = inputs.reclaimable();
    if reclaimable_bytes >= short_bytes || unknown {
        RoomDecision::Hold {
            short_bytes,
            reclaimable_bytes,
        }
    } else {
        RoomDecision::CannotFit { short_bytes }
    }
}

/// While held: whether the batch may go on. It needs the next clip's room,
/// plus, while renders can still free more, one more clip's worth, so it
/// doesn't stop again right after the next clip. With nothing left to free,
/// the next clip's room alone is enough.
pub fn may_resume(inputs: &RoomInputs) -> bool {
    let needed = inputs.needed_bytes();
    if inputs.free_bytes < needed {
        return false;
    }
    let (reclaimable, unknown) = inputs.reclaimable();
    if reclaimable == 0 && !unknown {
        return true;
    }
    inputs.free_bytes >= needed.saturating_add(inputs.next_clip_bytes)
}

/// About how long a hold lasts: the bytes still short over what renders free
/// per second. `None` before a render rate is known.
pub fn hold_seconds(short_bytes: u64, freed_bytes_per_sec: Option<f64>) -> Option<f64> {
    let rate = freed_bytes_per_sec.filter(|r| r.is_finite() && *r > 0.0)?;
    Some(short_bytes as f64 / rate)
}

/// What a clip is expected to need, learned from the takes this batch has
/// already recorded.
///
/// The planner's figure (`estimate_batch_capture_bytes`) is a fixed rate per
/// pixel: the uncompressed size for frame sequences, a lossless-video guess for
/// Video mode (#1103). Once a take has landed, its bytes per recorded frame
/// give a closer figure; the largest seen, plus a margin, since footage
/// varies (smoke and fast motion compress worse). Never above the planner's:
/// the planner placed each block against its own figure, so a larger one
/// could hold for room the drive never had. In Video mode that cap can trim
/// the margin (1.66 B/px seen x 1.5 is above #1103's 2 B/px);
/// `MIN_DRIVE_HEADROOM_BYTES` covers the difference.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipSizeModel {
    bytes_per_frame: Option<f64>,
}

/// Head-room over the largest take rate seen.
pub const CLIP_SIZE_MARGIN: f64 = 1.5;

impl ClipSizeModel {
    /// A take of `frames` frames wrote `bytes`.
    pub fn learn(&mut self, bytes: u64, frames: u64) {
        if bytes == 0 || frames == 0 {
            return;
        }
        let rate = bytes as f64 / frames as f64;
        self.bytes_per_frame = Some(self.bytes_per_frame.map_or(rate, |r| r.max(rate)));
    }

    /// The expected bytes of a clip of `frames` frames the planner priced at
    /// `planned_bytes`.
    pub fn estimate(&self, frames: u64, planned_bytes: u64) -> u64 {
        match self.bytes_per_frame {
            Some(rate) => {
                ((rate * CLIP_SIZE_MARGIN * frames as f64).ceil() as u64).min(planned_bytes)
            }
            None => planned_bytes,
        }
    }
}

/// Clip bytes per take byte, from the renders that finished in this batch:
/// the largest seen, so one small clip can't talk the estimate down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputRatio {
    ratio: Option<f64>,
}

impl OutputRatio {
    pub fn learn(&mut self, raw_bytes: u64, output_bytes: u64) {
        if raw_bytes == 0 {
            return;
        }
        let ratio = output_bytes as f64 / raw_bytes as f64;
        self.ratio = Some(self.ratio.map_or(ratio, |r| r.max(ratio)));
    }

    pub fn get(&self) -> Option<f64> {
        self.ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    fn take(raw: u64) -> PendingTake {
        PendingTake {
            raw_bytes: raw,
            output_bytes: None,
            output_on_same_drive: false,
            deleted_after_render: true,
            render_coming: true,
        }
    }

    fn inputs(free: u64, next: u64, pending: &[PendingTake]) -> RoomInputs<'_> {
        RoomInputs {
            free_bytes: free,
            headroom_bytes: 15 * GB,
            writing_bytes: 0,
            next_clip_bytes: next,
            pending,
            output_ratio: None,
        }
    }

    #[test]
    fn a_clip_that_fits_past_the_headroom_goes_on() {
        assert_eq!(
            decide(&inputs(20 * GB, 5 * GB, &[])),
            RoomDecision::Continue
        );
        assert_eq!(
            decide(&inputs(20 * GB - 1, 5 * GB, &[])),
            RoomDecision::CannotFit { short_bytes: 1 }
        );
    }

    #[test]
    fn the_clip_still_recording_counts_against_the_next() {
        let mut i = inputs(25 * GB, 5 * GB, &[]);
        i.writing_bytes = 6 * GB;
        assert_eq!(i.needed_bytes(), 26 * GB);
        assert_eq!(decide(&i), RoomDecision::CannotFit { short_bytes: GB });
    }

    #[test]
    fn it_holds_when_renders_can_free_enough() {
        let pending = [take(3 * GB), take(3 * GB)];
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &pending)),
            RoomDecision::Hold {
                short_bytes: 4 * GB,
                reclaimable_bytes: 6 * GB
            }
        );
    }

    #[test]
    fn it_cannot_fit_when_every_render_would_still_leave_it_short() {
        let pending = [take(GB)];
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &pending)),
            RoomDecision::CannotFit {
                short_bytes: 4 * GB
            }
        );
    }

    #[test]
    fn kept_or_recycled_takes_free_nothing() {
        let mut kept = take(10 * GB);
        kept.deleted_after_render = false;
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &[kept])),
            RoomDecision::CannotFit {
                short_bytes: 4 * GB
            }
        );
    }

    #[test]
    fn a_clip_written_to_the_same_drive_eats_into_what_its_take_frees() {
        let mut t = take(10 * GB);
        t.output_on_same_drive = true;
        t.output_bytes = Some(7 * GB);
        // Frees 3 GB, short 4 GB.
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &[t.clone()])),
            RoomDecision::CannotFit {
                short_bytes: 4 * GB
            }
        );
        t.output_bytes = Some(5 * GB);
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &[t])),
            RoomDecision::Hold {
                short_bytes: 4 * GB,
                reclaimable_bytes: 5 * GB
            }
        );
    }

    #[test]
    fn an_unrendered_clip_on_the_same_drive_is_priced_by_the_learned_ratio() {
        let mut t = take(10 * GB);
        t.output_on_same_drive = true;
        let pending = [t];
        let mut i = inputs(16 * GB, 5 * GB, &pending);
        i.output_ratio = Some(0.7);
        assert_eq!(
            decide(&i),
            RoomDecision::CannotFit {
                short_bytes: 4 * GB
            }
        );
        i.output_ratio = Some(0.5);
        assert_eq!(
            decide(&i),
            RoomDecision::Hold {
                short_bytes: 4 * GB,
                reclaimable_bytes: 5 * GB
            }
        );
    }

    #[test]
    fn before_any_render_finished_it_waits_to_learn_rather_than_giving_up() {
        let mut t = take(GB);
        t.output_on_same_drive = true;
        let pending = [t];
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &pending)),
            RoomDecision::Hold {
                short_bytes: 4 * GB,
                reclaimable_bytes: 0
            }
        );
    }

    /// #1438: a take whose render failed, was cancelled or never got queued
    /// frees nothing. Counted on, it held the batch for good; now the clip
    /// can't fit and the batch stops with the reason.
    #[test]
    fn a_take_no_render_will_delete_frees_nothing() {
        let mut stuck = take(10 * GB);
        stuck.render_coming = false;
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &[stuck.clone()])),
            RoomDecision::CannotFit {
                short_bytes: 4 * GB
            }
        );
        // Not even as a take whose clip size can't be priced yet: that one
        // is waited for only while its render can still teach the ratio.
        stuck.output_on_same_drive = true;
        assert_eq!(
            decide(&inputs(16 * GB, 5 * GB, &[stuck])),
            RoomDecision::CannotFit {
                short_bytes: 4 * GB
            }
        );
        // With nothing left that will free space, the next clip's room alone
        // lets the batch go on.
        let mut gone = take(10 * GB);
        gone.render_coming = false;
        assert!(may_resume(&inputs(20 * GB, 5 * GB, &[gone])));
    }

    fn coming(statuses: &[&str], since_handover: Option<Duration>) -> bool {
        render_coming(&TakeRenderState {
            statuses,
            since_handover,
            ..TakeRenderState::default()
        })
    }

    #[test]
    fn a_render_is_coming_while_queued_running_or_just_handed_over() {
        let secs = |s| Some(Duration::from_secs(s));
        assert!(coming(&["Queued"], None));
        assert!(coming(&["Finished", "Rendering"], None));
        assert!(coming(&["Error", "Queued"], None));
        // All finished, just seen: the take goes now.
        assert!(coming(&["Finished", "Finished"], None));
        // Not in the list yet: coming for a while after the hand-over.
        assert!(coming(&[], secs(5)));
        assert!(!coming(&[], Some(QUEUE_GRACE)));
        assert!(!coming(&[], None));
        // Every row ended and not all finished: nothing will delete it.
        assert!(!coming(&["Error"], None));
        assert!(!coming(&["Cancelled"], secs(5)));
        assert!(!coming(&["Finished", "Error"], None));
    }

    /// Take cleanup takes a rendered take's rows out of the list before it
    /// deletes its files: in that window it is still coming, however long
    /// ago it was handed over.
    #[test]
    fn a_take_being_deleted_is_still_coming_with_no_rows_left() {
        let deleting = TakeRenderState {
            statuses: &[],
            since_handover: Some(QUEUE_GRACE * 3),
            deleting: true,
            ..TakeRenderState::default()
        };
        assert!(render_coming(&deleting));
        assert!(!render_coming(&TakeRenderState {
            deleting: false,
            ..deleting
        }));
    }

    /// A take whose renders all finished but that cleanup declined to delete
    /// stops counting after [`FINISHED_GRACE`], instead of holding for good.
    #[test]
    fn a_finished_take_cleanup_never_deletes_stops_counting() {
        let finished = |since| TakeRenderState {
            statuses: &["Finished"],
            since_finished: Some(since),
            ..TakeRenderState::default()
        };
        assert!(render_coming(&finished(Duration::from_secs(5))));
        assert!(!render_coming(&finished(FINISHED_GRACE)));
    }

    #[test]
    fn a_held_batch_stops_only_after_it_stays_unable_to_fit() {
        let t0 = Instant::now();
        let at = |s| t0 + Duration::from_secs(s);
        let mut stop = StopDebounce::default();
        assert!(!stop.should_stop(true, 10 * GB, at(0)));
        assert!(!stop.should_stop(true, 10 * GB, at(19)));
        assert!(stop.should_stop(true, 10 * GB, at(20)));

        // A look that fits starts it over.
        let mut stop = StopDebounce::default();
        assert!(!stop.should_stop(true, 10 * GB, at(0)));
        assert!(!stop.should_stop(false, 10 * GB, at(10)));
        assert!(!stop.should_stop(true, 10 * GB, at(25)));
        assert!(stop.should_stop(true, 10 * GB, at(45)));

        // Free space rising (a take being deleted) starts it over; small
        // wobbles don't.
        let mut stop = StopDebounce::default();
        assert!(!stop.should_stop(true, 10 * GB, at(0)));
        assert!(!stop.should_stop(true, 12 * GB, at(15)));
        assert!(!stop.should_stop(true, 12 * GB, at(30)));
        assert!(!stop.should_stop(true, 12 * GB + FREEING_BYTES, at(34)));
        assert!(stop.should_stop(true, 12 * GB, at(35)));
    }

    #[test]
    fn it_resumes_with_a_clip_to_spare_while_renders_can_free_more() {
        let pending = [take(10 * GB)];
        // Room for the next clip only: wait for one more.
        assert!(!may_resume(&inputs(20 * GB, 5 * GB, &pending)));
        assert!(may_resume(&inputs(25 * GB, 5 * GB, &pending)));
        // Nothing left to free: the next clip's room is enough.
        assert!(may_resume(&inputs(20 * GB, 5 * GB, &[])));
        assert!(!may_resume(&inputs(19 * GB, 5 * GB, &[])));
    }

    #[test]
    fn a_hold_lasts_the_shortfall_over_the_rate_renders_free_space() {
        assert_eq!(hold_seconds(4 * GB, Some(GB as f64 / 2.0)), Some(8.0));
        assert_eq!(hold_seconds(4 * GB, None), None);
        assert_eq!(hold_seconds(4 * GB, Some(0.0)), None);
    }

    #[test]
    fn the_clip_size_learns_from_landed_takes_and_never_passes_the_planner() {
        let mut model = ClipSizeModel::default();
        assert_eq!(model.estimate(1000, 9 * GB), 9 * GB);
        model.learn(GB, 1000);
        model.learn(GB / 2, 1000);
        // The largest rate, 1 MB per frame, times the margin.
        assert_eq!(model.estimate(2000, 9 * GB), 3 * GB);
        assert_eq!(model.estimate(20_000, 9 * GB), 9 * GB);
        model.learn(0, 0);
        assert_eq!(model.estimate(2000, 9 * GB), 3 * GB);
    }

    #[test]
    fn the_output_ratio_keeps_the_largest_seen() {
        let mut ratio = OutputRatio::default();
        assert_eq!(ratio.get(), None);
        ratio.learn(10, 4);
        ratio.learn(10, 2);
        assert_eq!(ratio.get(), Some(0.4));
        ratio.learn(0, 5);
        assert_eq!(ratio.get(), Some(0.4));
    }
}
