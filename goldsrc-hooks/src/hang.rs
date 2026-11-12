//! Says *where* the game is stuck when it stops drawing frames.
//!
//! A freeze leaves even less behind than a crash (`crash.rs`): Windows records
//! an AppHang event with no stack, and the player closes the game. #1392's
//! POV-rewind freeze left exactly that. This watches the engine's thread and,
//! when it has drawn no frame for [`DEFAULT_SECONDS`], writes where it is:
//!
//! - a `HANG:` block in the hook log, in the shape `tools/crash_report.py`
//!   reads: the registers, the return addresses up the `ebp` chain, then every
//!   stack dword that points into a module (the same scan `crash.rs` does,
//!   since GoldSrc omits frame pointers in places);
//! - a minidump in `<log folder>\dumps\` ([`dump_dir`]), the newest
//!   [`MAX_DUMPS`] kept, with every thread's stack and the modules' data
//!   sections, so a debugger can walk the stack properly and read
//!   `DemoPlayer.dll`'s globals.
//!
//! Then one `HANG-END:` line if frames come back, saying how long they were
//! gone. A hang reported with no `HANG-END:` after it is one the game never
//! came out of.
//!
//! ## Heartbeat and watchdog
//!
//! `commands::poll` calls [`beat`] on every frame `HUD_Frame` runs: one atomic
//! add, plus a load to see whether the thread is known yet. No lock, no
//! allocation. A thread of its own ([`watch`]) wakes every
//! [`WATCH_INTERVAL_MS`] and compares the count.
//!
//! ## Never in the way
//!
//! It only reads. The engine thread is suspended for the copy of its
//! registers and stack and resumed straight after; while it is suspended the
//! watchdog takes no lock and allocates nothing, because the suspended thread
//! may hold the heap's lock. Everything that allocates, logs or resolves a
//! module runs after the resume. The stack is read with `ReadProcessMemory`
//! over a range `VirtualQuery` says is committed, readable and not a guard
//! page, so a bad pointer is a failed read, never a fault. The minidump is
//! written after the resume too. Nothing here ends the game, and no setting
//! makes it.
//!
//! ## The threshold
//!
//! Loads stop frames for a while on purpose: `viewdemo` reading a demo, a map
//! load. [`DEFAULT_SECONDS`] sits above the longest of those measured on both
//! builds (see the constant). A load that does run past it gets a `HANG:`
//! block and a `HANG-END:` line a few seconds later, which `crash_report.py`
//! lists as a stall the game came out of. `ds_debug_hang_report_seconds`
//! changes the threshold for the session; `0` turns the watchdog off. While a
//! debugger is attached nothing is reported: a breakpoint stops frames too.
//! A modal message box on the engine thread stops them as well, and is
//! reported: HLAE's "you are about to connect" warning, which `map` raises,
//! showed up as `AfxHookGoldSrc.dll+0x9309` under `USER32.dll`'s dialog loop.
//! A window behind others still draws (about 45 frames a second, see
//! `run_in_background.rs`); a minimised window is not measured here.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::names::console_name;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("debug_hang_report_seconds");

/// Seconds without a frame before a hang is reported.
///
/// Above the longest normal stall measured: `viewdemo` loads froze the game
/// for up to 13 s in the #1392 work. With `fast_load` spreading the read over
/// frames, the longest single frame of eight loads of 110-430 MB demos was
/// 9.3 s (the session's first load, which loads the map; POST), the rest
/// 0.5-3.2 s, on both builds. 20 s leaves headroom for a slow disk without
/// waiting so long that the player has closed the game first.
pub const DEFAULT_SECONDS: f32 = 20.0;

/// The cvar's range. Under a second would report ordinary hitches.
const MIN_SECONDS: f32 = 1.0;
const MAX_SECONDS: f32 = 3600.0;

/// How often the watchdog looks. A hang is reported at most this late.
const WATCH_INTERVAL_MS: u64 = 500;

