//! Assigning capture blocks to output drives.

/// The free space every drive keeps beside a block routed to it. The Windows
/// drive keeps "Keep free on the Windows drive" when that is more (#1430):
/// [`plannable_free_bytes`] takes the difference off its budget.
pub(super) const FAILOVER_THRESHOLD: u64 = crate::sys::disk::MIN_DRIVE_HEADROOM_BYTES;

/// A capture directory's free bytes as the plan counts them: on the Windows
/// drive, less what "Keep free on the Windows drive" keeps back beyond
/// [`FAILOVER_THRESHOLD`] (#1430), so first-fit routes a block there only
/// while the drive keeps the setting free beside it. `None` when unknown.
/// Reads the setting now, at planning.
pub(super) fn plannable_free_bytes(dir: &std::path::Path) -> Option<u64> {
    crate::sys::disk::get_available_bytes(dir)
        .map(|free| crate::sys::windows_drive::free_for(dir, FAILOVER_THRESHOLD, free))
}

/// Logs each of `capture_directories` on the Windows drive, with its free
/// space and the `keep_free` it keeps there (#1430): one line per folder per
/// batch, so a batch that records nothing there says why.
pub(super) fn log_windows_drive_folders(
    capture_directories: &[std::path::PathBuf],
    keep_free: u64,
) {
    for dir in capture_directories {
        if crate::sys::windows_drive::is_on_windows_drive(dir) {
            let free = crate::sys::disk::get_available_bytes(dir).unwrap_or(0);
            crate::log_markdown(
                &crate::messages::windows_drive::capture_folder_on_windows_drive(
                    dir.display(),
                    free,
                    keep_free,
                ),
            );
        }
    }
}

/// How many of the batch's latest takes a Render While Capturing plan counts
/// as still on their drive when the next clip's room is checked (#1438): the
/// clip recording then (`capture_room_manager` checks the next clip's room as
/// each clip starts, counting the one recording) and the one before it,
/// still rendering. A drive is only first choice for a clip while it has
/// room beside those; two clips in a row on a drive that holds one at a time
/// held a batch until the first one rendered, behind every other
/// drive's queued renders.
pub(super) const TAKES_STILL_ON_DISK: usize = 2;

/// A batch's capture drives and the room left on their volumes as its blocks
/// are routed, demo by demo (the AOT disk-space simulation). Directories on
/// one volume share one budget (#774).
pub(super) struct DriveBudget {
    /// Each capture directory's volume, an index into `free` and `capacity`.
    volume_of_drive: Vec<usize>,
    /// What each volume has left after the blocks routed so far.
    free: Vec<u64>,
    /// What each volume had when the batch was planned: the most Render
    /// While Capturing (#861) can ever free there for one block.
    capacity: Vec<u64>,
    /// The drive the last block went to, where the next search starts.
    active_drive: usize,
    /// What "Keep free on the Windows drive" keeps there, when it raised a
    /// capture directory's headroom (#1430): the refusal says so.
    windows_drive_keep_free: Option<u64>,
    /// The latest blocks routed, in record order across the whole batch,
    /// newest last: their volume and estimate. At most
    /// [`TAKES_STILL_ON_DISK`].
    recent: std::collections::VecDeque<(usize, u64)>,
    /// What the Render While Capturing plan routed one at a time: how many
    /// blocks, and how many of them have room only once the takes before
    /// them on their drive have rendered (the batch may hold before those).
    pub one_at_a_time: OneAtATimeCounts,
    /// Per volume, the least room the one-at-a-time plan left beside a block
    /// there, across the whole batch: the figure the pre-launch check gets
    /// for that volume, in place of `free`, which stops counting once the
    /// batch goes one at a time.
    least_left: Vec<Option<u64>>,
}

/// [`DriveBudget::one_at_a_time`]'s counts, for the plan's log line.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct OneAtATimeCounts {
    pub blocks: usize,
    pub waits: usize,
}

