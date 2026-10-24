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
}

impl PendingTake {
    /// What deleting this take after its render frees on the drive, with
    /// `output_ratio` (clip bytes per take byte) standing in for a clip not
    /// written yet. `None` when that can't be known yet.
    fn frees(&self, output_ratio: Option<f64>) -> Option<u64> {
        if !self.deleted_after_render {
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
/// The planner's figure (`estimate_batch_capture_bytes`) is the uncompressed
/// size, an upper bound that is far above what a direct-to-video take
/// actually writes. Once a take has landed, its bytes per recorded frame give
/// a much closer figure; the largest seen, plus a margin, since footage
/// varies (smoke and fast motion compress worse). Never above the planner's.
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
