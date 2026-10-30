//! Assigning capture blocks to output drives.

/// The free space every drive keeps beside a block routed to it.
pub(super) const FAILOVER_THRESHOLD: u64 = crate::sys::disk::MIN_DRIVE_HEADROOM_BYTES;

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
}

impl DriveBudget {
    /// Snapshots the free bytes of every capture directory. With none
    /// configured, one sentinel drive with unlimited room stands in, so a
    /// single-drive batch plans without an error.
    pub fn snapshot(capture_directories: &[std::path::PathBuf]) -> Self {
        let (mut volume_of_drive, mut free) = crate::sys::disk::group_by_volume(
            capture_directories,
            crate::sys::disk::get_available_bytes,
        );
        if free.is_empty() {
            volume_of_drive.push(0);
            free.push(u64::MAX);
        }
        let capacity = free.clone();
        DriveBudget {
            volume_of_drive,
            free,
            capacity,
            active_drive: 0,
        }
    }

    /// Routes one chain job's blocks, `estimates` bytes each, to drives:
    /// `(block_index, drive_index)` pairs in allocation order, from
    /// [`allocate_blocks_first_fit_decreasing`].
    ///
    /// When they don't all fit and `config.render_while_capturing` is on, they
    /// are routed [`allocate_blocks_one_at_a_time`] instead: renders free
    /// space as the batch runs and it holds between clips until there is
    /// room, so each block only has to fit a drive on its own, counted as if
    /// the earlier blocks will have been freed by the time it records. A
    /// block bigger than any drive could ever hold is still refused, rather
    /// than starting a batch that can't finish.
    ///
    /// Errors with [`no_room_message`] for the first block that fits nowhere.
    pub fn allocate(
        &mut self,
        estimates: &[u64],
        config: &crate::patch::types::PatcherConfig,
    ) -> std::io::Result<Vec<(usize, usize)>> {
        match allocate_blocks_first_fit_decreasing(
            estimates,
            &self.volume_of_drive,
            &mut self.free,
            &mut self.active_drive,
            FAILOVER_THRESHOLD,
        ) {
            Ok(assignments) => Ok(assignments),
            Err(_) if config.render_while_capturing => {
                let (pairs, left) = allocate_blocks_one_at_a_time(
                    estimates,
                    &self.volume_of_drive,
                    &self.capacity,
                    &mut self.active_drive,
                    FAILOVER_THRESHOLD,
                )
                .map_err(|block_index| {
                    std::io::Error::other(no_room_message(
                        estimates[block_index] + FAILOVER_THRESHOLD,
                        self.capacity.iter().copied().max().unwrap_or(0),
                    ))
                })?;
                for (free, left) in self.free.iter_mut().zip(left) {
                    if let Some(left) = left {
                        *free = left;
                    }
                }
                Ok(pairs)
            }
            Err(block_index) => Err(std::io::Error::other(no_room_message(
                estimates[block_index] + FAILOVER_THRESHOLD,
                self.free.iter().copied().max().unwrap_or(0),
            ))),
        }
    }

    /// The room left beside this batch's blocks on each drive in `drives`
    /// that is a configured capture directory, handed back so the pre-launch
    /// abort in `capture_engine.rs` re-validates the exact numbers this
    /// allocation pass already computed instead of recomputing a third,
    /// narrower (primary-drive-only) answer.
    pub fn headroom(
        &self,
        drives: impl IntoIterator<Item = usize>,
        capture_directories: &[std::path::PathBuf],
    ) -> Vec<crate::patch::types::DriveHeadroom> {
        drives
            .into_iter()
            .filter_map(|idx| {
                capture_directories
                    .get(idx)
                    .map(|p| crate::patch::types::DriveHeadroom {
                        path: p.clone(),
                        free_bytes: self.free[self.volume_of_drive[idx]],
                    })
            })
            .collect()
    }
}