impl DriveBudget {
    /// Snapshots the free bytes of every capture directory. With none
    /// configured, one sentinel drive with unlimited room stands in, so a
    /// single-drive batch plans without an error.
    ///
    /// `free_of` reads a directory's free bytes (`None`: unknown, an
    /// unbounded budget): [`crate::sys::disk::get_available_bytes`] for a real
    /// batch, a fixed size in tests that must not depend on the machine's.
    pub fn snapshot(
        capture_directories: &[std::path::PathBuf],
        free_of: impl FnMut(&std::path::Path) -> Option<u64>,
    ) -> Self {
        let (mut volume_of_drive, mut free) =
            crate::sys::disk::group_by_volume(capture_directories, free_of);
        if free.is_empty() {
            volume_of_drive.push(0);
            free.push(u64::MAX);
        }
        let capacity = free.clone();
        let least_left = vec![None; free.len()];
        DriveBudget {
            least_left,
            volume_of_drive,
            free,
            capacity,
            active_drive: 0,
            windows_drive_keep_free: None,
            recent: std::collections::VecDeque::new(),
            one_at_a_time: OneAtATimeCounts::default(),
        }
    }

    /// Notes routed blocks, `(block_index, drive_index)` in any order, as
    /// the batch's latest takes, in record (block index) order.
    fn note_recent(&mut self, pairs: &[(usize, usize)], estimates: &[u64]) {
        let mut in_order = pairs.to_vec();
        in_order.sort_unstable();
        for (block_index, drive) in in_order {
            push_recent(
                &mut self.recent,
                self.volume_of_drive[drive],
                estimates[block_index],
            );
        }
    }

    /// Notes what "Keep free on the Windows drive" keeps there, when it raised
    /// a capture directory's headroom (#1430), so a refusal explains why that
    /// folder counts less than its drive has free.
    pub fn note_windows_drive(&mut self, keep_free: Option<u64>) {
        self.windows_drive_keep_free = keep_free;
    }

    /// Routes one chain job's blocks, `estimates` bytes each, to drives:
    /// `(block_index, drive_index)` pairs in allocation order, from
    /// [`allocate_blocks_first_fit_decreasing`].
    ///
    /// When they don't all fit and `config.render_while_capturing` is on, they
    /// are routed [`allocate_blocks_one_at_a_time`] instead: renders free
    /// space as the batch runs and it holds between clips until there is
    /// room, so each block goes first to a drive with room beside the
    /// batch's latest takes still on it, and else to one it fits on its own
    /// once the earlier takes there have rendered. A block bigger than any
    /// drive could ever hold is still refused, rather than starting a batch
    /// that can't finish. From then on every later demo's blocks go one at a
    /// time too.
    ///
    /// Errors with [`no_room_message`] for the first block that fits nowhere.
    pub fn allocate(
        &mut self,
        estimates: &[u64],
        config: &crate::patch::types::PatcherConfig,
    ) -> std::io::Result<Vec<(usize, usize)>> {
        // Once one demo's blocks went one at a time, the rest of the batch
        // does too: `free` no longer says what the drives hold beside the
        // takes still on them, so a later demo that "fits at once" against
        // it would skip the latest takes and go uncounted (#1438).
        let at_once = if self.one_at_a_time.blocks == 0 {
            allocate_blocks_first_fit_decreasing(
                estimates,
                &self.volume_of_drive,
                &mut self.free,
                &mut self.active_drive,
                FAILOVER_THRESHOLD,
            )
        } else {
            Err(0)
        };
        match at_once {
            Ok(assignments) => {
                self.note_recent(&assignments, estimates);
                Ok(assignments)
            }
            Err(_) if config.render_while_capturing => {
                let plan = allocate_blocks_one_at_a_time(
                    estimates,
                    &self.volume_of_drive,
                    &self.capacity,
                    &mut self.recent,
                    &mut self.active_drive,
                    FAILOVER_THRESHOLD,
                )
                .map_err(|block_index| {
                    std::io::Error::other(no_room_message(
                        estimates[block_index] + FAILOVER_THRESHOLD,
                        self.capacity.iter().copied().max().unwrap_or(0),
                        self.windows_drive_keep_free,
                    ))
                })?;
                for (least, left) in self.least_left.iter_mut().zip(plan.left) {
                    if let Some(left) = left {
                        *least = Some(least.map_or(left, |l| l.min(left)));
                    }
                }
                self.one_at_a_time.blocks += plan.pairs.len();
                self.one_at_a_time.waits += plan.waits;
                Ok(plan.pairs)
            }
            Err(block_index) => Err(std::io::Error::other(no_room_message(
                estimates[block_index] + FAILOVER_THRESHOLD,
                self.free.iter().copied().max().unwrap_or(0),
                self.windows_drive_keep_free,
            ))),
        }
    }

