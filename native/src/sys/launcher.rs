//! Starts the HLAE launcher so the game outlives a dev Studio (#545), and
//! tracks the launcher however it was started.
//!
//! An installed Studio spawns HLAE as its child, as it always has. A Studio
//! in a job that ends its processes when it closes (`cargo run`, so every
//! `npm run tauri dev`; `sys::job`) would take the game down with it, since
//! hl.exe joins the job through HLAE. There the launcher is started outside
//! the job: with `CREATE_BREAKAWAY_FROM_JOB` when the job allows it (cargo's
//! doesn't), else through WMI (`sys::wmi_launch`). A WMI-started launcher is
//! not Studio's child, so it is tracked by its pid in the process list, the
//! way hl.exe already is.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use sysinfo::{Pid, System};

/// `CREATE_BREAKAWAY_FROM_JOB`.
#[cfg(windows)]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
/// `ERROR_ACCESS_DENIED`: what a job without `BREAKAWAY_OK` answers.
#[cfg(windows)]
const ACCESS_DENIED: i32 = 5;
/// How long a killed launcher Studio doesn't own has to leave the list.
const KILL_WAIT: Duration = Duration::from_secs(2);

/// The HLAE launcher a launch started.
pub struct Launcher {
    kind: Kind,
}

enum Kind {
    /// Studio's own child: waited on through its handle.
    Child(Child),
    /// Started through WMI: found by pid and executable name.
    Detached { pid: u32, exe_name: String },
}

impl Launcher {
    /// Starts `cmd`, outside Studio's job when that job would end it when
    /// Studio closes. May set `cmd`'s creation flags, so callers set none.
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
        if !crate::sys::job::kills_children_on_close() {
            return cmd.spawn().map(Self::child);
        }
        if let Some(child) = spawn_breaking_away(cmd)? {
            crate::log_markdown(
                "[launch] Started outside Studio's job, so the game outlives Studio (#545)",
            );
            return Ok(Self::child(child));
        }
        match crate::sys::wmi_launch::spawn(cmd) {
            Ok(pid) => {
                crate::log_markdown(&format!(
                    "[launch] Started through WMI (PID {pid}), so the game outlives this dev Studio (#545)"
                ));
                Ok(Self {
                    kind: Kind::Detached {
                        pid,
                        exe_name: exe_name(cmd),
                    },
                })
            }
            Err(e) => {
                crate::log_markdown(&format!(
                    "[launch] Could not start {} outside Studio's job ({e}); started it as Studio's own, \
                     so closing this dev Studio closes the game too",
                    std::path::Path::new(cmd.get_program()).display()
                ));
                cmd.spawn().map(Self::child)
            }
        }
    }

    fn child(child: Child) -> Self {
        Self {
            kind: Kind::Child(child),
        }
    }

    /// The launcher's process id.
    pub fn id(&self) -> u32 {
        match &self.kind {
            Kind::Child(child) => child.id(),
            Kind::Detached { pid, .. } => *pid,
        }
    }

    /// How the launcher exited, once it has; `None` while it runs. A launcher
    /// Studio doesn't own is looked for in `sys`, which the caller refreshes.
    pub fn exit_status(&mut self, sys: &System) -> std::io::Result<Option<String>> {
        match &mut self.kind {
            Kind::Child(child) => Ok(child.try_wait()?.map(|status| status.to_string())),
            Kind::Detached { pid, exe_name } => Ok((!is_running_in(sys, *pid, exe_name))
                .then(|| "unknown: started through WMI".to_string())),
        }
    }

    /// Ends the launcher and waits for it to go.
    pub fn kill(&mut self) {
        match &mut self.kind {
            Kind::Child(child) => {
                // A launcher that already exited can't be killed, and that is
                // the outcome wanted.
                let _ = child.kill();
                let _ = child.wait();
            }
            Kind::Detached { pid, exe_name } => kill_detached(*pid, exe_name),
        }
    }
}

/// Spawns `cmd` with `CREATE_BREAKAWAY_FROM_JOB`. `Ok(None)` when the job
/// doesn't allow it, so another route is tried.
#[cfg(windows)]
fn spawn_breaking_away(cmd: &mut Command) -> std::io::Result<Option<Child>> {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_BREAKAWAY_FROM_JOB);
    match cmd.spawn() {
        Ok(child) => Ok(Some(child)),
        Err(e) if e.raw_os_error() == Some(ACCESS_DENIED) => {
            cmd.creation_flags(0);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[cfg(not(windows))]
fn spawn_breaking_away(_cmd: &mut Command) -> std::io::Result<Option<Child>> {
    Ok(None)
}

/// The file name `cmd` runs, as the process list names it.
fn exe_name(cmd: &Command) -> String {
    std::path::Path::new(cmd.get_program())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether `pid` is still the process named `exe_name`. A pid now held by
/// another process means the launcher has gone.
fn is_running_in(sys: &System, pid: u32, exe_name: &str) -> bool {
    sys.process(Pid::from_u32(pid))
        .is_some_and(|process| crate::sys::process::is_named(process, &[exe_name]))
}

/// Ends launcher `pid` and polls until it has left the process list, for at
/// most [`KILL_WAIT`]. A pid that is no longer the launcher is left alone.
fn kill_detached(pid: u32, exe_name: &str) {
    let mut sys = crate::sys::process::snapshot();
    if !is_running_in(&sys, pid, exe_name) {
        return;
    }
    crate::sys::process::kill_pid(pid);
    let since = Instant::now();
    while since.elapsed() < KILL_WAIT {
        crate::sys::process::refresh(&mut sys);
        if !is_running_in(&sys, pid, exe_name) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    crate::log_markdown(&format!(
        "[launch] The launcher (PID {pid}) was still running {} s after it was ended",
        KILL_WAIT.as_secs()
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A detached launcher is running while its pid has its name, and gone
    /// once the pid has another name: the test binary stands in for it.
    #[test]
    fn tracks_a_detached_launcher_by_pid_and_name() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        let sys = crate::sys::process::snapshot();
        let mut running = Launcher {
            kind: Kind::Detached {
                pid: std::process::id(),
                exe_name: name,
            },
        };
        assert_eq!(running.exit_status(&sys).unwrap(), None);
        assert_eq!(running.id(), std::process::id());
        let mut renamed = Launcher {
            kind: Kind::Detached {
                pid: std::process::id(),
                exe_name: "no-such-launcher-dodstudio.exe".to_string(),
            },
        };
        assert!(renamed.exit_status(&sys).unwrap().is_some());
        // Not this process's name, so it is never ended.
        renamed.kill();
    }

    #[cfg(windows)]
    #[test]
    fn names_the_program_by_its_file() {
        assert_eq!(
            exe_name(&Command::new(r"C:\Programs\HLAE\HLAE.exe")),
            "HLAE.exe"
        );
    }
}
