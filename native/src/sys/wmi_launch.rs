//! Starts a process through WMI's `Win32_Process.Create`, so it belongs to
//! no job of Studio's (#545).
//!
//! A process WMI creates is made by the WMI service, not by Studio, and is in
//! no job at all (measured 2026-10-30: `IsProcessInJob` false). That is how
//! a dev Studio's game escapes the job `cargo run` puts Studio in, which
//! refuses `CREATE_BREAKAWAY_FROM_JOB` (`sys::job`).
//!
//! The call goes through PowerShell's `Invoke-CimMethod` rather than COM from
//! Rust: it runs once per game launch, in dev builds only, and costs about a
//! second. Its inputs reach PowerShell on stdin, base64 JSON, so no path or
//! argument is ever spliced into the script.
//!
//! `Win32_ProcessStartup.EnvironmentVariables` replaces the new process's
//! environment rather than adding to it (measured: given only `SteamAppId`,
//! the process had no `PATH`), so Studio's whole environment is passed, with
//! the command's own changes on top, as a direct spawn would have it.
//!
//! A call that times out, is cancelled or answers unreadably may still have
//! started the process: WMI creates it in its own service, whatever becomes of
//! the PowerShell that asked. So before such a call is given up on, the new
//! process is looked for by name and command line, among the ones that weren't
//! running before the call; the caller then has it rather than starting a
//! second one (follows #1198).

use std::ffi::OsStr;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;

/// How long PowerShell has to start the process. It takes about a second.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How often PowerShell, and the process list after a failed call, are
/// checked.
const POLL: Duration = Duration::from_millis(50);
/// How long after a failed call its process may still turn up. WMI's own
/// `Create` takes well under a second once it has the request.
const SETTLE: Duration = Duration::from_secs(3);

/// Reads the base64 JSON from stdin, asks WMI to start the process, and
/// prints `<ReturnValue> <ProcessId>`.
const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$json = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String([Console]::In.ReadToEnd().Trim()))
$p = $json | ConvertFrom-Json
$startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{ EnvironmentVariables = [string[]]$p.environment }
$arguments = @{ CommandLine = $p.command_line; ProcessStartupInformation = $startup }
if ($p.current_directory) { $arguments.CurrentDirectory = $p.current_directory }
$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments $arguments
"$($r.ReturnValue) $($r.ProcessId)"
"#;

/// What went wrong starting a process through WMI.
#[derive(Debug)]
pub enum WmiLaunchError {
    /// PowerShell itself could not be started or talked to.
    PowerShell(std::io::Error),
    /// PowerShell did not finish within [`TIMEOUT`].
    TimedOut,
    /// PowerShell failed; its error output.
    Script(String),
    /// `Win32_Process.Create` refused, with its return value.
    Refused(u32),
    /// PowerShell's answer could not be read.
    Unreadable(String),
    /// The launch was cancelled while WMI was starting the process; the
    /// process it had started by then, if any, which the caller ends.
    Cancelled(Option<u32>),
}

impl WmiLaunchError {
    /// Whether WMI may have started the process anyway: PowerShell ran, but
    /// how its call ended is not known.
    fn may_have_started(&self) -> bool {
        matches!(
            self,
            Self::TimedOut | Self::Script(_) | Self::Unreadable(_) | Self::Cancelled(_)
        )
    }
}

impl std::fmt::Display for WmiLaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PowerShell(e) => write!(f, "could not run PowerShell for WMI: {e}"),
            Self::TimedOut => write!(
                f,
                "WMI did not start the process within {} s",
                TIMEOUT.as_secs()
            ),
            Self::Script(text) => write!(f, "PowerShell's WMI call failed: {text}"),
            Self::Refused(code) => {
                write!(f, "Win32_Process.Create refused with return value {code}")
            }
            Self::Unreadable(text) => write!(f, "unreadable answer from WMI: {text:?}"),
            Self::Cancelled(_) => write!(f, "cancelled while WMI was starting the process"),
        }
    }
}

impl std::error::Error for WmiLaunchError {}