/// Bytes of the engine thread's stack copied, from `esp` up. 64 KB holds
/// every frame of the engine's own calls many times over.
const STACK_COPY_BYTES: usize = 64 * 1024;

/// Return addresses logged from the `ebp` chain, and from the stack scan.
const MAX_CHAIN_FRAMES: usize = 24;
const MAX_SCAN_FRAMES: usize = 24;
/// Dwords the scan looks through, from `esp` up (as `crash.rs`, but deeper:
/// a hang has time to be thorough).
const SCAN_DWORDS: usize = 1024;

/// Dumps kept in [`dump_dir`]; the oldest go first.
const MAX_DUMPS: usize = 5;
const DUMP_PREFIX: &str = "hang_";
const DUMP_SUFFIX: &str = ".dmp";

pub static CVAR: Cvar = Cvar::new();

/// Frames drawn, bumped by [`beat`].
static HEARTBEAT: AtomicU32 = AtomicU32::new(0);
/// The engine thread's id, from its first [`beat`]; 0 until then.
static MAIN_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
/// Whether the watchdog thread is running, for `ds_debug_status`.
static WATCHING: AtomicBool = AtomicBool::new(false);
/// Hangs reported, and how many of them the game came out of.
static HANGS: AtomicU32 = AtomicU32::new(0);
static RESUMED: AtomicU32 = AtomicU32::new(0);

/// One frame drawn. Called from `commands::poll` while `HUD_Frame` drives it,
/// so on the engine's thread, every frame.
#[inline]
pub fn beat() {
    if MAIN_THREAD_ID.load(Ordering::Relaxed) == 0 {
        // Safety: no arguments, cannot fail.
        let id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        MAIN_THREAD_ID.store(id, Ordering::Relaxed);
    }
    HEARTBEAT.fetch_add(1, Ordering::Relaxed);
}

/// Starts the watchdog thread, once. Called from `commands::install`.
pub fn start() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("ds_hang_watch".into())
        .spawn(watch);
    let how = match spawned {
        Ok(_) => "watching for frames that stop",
        Err(_) => "the watchdog thread did not start -- a freeze will leave no record",
    };
    unsafe { crate::debug::report(&format!("hang reporter: {how}")) };
}

/// `ds_debug_status`'s line.
pub fn status_line() -> Option<String> {
    if !WATCHING.load(Ordering::Relaxed) {
        return None;
    }
    let hangs = HANGS.load(Ordering::Relaxed);
    let resumed = RESUMED.load(Ordering::Relaxed);
    let when = match threshold(CVAR.value()) {
        Some(after) => format!("reports after {} s without a frame", after.as_secs_f32()),
        None => format!("off ({NAME} 0)"),
    };
    Some(format!(
        "hang reporter: {when}; {hangs} reported this session, {resumed} of them came back"
    ))
}

/// The threshold the cvar asks for, or `None` when it turns the watchdog off.
/// An unregistered cvar reads as the default.
fn threshold(cvar: Option<f32>) -> Option<Duration> {
    let seconds = cvar.unwrap_or(DEFAULT_SECONDS);
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f32(
        seconds.clamp(MIN_SECONDS, MAX_SECONDS),
    ))
}

/// What one look at the heartbeat found.
#[derive(Debug, PartialEq)]
enum Event {
    Nothing,
    /// No frame for this long: report it.
    Hung(Duration),
    /// Frames came back after a reported hang that lasted this long.
    Resumed(Duration),
}

/// The heartbeat's history, as the watchdog last saw it.
struct Watch {
    count: u32,
    /// When `count` last changed.
    since: Duration,
    /// Whether the current stall has been reported.
    reported: bool,
}

impl Watch {
    fn new(now: Duration) -> Self {
        Self {
            count: 0,
            since: now,
            reported: false,
        }
    }