/// The refusal when a capture block fits on no drive, in the player's words:
/// shown as the batch's error toast (#1097). `needed` includes the headroom
/// each drive keeps; `best` is the most free on any one drive.
pub(super) fn no_room_message(needed: u64, best: u64) -> String {
    use crate::hlcr::take_cleanup::format_gb;
    format!(
        "Not enough free space on any capture drive for this batch: a clip needs {} free on one drive, and the most any one has is {}.",
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

/// Render While Capturing (#861), for a demo whose blocks don't all fit at
/// once: renders free room between clips and the batch holds until they
/// have, so each block only has to fit **on its own**. Each goes to the
/// earliest drive, from `*active_drive_idx` and wrapping, whose volume's
/// `capacity` (its free space when the batch was planned) holds it plus
/// `threshold`. Deleting this batch's takes can never give a volume more room
/// than that, so a block no volume holds alone can never be recorded:
/// `Err(block_index)`.
///
/// Returns the pairs like [`allocate_blocks_first_fit_decreasing`] and, per
/// volume, the room left beside its largest block, which is all the batch can
/// count on there from now on.
pub(super) fn allocate_blocks_one_at_a_time(
    block_estimates: &[u64],
    volume_of_drive: &[usize],
    capacity: &[u64],
    active_drive_idx: &mut usize,
    threshold: u64,
) -> Result<(Vec<(usize, usize)>, Vec<Option<u64>>), usize> {
    let num_drives = volume_of_drive.len();
    let mut result = Vec::with_capacity(block_estimates.len());
    let mut largest: Vec<Option<u64>> = vec![None; capacity.len()];
    for (block_index, &estimate) in block_estimates.iter().enumerate() {
        let drive = (0..num_drives)
            .map(|step| (*active_drive_idx + step) % num_drives)
            .find(|&d| capacity[volume_of_drive[d]] >= estimate.saturating_add(threshold))
            .ok_or(block_index)?;
        result.push((block_index, drive));
        *active_drive_idx = drive;
        let volume = volume_of_drive[drive];
        largest[volume] = Some(largest[volume].map_or(estimate, |l| l.max(estimate)));
    }
    let left = largest
        .iter()
        .zip(capacity)
        .map(|(l, &cap)| l.map(|l| cap - l))
        .collect();
    Ok((result, left))
}

#[cfg(test)]
mod one_at_a_time_tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn blocks_that_fit_alone_go_to_the_first_drive_that_holds_each() {
        let mut active = 0;
        let (pairs, left) = allocate_blocks_one_at_a_time(
            &[30 * GB, 50 * GB],
            &[0, 1],
            &[40 * GB, 100 * GB],
            &mut active,
            5 * GB,
        )
        .unwrap();
        // 30 GB fits drive 0 alone; 50 GB only fits drive 1.
        assert_eq!(pairs, vec![(0, 0), (1, 1)]);
        assert_eq!(active, 1);
        assert_eq!(left, vec![Some(10 * GB), Some(50 * GB)]);
    }

    #[test]
    fn a_block_no_drive_holds_alone_is_refused() {
        let mut active = 0;
        assert_eq!(
            allocate_blocks_one_at_a_time(
                &[10 * GB, 96 * GB],
                &[0, 1],
                &[40 * GB, 100 * GB],
                &mut active,
                5 * GB
            ),
            Err(1)
        );
    }

    #[test]
    fn folders_on_one_volume_get_drive_indexes_and_share_its_room() {
        let mut active = 0;
        // Folders 0 and 2 share volume 0; folder 1 is volume 1.
        let (pairs, left) = allocate_blocks_one_at_a_time(
            &[30 * GB, 50 * GB],
            &[0, 1, 0],
            &[40 * GB, 100 * GB],
            &mut active,
            5 * GB,
        )
        .unwrap();
        assert_eq!(pairs, vec![(0, 0), (1, 1)]);
        assert_eq!(left, vec![Some(10 * GB), Some(50 * GB)]);

        // Starting from folder 2 (volume 0), a block that fits volume 0 stays
        // on folder 2: the pair names the folder, not the volume.
        let mut active = 2;
        let (pairs, _) = allocate_blocks_one_at_a_time(
            &[30 * GB],
            &[0, 1, 0],
            &[40 * GB, 100 * GB],
            &mut active,
            5 * GB,
        )
        .unwrap();
        assert_eq!(pairs, vec![(0, 2)]);
    }
}

// ── Batch queue builder ───────────────────────────────────────────────────────
