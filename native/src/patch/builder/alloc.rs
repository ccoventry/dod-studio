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

// ── Batch queue builder ───────────────────────────────────────────────────────
