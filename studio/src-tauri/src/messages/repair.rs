//! Activity-log lines and errors of the damaged-demo repair (#15):
//! `demo_repair_manager.rs`.

pub fn not_a_folder(folder: &str) -> String {
    format!("{folder} is not a folder")
}

pub fn damaged_scan_started(folder: &str, recursive: bool) -> String {
    let sub = if recursive { " and its subfolders" } else { "" };
    format!("[repair] looking for damaged demos in {folder}{sub}")
}

pub fn damaged_scan_ended(found: usize, took: &str, cancelled: bool) -> String {
    let how = if cancelled { "cancelled" } else { "done" };
    format!("[repair] damaged-demo search {how} in {took}: {found} damaged demo(s) found")
}

pub fn repair_started(path: &str) -> String {
    format!("[repair] started: {path}")
}

pub fn repair_done(
    path: &str,
    written: &str,
    holes: usize,
    seconds_lost: f32,
    took: &str,
) -> String {
    format!(
        "[repair] done in {took}: {path} -> {written}, {holes} gap(s) cut out, {seconds_lost:.0} s of recording lost"
    )
}

pub fn repair_not_damaged(path: &str, took: &str) -> String {
    format!("[repair] {path}: no gap found in {took}, nothing written")
}

pub fn repair_refused(path: &str, reasons: &[String], took: &str) -> String {
    format!(
        "[repair] {path}: the repair failed its checks after {took}, nothing written: {}",
        reasons.join("; ")
    )
}

pub fn repair_failed(path: &str, err: &str, took: &str) -> String {
    format!("[repair] {path}: failed after {took}: {err}")
}

/// A second repair of a demo whose repair is still running.
pub fn already_repairing(path: &str) -> String {
    format!("{path} is already being repaired")
}

pub fn already_repairing_log(path: &str) -> String {
    format!("[repair] {path}: not started, it is already being repaired")
}

pub fn repair_cancelled(path: &str, took: &str) -> String {
    format!("[repair] {path}: cancelled after {took}, nothing written")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repair_logs_its_gaps_and_how_long_it_took() {
        assert_eq!(
            repair_done("a.dem", "a_repaired.dem", 2, 42.4, "3 s"),
            "[repair] done in 3 s: a.dem -> a_repaired.dem, 2 gap(s) cut out, 42 s of recording lost"
        );
    }
}