    /// The room left beside this batch's blocks on each drive in `drives`
    /// that is a configured capture directory, handed back so the pre-launch
    /// abort in `capture_engine.rs` re-validates the exact numbers this
    /// allocation pass already computed instead of recomputing a third,
    /// narrower (primary-drive-only) answer. A drive planned one block at a
    /// time reports the least room the plan left beside a block there, the
    /// takes still on it counted ([`OneAtATime::left`]).
    pub fn headroom(
        &self,
        drives: impl IntoIterator<Item = usize>,
        capture_directories: &[std::path::PathBuf],
    ) -> Vec<crate::patch::types::DriveHeadroom> {
        // In drive order, so the list is the same for the same batch (#1191).
        let mut drives: Vec<usize> = drives.into_iter().collect();
        drives.sort_unstable();
        drives
            .into_iter()
            .filter_map(|idx| {
                capture_directories
                    .get(idx)
                    .map(|p| crate::patch::types::DriveHeadroom {
                        path: p.clone(),
                        free_bytes: {
                            let volume = self.volume_of_drive[idx];
                            self.least_left[volume].unwrap_or(self.free[volume])
                        },
                    })
            })
            .collect()
    }
}

/// The refusal when a capture block fits on no drive, in the player's words:
/// shown as the batch's error toast (#1097). `needed` includes the headroom
/// each drive keeps; `best` is the most free on any one drive, the Windows
/// drive counted only above what `windows_drive_keep_free` keeps there, which
/// the message then names (#1430).
pub(super) fn no_room_message(
    needed: u64,
    best: u64,
    windows_drive_keep_free: Option<u64>,
) -> String {
    use crate::hlcr::take_cleanup::format_gb;
    let note = windows_drive_keep_free
        .map(crate::messages::windows_drive::kept_free_note)
        .unwrap_or_default();
    format!(
        "Not enough free space on any capture drive for this batch: a clip needs {} free on one drive, and the most any one has is {}.{note}",
        format_gb(needed),
        format_gb(best)
    )
}

/// First-Fit-Decreasing: assigns each block (identified by its index into
/// `block_estimates`) to the earliest drive — starting from `*active_drive_idx`
/// and wrapping around — with at least `estimate + threshold` bytes free,
/// largest blocks first. Placing the biggest clips while every drive still has
/// maximum headroom means a later, smaller clip can backfill whatever's left,
/// instead of naive arrival-order first-fit where an earlier small clip can
/// strand a later large one on a drive that would otherwise have fit it.
///
/// Drive `i` draws on `volume_free[volume_of_drive[i]]`: folders on one volume
/// share one budget, so a second folder on a full drive is skipped like the
/// first (#774). Mutates `volume_free` in place (bytes consumed per volume) and
/// advances `*active_drive_idx` to the last drive used, biasing the next call
/// (e.g. the next demo's blocks) to keep filling it. Returns
/// `(block_index, drive_index)` pairs in allocation order, or
/// `Err(block_index)` for the first block that couldn't fit anywhere.
pub(super) fn allocate_blocks_first_fit_decreasing(
    block_estimates: &[u64],
    volume_of_drive: &[usize],
    volume_free: &mut [u64],
    active_drive_idx: &mut usize,
    threshold: u64,
) -> Result<Vec<(usize, usize)>, usize> {
    let num_drives = volume_of_drive.len();

    let mut allocation_order: Vec<usize> = (0..block_estimates.len()).collect();
    allocation_order.sort_by_key(|&i| std::cmp::Reverse(block_estimates[i]));

    let mut result = Vec::with_capacity(block_estimates.len());

    for block_index in allocation_order {
        let clip_byte_estimate = block_estimates[block_index];

        let mut allocated = false;
        let mut drives_checked = 0;
        let mut current_drive_idx = *active_drive_idx;
        loop {
            if drives_checked >= num_drives {
                break;
            }

            let free = &mut volume_free[volume_of_drive[current_drive_idx]];
            if *free >= clip_byte_estimate + threshold {
                *free -= clip_byte_estimate;
                result.push((block_index, current_drive_idx));
                *active_drive_idx = current_drive_idx;
                allocated = true;
                break;
            } else {
                current_drive_idx = (current_drive_idx + 1) % num_drives;
                drives_checked += 1;
            }
        }

        if !allocated {
            return Err(block_index);
        }
    }

    Ok(result)
}

