//! Assigning capture blocks to output drives.

/// First-Fit-Decreasing: assigns each block (identified by its index into
/// `block_estimates`) to the earliest drive — starting from `*active_drive_idx`
/// and wrapping around — with at least `estimate + threshold` bytes free,
/// largest blocks first. Placing the biggest clips while every drive still has
/// maximum headroom means a later, smaller clip can backfill whatever's left,
/// instead of naive arrival-order first-fit where an earlier small clip can
/// strand a later large one on a drive that would otherwise have fit it.
///
/// Mutates `drive_free` in place (bytes consumed per drive) and advances
/// `*active_drive_idx` to the last drive used, biasing the next call (e.g. the
/// next demo's blocks) to keep filling it. Returns `(block_index, drive_index)`
/// pairs in allocation order, or `Err(block_index)` for the first block that
/// couldn't fit anywhere.
pub(super) fn allocate_blocks_first_fit_decreasing(
    block_estimates: &[u64],
    drive_free: &mut [u64],
    active_drive_idx: &mut usize,
    threshold: u64,
) -> Result<Vec<(usize, usize)>, usize> {
    let num_drives = drive_free.len();

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

            if drive_free[current_drive_idx] >= clip_byte_estimate + threshold {
                drive_free[current_drive_idx] -= clip_byte_estimate;
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
/// earliest drive, from `*active_drive_idx` and wrapping, whose `capacity`
/// (its free space when the batch was planned) holds it plus `threshold`.
/// Deleting this batch's takes can never give a drive more room than that,
/// so a block no drive holds alone can never be recorded: `Err(block_index)`.
///
/// Returns the pairs like [`allocate_blocks_first_fit_decreasing`] and, per
/// drive, the room left beside its largest block, which is all the batch can
/// count on there from now on.
pub(super) fn allocate_blocks_one_at_a_time(
    block_estimates: &[u64],
    capacity: &[u64],
    active_drive_idx: &mut usize,
    threshold: u64,
) -> Result<(Vec<(usize, usize)>, Vec<Option<u64>>), usize> {
    let num_drives = capacity.len();
    let mut result = Vec::with_capacity(block_estimates.len());
    let mut largest: Vec<Option<u64>> = vec![None; num_drives];
    for (block_index, &estimate) in block_estimates.iter().enumerate() {
        let drive = (0..num_drives)
            .map(|step| (*active_drive_idx + step) % num_drives)
            .find(|&d| capacity[d] >= estimate.saturating_add(threshold))
            .ok_or(block_index)?;
        result.push((block_index, drive));
        *active_drive_idx = drive;
        largest[drive] = Some(largest[drive].map_or(estimate, |l| l.max(estimate)));
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
                &[40 * GB, 100 * GB],
                &mut active,
                5 * GB
            ),
            Err(1)
        );
    }
}

// ── Batch queue builder ───────────────────────────────────────────────────────
