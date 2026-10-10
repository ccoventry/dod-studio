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
    pids_named_in(&snapshot(), names)
}

/// The pids of every process in `sys` named one of `names`.
pub fn pids_named_in(sys: &System, names: &[&str]) -> Vec<u32> {
    sys.processes()
        .values()
        .filter(|p| is_named(p, names))
        .map(|p| p.pid().as_u32())
        .collect()
}

/// The pids of every process in `sys` named one of `names` whose parent is
/// `parent`, so one launcher's helpers are told apart from another's.
pub fn children_named_in(sys: &System, names: &[&str], parent: u32) -> Vec<u32> {
    sys.processes()
        .values()
        .filter(|p| is_named(p, names) && p.parent().map(|pp| pp.as_u32()) == Some(parent))
        .map(|p| p.pid().as_u32())
        .collect()
}

/// The game's executable. HLAE starts it, so Studio finds it by name rather
/// than owning a handle to it.
pub const GAME_EXE: &str = "hl.exe";

/// The pids of every running game -- the processes Studio could be talking
/// to. `hlae.exe` is left out: it is the launcher, not the game.
pub fn game_pids() -> Vec<u32> {
    pids_named(&[GAME_EXE])
}

/// The pids of every game in `sys`.
pub fn game_pids_in(sys: &System) -> Vec<u32> {
    pids_named_in(sys, &[GAME_EXE])
}

/// Whether `sys` holds a running game.
pub fn is_game_in(sys: &System) -> bool {
    any_named(sys, &[GAME_EXE])
}

/// The `injector.exe` processes the HLAE launcher `launcher_pid` started to
/// hook the game, while they are still running.
pub fn injector_pids_in(sys: &System, launcher_pid: u32) -> Vec<u32> {
    children_named_in(sys, &["injector.exe"], launcher_pid)
}

/// Asks Windows to end every running game (`taskkill /F /IM hl.exe`). It
/// returns when `taskkill` does, which can be before the game has left the
/// process list.
pub fn kill_game() {
    std::process::Command::new("taskkill")
        .args(["/F", "/IM", GAME_EXE])
        .output()
        .ok();
}

/// Asks Windows to end process `pid` (`taskkill /F /PID`).
pub fn kill_pid(pid: u32) {
    std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .ok();
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

    /// Children are matched by parent pid as well as name: the test binary
    /// is a child of its own parent, and of no other process.
    #[test]
    fn finds_children_by_name_and_parent() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        let sys = snapshot();
        let me = std::process::id();
        let parent = sys
            .process(sysinfo::Pid::from_u32(me))
            .and_then(|p| p.parent())
            .map(|p| p.as_u32())
            .expect("the test process has a parent");
        assert!(children_named_in(&sys, &[&name], parent).contains(&me));
        assert!(!children_named_in(&sys, &[&name], me).contains(&me));
        assert!(children_named_in(&sys, &["no-such-process-dodstudio.exe"], parent).is_empty());
        assert!(pids_named_in(&sys, &[&name]).contains(&me));
    }
}