/// Adds a routed block to the batch's latest takes, dropping the oldest past
/// [`TAKES_STILL_ON_DISK`].
fn push_recent(recent: &mut std::collections::VecDeque<(usize, u64)>, volume: usize, bytes: u64) {
    recent.push_back((volume, bytes));
    while recent.len() > TAKES_STILL_ON_DISK {
        recent.pop_front();
    }
}

/// What [`allocate_blocks_one_at_a_time`] planned.
#[derive(Debug, PartialEq)]
pub(super) struct OneAtATime {
    /// `(block_index, drive_index)` pairs, like
    /// [`allocate_blocks_first_fit_decreasing`]'s.
    pub pairs: Vec<(usize, usize)>,
    /// Per volume, the least room the plan left beside a block there: all
    /// the batch can count on there from now on, and what the pre-launch
    /// check re-validates. `None` for a volume that got no block.
    pub left: Vec<Option<u64>>,
    /// How many blocks have room only once the takes before them on their
    /// drive have rendered: the batch may hold before each.
    pub waits: usize,
}

/// Render While Capturing (#861), for a demo whose blocks don't all fit at
/// once: renders free room between clips and the batch holds until they
/// have. Each block goes, from `*active_drive_idx` and wrapping:
///
/// 1. to the earliest drive whose volume's `capacity` (its free space when
///    the batch was planned) holds it plus `threshold` beside the batch's
///    latest takes still on it (`recent`, [`TAKES_STILL_ON_DISK`]), so the
///    batch goes on without waiting for a render (#1438);
/// 2. else, of the drives whose `capacity` holds it on its own, to the one
///    with the fewest of those latest takes on it (ties: the one whose
///    newest take is oldest): the batch holds before it until the earlier
///    takes there have rendered (counted in [`OneAtATime::waits`]).
///
/// The plan is the best case, renders one clip behind: with a deeper render
/// backlog the running batch finds more takes on a drive than counted here,
/// and holds where the plan expected none.
///
/// Deleting this batch's takes can never give a volume more room than its
/// `capacity`, so a block no volume holds alone can never be recorded:
/// `Err(block_index)`. `recent` carries on from one demo's blocks to the
/// next's, since they record one after another.
pub(super) fn allocate_blocks_one_at_a_time(
    block_estimates: &[u64],
    volume_of_drive: &[usize],
    capacity: &[u64],
    recent: &mut std::collections::VecDeque<(usize, u64)>,
    active_drive_idx: &mut usize,
    threshold: u64,
) -> Result<OneAtATime, usize> {
    let num_drives = volume_of_drive.len();
    let mut pairs = Vec::with_capacity(block_estimates.len());
    let mut left: Vec<Option<u64>> = vec![None; capacity.len()];
    let mut waits = 0;
    for (block_index, &estimate) in block_estimates.iter().enumerate() {
        let needed = estimate.saturating_add(threshold);
        let on_disk = |volume: usize| -> u64 {
            recent
                .iter()
                .filter(|(v, _)| *v == volume)
                .map(|(_, bytes)| bytes)
                .sum()
        };
        let room_beside_recent = |d: usize| {
            let volume = volume_of_drive[d];
            capacity[volume].saturating_sub(on_disk(volume))
        };
        let order = || (0..num_drives).map(|step| (*active_drive_idx + step) % num_drives);
        let (drive, room) = match order().find(|&d| room_beside_recent(d) >= needed) {
            Some(d) => (d, room_beside_recent(d)),
            None => {
                // Of the drives that hold it alone, the one with the fewest
                // of the latest takes on it, and of those the one whose
                // newest take is oldest, so the clip waits for a take that
                // has had the longest to render. Taking the first from the
                // active drive picked the drive just recorded to: two clips
                // in a row on a drive that holds one, #1438's shape.
                let newest_take = |volume: usize| recent.iter().rposition(|(v, _)| *v == volume);
                let d = order()
                    .filter(|&d| capacity[volume_of_drive[d]] >= needed)
                    .min_by_key(|&d| {
                        let volume = volume_of_drive[d];
                        (on_disk(volume), newest_take(volume))
                    })
                    .ok_or(block_index)?;
                waits += 1;
                (d, capacity[volume_of_drive[d]])
            }
        };
        pairs.push((block_index, drive));
        *active_drive_idx = drive;
        let volume = volume_of_drive[drive];
        let beside = room - estimate;
        left[volume] = Some(left[volume].map_or(beside, |l| l.min(beside)));
        push_recent(recent, volume, estimate);
    }
    Ok(OneAtATime { pairs, left, waits })
}