/// What `Win32_Process.Create` is given.
#[derive(serde::Serialize)]
struct Request {
    command_line: String,
    current_directory: Option<String>,
    environment: Vec<String>,
}

/// Starts `cmd` through WMI and returns its process id. The process is not
/// Studio's child: it is tracked by that id (`sys::launcher`). `cancel`
/// stops the wait; the process WMI started by then comes back in
/// [`WmiLaunchError::Cancelled`].
pub fn spawn(cmd: &Command, cancel: &AtomicBool) -> Result<u32, WmiLaunchError> {
    spawn_within(cmd, cancel, TIMEOUT)
}

/// [`spawn`] with PowerShell given `timeout`, so a test can make it time out
/// around WMI's own call.
fn spawn_within(
    cmd: &Command,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<u32, WmiLaunchError> {
    let request = Request {
        command_line: command_line(cmd),
        current_directory: cmd.get_current_dir().map(|dir| dir.display().to_string()),
        environment: environment(std::env::vars_os(), cmd.get_envs()),
    };
    let payload =
        serde_json::to_vec(&request).map_err(|e| WmiLaunchError::Unreadable(e.to_string()))?;
    let exe_name = exe_name(cmd);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let running_before: Vec<u32> = crate::sys::process::pids_named(&[&exe_name]);
    let result = run_script(
        &base64::engine::general_purpose::STANDARD.encode(payload),
        cancel,
        timeout,
    )
    .and_then(|answer| read_answer(&answer));
    match result {
        Err(error) if error.may_have_started() => {
            let found = look_for_started(&exe_name, &args, &running_before);
            settle(error, found)
        }
        other => other,
    }
}

/// What a call that ended in `error` comes to, given the process it may have
/// started (`found`): a cancelled one stays cancelled and hands the process
/// over to be ended; any other is that process, so no second one is started.
fn settle(error: WmiLaunchError, found: Option<u32>) -> Result<u32, WmiLaunchError> {
    match (error, found) {
        (WmiLaunchError::Cancelled(_), found) => Err(WmiLaunchError::Cancelled(found)),
        (error, Some(pid)) => {
            crate::log_markdown(&format!(
                "[launch] WMI's call ended without an answer ({error}), but it had started the \
                 launcher (PID {pid}): that one is used, not a second"
            ));
            Ok(pid)
        }
        (error, None) => Err(error),
    }
}

/// Polls the process list for up to [`SETTLE`] for the process a WMI call
/// may have started: one named `exe_name`, not running before the call, with
/// `args` (`started_process`).
fn look_for_started(exe_name: &str, args: &[String], running_before: &[u32]) -> Option<u32> {
    let since = Instant::now();
    let waited = wait_until(
        || {
            let running = crate::sys::process::launches_of(&[exe_name]);
            Ok(started_process(running_before, &running, args))
        },
        || false,
        SETTLE,
        || since.elapsed(),
        || std::thread::sleep(POLL),
    );
    match waited {
        Ok(Waited::Done(pid)) => Some(pid),
        _ => None,
    }
}

/// Which of `running` (processes with the launched program's name) the call
/// started: one not running before it whose arguments are the command's. One
/// whose command line Windows wouldn't give counts when it is the only new
/// one.
fn started_process(
    running_before: &[u32],
    running: &[crate::patch::launch_settings::RunningGame],
    args: &[String],
) -> Option<u32> {
    let new: Vec<_> = running
        .iter()
        .filter(|p| !running_before.contains(&p.pid))
        .collect();
    let unreadable_only = match new.as_slice() {
        [only] if only.cmd.is_empty() => Some(only.pid),
        _ => None,
    };
    new.iter()
        .find(|p| p.cmd.get(1..) == Some(args))
        .map(|p| p.pid)
        .or(unreadable_only)
}

/// The file name `cmd` runs, as the process list names it.
pub(crate) fn exe_name(cmd: &Command) -> String {
    std::path::Path::new(cmd.get_program())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// How a [`wait_until`] ended.
#[derive(Debug, PartialEq)]
enum Waited<T> {
    Done(T),
    TimedOut,
    Cancelled,
}

/// Calls `poll` until it has an answer, `cancelled` says stop, or `elapsed`
/// passes `timeout`, with `pause` between calls. Cancel is checked before
/// every call, so it is answered within one pause.
fn wait_until<T>(
    mut poll: impl FnMut() -> std::io::Result<Option<T>>,
    cancelled: impl Fn() -> bool,
    timeout: Duration,
    elapsed: impl Fn() -> Duration,
    mut pause: impl FnMut(),
) -> std::io::Result<Waited<T>> {
    loop {
        if cancelled() {
            return Ok(Waited::Cancelled);
        }
        if let Some(done) = poll()? {
            return Ok(Waited::Done(done));
        }
        if elapsed() > timeout {
            return Ok(Waited::TimedOut);
        }
        pause();
    }
}

/// Runs [`SCRIPT`] with `input` on its stdin and returns what it printed.
/// PowerShell is polled, never waited on (CLAUDE.md, process lifecycles),
/// and ended on `cancel` or once `timeout` passes.
fn run_script(
    input: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String, WmiLaunchError> {
    let mut powershell = Command::new("powershell");
    powershell
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_script(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        powershell.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = powershell.spawn().map_err(WmiLaunchError::PowerShell)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(WmiLaunchError::PowerShell)?;
    }
    let started = Instant::now();
    let waited = wait_until(
        || child.try_wait(),
        || cancel.load(Ordering::Relaxed),
        timeout,
        || started.elapsed(),
        || std::thread::sleep(POLL),
    )
    .map_err(WmiLaunchError::PowerShell)?;
    let status = match waited {
        Waited::Done(status) => status,
        ended => {
            // Already failing: a kill that fails leaves nothing more to do.
            let _ = child.kill();
            return Err(if ended == Waited::TimedOut {
                WmiLaunchError::TimedOut
            } else {
                WmiLaunchError::Cancelled(None)
            });
        }
    };
    let output = child
        .wait_with_output()
        .map_err(WmiLaunchError::PowerShell)?;
    if !status.success() {
        return Err(WmiLaunchError::Script(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// [`SCRIPT`] as `-EncodedCommand` takes it: base64 of UTF-16LE.
fn encoded_script() -> String {
    let utf16: Vec<u8> = SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// The process id from `<ReturnValue> <ProcessId>`.
fn read_answer(answer: &str) -> Result<u32, WmiLaunchError> {
    let unreadable = || WmiLaunchError::Unreadable(answer.trim().to_string());
    let mut words = answer.split_whitespace();
    let code: u32 = words
        .next()
        .and_then(|w| w.parse().ok())
        .ok_or_else(unreadable)?;
    if code != 0 {
        return Err(WmiLaunchError::Refused(code));
    }
    words
        .next()
        .and_then(|w| w.parse().ok())
        .ok_or_else(unreadable)
}

/// `cmd`'s command line, quoted the way `std::process::Command` quotes it
/// for `CreateProcessW`: the program always, an argument when it is empty or
/// has a space or tab, with backslashes before a quote doubled.
fn command_line(cmd: &Command) -> String {
    let mut line = String::new();
    append_quoted(&mut line, cmd.get_program(), true);
    for arg in cmd.get_args() {
        line.push(' ');
        append_quoted(&mut line, arg, false);
    }
    line
}

fn append_quoted(line: &mut String, arg: &OsStr, always: bool) {
    let arg = arg.to_string_lossy();
    let quote = always || arg.is_empty() || arg.contains([' ', '\t']);
    if quote {
        line.push('"');
    }
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                line.extend(std::iter::repeat_n('\\', backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(c);
    }
    if quote {
        line.extend(std::iter::repeat_n('\\', backslashes));
        line.push('"');
    }
}

/// `NAME=value` lines for `inherited` with `changes` applied, as a spawned
/// child's environment would be. Names compare case-insensitively, as
/// Windows does. The hidden per-drive `=C:` entries are left out.
fn environment<'a>(
    inherited: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    changes: impl Iterator<Item = (&'a OsStr, Option<&'a OsStr>)>,
) -> Vec<String> {
    let mut vars: Vec<(String, String)> = inherited
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .filter(|(name, _)| !name.is_empty() && !name.starts_with('='))
        .collect();
    for (name, value) in changes {
        let name = name.to_string_lossy();
        vars.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        if let Some(value) = value {
            vars.push((name.into_owned(), value.to_string_lossy().into_owned()));
        }
    }
    vars.into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn quotes_like_std() {
        let mut cmd = Command::new(r"C:\Programs\HLAE\HLAE.exe");
        cmd.args([
            "-customLoader",
            "",
            r"C:\a b\x.dll",
            r#"say "hi""#,
            r"C:\dir with space\",
        ]);
        assert_eq!(
            command_line(&cmd),
            r#""C:\Programs\HLAE\HLAE.exe" -customLoader "" "C:\a b\x.dll" "say \"hi\"" "C:\dir with space\\""#
        );
    }

    #[test]
    fn applies_the_commands_environment() {
        let inherited = vec![
            (OsString::from("Path"), OsString::from(r"C:\Windows")),
            (OsString::from("steamappid"), OsString::from("70")),
            (OsString::from("=C:"), OsString::from(r"C:\")),
            (OsString::from("GONE"), OsString::from("1")),
        ];
        let set = OsString::from("30");
        let changes = vec![
            (OsStr::new("SteamAppId"), Some(set.as_os_str())),
            (OsStr::new("gone"), None),
        ];
        assert_eq!(
            environment(inherited.into_iter(), changes.into_iter()),
            vec![r"Path=C:\Windows".to_string(), "SteamAppId=30".to_string()]
        );
    }

    #[test]
    fn reads_wmis_answer() {
        assert_eq!(read_answer("0 4242\r\n").unwrap(), 4242);
        assert!(matches!(
            read_answer("9 0"),
            Err(WmiLaunchError::Refused(9))
        ));
        assert!(matches!(
            read_answer(""),
            Err(WmiLaunchError::Unreadable(_))
        ));
    }

    /// A wait ends on its answer, on cancel (checked before each poll, so a
    /// cancel is seen within one pause) and on its timeout, by the clock it
    /// is given.
    #[test]
    fn a_wait_ends_on_its_answer_a_cancel_or_its_timeout() {
        let polls = std::cell::Cell::new(0);
        let poll = || {
            polls.set(polls.get() + 1);
            Ok((polls.get() == 3).then_some("done"))
        };
        let clock = || Duration::ZERO;
        assert_eq!(
            wait_until(poll, || false, TIMEOUT, clock, || {}).unwrap(),
            Waited::Done("done")
        );

        let cancel = AtomicBool::new(false);
        let pauses = std::cell::Cell::new(0);
        let waited = wait_until(
            || Ok(None::<()>),
            || cancel.load(Ordering::Relaxed),
            TIMEOUT,
            clock,
            || {
                pauses.set(pauses.get() + 1);
                if pauses.get() == 2 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert_eq!(waited.unwrap(), Waited::Cancelled);
        assert_eq!(pauses.get(), 2);

        let now = std::cell::Cell::new(Duration::ZERO);
        let waited = wait_until(
            || Ok(None::<()>),
            || false,
            TIMEOUT,
            || now.get(),
            || now.set(now.get() + Duration::from_secs(10)),
        );
        assert_eq!(waited.unwrap(), Waited::TimedOut);
        assert_eq!(now.get(), Duration::from_secs(40));
    }

    fn launcher(pid: u32, cmd: &[&str]) -> crate::patch::launch_settings::RunningGame {
        crate::patch::launch_settings::RunningGame {
            pid,
            exe: None,
            cmd: cmd.iter().map(|a| a.to_string()).collect(),
            hook: Default::default(),
        }
    }

    /// The process a failed call started is the new one with the command's
    /// arguments: not one running before, nor a new one started with other
    /// arguments (another launch). A new one whose command line can't be
    /// read counts only when it is the only new one.
    #[test]
    fn finds_the_launcher_the_call_started() {
        let args: Vec<String> = ["-customLoader", "-noGui"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        let ours = ["HLAE.exe", "-customLoader", "-noGui"];
        let before = [launcher(4, &ours)];
        assert_eq!(started_process(&[4], &before, &args), None);
        let running = [
            launcher(4, &ours),
            launcher(8, &["HLAE.exe", "-other"]),
            launcher(12, &ours),
        ];
        assert_eq!(started_process(&[4], &running, &args), Some(12));
        assert_eq!(
            started_process(&[4], &[launcher(8, &["HLAE.exe", "-other"])], &args),
            None
        );
        assert_eq!(started_process(&[], &[launcher(16, &[])], &args), Some(16));
        assert_eq!(
            started_process(&[], &[launcher(16, &[]), launcher(20, &[])], &args),
            None
        );
    }

    /// A call that ended without an answer and had started the process is
    /// that process; with none found, it stays the failure that falls back
    /// to a plain start. A cancelled one is never a launch: it hands over
    /// what it started, to be ended.
    #[test]
    fn a_failed_call_that_started_the_launcher_uses_it() {
        assert_eq!(settle(WmiLaunchError::TimedOut, Some(12)).unwrap(), 12);
        assert!(matches!(
            settle(WmiLaunchError::TimedOut, None),
            Err(WmiLaunchError::TimedOut)
        ));
        assert!(matches!(
            settle(WmiLaunchError::Cancelled(None), Some(12)),
            Err(WmiLaunchError::Cancelled(Some(12)))
        ));
        assert!(matches!(
            settle(WmiLaunchError::Cancelled(None), None),
            Err(WmiLaunchError::Cancelled(None))
        ));
        // Only a call PowerShell made is looked after: a refusal or a
        // PowerShell that never ran started nothing.
        assert!(WmiLaunchError::TimedOut.may_have_started());
        assert!(WmiLaunchError::Unreadable(String::new()).may_have_started());
        assert!(!WmiLaunchError::Refused(9).may_have_started());
        assert!(
            !WmiLaunchError::PowerShell(std::io::Error::other("no powershell")).may_have_started()
        );
    }

    #[cfg(windows)]
    #[test]
    fn names_the_program_by_its_file() {
        assert_eq!(
            exe_name(&Command::new(r"C:\Programs\HLAE\HLAE.exe")),
            "HLAE.exe"
        );
    }

    /// Real `Win32_Process.Create` calls cut off by timeouts around the time
    /// WMI takes: each ends with exactly one process started and that one's
    /// pid, or none started and an error, never a process the caller doesn't
    /// know about. Ignored, as it needs the WMI service; each start is a
    /// `ping` of its own address, ended afterwards.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn a_timed_out_call_never_leaves_a_process_unaccounted_for() {
        for (i, millis) in (150..=350).step_by(10).enumerate() {
            let address = format!("127.0.0.{}", i + 10);
            let mut cmd = Command::new(r"C:\Windows\System32\PING.EXE");
            cmd.args(["-n", "20", &address]);
            let since = Instant::now();
            let result = spawn_within(&cmd, &AtomicBool::new(false), Duration::from_millis(millis));
            // A call answered after its timeout found its process afterwards.
            eprintln!(
                "{millis} ms: {:?} after {} ms",
                result.as_ref().map_err(ToString::to_string),
                since.elapsed().as_millis()
            );
            std::thread::sleep(SETTLE);
            let ours: Vec<u32> = crate::sys::process::launches_of(&["PING.EXE"])
                .into_iter()
                .filter(|p| p.cmd.last() == Some(&address))
                .map(|p| p.pid)
                .collect();
            for pid in &ours {
                crate::sys::process::kill_pid(*pid);
            }
            match result {
                Ok(pid) => assert_eq!(ours, [pid], "{millis} ms"),
                Err(e) => assert!(ours.is_empty(), "{millis} ms: {e}, yet {ours:?} run"),
            }
        }
    }

    /// A real `Win32_Process.Create`: ignored, as it needs the WMI service.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn starts_a_process_through_wmi() {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c", "exit", "0"]).env("SteamAppId", "30");
        let pid = spawn(&cmd, &AtomicBool::new(false)).expect("WMI starts cmd.exe");
        assert_ne!(pid, 0);
    }
}
