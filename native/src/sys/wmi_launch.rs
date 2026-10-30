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

use std::ffi::OsStr;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine as _;

/// How long PowerShell has to start the process. It takes about a second.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How often PowerShell is checked while it runs.
const POLL: Duration = Duration::from_millis(50);

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
/// Studio's child: it is tracked by that id (`sys::launcher`).
pub fn spawn(cmd: &Command) -> Result<u32, WmiLaunchError> {
    let request = Request {
        command_line: command_line(cmd),
        current_directory: cmd.get_current_dir().map(|dir| dir.display().to_string()),
        environment: environment(std::env::vars_os(), cmd.get_envs()),
    };
    let payload =
        serde_json::to_vec(&request).map_err(|e| WmiLaunchError::Unreadable(e.to_string()))?;
    let answer = run_script(&base64::engine::general_purpose::STANDARD.encode(payload))?;
    read_answer(&answer)
}

/// Runs [`SCRIPT`] with `input` on its stdin and returns what it printed.
/// PowerShell is polled, never waited on (CLAUDE.md, process lifecycles).
fn run_script(input: &str) -> Result<String, WmiLaunchError> {
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
    let status = loop {
        if let Some(status) = child.try_wait().map_err(WmiLaunchError::PowerShell)? {
            break status;
        }
        if started.elapsed() > TIMEOUT {
            // Already failing: a kill that fails leaves nothing more to do.
            let _ = child.kill();
            return Err(WmiLaunchError::TimedOut);
        }
        std::thread::sleep(POLL);
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

    /// A real `Win32_Process.Create`: ignored, as it needs the WMI service.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn starts_a_process_through_wmi() {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c", "exit", "0"]).env("SteamAppId", "30");
        let pid = spawn(&cmd).expect("WMI starts cmd.exe");
        assert_ne!(pid, 0);
    }
}
