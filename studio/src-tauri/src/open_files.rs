//! Files Windows hands DoD Studio to open (#528): a `.dodstudio` project
//! double-clicked in Explorer, or a `.dem` opened through Open With.
//!
//! Windows starts `dod-studio.exe "<path>"`. The path is read from the command
//! line at startup and kept in [`PendingOpenFiles`] until the window asks for
//! it (`take_open_files`), so the page opens it with the same code as File →
//! Load Project, once its own startup is done.
//!
//! A second double-click while Studio runs starts a second copy. That copy
//! hands its paths to the running one over a named pipe and exits. The pipe is
//! named after this exe's path, so an installed Studio and a dev build never
//! take each other's files, and a launch with no file to open still opens a
//! window of its own, as before.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Emitted to the page when the running copy is handed files: it then calls
/// `take_open_files`.
pub const OPEN_FILES_EVENT: &str = "open_files_pending";

/// Extensions Studio opens from the command line: projects (`.dodstudio`, and
/// `.json` for projects saved before #528) and demos.
const OPENABLE_EXTENSIONS: [&str; 3] = ["dodstudio", "json", "dem"];

/// The longest request read from the pipe: many paths, with room to spare.
#[cfg(windows)]
const MAX_REQUEST: u64 = 256 * 1024;

/// Files waiting for the page to open them, oldest first.
#[derive(Default)]
pub struct PendingOpenFiles(RwLock<Vec<String>>);

impl PendingOpenFiles {
    pub fn new(paths: &[PathBuf]) -> Self {
        let pending = Self::default();
        pending.push(paths);
        pending
    }

    pub fn push(&self, paths: &[PathBuf]) {
        let Ok(mut list) = self.0.write() else {
            return;
        };
        // A path that isn't valid UTF-16 can't reach the page as a string.
        list.extend(paths.iter().filter_map(|p| p.to_str()).map(String::from));
    }

    pub fn take(&self) -> Vec<String> {
        self.0
            .write()
            .map(|mut list| std::mem::take(&mut *list))
            .unwrap_or_default()
    }
}

/// The files to open out of a command line: every argument after the exe that
/// isn't a flag and has an openable extension, made absolute against `cwd`.
pub fn openable_paths<I: IntoIterator<Item = OsString>>(args: I, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .skip(1)
        .filter(|arg| !arg.to_string_lossy().starts_with('-'))
        .map(PathBuf::from)
        .filter(|path| is_openable(path))
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .collect()
}

fn is_openable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        OPENABLE_EXTENSIONS
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

#[tauri::command]
pub fn take_open_files(pending: tauri::State<'_, PendingOpenFiles>) -> Vec<String> {
    pending.take()
}

/// The pipe the copy of Studio at `exe` serves: one per exe path and user.
pub fn pipe_name(exe: &Path, user: &str) -> String {
    let key = format!("{}|{}", exe.to_string_lossy().to_lowercase(), user);
    format!(r"\\.\pipe\dodstudio-open-{:016x}", fnv1a(key.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// This exe's pipe, or `None` when the exe path can't be read.
#[cfg(windows)]
pub fn own_pipe_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(pipe_name(
        &exe,
        &std::env::var("USERNAME").unwrap_or_default(),
    ))
}

/// What a second copy sends: one `open <path>` line per file.
fn request_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| format!("open {}\n", p.display()))
        .collect()
}

/// The files a request names: absolute paths with an openable extension.
/// Anything else on a line is ignored.
fn parse_request(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| line.trim_end_matches('\r').strip_prefix("open "))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && is_openable(path))
        .collect()
}

