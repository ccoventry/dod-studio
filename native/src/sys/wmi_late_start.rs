//! Ends a launcher that a cancelled WMI call starts after Studio stopped
//! looking for it (#1243's review).
//!
//! A cancelled `Win32_Process.Create` call may still start its process: WMI
//! creates it in its own service, whatever becomes of the PowerShell that
//! asked. `sys::wmi_launch` looks for it for a few seconds and ends what it
//! finds; one that turns up later would run a game nobody asked for, past the
//! batch that was cancelled. So after a cancelled call that found nothing, a
//! thread keeps watching for a while and ends that launcher, with the game it
//! started.
//!
//! Only a process the cancelled call started is ever ended: one with the
//! launcher's name that wasn't running before the call, whose arguments are
//! the call's own exactly (they carry this Studio's `-dodstudio_pipe`, #1235),
//! and, for the game, an hl.exe whose parent is that launcher, that wasn't
//! running before the call and started after the launcher did. Unlike the
//! look after a failed call, a process whose command line can't be read is
//! never taken for it. The watch stops as soon as Studio starts another WMI
//! launch, whose launcher would match too.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::patch::launch_settings::RunningGame;

/// How long after a cancelled call its launcher is watched for. Generous:
/// WMI's own `Create` takes well under a second once it has the request,
/// and the watch costs one process-list read per [`POLL`].
pub(crate) const WATCH: Duration = Duration::from_secs(30);
/// How often the process list is read. A launcher takes longer than this to
/// start its game.
const POLL: Duration = Duration::from_millis(200);

/// Counts WMI launches, so a watch can tell a later one has started.
static LAUNCHES: AtomicU64 = AtomicU64::new(0);

/// Marks the start of a WMI launch and returns its number: every watch
/// started before it stops.
pub(crate) fn begin_launch() -> u64 {
    LAUNCHES.fetch_add(1, Ordering::SeqCst) + 1
}

/// Whether a launch after `launch` has begun.
fn is_superseded(launch: u64) -> bool {
    LAUNCHES.load(Ordering::SeqCst) != launch
}

/// What a cancelled call would have started.
pub(crate) struct CancelledCall {
    /// Its number, from [`begin_launch`].
    pub launch: u64,
    /// The launcher's file name, as the process list names it.
    pub exe_name: String,
    /// The launcher's arguments.
    pub args: Vec<String>,
    /// The processes with that name that ran before the call.
    pub running_before: Vec<u32>,
    /// The games (hl.exe) that ran before the call: never ended.
    pub games_before: Vec<u32>,
}

/// Starts a thread that watches for [`WATCH`] for the launcher `call` may
/// still start, and ends it and its game if it does. Logs the watch's start
/// and end to the activity log.
pub(crate) fn watch_for_late_start(call: CancelledCall) {
    crate::log_markdown(&crate::messages::late_launcher_watch_started(
        WATCH.as_secs(),
    ));
    let spawned = std::thread::Builder::new()
        .name("wmi-late-start".to_string())
        .spawn(move || watch(&call));
    if let Err(e) = spawned {
        crate::log_markdown(&crate::messages::late_launcher_watch_failed(e));
    }
}

/// The body of [`watch_for_late_start`]'s thread.
fn watch(call: &CancelledCall) {
    let since = Instant::now();
    let ended = loop {
        if is_superseded(call.launch) {
            break Watch::Superseded;
        }
        let running = crate::sys::process::launches_of(&[&call.exe_name]);
        if let Some(pid) = late_start(&call.running_before, &running, &call.args) {
            break Watch::Found(pid);
        }
        if since.elapsed() > WATCH {
            break Watch::NothingStarted;
        }
        std::thread::sleep(POLL);
    };
    let line = match ended {
        Watch::Found(pid) => {
            let games = end_launcher_and_its_games(call, pid);
            crate::messages::late_launcher_ended(pid, games, since.elapsed().as_secs_f64())
        }
        Watch::NothingStarted => crate::messages::late_launcher_none(WATCH.as_secs()),
        Watch::Superseded => crate::messages::late_launcher_watch_superseded(),
    };
    crate::log_markdown(&line);
}

/// How a watch ended.
enum Watch {
    Found(u32),
    NothingStarted,
    Superseded,
}

/// Which of `running` (processes with the launcher's name) the cancelled call
/// started: one not running before it whose arguments are exactly `args`.
/// Never one whose command line can't be read: ending a process is only for
/// one known to be Studio's.
fn late_start(running_before: &[u32], running: &[RunningGame], args: &[String]) -> Option<u32> {
    running
        .iter()
        .find(|p| !running_before.contains(&p.pid) && p.cmd.get(1..) == Some(args))
        .map(|p| p.pid)
}

/// Ends launcher `pid`, then every hl.exe it had started, and returns how
/// many games were ended. The games are listed after the launcher is gone,
/// so it can't start another in between. A pid that is no longer the
/// launcher is left alone, and so are its "children" ([`its_games`]).
fn end_launcher_and_its_games(call: &CancelledCall, pid: u32) -> usize {
    use crate::sys::process::{GAME_EXE, is_named, kill_pid, snapshot};
    let sys = snapshot();
    let Some(launcher) = sys
        .process(sysinfo::Pid::from_u32(pid))
        .filter(|p| is_named(p, &[&call.exe_name]))
    else {
        return 0;
    };
    let launcher_start = launcher.start_time();
    kill_pid(pid);
    let sys = snapshot();
    let children: Vec<(u32, u64)> = crate::sys::process::children_named_in(&sys, &[GAME_EXE], pid)
        .into_iter()
        .filter_map(|game| {
            let start = sys.process(sysinfo::Pid::from_u32(game))?.start_time();
            Some((game, start))
        })
        .collect();
    let games = its_games(&call.games_before, launcher_start, &children);
    for game in &games {
        kill_pid(*game);
    }
    games.len()
}

