//! How many renders share the machine's threads, and how many FFmpeg threads
//! each render takes from that (#1512). The render scheduler counts the
//! share on every pass and hands each job a [`LiveThreadShare`] it keeps
//! current; a segmented render (#1451) reads it again as each segment starts,
//! so a long render takes the threads the renders that finished left free.
//! A one-piece render keeps the count it started with: it is one FFmpeg run.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::ffmpeg_args::threads_per_process;

/// Whether a capture is recording beside the renders (#861).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    /// A capture records: the game needs its share of the machine.
    Capturing,
    /// No capture, or one held between clips: nothing records.
    Idle,
}

impl Game {
    /// `Capturing` when a capture is `recording` now, else `Idle`.
    pub fn recording_if(recording: bool) -> Self {
        if recording {
            Self::Capturing
        } else {
            Self::Idle
        }
    }
}

/// What a thread share is counted from.
#[derive(Clone, Copy, Debug)]
pub struct ShareCount {
    /// The renders running now, the one asking included.
    pub running: usize,
    /// The batch's own "renders at once".
    pub batch_renders: usize,
    pub game: Game,
}

/// How many renders the machine's threads are shared between: the renders
/// running now, or, while a capture records, the batch's own "renders at
/// once", so each render takes the threads one of the batch's usual renders
/// would and the game keeps the rest. At least 1.
pub fn thread_share(count: ShareCount) -> usize {
    match count.game {
        Game::Capturing => count.batch_renders.max(1),
        Game::Idle => count.running.max(1),
    }
}

/// The thread share as the render scheduler last counted it, shared with
/// the jobs it started. The default is not linked to a scheduler: a job
/// given it keeps the share it started with (`RenderConfig::max_concurrent_renders`).
#[derive(Clone, Debug, Default)]
pub struct LiveThreadShare(Option<Arc<AtomicUsize>>);

impl LiveThreadShare {
    /// A share linked to whoever holds a clone of it, starting at `share`.
    pub fn new(share: usize) -> Self {
        Self(Some(Arc::new(AtomicUsize::new(share))))
    }

    /// Sets the share every holder reads next. Does nothing on the
    /// unlinked default.
    pub fn set(&self, share: usize) {
        if let Some(live) = &self.0 {
            live.store(share, Ordering::Relaxed);
        }
    }

    /// The share as last set, or `None` when unlinked.
    pub fn current(&self) -> Option<usize> {
        self.0.as_ref().map(|live| live.load(Ordering::Relaxed))
    }
}

/// The FFmpeg threads a segmented render's segments take, each counted as
/// it starts from the live share, and the changes on the way for one
/// activity-log line at the end.
pub(super) struct SegmentThreads {
    available: Option<usize>,
    starting_share: usize,
    live: LiveThreadShare,
    total_segments: usize,
    /// The threads the first counted segment took.
    first: Option<usize>,
    /// Each change after it: the segment (counted from 1) and its threads.
    changes: Vec<(usize, usize)>,
}

impl SegmentThreads {
    /// For a render of `total_segments` segments on a machine with
    /// `available` threads (`None` when it won't say), which started with
    /// `starting_share` and follows `live`.
    pub(super) fn new(
        available: Option<usize>,
        starting_share: usize,
        live: LiveThreadShare,
        total_segments: usize,
    ) -> Self {
        Self {
            available,
            starting_share,
            live,
            total_segments,
            first: None,
            changes: Vec::new(),
        }
    }

    /// The threads a segment starting now would take: from the live share,
    /// or the starting share when unlinked or set to 0.
    pub(super) fn threads_now(&self) -> usize {
        let share = self
            .live
            .current()
            .filter(|&share| share > 0)
            .unwrap_or(self.starting_share);
        threads_per_process(self.available, share)
    }

    /// The threads segment `index` (counted from 0) takes, counted now
    /// ([`Self::threads_now`]), and noted when it differs from the segment
    /// before.
    pub(super) fn for_segment(&mut self, index: usize) -> usize {
        let threads = self.threads_now();
        let previous = self.changes.last().map(|&(_, t)| t).or(self.first);
        match previous {
            None => self.first = Some(threads),
            Some(previous) if previous != threads => self.changes.push((index + 1, threads)),
            Some(_) => {}
        }
        threads
    }

    /// How the count went: the first segment's threads and each change
    /// after it (segment counted from 1, its threads), or `None` when every
    /// segment took the same.
    pub(super) fn changes(&self) -> Option<(usize, &[(usize, usize)])> {
        let first = self.first?;
        (!self.changes.is_empty()).then_some((first, self.changes.as_slice()))
    }