/// Hands `paths` to the copy of Studio serving `pipe`. False when none is
/// running (or it couldn't be reached): this copy then opens them itself.
#[cfg(windows)]
pub fn forward_to_running(pipe: &str, paths: &[PathBuf]) -> bool {
    use std::io::Write;
    // The running copy makes a new pipe instance right after each request;
    // a second request in that gap finds them all busy.
    for _ in 0..20 {
        match std::fs::OpenOptions::new().write(true).open(pipe) {
            Ok(mut client) => {
                // Lets the running copy bring its window to the front.
                // Explorer started this process, so this one may set the
                // foreground window and the running copy may not.
                // Safety: no pointers; ASFW_ANY names no particular process.
                unsafe { AllowSetForegroundWindow(ASFW_ANY) };
                return client
                    .write_all(request_text(paths).as_bytes())
                    .and_then(|()| client.flush())
                    .is_ok();
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
    false
}

/// Serves `pipe` on a thread of its own, calling `on_files` with each
/// request's files. Another copy of this exe already serving it keeps it.
#[cfg(windows)]
pub fn serve<F>(pipe: String, on_files: F)
where
    F: Fn(Vec<PathBuf>) + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("open-files-server".into())
        .spawn(move || serve_loop(&pipe, on_files));
}

#[cfg(windows)]
fn serve_loop<F: Fn(Vec<PathBuf>)>(name: &str, on_files: F) {
    use std::io::Read;
    use std::os::windows::io::FromRawHandle;
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut first = true;
    loop {
        let flags = PIPE_ACCESS_INBOUND
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // Safety: a NUL-terminated name, no security attributes (the default
        // DACL: only this user and the system can write to it).
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                0,
                64 * 1024,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            if first {
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
        let mut bytes = Vec::new();
        // The client closes its end once it has written; that ends the read.
        let _ = pipe.take(MAX_REQUEST).read_to_end(&mut bytes);
        let files = parse_request(&String::from_utf8_lossy(&bytes));
        if !files.is_empty() {
            on_files(files);
        }
    }
}

#[cfg(windows)]
const PIPE_ACCESS_INBOUND: u32 = 0x1;
#[cfg(windows)]
const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
#[cfg(windows)]
const PIPE_TYPE_BYTE: u32 = 0;
#[cfg(windows)]
const PIPE_READMODE_BYTE: u32 = 0;
#[cfg(windows)]
const PIPE_WAIT: u32 = 0;
#[cfg(windows)]
const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x8;
#[cfg(windows)]
const PIPE_UNLIMITED_INSTANCES: u32 = 255;
#[cfg(windows)]
const ERROR_PIPE_CONNECTED: u32 = 535;
#[cfg(windows)]
const ERROR_PIPE_BUSY: i32 = 231;
#[cfg(windows)]
const ASFW_ANY: u32 = u32::MAX;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;

#[cfg(windows)]
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
        security: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn ConnectNamedPipe(pipe: *mut std::ffi::c_void, overlapped: *mut std::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn AllowSetForegroundWindow(process_id: u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_double_clicked_project_is_opened() {
        let cwd = Path::new(r"C:\Work");
        assert_eq!(
            openable_paths(
                args(&[r"C:\Apps\dod-studio.exe", r"D:\Movies\WSOD25.dodstudio"]),
                cwd
            ),
            [PathBuf::from(r"D:\Movies\WSOD25.dodstudio")]
        );
    }

    #[test]
    fn only_projects_and_demos_are_opened() {
        let cwd = Path::new(r"C:\Work");
        let got = openable_paths(
            args(&[
                "dod-studio.exe",
                "--some-flag",
                r"D:\a.DODSTUDIO",
                r"D:\old.json",
                r"D:\match.dem",
                r"D:\notes.txt",
                r"D:\no_extension",
            ]),
            cwd,
        );
        assert_eq!(
            got,
            [
                PathBuf::from(r"D:\a.DODSTUDIO"),
                PathBuf::from(r"D:\old.json"),
                PathBuf::from(r"D:\match.dem"),
            ]
        );
    }

    #[test]
    fn the_exe_itself_and_a_bare_launch_open_nothing() {
        let cwd = Path::new(r"C:\Work");
        assert!(openable_paths(args(&[r"C:\Apps\x.dem"]), cwd).is_empty());
        assert!(openable_paths(args(&[]), cwd).is_empty());
    }

    #[test]
    fn a_relative_path_is_read_against_the_launch_folder() {
        let cwd = Path::new(r"C:\Work");
        assert_eq!(
            openable_paths(args(&["dod-studio.exe", r"projects\a.dodstudio"]), cwd),
            [PathBuf::from(r"C:\Work\projects\a.dodstudio")]
        );
    }

    #[test]
    fn a_request_carries_its_paths_and_nothing_else() {
        let paths = [
            PathBuf::from(r"D:\Movies\WSOD 25.dodstudio"),
            PathBuf::from(r"D:\demos\match.dem"),
        ];
        assert_eq!(parse_request(&request_text(&paths)), paths);
        assert!(parse_request("open relative.dem\nhello\r\nopen C:\\x.exe\n").is_empty());
        assert_eq!(
            parse_request("open D:\\a.dodstudio\r\n"),
            [PathBuf::from(r"D:\a.dodstudio")]
        );
    }

    #[test]
    fn pending_files_are_taken_once() {
        let pending = PendingOpenFiles::new(&[PathBuf::from(r"D:\a.dodstudio")]);
        pending.push(&[PathBuf::from(r"D:\b.dem")]);
        assert_eq!(pending.take(), [r"D:\a.dodstudio", r"D:\b.dem"]);
        assert!(pending.take().is_empty());
    }

    #[test]
    fn each_exe_and_user_has_its_own_pipe() {
        let installed = Path::new(r"C:\Users\x\AppData\Local\DoD Studio\dod-studio.exe");
        let dev = Path::new(r"D:\src\dod-studio\target\debug\dod-studio.exe");
        assert_eq!(
            pipe_name(installed, "x"),
            pipe_name(
                Path::new(r"c:\users\X\appdata\local\dod studio\DOD-STUDIO.EXE"),
                "x"
            )
        );
        assert_ne!(pipe_name(installed, "x"), pipe_name(dev, "x"));
        assert_ne!(pipe_name(installed, "x"), pipe_name(installed, "y"));
        assert!(pipe_name(installed, "x").starts_with(r"\\.\pipe\dodstudio-open-"));
    }

    /// A real round trip: a second copy's files reach the running one.
    #[cfg(windows)]
    #[test]
    fn a_second_copy_hands_its_files_to_the_running_one() {
        let pipe = pipe_name(
            Path::new(&format!("test-{}", std::process::id())),
            "open-files-test",
        );
        let (tx, rx) = std::sync::mpsc::channel();
        serve(pipe.clone(), move |files| {
            let _ = tx.send(files);
        });
        let files = [PathBuf::from(r"D:\Movies\a.dodstudio")];
        let mut sent = false;
        for _ in 0..50 {
            if forward_to_running(&pipe, &files) {
                sent = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(sent, "the server never opened its pipe");
        let got = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the server never passed the files on");
        assert_eq!(got, files);
        // Nothing serves this name: the copy opens its files itself.
        assert!(!forward_to_running(
            r"\\.\pipe\dodstudio-open-nobody-serves-this",
            &files
        ));
    }
}
