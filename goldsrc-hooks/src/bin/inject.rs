//! Minimal manual DLL injector for testing goldsrc-hooks against a running
//! hl.exe, independent of any future DoD Studio capture-pipeline wiring.
//!
//! Usage:
//!
//! ```text
//! inject <pid> <path-to-dll>
//! inject --launch <path-to-hl.exe> <path-to-dll> [game arguments...]
//! ```
//!
//! Standard technique: allocate a small buffer in the target process for the
//! DLL path, write the path into it, then start a remote thread whose entry
//! point is kernel32!LoadLibraryA with that buffer as its argument -- the
//! same effect as the target process calling LoadLibraryA itself.
//!
//! ## `--launch`: the game without HLAE
//!
//! Injecting into a game that is already running is late: `client.dll` has
//! loaded and the engine has already resolved its entry points, so every hook
//! that goes in through them (`engine.rs`) is missed. `--launch` starts
//! `hl.exe` suspended, loads the DLL before the game's first instruction, then
//! lets it run, which is what HLAE does too. It is for grenade practice on a
//! local server (#594), where HLAE adds nothing but its connect warning on
//! `map`. Unless other arguments are given, the game gets `-game dod
//! -insecure -nomaster -windowed +sv_lan 1 +maxplayers 2`: VAC off for the
//! server you start ("VAC secure mode disabled." in the console), never
//! announced to Steam's server list ("Master server communication
//! disabled."), LAN only, and a second slot because DoD never spawns you with
//! `maxplayers 1`'s single-player rules. It also gets `SteamAppId=30` like
//! every DoD Studio launch, or Steam refuses it. It refuses the stock install (`steamapps\common\Half-Life`): only ever
//! a movie copy.

use std::env;
use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::ExitCode;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateRemoteThread, GetExitCodeThread, INFINITE, OpenProcess,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE, TerminateProcess, WaitForSingleObject,
};

/// Printed on every run, before anything else: this route skips the connect
/// warning HLAE shows in every DoD Studio launch (docs/vac_safety.md).
const VAC_WARNING: &str = "WARNING: only inject into a separate movie copy of Half-Life, never the one \
                           you play online with, and never join a server afterwards -- this DLL patches \
                           the game in memory, which is what VAC detects. See docs/vac_safety.md.";

const USAGE: &str = "usage: inject <pid> <path-to-dll>\n       inject --launch <path-to-hl.exe> <path-to-dll> [game arguments...]";

fn main() -> ExitCode {
    eprintln!("{VAC_WARNING}\n");
    let args: Vec<String> = env::args().collect();
    let result = match args.as_slice() {
        [_, flag, hl, dll, game_args @ ..] if flag == "--launch" => launch(hl, dll, game_args),
        [_, pid_arg, dll] => attach(pid_arg, dll),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

/// The DLL's absolute path, NUL-terminated for `LoadLibraryA`, and as text.
fn dll_path(dll_path: &str) -> Result<(Vec<u8>, String), String> {
    let abs = std::fs::canonicalize(dll_path)
        .map_err(|e| format!("could not resolve '{dll_path}': {e}"))?;
    // canonicalize() on Windows yields a \\?\-prefixed path; LoadLibraryA
    // handles that fine, but strip it for a friendlier printed path.
    let text = abs.to_string_lossy().replace("\\\\?\\", "");
    let mut c = text.as_bytes().to_vec();
    c.push(0);
    Ok((c, text))
}

fn attach(pid_arg: &str, dll: &str) -> Result<(), String> {
    let pid: u32 = pid_arg
        .parse()
        .map_err(|_| format!("'{pid_arg}' is not a valid process id"))?;
    let (dll_c, dll_text) = dll_path(dll)?;
    unsafe {
        let process: HANDLE = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        );
        if process.is_null() {
            return Err(format!(
                "OpenProcess({pid}) failed -- is that PID correct, and are you running as the same user (or elevated)?"
            ));
        }
        let result = inject(process, &dll_c);
        CloseHandle(process);
        result?;
    }
    println!("Injected {dll_text} into process {pid}.");
    println!("Check %APPDATA%\\dod-studio\\logs\\ for its own diagnostics.");
    Ok(())
}

fn launch(hl: &str, dll: &str, game_args: &[String]) -> Result<(), String> {
    let hl = Path::new(hl);
    if !hl.is_file() {
        return Err(format!("no hl.exe at {}", hl.display()));
    }
    let folder = hl.parent().unwrap_or(Path::new("."));
    let stock = folder
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("Half-Life"));
    if stock {
        return Err(format!(
            "{} is the stock Half-Life install, the one you play online with. Use a movie copy.",
            folder.display()
        ));
    }
    let (dll_c, dll_text) = dll_path(dll)?;
    let default_args = [
        "-game",
        "dod",
        "-insecure",
        "-nomaster",
        "-windowed",
        "+sv_lan",
        "1",
        "+maxplayers",
        "2",
    ]
    .map(String::from);
    let game_args = if game_args.is_empty() {
        &default_args[..]
    } else {
        game_args
    };

    let child = std::process::Command::new(hl)
        .args(game_args)
        .current_dir(folder)
        .env("SteamAppId", "30")
        .creation_flags(CREATE_SUSPENDED)
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", hl.display()))?;
    let process = child.as_raw_handle() as HANDLE;
    if let Err(why) = unsafe { inject(process, &dll_c) } {
        unsafe { TerminateProcess(process, 1) };
        return Err(why);
    }
    // The main thread is still suspended; resume the whole process.
    let resumed = unsafe { resume(process) };
    if !resumed {
        unsafe { TerminateProcess(process, 1) };
        return Err("could not resume the game after loading the DLL".to_string());
    }
    println!(
        "Started {} (pid {}) with {dll_text}.",
        hl.display(),
        child.id()
    );
    Ok(())
}

