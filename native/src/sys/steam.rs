//! Whether Steam is running and signed in, checked before DoD Studio starts
//! the game.
//!
//! `hl.exe` started with Steam closed exits straight away with its own
//! "Failed to initalize authentication interface. Exiting..." box, and on
//! Anniversary a `!m_bMounted` assert from `filesystem_stdio.dll` on the way
//! out (both seen 2026-09-29). Steam records its own state in the registry:
//! `HKCU\Software\Valve\Steam\ActiveProcess` holds `pid` (Steam's process)
//! and `ActiveUser` (the signed-in account's id, 0 while nobody is signed in,
//! including while Steam is still starting). Whether that account owns the
//! game is not recorded anywhere readable, so that can only be reported after
//! a launch fails.

use std::path::PathBuf;
use std::process::Command;

const STEAM_KEY: &str = r"HKCU\Software\Valve\Steam";
const ACTIVE_PROCESS_KEY: &str = r"HKCU\Software\Valve\Steam\ActiveProcess";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamState {
    /// No `steam.exe` process.
    NotRunning,
    /// Steam is up, but no account is signed in yet.
    SignedOut,
    Ready,
}

impl SteamState {
    pub fn as_str(self) -> &'static str {
        match self {
            SteamState::NotRunning => "not_running",
            SteamState::SignedOut => "signed_out",
            SteamState::Ready => "ready",
        }
    }
}

/// Steam's state right now.
pub fn state() -> SteamState {
    if !super::process::is_running(&["steam.exe"]) {
        return SteamState::NotRunning;
    }
    let active_user = reg_value(ACTIVE_PROCESS_KEY, "ActiveUser").and_then(|v| parse_dword(&v));
    match active_user {
        Some(0) => SteamState::SignedOut,
        // No value at all is an unusual Steam, not a reason to refuse a launch.
        _ => SteamState::Ready,
    }
}

/// Where Steam is installed, as Steam itself recorded it.
pub fn steam_exe() -> Option<PathBuf> {
    let exe = PathBuf::from(reg_value(STEAM_KEY, "SteamExe")?);
    exe.is_file().then_some(exe)
}

/// One value from `reg query`, as text. No new dependency for two reads.
fn reg_value(key: &str, name: &str) -> Option<String> {
    let mut cmd = Command::new("reg");
    cmd.args(["query", key, "/v", name]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    parse_reg_query(&String::from_utf8_lossy(&out.stdout), name)
}

/// The data of `name` in `reg query` output: a line of name, type and data
/// separated by runs of spaces (the data may itself contain spaces).
fn parse_reg_query(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix(name)?.trim_start();
        let (kind, data) = rest.split_once(char::is_whitespace)?;
        kind.starts_with("REG_").then(|| data.trim().to_string())
    })
}

/// `0x1a2b` (how `reg query` prints a REG_DWORD) as a number.
fn parse_dword(data: &str) -> Option<u64> {
    u64::from_str_radix(data.strip_prefix("0x")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACTIVE_PROCESS: &str = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\\ActiveProcess\r\n    ActiveUser    REG_DWORD    0x1f2e3d\r\n\r\n";
    const STEAM: &str = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamExe    REG_SZ    c:/program files (x86)/steam/steam.exe\r\n\r\n";

    #[test]
    fn reads_a_dword_and_a_path_with_spaces() {
        let user = parse_reg_query(ACTIVE_PROCESS, "ActiveUser").unwrap();
        assert_eq!(parse_dword(&user), Some(0x1f2e3d));
        assert_eq!(
            parse_reg_query(STEAM, "SteamExe").as_deref(),
            Some("c:/program files (x86)/steam/steam.exe")
        );
    }

    #[test]
    fn signed_out_reads_as_zero_and_other_names_are_not_matched() {
        let signed_out = ACTIVE_PROCESS.replace("0x1f2e3d", "0x0");
        assert_eq!(
            parse_reg_query(&signed_out, "ActiveUser").and_then(|v| parse_dword(&v)),
            Some(0)
        );
        assert_eq!(parse_reg_query(ACTIVE_PROCESS, "Active"), None);
        assert_eq!(parse_reg_query(ACTIVE_PROCESS, "pid"), None);
    }
}