#[cfg(test)]
mod one_at_a_time_tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    /// A capture folder whose free space can't be read (a relative path) and
    /// no folder at all both plan as unbounded, as they did when "unknown"
    /// was `u64::MAX`.
    #[test]
    fn unknown_free_space_snapshots_as_an_unbounded_budget() {
        let unreadable = DriveBudget::snapshot(
            &[std::path::PathBuf::from("relative/takes")],
            crate::sys::disk::get_available_bytes,
        );
        assert_eq!(unreadable.free, vec![u64::MAX]);
        let none = DriveBudget::snapshot(&[], crate::sys::disk::get_available_bytes);
        assert_eq!(none.volume_of_drive, vec![0]);
        assert_eq!(none.free, vec![u64::MAX]);
    }

    /// [`allocate_blocks_one_at_a_time`] with no earlier takes.
    fn plan(
        estimates: &[u64],
        volume_of_drive: &[usize],
        capacity: &[u64],
        active: &mut usize,
    ) -> Result<OneAtATime, usize> {
        allocate_blocks_one_at_a_time(
            estimates,
            volume_of_drive,
            capacity,
            &mut std::collections::VecDeque::new(),
            active,
            5 * GB,
        )
    }

    #[test]
    fn blocks_that_fit_alone_go_to_the_first_drive_that_holds_each() {
        let mut active = 0;
        let plan = plan(
            &[30 * GB, 50 * GB],
            &[0, 1],
            &[40 * GB, 100 * GB],
            &mut active,
        )
        .unwrap();
        // 30 GB fits drive 0 alone; 50 GB only fits drive 1.
        assert_eq!(plan.pairs, vec![(0, 0), (1, 1)]);
        assert_eq!(active, 1);
        assert_eq!(plan.left, vec![Some(10 * GB), Some(50 * GB)]);
        assert_eq!(plan.waits, 0);
    }

    #[test]
    fn a_block_no_drive_holds_alone_is_refused() {
        let mut active = 0;
        assert_eq!(
            plan(
                &[10 * GB, 96 * GB],
                &[0, 1],
                &[40 * GB, 100 * GB],
                &mut active
            ),
            Err(1)
        );
    }

    #[test]
    fn folders_on_one_volume_get_drive_indexes_and_share_its_room() {
        let mut active = 0;
        // Folders 0 and 2 share volume 0; folder 1 is volume 1.
        let first = plan(
            &[30 * GB, 50 * GB],
            &[0, 1, 0],
            &[40 * GB, 100 * GB],
            &mut active,
        )
        .unwrap();
        assert_eq!(first.pairs, vec![(0, 0), (1, 1)]);
        assert_eq!(first.left, vec![Some(10 * GB), Some(50 * GB)]);

        // Starting from folder 2 (volume 0), a block that fits volume 0 stays
        // on folder 2: the pair names the folder, not the volume.
        let mut active = 2;
        let second = plan(&[30 * GB], &[0, 1, 0], &[40 * GB, 100 * GB], &mut active).unwrap();
        assert_eq!(second.pairs, vec![(0, 2)]);
    }

    /// #1438: F: held one clip at a time, and the plan put two in a row
    /// there; the batch then held before the second until the first had
    /// rendered. Now the second goes to a drive with room beside it.
    #[test]
    fn a_drive_holding_one_clip_at_a_time_does_not_get_two_in_a_row() {
        // Folders C: D: E: F: G: H:, starting on F:.
        let capacity = [19 * GB, 540 * GB, 1068 * GB, 160 * GB, 743 * GB, 1998 * GB];
        let mut active = 3;
        let plan = plan(
            &[132 * GB, 132 * GB, 132 * GB],
            &[0, 1, 2, 3, 4, 5],
            &capacity,
            &mut active,
        )
        .unwrap();
        assert_eq!(plan.pairs, vec![(0, 3), (1, 4), (2, 4)]);
        assert_eq!(plan.waits, 0);
        // F: keeps 28 GB beside its clip; G: 743 - 132 - 132.
        assert_eq!(plan.left[3], Some(28 * GB));
        assert_eq!(plan.left[4], Some(479 * GB));
    }

    /// Two drives that each hold one clip: once neither has room beside the
    /// latest takes, a clip waits on the drive whose take is older, never
    /// the one just recorded to. Taking the first drive from the active one
    /// planned A B B A A B B A A: each pair is #1438's shape, the second clip
    /// waiting for the first's whole render.
    #[test]
    fn with_no_room_beside_the_latest_takes_a_clip_waits_on_the_older_take() {
        let mut active = 0;
        let plan = plan(&[132 * GB; 9], &[0, 1], &[160 * GB, 160 * GB], &mut active).unwrap();
        let drives: Vec<usize> = plan.pairs.iter().map(|&(_, d)| d).collect();
        assert_eq!(drives, vec![0, 1, 0, 1, 0, 1, 0, 1, 0]);
        assert_eq!(plan.waits, 7);
    }

    /// The figure the pre-launch check gets is the least room left on a
    /// drive across every demo, not only the last demo's.
    #[test]
    fn the_pre_launch_figure_is_the_least_across_demos() {
        let dirs = [std::path::PathBuf::from("takes_a")];
        let mut budget = DriveBudget::snapshot(&dirs, |_| Some(300 * GB));
        let config = crate::patch::types::PatcherConfig {
            render_while_capturing: true,
            ..Default::default()
        };
        // Three 140 GB clips: the second leaves 20 GB beside it and the
        // first's take.
        budget
            .allocate(&[140 * GB, 140 * GB, 140 * GB], &config)
            .unwrap();
        // Then a small clip, planned with 290 GB beside it: the figure
        // stays the batch's least.
        budget.allocate(&[10 * GB], &config).unwrap();
        let headroom = budget.headroom([0], &dirs);
        assert_eq!(headroom[0].free_bytes, 20 * GB);
    }

    /// One drive that holds a clip at a time still plans every clip (the
    /// batch holds between them), and says how many will wait.
    #[test]
    fn a_lone_drive_holding_one_clip_at_a_time_still_plans_and_counts_the_waits() {
        let mut active = 0;
        let plan = plan(&[132 * GB; 3], &[0], &[160 * GB], &mut active).unwrap();
        assert_eq!(plan.pairs, vec![(0, 0), (1, 0), (2, 0)]);
        assert_eq!(plan.waits, 2);
        assert_eq!(plan.left, vec![Some(28 * GB)]);
    }

    /// The room left on a drive is the least beside any of its blocks, the
    /// takes still on it counted: what the pre-launch check re-validates.
    #[test]
    fn the_room_left_is_the_least_beside_any_block_with_the_takes_on_disk() {
        let mut active = 0;
        let plan = plan(&[100 * GB, 100 * GB], &[0], &[300 * GB], &mut active).unwrap();
        assert_eq!(plan.waits, 0);
        assert_eq!(plan.left, vec![Some(100 * GB)]);
    }

    /// Only the latest takes count: one three clips back has rendered.
    #[test]
    fn a_take_older_than_the_latest_two_no_longer_counts() {
        let mut active = 0;
        let plan = plan(&[100 * GB; 4], &[0], &[310 * GB], &mut active).unwrap();
        // Each clip has the two before it beside it: 310 - 200 >= 105.
        assert_eq!(plan.waits, 0);
        assert_eq!(plan.left, vec![Some(10 * GB)]);
    }

    /// The takes recorded at the end of one demo are still on disk when the
    /// next demo's first clip records.
    #[test]
    fn the_latest_takes_carry_from_one_demo_to_the_next() {
        let mut recent = std::collections::VecDeque::new();
        let mut active = 0;
        let capacity = [160 * GB, 1000 * GB];
        let mut demo = |active: &mut usize| {
            allocate_blocks_one_at_a_time(
                &[132 * GB],
                &[0, 1],
                &capacity,
                &mut recent,
                active,
                5 * GB,
            )
            .unwrap()
            .pairs
        };
        assert_eq!(demo(&mut active), vec![(0, 0)]);
        // Back on drive 0, whose last take is still there.
        active = 0;
        assert_eq!(demo(&mut active), vec![(0, 1)]);
    }

    /// A batch through [`DriveBudget`] with Render While Capturing on: the
    /// budget after it, and each block's drive and estimate in record order.
    fn run_batch(
        dirs: &[std::path::PathBuf],
        free: impl Fn(&std::path::Path) -> Option<u64>,
        demos: &[Vec<u64>],
    ) -> (DriveBudget, Vec<(usize, u64)>) {
        let mut budget = DriveBudget::snapshot(dirs, &free);
        let config = crate::patch::types::PatcherConfig {
            render_while_capturing: true,
            ..Default::default()
        };
        let mut routed = Vec::new();
        for estimates in demos {
            let mut pairs = budget.allocate(estimates, &config).unwrap();
            pairs.sort_unstable();
            routed.extend(pairs.iter().map(|&(b, d)| (d, estimates[b])));
        }
        (budget, routed)
    }

    /// The blocks in `routed` without room beside the latest takes on their
    /// drive, recounted from scratch: what the plan's `waits` must say.
    fn rule_waits(routed: &[(usize, u64)], capacity: impl Fn(usize) -> u64) -> usize {
        routed
            .iter()
            .enumerate()
            .filter(|&(i, &(drive, bytes))| {
                let before: u64 = routed[i.saturating_sub(TAKES_STILL_ON_DISK)..i]
                    .iter()
                    .filter(|(d, _)| *d == drive)
                    .map(|(_, b)| b)
                    .sum();
                capacity(drive) < before + bytes + FAILOVER_THRESHOLD
            })
            .count()
    }

    fn dirs(names: &[&str]) -> Vec<std::path::PathBuf> {
        names
            .iter()
            .map(|d| std::path::PathBuf::from(format!("takes_{d}")))
            .collect()
    }

    /// One 300 GB drive and ten one-clip demos of 100 GB: the first two fit
    /// at once, and every clip after waits for the take two before it. Each
    /// demo used to try "fits at once" again against what the last one left
    /// and go uncounted: the log said 4 waits of 4.
    #[test]
    fn every_clip_after_the_batch_goes_one_at_a_time_is_counted() {
        let dirs = dirs(&["a"]);
        let (budget, routed) = run_batch(&dirs, |_| Some(300 * GB), &vec![vec![100 * GB]; 10]);
        assert_eq!(routed.len(), 10);
        assert_eq!(
            budget.one_at_a_time,
            OneAtATimeCounts {
                blocks: 8,
                waits: 8
            }
        );
        assert_eq!(
            budget.one_at_a_time.waits,
            rule_waits(&routed, |_| 300 * GB)
        );
    }

    /// Drives of 300 and 110 GB, demos [100, 100], [100], [90]: the last
    /// clip has room on the small drive, so it goes there rather than
    /// waiting on the big one, whose two takes are still on it.
    #[test]
    fn a_later_demo_sees_the_takes_still_on_disk() {
        let dirs = dirs(&["v", "w"]);
        let free = |d: &std::path::Path| Some(if d.ends_with("takes_v") { 300 } else { 110 } * GB);
        let (budget, routed) = run_batch(
            &dirs,
            free,
            &[vec![100 * GB, 100 * GB], vec![100 * GB], vec![90 * GB]],
        );
        let drives: Vec<usize> = routed.iter().map(|&(d, _)| d).collect();
        assert_eq!(drives, vec![0, 0, 0, 1]);
        assert_eq!(
            budget.one_at_a_time,
            OneAtATimeCounts {
                blocks: 2,
                waits: 1
            }
        );
    }

    /// Draws a batch of 67 demos, 1 to 6 clips each, `lo`..`lo + span` GB
    /// apiece, the same for the same `seed` (xorshift).
    fn random_batch(seed: u64, lo: u64, span: u64) -> Vec<Vec<u64>> {
        let mut x = seed * 2_654_435_761 + 1;
        let mut draw = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        (0..67)
            .map(|_| {
                let clips = 1 + draw(6) as usize;
                (0..clips).map(|_| (lo + draw(span)) * GB).collect()
            })
            .collect()
    }

    /// Small drives that can't keep up: the plan's wait count is exactly the
    /// clips without room beside the latest takes, in every batch, and the
    /// pre-launch figures all pass.
    #[test]
    fn the_wait_count_matches_the_rule_across_many_demos() {
        let dirs = dirs(&["a", "b"]);
        let capacity = |d: usize| [220, 160][d] * GB;
        let free = |p: &std::path::Path| Some(capacity(usize::from(p.ends_with("takes_b"))));
        let mut any_waits = false;
        for seed in 0..40 {
            let (budget, routed) = run_batch(&dirs, free, &random_batch(seed, 70, 60));
            assert_eq!(
                budget.one_at_a_time.waits,
                rule_waits(&routed, capacity),
                "seed {seed}"
            );
            any_waits |= budget.one_at_a_time.waits > 0;
            let used: std::collections::HashSet<usize> = routed.iter().map(|&(d, _)| d).collect();
            for entry in budget.headroom(used, &dirs) {
                assert!(entry.free_bytes >= FAILOVER_THRESHOLD, "seed {seed}");
            }
        }
        assert!(any_waits, "the layout is small enough that some clips wait");
    }

    /// A batch of six capture drives and too many clips to hold at once,
    /// laid out like the one in #1438: C: with 54 GB free, short of its
    /// 50 GiB "Keep free on the Windows drive", F: holding about one clip,
    /// H: with 16 earlier takes still queued on it. Forty batches of 67
    /// demos with 1 to 6 clips each, 40-260 GB apiece by the planner's
    /// estimate: every clip's drive has room for it beside the two takes
    /// before it there, so the batch never waits for a render. Counting
    /// only each clip alone (before #1438), 21 of these batches had clips
    /// that would.
    #[test]
    fn six_drives_like_1438s_plan_every_clip_beside_the_takes_still_on_disk() {
        let dirs = dirs(&["c", "d", "e", "f", "g", "h"]);
        // What `plannable_free_bytes` counts on the Windows drive: its free
        // space less what the setting keeps beyond the usual headroom. Done
        // here by hand: the setting is process-wide, and other tests plan
        // into the temp folder at the same time.
        let keep_free: u64 = 50 * 1024 * 1024 * 1024;
        let c = (54 * GB).saturating_sub(keep_free - FAILOVER_THRESHOLD);
        let capacity = |d: usize| {
            [
                c,
                540 * GB,
                1068 * GB,
                160 * GB,
                743 * GB,
                (2478 - 16 * 30) * GB,
            ][d]
        };
        let free = |p: &std::path::Path| {
            let d = dirs.iter().position(|n| n == p).unwrap();
            Some(capacity(d))
        };
        for seed in 0..40 {
            let (budget, routed) = run_batch(&dirs, free, &random_batch(seed, 40, 220));
            assert!(
                routed.iter().map(|&(_, b)| b).sum::<u64>() > 6000 * GB,
                "seed {seed}: too many to hold at once"
            );
            assert_eq!(budget.one_at_a_time.waits, 0, "seed {seed}");
            assert_eq!(rule_waits(&routed, capacity), 0, "seed {seed}");
            assert!(
                routed.iter().all(|&(d, _)| d != 0),
                "C: never holds a clip past what it keeps free"
            );
            let used: std::collections::HashSet<usize> = routed.iter().map(|&(d, _)| d).collect();
            for entry in budget.headroom(used, &dirs) {
                assert!(
                    entry.free_bytes >= FAILOVER_THRESHOLD,
                    "seed {seed}: {}",
                    entry.path.display()
                );
            }
        }
    }
}

// ── Batch queue builder ───────────────────────────────────────────────────────