/// Which of `children` (hl.exe pids whose parent pid is the launcher's, with
/// their start times) the launcher started. HLAE exits once it has started
/// its game, so an hl.exe whose own launcher exited keeps that launcher's pid
/// as its parent, and Windows hands pids out again: the late launcher may
/// have the pid of the one that started the user's game. So a game that ran
/// before the call, or started before the launcher did, is never its. A
/// start time of 0 is one Windows wouldn't give, and is not compared.
fn its_games(games_before: &[u32], launcher_start: u64, children: &[(u32, u64)]) -> Vec<u32> {
    children
        .iter()
        .filter(|(pid, start)| {
            !games_before.contains(pid) && (launcher_start == 0 || *start >= launcher_start)
        })
        .map(|(pid, _)| *pid)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launcher(pid: u32, cmd: &[&str]) -> RunningGame {
        RunningGame {
            pid,
            exe: None,
            cmd: cmd.iter().map(|a| a.to_string()).collect(),
            hook: Default::default(),
        }
    }

    fn args() -> Vec<String> {
        ["-customLoader", "-dodstudio_pipe", "a"]
            .iter()
            .map(|a| a.to_string())
            .collect()
    }

    const OURS: [&str; 4] = ["HLAE.exe", "-customLoader", "-dodstudio_pipe", "a"];

    /// The late launcher is the new one with the call's own arguments.
    #[test]
    fn a_late_start_is_the_new_launcher_with_the_calls_arguments() {
        let running = [launcher(4, &OURS), launcher(12, &OURS)];
        assert_eq!(late_start(&[4], &running, &args()), Some(12));
    }

    /// One running before the call is the user's or an earlier launch's.
    #[test]
    fn a_launcher_running_before_the_call_is_never_a_late_start() {
        assert_eq!(late_start(&[4], &[launcher(4, &OURS)], &args()), None);
    }

    /// Another Studio's launch (its own pipe) or the user's own HLAE.
    #[test]
    fn a_new_launcher_with_other_arguments_is_never_a_late_start() {
        let theirs = ["HLAE.exe", "-customLoader", "-dodstudio_pipe", "b"];
        assert_eq!(late_start(&[], &[launcher(8, &theirs)], &args()), None);
    }

    /// The look after a failed call takes a lone unreadable one; ending one
    /// needs proof that it is Studio's.
    #[test]
    fn a_launcher_whose_command_line_cant_be_read_is_never_a_late_start() {
        assert_eq!(late_start(&[], &[launcher(16, &[])], &args()), None);
    }

    /// An hl.exe whose parent pid is the late launcher's is only its game
    /// when it started after the launcher and wasn't running before the
    /// call: one whose own (exited) HLAE had that pid before is the user's.
    #[test]
    fn only_a_game_the_late_launcher_started_is_ended() {
        let children = [(20, 100), (21, 99), (22, 105), (23, 100)];
        assert_eq!(its_games(&[23], 100, &children), vec![20, 22]);
        // A launcher start time Windows wouldn't give leaves the pids alone
        // to decide.
        assert_eq!(its_games(&[23], 0, &children), vec![20, 21, 22]);
    }

    /// The plain process list carries start times, so [`its_games`] has them
    /// to compare.
    #[cfg(windows)]
    #[test]
    fn the_process_list_gives_start_times() {
        let sys = crate::sys::process::snapshot();
        let me = sys
            .process(sysinfo::Pid::from_u32(std::process::id()))
            .expect("the test process is listed");
        assert_ne!(me.start_time(), 0);
    }

    /// A real process that turns up after the watch began, with the call's
    /// arguments, is ended; one started before it with the same arguments
    /// is left alone. Ignored: it starts `ping`s and takes a few seconds;
    /// run it on its own, since any WMI launch (another test's) stops the
    /// watch. The pings get no window (`CREATE_NO_WINDOW`), so a run never
    /// opens one on the desktop.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn a_launcher_that_starts_during_the_watch_is_ended() {
        use std::os::windows::process::CommandExt;
        let args: Vec<String> = ["-n", "30", "127.0.0.77"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        let ping = move |args: &[String]| {
            std::process::Command::new(r"C:\Windows\System32\PING.EXE")
                .args(args)
                .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        };
        let mut earlier = ping(&args);
        let call = CancelledCall {
            launch: begin_launch(),
            exe_name: "PING.EXE".to_string(),
            args: args.clone(),
            running_before: crate::sys::process::pids_named(&["PING.EXE"]),
            games_before: Vec::new(),
        };
        let starter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            ping(&args)
        });
        let since = Instant::now();
        watch(&call);
        let mut late = starter.join().unwrap();
        assert!(since.elapsed() < WATCH, "the watch ran out instead");
        let ended = late.wait().unwrap();
        assert!(!ended.success(), "the late ping ran to its end");
        assert_eq!(
            earlier.try_wait().unwrap(),
            None,
            "the earlier ping was ended"
        );
        earlier.kill().unwrap();
    }

    /// A watch stops once any later launch begins, so it can't end that
    /// launch's launcher.
    #[test]
    fn a_later_launch_supersedes_a_watch() {
        let launch = begin_launch();
        begin_launch();
        assert!(is_superseded(launch));
    }
}