    /// One look: `count` is the heartbeat, `now` the watchdog's clock.
    /// Reports a stall once, and its end once.
    fn step(&mut self, count: u32, now: Duration, threshold: Option<Duration>) -> Event {
        let stalled = now.saturating_sub(self.since);
        if count != self.count {
            self.count = count;
            self.since = now;
            if std::mem::take(&mut self.reported) {
                return Event::Resumed(stalled);
            }
            return Event::Nothing;
        }
        match threshold {
            Some(after) if !self.reported && stalled >= after => {
                self.reported = true;
                Event::Hung(stalled)
            }
            _ => Event::Nothing,
        }
    }
}

/// The watchdog thread's body. Never returns.
fn watch() {
    WATCHING.store(true, Ordering::Relaxed);
    let epoch = Instant::now();
    let mut state = Watch::new(Duration::ZERO);
    // Allocated once, here, where nothing is suspended.
    let mut stack = vec![0u8; STACK_COPY_BYTES];
    let dumper = Dumper::load();
    let mut thread = MainThread::default();
    loop {
        std::thread::sleep(Duration::from_millis(WATCH_INTERVAL_MS));
        // Nothing to watch until the engine thread has drawn a frame.
        let id = MAIN_THREAD_ID.load(Ordering::Relaxed);
        if id == 0 {
            continue;
        }
        let count = HEARTBEAT.load(Ordering::Relaxed);
        match state.step(count, epoch.elapsed(), threshold(CVAR.value())) {
            Event::Nothing => {}
            Event::Hung(stalled) => {
                // Safety: no arguments.
                if unsafe { windows_sys::Win32::System::Diagnostics::Debug::IsDebuggerPresent() }
                    != 0
                {
                    continue;
                }
                HANGS.fetch_add(1, Ordering::Relaxed);
                report_hang(id, stalled, thread.handle(id), &mut stack, dumper.as_ref());
            }
            Event::Resumed(stalled) => {
                RESUMED.fetch_add(1, Ordering::Relaxed);
                unsafe {
                    crate::debug::report(&format!(
                        "HANG-END: frames resumed after {:.1} s",
                        stalled.as_secs_f32()
                    ))
                };
            }
        }
    }
}

/// The `HANG:` block, then the dump.
fn report_hang(
    id: u32,
    stalled: Duration,
    handle: Option<Handle>,
    stack: &mut [u8],
    dumper: Option<&Dumper>,
) {
    let snapshot = handle.and_then(|h| capture(h, stack));
    let head = format!(
        "HANG: no frame for {:.1} s (engine thread {id})",
        stalled.as_secs_f32()
    );
    let Some(snapshot) = snapshot else {
        unsafe {
            crate::debug::report(&format!(
                "{head} -- its stack could not be read (thread not opened or not suspended)"
            ))
        };
        write_dump(dumper);
        return;
    };
    unsafe {
        crate::debug::report(&format!(
            "{head} at {}",
            crate::crash::describe_address(snapshot.eip)
        ))
    };
    unsafe { crate::debug::report(&format!("HANG:   {}", snapshot.registers)) };
    let copied = &stack[..snapshot.stack_len];
    for (n, address) in frame_chain(copied, snapshot.esp, snapshot.ebp, MAX_CHAIN_FRAMES)
        .into_iter()
        .enumerate()
    {
        let line = match crate::crash::locate(address) {
            Some((module, rva)) => format!("HANG:   [ebp#{n:02}] {module}+{rva:#x}"),
            None => format!("HANG:   [ebp#{n:02}] {address:#x} (no module)"),
        };
        unsafe { crate::debug::report(&line) };
    }
    let mut shown = 0;
    for (offset, value) in stack_dwords(copied, SCAN_DWORDS) {
        if let Some((module, rva)) = crate::crash::locate(value) {
            unsafe {
                crate::debug::report(&format!("HANG:   [esp+{offset:#05x}] {module}+{rva:#x}"))
            };
            shown += 1;
            if shown == MAX_SCAN_FRAMES {
                break;
            }
        }
    }
    write_dump(dumper);
}