/// `NtResumeProcess`, which resumes every thread of a process: `Command`
/// keeps the main thread's handle to itself.
unsafe fn resume(process: HANDLE) -> bool {
    unsafe {
        let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr() as *const u8);
        if ntdll.is_null() {
            return false;
        }
        let Some(f) = GetProcAddress(ntdll, c"NtResumeProcess".as_ptr() as *const u8) else {
            return false;
        };
        let f: unsafe extern "system" fn(HANDLE) -> i32 = std::mem::transmute(f);
        f(process) >= 0
    }
}

/// Loads the DLL into `process` with a remote `LoadLibraryA` and waits for it.
unsafe fn inject(process: HANDLE, dll_c: &[u8]) -> Result<(), String> {
    unsafe {
        let remote_buf = VirtualAllocEx(
            process,
            std::ptr::null(),
            dll_c.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote_buf.is_null() {
            return Err("VirtualAllocEx failed".to_string());
        }

        let mut written = 0usize;
        let ok = WriteProcessMemory(
            process,
            remote_buf,
            dll_c.as_ptr() as *const c_void,
            dll_c.len(),
            &mut written,
        );
        if ok == 0 || written != dll_c.len() {
            return Err(format!(
                "WriteProcessMemory failed (wrote {written}/{} bytes)",
                dll_c.len()
            ));
        }

        let kernel32 = GetModuleHandleA(c"kernel32.dll".as_ptr() as *const u8);
        if kernel32.is_null() {
            return Err(
                "could not get a handle to our own kernel32.dll (should never happen)".to_string(),
            );
        }
        let Some(load_library_a) = GetProcAddress(kernel32, c"LoadLibraryA".as_ptr() as *const u8)
        else {
            return Err("could not resolve LoadLibraryA (should never happen)".to_string());
        };

        // kernel32.dll is loaded at the same address in every process on a
        // given Windows session (ASLR notwithstanding, it's still mapped
        // system-wide from the same base for a given boot), so this address,
        // taken from our own process, is valid to use as the remote thread's
        // start address in the target process too -- the standard technique.
        let load_library_a_thread_start: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(load_library_a);
        let thread = CreateRemoteThread(
            process,
            std::ptr::null(),
            0,
            Some(load_library_a_thread_start),
            remote_buf,
            0,
            std::ptr::null_mut(),
        );
        if thread.is_null() {
            return Err("CreateRemoteThread failed".to_string());
        }

        WaitForSingleObject(thread, INFINITE);
        // LoadLibraryA's return value, truncated to 32 bits: 0 means it failed.
        let mut loaded = 0u32;
        GetExitCodeThread(thread, &mut loaded);
        CloseHandle(thread);
        if loaded == 0 {
            return Err("the game could not load the DLL (LoadLibraryA returned 0)".to_string());
        }
    }
    Ok(())
}
