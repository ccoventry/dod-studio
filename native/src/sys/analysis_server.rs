//! Studio analyses demos for the game (#565).
//!
//! The in-game DoD Studio window's Highlights tab needs a demo's analysis.
//! The game is a 32-bit process, and the pre-Anniversary one has under 1 GB
//! of address space left, while an analysis peaks at about 12 times the
//! demo's size: a big demo doesn't fit. So while Studio runs, it serves a
//! named pipe, and the game asks it first. Studio analyses the demo through
//! `run_analyzer_cached`, which saves it to the analyzer cache the game then
//! reads.
//!
//! ## Protocol
//!
//! One request per connection, text lines:
//!
//! - the game: `analyze <absolute path to the .dem>`
//! - Studio: `progress <0-100>` while it reads, then `cache v<N>` and `done`,
//!   or `failed <reason>`.
//!
//! `cache v<N>` is the analyzer cache version Studio saved the result in
//! (`analysis::cache::SCHEMA_VERSION`). A hook DLL built for another version
//! reads another folder and would find nothing, so it says so instead of
//! reporting the result missing (#684). A hook from before the line ignores
//! it, as it ignores any line it doesn't know.
//!
//! The pipe takes local clients only, and a request only ever reads a `.dem`
//! file and writes its cache entry.
//!
//! The hook DLL's side is `goldsrc-hooks/src/streaks.rs`; [`PIPE_NAME`](crate::sys::analysis_server::PIPE_NAME) must
//! match its `STUDIO_PIPE` to the character.

use std::ffi::c_void;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const PIPE_NAME: &str = r"\\.\pipe\dodstudio-analyzer";

/// The longest request line read: a path, with room to spare.
const MAX_REQUEST: u64 = 8192;

static STARTED: AtomicBool = AtomicBool::new(false);

/// Starts serving the pipe on a thread of its own, once per process. Another
/// Studio already serving it keeps it; this one then serves nothing.
pub fn start() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("analysis-server".into())
        .spawn(|| serve(PIPE_NAME));
}

/// Accepts connections forever, each handled on its own thread.
fn serve(name: &str) {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut first = true;
    loop {
        let flags = PIPE_ACCESS_DUPLEX
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // Safety: a NUL-terminated name, no security attributes.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            if first {
                crate::log_markdown(&format!(
                    "Analysis server: {name} is already served (another DoD Studio), so this one is not"
                ));
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        first = false;
        // Safety: the handle just created; blocks until a client connects.
        let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        // Safety: an owned pipe handle; the File closes it.
        let pipe = unsafe { std::fs::File::from_raw_handle(handle) };
        if !connected {
            continue;
        }
        let _ = std::thread::Builder::new()
            .name("analysis-request".into())
            .spawn(move || handle_client(pipe));
    }
}

fn handle_client(pipe: std::fs::File) {
    let Ok(mut writer) = pipe.try_clone() else {
        return;
    };
    let mut line = String::new();
    if BufReader::new(std::io::Read::take(&pipe, MAX_REQUEST))
        .read_line(&mut line)
        .is_err()
    {
        return;
    }
    let reply =
        |writer: &mut std::fs::File, text: &str| writer.write_all(format!("{text}\n").as_bytes());
    let path = match parse_request(&line) {
        Ok(path) => path,
        Err(why) => {
            let _ = reply(&mut writer, &format!("failed {why}"));
            return;
        }
    };
    crate::log_markdown(&format!(
        "Analysis server: the game asked for {}",
        path.display()
    ));
    let mut last = None;
    let result = crate::run_analyzer_cached(&path, |done, total| {
        let percent = progress(done, total);
        if last != Some(percent) {
            last = Some(percent);
            // The game may have gone; the analysis still lands in the cache.
            let _ = reply(&mut writer, &format!("progress {percent}"));
        }
    });
    for line in final_reply(result.map(|_| ())) {
        let _ = reply(&mut writer, &line);
    }
    let _ = writer.flush();
    // Safety: the pipe's own handle; lets the game read the last line before
    // the handle closes.
    unsafe { FlushFileBuffers(std::os::windows::io::AsRawHandle::as_raw_handle(&writer)) };
}

