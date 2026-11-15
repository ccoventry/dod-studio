//! Sends files and folders to the Recycle Bin, so a wrong click can be undone
//! from Explorer (Delete Take #644, the Demo Auditor's Duplicates #844).

use std::path::PathBuf;
use std::sync::mpsc;

/// Paths handed to the shell in one call. One call per path costs a shell
/// operation each, which for a thousand duplicate demos adds up to minutes.
const BATCH: usize = 50;

/// Sends `paths` to the Recycle Bin, in batches, and returns one entry per
/// path: `None` when it went (or was already gone), else why it didn't.
/// `on_progress` gets how many paths have been handled so far after each
/// batch, on the caller's thread.
///
/// On a fresh thread: `trash` initialises COM apartment-threaded on whatever
/// thread calls it and panics if that thread already chose the other model,
/// which a reused worker thread may have. A panic aborts a release build.
pub fn recycle(paths: Vec<PathBuf>, mut on_progress: impl FnMut(usize)) -> Vec<Option<String>> {
    let total = paths.len();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut outcome = vec![None; paths.len()];
        for (batch_idx, batch) in paths.chunks(BATCH).enumerate() {
            // A failed batch may have moved some of its paths before it
            // stopped: go one by one, and count a path already gone as done.
            if trash::delete_all(batch).is_err() {
                for (i, path) in batch.iter().enumerate() {
                    if path.symlink_metadata().is_ok()
                        && let Err(e) = trash::delete(path)
                    {
                        outcome[batch_idx * BATCH + i] = Some(e.to_string());
                    }
                }
            }
            let _ = tx.send(batch_idx * BATCH + batch.len());
        }
        outcome
    });
    for done in rx {
        on_progress(done);
    }
    worker.join().unwrap_or_else(|_| {
        vec![Some("the Recycle Bin call stopped unexpectedly".to_string()); total]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_already_gone_counts_as_done_and_progress_reaches_the_end() {
        let gone = std::env::temp_dir().join(format!("recycle_gone_{}.dem", std::process::id()));
        let mut seen = vec![];
        let outcome = recycle(vec![gone.clone(), gone], |done| seen.push(done));
        assert_eq!(outcome, [None, None]);
        assert_eq!(seen, [2]);
    }

    #[test]
    fn nothing_to_recycle_reports_nothing() {
        let mut seen = vec![];
        assert!(recycle(vec![], |done| seen.push(done)).is_empty());
        assert!(seen.is_empty());
    }
}