/// Writes the minidump and logs where it went, or why it didn't.
fn write_dump(dumper: Option<&Dumper>) {
    let line = match dumper {
        None => "HANG:   no dump: dbghelp.dll's MiniDumpWriteDump did not load".to_string(),
        Some(dumper) => match dumper.write() {
            Ok((path, bytes)) => format!(
                "HANG:   dump {} ({:.1} MB)",
                path.display(),
                bytes as f64 / (1024.0 * 1024.0)
            ),
            Err(why) => format!("HANG:   no dump: {why}"),
        },
    };
    unsafe { crate::debug::report(&line) };
}

/// Where the dumps go: `dumps\` in the hook log's folder
/// (`%APPDATA%\dod-studio\logs\dumps\`, or under `DOD_STUDIO_LOG_DIR`), so
/// the log line that names a dump and the dump travel together when the
/// logs folder is zipped for a bug report.
fn dump_dir() -> Option<std::path::PathBuf> {
    crate::debug::log_dir().map(|dir| dir.join("dumps"))
}

/// `hang_YYYYMMDD_HHMMSS_<pid>.dmp`, so file order is time order.
fn dump_file_name(time: &windows_sys::Win32::Foundation::SYSTEMTIME, pid: u32) -> String {
    format!(
        "{DUMP_PREFIX}{:04}{:02}{:02}_{:02}{:02}{:02}_{pid}{DUMP_SUFFIX}",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
    )
}

/// Deletes all but the newest `keep` dumps of ours in `dir`.
fn prune_dumps(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dumps: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(DUMP_PREFIX) && n.ends_with(DUMP_SUFFIX))
        })
        .collect();
    dumps.sort();
    if dumps.len() > keep {
        for old in &dumps[..dumps.len() - keep] {
            // Best-effort cleanup of a dump this module wrote.
            let _ = std::fs::remove_file(old);
        }
    }
}

/// Return addresses up the saved-`ebp` chain, read from `stack`, a copy of
/// the stack starting at `esp`. Stops at the first frame pointer outside the
/// copy, misaligned, or not above the one before (a chain must climb).
fn frame_chain(stack: &[u8], esp: usize, ebp: usize, max: usize) -> Vec<usize> {
    let mut frames = Vec::new();
    let mut frame = ebp;
    while frames.len() < max {
        let Some(offset) = frame.checked_sub(esp) else {
            break;
        };
        if !frame.is_multiple_of(4) {
            break;
        }
        let (Some(next), Some(ret)) = (dword_at(stack, offset), dword_at(stack, offset + 4)) else {
            break;
        };
        if ret == 0 {
            break;
        }
        frames.push(ret);
        if next <= frame {
            break;
        }
        frame = next;
    }
    frames
}

/// `(offset from esp, value)` for the first `max` dwords of `stack`.
fn stack_dwords(stack: &[u8], max: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    stack
        .as_chunks::<4>()
        .0
        .iter()
        .take(max)
        .enumerate()
        .map(|(i, b)| (i * 4, u32::from_le_bytes(*b) as usize))
}

fn dword_at(stack: &[u8], offset: usize) -> Option<usize> {
    let bytes = stack.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
}

/// A thread handle the watchdog owns.
#[derive(Clone, Copy)]
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
struct Handle(windows_sys::Win32::Foundation::HANDLE);

/// The engine thread's handle, opened the first time it is needed and kept.
#[derive(Default)]
struct MainThread {
    id: u32,
    handle: Option<Handle>,
}

impl MainThread {
    fn handle(&mut self, id: u32) -> Option<Handle> {
        if self.id != id || self.handle.is_none() {
            use windows_sys::Win32::System::Threading::{
                OpenThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME,
            };
            // Safety: opens a handle by id; null on failure. Kept for the
            // session: the engine thread lives as long as the process.
            let handle = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                    0,
                    id,
                )
            };
            self.id = id;
            self.handle = (!handle.is_null()).then_some(Handle(handle));
        }
        self.handle
    }
}

/// What the suspended thread was doing.
struct Snapshot {
    eip: usize,
    esp: usize,
    ebp: usize,
    registers: String,
    /// Bytes copied into the caller's buffer, from `esp` up.
    stack_len: usize,
}