/// The demo a request names: `analyze <absolute path to an existing .dem>`.
pub fn parse_request(line: &str) -> Result<PathBuf, String> {
    let path = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("analyze ")
        .ok_or("not an analyze request")?;
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("the path is not absolute".to_string());
    }
    let is_demo = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dem"));
    if !is_demo {
        return Err("not a .dem file".to_string());
    }
    if !path.is_file() {
        return Err("no such demo".to_string());
    }
    Ok(path.to_path_buf())
}

/// The lines that end a request: which cache the result went to, then
/// `done`; or why it failed.
fn final_reply(result: Result<(), String>) -> Vec<String> {
    match result {
        Ok(()) => vec![
            format!("cache v{}", analysis::cache::SCHEMA_VERSION),
            "done".to_string(),
        ],
        Err(why) => vec![format!("failed {}", one_line(&why))],
    }
}

fn progress(done: usize, total: usize) -> u32 {
    if total == 0 {
        0
    } else {
        (done as u128 * 100 / total as u128).min(100) as u32
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

const PIPE_ACCESS_DUPLEX: u32 = 0x3;
const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
const PIPE_TYPE_BYTE: u32 = 0;
const PIPE_READMODE_BYTE: u32 = 0;
const PIPE_WAIT: u32 = 0;
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x8;
const PIPE_UNLIMITED_INSTANCES: u32 = 255;
const ERROR_PIPE_CONNECTED: u32 = 535;
const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        open_mode: u32,
        pipe_mode: u32,
        max_instances: u32,
        out_buffer: u32,
        in_buffer: u32,
        default_timeout: u32,
        security: *mut c_void,
    ) -> *mut c_void;
    fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
    fn FlushFileBuffers(file: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn a_request_names_an_existing_absolute_demo() {
        assert!(parse_request("hello").is_err());
        assert!(parse_request("analyze relative.dem\n").is_err());
        assert!(parse_request(r"analyze C:\no\such\demo.dem").is_err());
        let dir = std::env::temp_dir().join(format!("analysis_server_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let demo = dir.join("x.dem");
        std::fs::write(&demo, b"x").unwrap();
        let other = dir.join("x.txt");
        std::fs::write(&other, b"x").unwrap();
        assert_eq!(
            parse_request(&format!("analyze {}\r\n", demo.display())),
            Ok(demo.clone())
        );
        assert!(parse_request(&format!("analyze {}", other.display())).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_is_a_percentage() {
        assert_eq!(progress(0, 0), 0);
        assert_eq!(progress(50, 200), 25);
        assert_eq!(progress(300, 200), 100);
        // No overflow however big the counts.
        assert_eq!(progress(usize::MAX / 2, usize::MAX), 49);
    }

    #[test]
    fn a_finished_request_names_its_cache_version_before_done() {
        assert_eq!(
            final_reply(Ok(())),
            [
                format!("cache v{}", analysis::cache::SCHEMA_VERSION),
                "done".to_string()
            ]
        );
        assert_eq!(
            final_reply(Err("bad\r\nframe".to_string())),
            ["failed bad  frame".to_string()]
        );
    }

    /// A real round trip through the pipe: a file that isn't a demo comes
    /// back `failed`.
    #[test]
    fn the_pipe_answers_a_request() {
        let name = format!(r"\\.\pipe\dodstudio-analyzer-test-{}", std::process::id());
        let server_name = name.clone();
        std::thread::spawn(move || serve(&server_name));
        let dir = std::env::temp_dir().join(format!("analysis_server_pipe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let demo = dir.join("broken.dem");
        std::fs::write(&demo, b"not a demo at all").unwrap();

        let mut client = None;
        for _ in 0..50 {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&name)
            {
                Ok(pipe) => {
                    client = Some(pipe);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        let mut client = client.expect("the server never opened its pipe");
        client
            .write_all(format!("analyze {}\n", demo.display()).as_bytes())
            .unwrap();
        let mut answer = String::new();
        client.read_to_string(&mut answer).unwrap();
        assert!(
            answer.lines().last().unwrap().starts_with("failed "),
            "{answer:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
