//! Finding running processes by executable name (`hl.exe`, `hlae.exe`, OBS).
//!
//! Every caller only needs names and pids, so the snapshot refreshes the
//! process list alone -- no command lines, environments, CPU, memory or
//! disks, which `System::new_all()` used to gather on every check.

use sysinfo::{Process, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

/// A process list with names and pids only. Call [`refresh`] to update it.
pub fn snapshot() -> System {
    System::new_with_specifics(RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()))
}

/// Re-reads the process list of a [`snapshot`], dropping exited processes.
pub fn refresh(sys: &mut System) {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
}

/// Whether `process`'s executable is one of `names` (case-insensitive).
pub fn is_named(process: &Process, names: &[&str]) -> bool {
    let name = process.name().to_string_lossy();
    names.iter().any(|n| name.eq_ignore_ascii_case(n))
}

/// Whether any process in `sys` is one of `names`.
pub fn any_named(sys: &System, names: &[&str]) -> bool {
    sys.processes().values().any(|p| is_named(p, names))
}

/// Whether a process named one of `names` is running right now.
pub fn is_running(names: &[&str]) -> bool {
    any_named(&snapshot(), names)
}

/// The pids of every running process named one of `names`.
pub fn pids_named(names: &[&str]) -> Vec<u32> {
    snapshot()
        .processes()
        .values()
        .filter(|p| is_named(p, names))
        .map(|p| p.pid().as_u32())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test binary itself is a running process, found by its own name.
    #[test]
    fn finds_this_process_by_name() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().to_uppercase();
        let pids = pids_named(&[&name]);
        assert!(pids.contains(&std::process::id()), "{name}: {pids:?}");
        assert!(!is_running(&["no-such-process-dodstudio.exe"]));
    }
}