/// Resumes the thread when dropped, so no path out of [`capture`] leaves it
/// suspended.
#[cfg(target_arch = "x86")]
struct Suspended(Handle);

#[cfg(target_arch = "x86")]
impl Drop for Suspended {
    fn drop(&mut self) {
        // Safety: the handle was suspended by `capture` and has
        // THREAD_SUSPEND_RESUME.
        unsafe { windows_sys::Win32::System::Threading::ResumeThread(self.0.0) };
    }
}

/// Suspends the thread, copies its registers and stack into `stack`, and
/// resumes it. Takes no lock and allocates nothing while it is suspended.
#[cfg(target_arch = "x86")]
fn capture(thread: Handle, stack: &mut [u8]) -> Option<Snapshot> {
    use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, GetThreadContext};
    use windows_sys::Win32::System::Threading::SuspendThread;

    /// `CONTEXT_CONTROL | CONTEXT_INTEGER` for x86: eip, esp, ebp and the
    /// general registers.
    const CONTEXT_CONTROL_AND_INTEGER: u32 = 0x0001_0001 | 0x0001_0002;

    // Safety: suspending a thread we hold a handle to; u32::MAX is failure.
    if unsafe { SuspendThread(thread.0) } == u32::MAX {
        return None;
    }
    let suspended = Suspended(thread);
    // Safety: a plain struct we own, filled by the OS. GetThreadContext waits
    // for the suspension to take effect.
    let mut context: CONTEXT = unsafe { std::mem::zeroed() };
    context.ContextFlags = CONTEXT_CONTROL_AND_INTEGER;
    if unsafe { GetThreadContext(thread.0, &mut context) } == 0 {
        return None;
    }
    let esp = context.Esp as usize;
    let stack_len = copy_stack(esp, stack);
    drop(suspended);

    let c = &context;
    Some(Snapshot {
        eip: c.Eip as usize,
        esp,
        ebp: c.Ebp as usize,
        registers: format!(
            "eip={:#010x} eax={:#010x} ebx={:#010x} ecx={:#010x} edx={:#010x} \
             esi={:#010x} edi={:#010x} esp={:#010x} ebp={:#010x}",
            c.Eip, c.Eax, c.Ebx, c.Ecx, c.Edx, c.Esi, c.Edi, c.Esp, c.Ebp
        ),
        stack_len,
    })
}

/// The workspace is linted on the 64-bit host; the DLL only ships 32-bit.
#[cfg(not(target_arch = "x86"))]
fn capture(_thread: Handle, _stack: &mut [u8]) -> Option<Snapshot> {
    None
}

/// Copies the readable part of the stack from `esp` up into `out`; returns
/// how many bytes. No allocation: runs while the engine thread is suspended.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
fn copy_stack(esp: usize, out: &mut [u8]) -> usize {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    if esp == 0 {
        return 0;
    }
    // Safety: writes a struct we own; an unmapped address returns 0.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let written = unsafe {
        VirtualQuery(
            esp as *const c_void,
            &mut info,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0
        || info.State != MEM_COMMIT
        || info.Protect & (PAGE_GUARD | PAGE_NOACCESS) != 0
        || info.Protect == 0
    {
        return 0;
    }
    let region_end = (info.BaseAddress as usize).saturating_add(info.RegionSize);
    let len = region_end.saturating_sub(esp).min(out.len());
    let mut read = 0usize;
    // Safety: ReadProcessMemory fails rather than faults on a bad range, and
    // `out` holds at least `len` bytes.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            esp as *const c_void,
            out.as_mut_ptr().cast(),
            len,
            &mut read,
        )
    };
    if ok == 0 { 0 } else { read.min(len) }
}

/// `MiniDumpWriteDump`, from the system's `dbghelp.dll`, loaded at the
/// watchdog's start (while nothing is stuck) rather than imported, so the
/// DLL's own load never depends on it.
struct Dumper {
    write_dump: MiniDumpWriteDumpFn,
}