    /// The activity-log line for how the count changed, or `None` when it
    /// didn't.
    pub(super) fn changes_line(&self, job_id: &str) -> Option<String> {
        let (first, changes) = self.changes()?;
        Some(crate::messages::render::segment_threads_changed(
            job_id,
            first,
            changes,
            self.total_segments,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle(running: usize, batch_renders: usize) -> usize {
        thread_share(ShareCount {
            running,
            batch_renders,
            game: Game::Idle,
        })
    }

    fn capturing(running: usize, batch_renders: usize) -> usize {
        thread_share(ShareCount {
            running,
            batch_renders,
            game: Game::Capturing,
        })
    }

    #[test]
    fn a_lone_render_shares_with_no_one() {
        assert_eq!(idle(1, 3), 1);
        // Nothing counted yet still divides by one, never zero.
        assert_eq!(idle(0, 3), 1);
    }

    #[test]
    fn running_renders_share_the_threads_between_them() {
        assert_eq!(idle(3, 3), 3);
        assert_eq!(idle(2, 3), 2);
    }

    /// #861: beside a capture each render takes one usual render's share,
    /// however many are running.
    #[test]
    fn during_a_capture_each_render_takes_the_batchs_usual_share() {
        assert_eq!(capturing(1, 3), 3);
        assert_eq!(capturing(2, 4), 4);
        assert_eq!(capturing(1, 0), 1);
    }

    #[test]
    fn an_unlinked_share_reads_nothing_and_ignores_sets() {
        let unlinked = LiveThreadShare::default();
        unlinked.set(4);
        assert_eq!(unlinked.current(), None);
    }

    #[test]
    fn a_linked_share_is_read_by_every_clone() {
        let scheduler = LiveThreadShare::new(3);
        let job = scheduler.clone();
        assert_eq!(job.current(), Some(3));
        scheduler.set(1);
        assert_eq!(job.current(), Some(1));
    }

    /// The case #1512 is about: three renders at once on 16 threads, then
    /// the other two finish while the long one is at segment 11 of 30.
    #[test]
    fn a_segment_started_after_the_others_finished_takes_every_thread() {
        let scheduler = LiveThreadShare::new(3);
        let mut threads = SegmentThreads::new(Some(16), 3, scheduler.clone(), 30);
        assert_eq!(threads.threads_now(), 5);
        for index in 0..10 {
            assert_eq!(threads.for_segment(index), 5);
        }
        scheduler.set(idle(1, 3));
        assert_eq!(threads.for_segment(10), 16);
        assert_eq!(threads.for_segment(11), 16);
        assert_eq!(threads.changes(), Some((5, &[(11, 16)][..])));
        assert_eq!(
            threads.changes_line("4").as_deref(),
            Some("[render] job 4: FFmpeg threads went 5 → 16 at segment 11 of 30")
        );
    }

    /// While a capture runs the scheduler keeps the batch's share, so the
    /// segments keep theirs when the other renders finish.
    #[test]
    fn during_a_capture_a_segment_keeps_the_batch_share() {
        let scheduler = LiveThreadShare::new(capturing(3, 3));
        let mut threads = SegmentThreads::new(Some(16), 3, scheduler.clone(), 30);
        assert_eq!(threads.for_segment(0), 5);
        scheduler.set(capturing(1, 3));
        assert_eq!(threads.for_segment(1), 5);
        assert_eq!(threads.changes_line("4"), None);
    }

    #[test]
    fn each_change_is_noted_with_its_segment() {
        let scheduler = LiveThreadShare::new(3);
        let mut threads = SegmentThreads::new(Some(16), 3, scheduler.clone(), 30);
        threads.for_segment(0);
        scheduler.set(2);
        threads.for_segment(1);
        threads.for_segment(2);
        scheduler.set(1);
        threads.for_segment(3);
        assert_eq!(
            threads.changes_line("4").as_deref(),
            Some("[render] job 4: FFmpeg threads went 5 → 8 at segment 2, → 16 at segment 4 of 30")
        );
    }

    /// A job not started by the scheduler (a test, a tool) keeps the count
    /// its config started with.
    #[test]
    fn an_unlinked_render_keeps_its_starting_share() {
        let mut threads = SegmentThreads::new(Some(16), 2, LiveThreadShare::default(), 5);
        assert_eq!(threads.for_segment(0), 8);
        assert_eq!(threads.for_segment(1), 8);
        assert_eq!(threads.changes(), None);
    }

    #[test]
    fn a_resumed_render_counts_from_its_first_segment_rendered_now() {
        let scheduler = LiveThreadShare::new(1);
        let mut threads = SegmentThreads::new(Some(16), 2, scheduler, 30);
        // Segments 1-7 were on disk; the first rendered now is 8.
        assert_eq!(threads.for_segment(7), 16);
        assert_eq!(threads.changes(), None);
    }
}