type MiniDumpWriteDumpFn = unsafe extern "system" fn(
    process: windows_sys::Win32::Foundation::HANDLE,
    process_id: u32,
    file: windows_sys::Win32::Foundation::HANDLE,
    dump_type: u32,
    exception: *const c_void,
    user_streams: *const c_void,
    callback: *const c_void,
) -> i32;

/// What the dump holds: every thread's stack and context (always), the
/// modules' data sections (`DemoPlayer.dll`'s and the engine's globals), the
/// memory the stacks point into, handles, and thread and unloaded-module
/// lists. Not the whole heap: that is hundreds of MB per dump.
const DUMP_TYPE: u32 = 0x0000_0001 // MiniDumpWithDataSegs
    | 0x0000_0004 // MiniDumpWithHandleData
    | 0x0000_0020 // MiniDumpWithUnloadedModules
    | 0x0000_0040 // MiniDumpWithIndirectlyReferencedMemory
    | 0x0000_0100 // MiniDumpWithProcessThreadData
    | 0x0000_1000; // MiniDumpWithThreadInfo

impl Dumper {
    fn load() -> Option<Self> {
        use windows_sys::Win32::System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExA,
        };
        // Safety: loads a system DLL by name; null on failure.
        let module = unsafe {
            LoadLibraryExA(
                c"dbghelp.dll".as_ptr().cast(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return None;
        }
        // Safety: looks up an export; None when it is missing.
        let proc = unsafe { GetProcAddress(module, c"MiniDumpWriteDump".as_ptr().cast()) }?;
        // Safety: MiniDumpWriteDump has this signature on every Windows.
        let write_dump: MiniDumpWriteDumpFn = unsafe { std::mem::transmute(proc) };
        Some(Self { write_dump })
    }

    /// Writes a dump of this process; returns its path and size.
    fn write(&self) -> Result<(std::path::PathBuf, u64), String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};

        let dir = dump_dir().ok_or("no log folder")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        prune_dumps(&dir, MAX_DUMPS.saturating_sub(1));
        // Safety: fills a plain struct we own; cannot fail.
        let mut now = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut now) };
        let pid = unsafe { GetCurrentProcessId() };
        let path = dir.join(dump_file_name(&now, pid));
        let file = std::fs::File::create(&path)
            .map_err(|e| format!("could not create {}: {e}", path.display()))?;
        // Safety: a valid process pseudo-handle and an open file handle; the
        // three optional pointers are null.
        let ok = unsafe {
            (self.write_dump)(
                GetCurrentProcess(),
                pid,
                file.as_raw_handle(),
                DUMP_TYPE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            drop(file);
            // Best-effort cleanup of the empty file this wrote.
            let _ = std::fs::remove_file(&path);
            return Err(format!("MiniDumpWriteDump failed: {error}"));
        }
        let bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok((path, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFTER: Option<Duration> = Some(Duration::from_secs(20));

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn a_stall_past_the_threshold_is_reported_once() {
        let mut watch = Watch::new(secs(0));
        assert_eq!(watch.step(1, secs(1), AFTER), Event::Nothing);
        assert_eq!(watch.step(1, secs(20), AFTER), Event::Nothing);
        assert_eq!(watch.step(1, secs(21), AFTER), Event::Hung(secs(20)));
        assert_eq!(watch.step(1, secs(40), AFTER), Event::Nothing);
        assert_eq!(watch.step(1, secs(90), AFTER), Event::Nothing);
    }

    #[test]
    fn frames_coming_back_after_a_report_say_how_long_they_were_gone() {
        let mut watch = Watch::new(secs(0));
        watch.step(1, secs(1), AFTER);
        watch.step(1, secs(30), AFTER);
        assert_eq!(watch.step(2, secs(35), AFTER), Event::Resumed(secs(34)));
        assert_eq!(watch.step(3, secs(36), AFTER), Event::Nothing);
    }

    #[test]
    fn a_stall_under_the_threshold_is_quiet_when_it_ends() {
        let mut watch = Watch::new(secs(0));
        watch.step(1, secs(1), AFTER);
        assert_eq!(watch.step(1, secs(14), AFTER), Event::Nothing);
        assert_eq!(watch.step(2, secs(15), AFTER), Event::Nothing);
    }

    #[test]
    fn turned_off_nothing_is_reported() {
        let mut watch = Watch::new(secs(0));
        watch.step(1, secs(1), None);
        assert_eq!(watch.step(1, secs(500), None), Event::Nothing);
    }

    #[test]
    fn the_cvar_sets_the_threshold_and_zero_turns_it_off() {
        assert_eq!(threshold(None), Some(Duration::from_secs(20)));
        assert_eq!(threshold(Some(30.0)), Some(Duration::from_secs(30)));
        assert_eq!(threshold(Some(0.0)), None);
        assert_eq!(threshold(Some(-5.0)), None);
        assert_eq!(threshold(Some(f32::NAN)), None);
        assert_eq!(threshold(Some(0.2)), Some(Duration::from_secs(1)));
        assert_eq!(threshold(Some(1e9)), Some(Duration::from_secs(3600)));
    }

    /// A stack copy from `esp` = 0x1000 holding dwords.
    fn stack(dwords: &[u32]) -> Vec<u8> {
        dwords.iter().flat_map(|d| d.to_le_bytes()).collect()
    }

    #[test]
    fn the_ebp_chain_yields_each_return_address_in_turn() {
        // ebp=0x1008 -> [0x1008]=0x1010 (next), [0x100c]=0xAAAA (ret);
        // [0x1010]=0 (end of chain), [0x1014]=0xBBBB.
        let s = stack(&[0, 0, 0x1010, 0xAAAA, 0, 0xBBBB]);
        assert_eq!(frame_chain(&s, 0x1000, 0x1008, 10), vec![0xAAAA, 0xBBBB]);
    }

    #[test]
    fn a_frame_pointer_outside_the_copy_or_going_down_stops_the_chain() {
        let s = stack(&[0, 0, 0x1000, 0xAAAA]);
        assert_eq!(
            frame_chain(&s, 0x1000, 0x1008, 10),
            vec![0xAAAA],
            "a chain that points back down stops"
        );
        assert!(frame_chain(&s, 0x1000, 0x0ff0, 10).is_empty(), "below esp");
        assert!(
            frame_chain(&s, 0x1000, 0x2000, 10).is_empty(),
            "past the copy"
        );
        assert!(frame_chain(&s, 0x1000, 0x1006, 10).is_empty(), "misaligned");
        assert!(frame_chain(&[], 0x1000, 0x1000, 10).is_empty());
    }

    #[test]
    fn the_chain_stops_at_its_cap() {
        let s = stack(&[0x1008, 1, 0x1010, 2, 0x1018, 3, 0, 4]);
        assert_eq!(frame_chain(&s, 0x1000, 0x1000, 2), vec![1, 2]);
    }

    #[test]
    fn the_scan_reads_each_dword_with_its_offset() {
        let s = stack(&[7, 8, 9]);
        let got: Vec<_> = stack_dwords(&s, 2).collect();
        assert_eq!(got, vec![(0, 7), (4, 8)]);
    }

    #[test]
    fn dump_names_sort_by_time_and_only_ours_are_pruned() {
        let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        t.wYear = 2026;
        t.wMonth = 11;
        t.wDay = 12;
        t.wHour = 9;
        t.wMinute = 5;
        t.wSecond = 7;
        assert_eq!(dump_file_name(&t, 42), "hang_20261112_090507_42.dmp");

        let dir = std::env::temp_dir().join(format!("ds_hang_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "hang_20261110_000000_1.dmp",
            "hang_20261111_000000_1.dmp",
            "hang_20261112_000000_1.dmp",
            "other.dmp",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        prune_dumps(&dir, 2);
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                "hang_20261111_000000_1.dmp",
                "hang_20261112_000000_1.dmp",
                "other.dmp"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_name_starts_no_other() {
        assert_eq!(NAME, "ds_debug_hang_report_seconds");
    }
}
