//! Says *where* the game died, in the DLL's own log.
//!
//! GoldSrc installs its own unhandled-exception filter and exits quietly, so a
//! crash inside `client.dll` leaves nothing behind: no Windows Error Reporting
//! record, no Application event-log entry, no dump. That makes a crash while
//! testing a hook almost unfalsifiable — "it crashed" is the whole of the
//! evidence, and the interesting question (whose code, at what offset) is the
//! part that is missing.
//!
//! A *vectored* handler runs before any frame-based one, so it sees the fault
//! first-chance — before the engine's filter can swallow it. It only reads and
//! logs: it always returns `EXCEPTION_CONTINUE_SEARCH`, so the process behaves
//! exactly as it would have. This is a recorder, not a recovery mechanism.
//!
//! Addresses are resolved to `module+RVA` through `VirtualQuery`, because a raw
//! address is worthless after the fact — modules land wherever the loader put
//! them that session, and an RVA is what a disassembler can be pointed at.
//!
//! ## Breakpoints get their own, quieter line
//!
//! A breakpoint exception (`0x80000003`, an `int3`) is usually harmless: the
//! engine raises them in normal running, and something always handles them.
//! But an unhandled one ends the game with that exit code and no other trace.
//! That happened once on 2026-10-05: an Anniversary launch died 2 s in, and
//! only Steam's log had the exit code. So each *distinct* breakpoint address is
//! logged once, as a `BREAKPOINT:` line (not `CRASH:`, so
//! `tools/crash_report.py` doesn't count it as a crash), for the first
//! [`MAX_BREAKPOINT_SITES`] addresses. A harmless one that repeats costs one
//! line; if the game then exits with `0x80000003`, the last `BREAKPOINT:` line
//! says where.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameA;
use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, VirtualQuery};

/// Log and get out of the way: never claim to have handled anything.
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

/// A fault can repeat every frame. Cap the noise rather than fill the disk.
const MAX_REPORTS: u32 = 8;
static REPORTS: AtomicU32 = AtomicU32::new(0);

/// `EXCEPTION_BREAKPOINT`: an `int3`.
const BREAKPOINT: u32 = 0x8000_0003;

/// How many distinct breakpoint addresses get a line each.
const MAX_BREAKPOINT_SITES: usize = 8;
static BREAKPOINT_SITES: [AtomicUsize; MAX_BREAKPOINT_SITES] =
    [const { AtomicUsize::new(0) }; MAX_BREAKPOINT_SITES];

/// Reading the faulting thread's stack can itself fault, which would re-enter
/// this handler. One flag is enough: the handler runs on the faulting thread.
static INSIDE: AtomicBool = AtomicBool::new(false);

/// How far up the stack to look for return addresses.
#[cfg(target_arch = "x86")]
const STACK_DWORDS: usize = 192;
/// How many resolved frames are worth printing.
#[cfg(target_arch = "x86")]
const MAX_FRAMES: usize = 12;

/// Installs the recorder. Called once, from the worker thread's startup.
pub fn install() {
    // Safety: `first = 1` puts us at the head of the vectored chain, which is
    // the point — the engine's own filter must not get there first.
    let handle = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    let how = if handle.is_null() {
        "failed -- a crash will leave no record"
    } else {
        "installed"
    };
    unsafe { crate::debug::report(&format!("crash recorder: {how}")) };
}

/// Only genuinely fatal codes. First-chance C++ exceptions (`0xe06d7363`), the
/// thread-naming exception (`0x406d1388`) and debugger breakpoints all occur
/// during normal engine operation; logging those would bury the one line that
/// matters under thousands that do not.
fn describe(code: u32) -> Option<&'static str> {
    Some(match code {
        0xc000_0005 => "access violation",
        0xc000_001d => "illegal instruction",
        0xc000_0025 => "noncontinuable exception",
        0xc000_0026 => "invalid disposition",
        0xc000_008c => "array bounds exceeded",
        0xc000_0090 => "float invalid operation",
        0xc000_0091 => "float divide by zero",
        0xc000_0093 => "float stack check",
        0xc000_0094 => "integer divide by zero",
        0xc000_0095 => "integer overflow",
        0xc000_0096 => "privileged instruction",
        0xc000_00fd => "stack overflow",
        0x8000_0002 => "datatype misalignment",
        _ => return None,
    })
}

/// `(module file name, RVA)` for an address, or `None` if it is not in a
/// mapped image. `AllocationBase` is the module's load address, so the
/// difference is exactly the RVA a disassembler wants.
fn locate(address: usize) -> Option<(String, usize)> {
    if address == 0 {
        return None;
    }
    // Safety: writes a plain struct we own; an unmapped address returns 0
    // rather than faulting.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let written = unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut info,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 || info.State != MEM_COMMIT {
        return None;
    }
    let base = info.AllocationBase as usize;
    if base == 0 {
        return None;
    }
    let mut name = [0u8; 260];
    // Safety: the `AllocationBase` of an image mapping is a valid module
    // handle; for a non-image mapping this returns 0 and we fall through.
    let len =
        unsafe { GetModuleFileNameA(base as *mut c_void, name.as_mut_ptr(), name.len() as u32) };
    if len == 0 {
        return None;
    }
    let path = String::from_utf8_lossy(&name[..len as usize]).into_owned();
    let file = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string();
    Some((file, address - base))
}

/// `module+0x1234 (0xdeadbeef)`, or the bare address when it belongs to no
/// module.
fn describe_address(address: usize) -> String {
    match locate(address) {
        Some((module, rva)) => format!("{module}+{rva:#x} ({address:#x})"),
        None => format!("{address:#x} (no module)"),
    }
}

/// Every dword on the stack that points into a mapped module, in stack order.
///
/// Not a real unwind: GoldSrc and `client.dll` ship no unwind data, and frame
/// pointers are omitted in places, so a correct walk is not available. A scan
/// over-reports — stale return addresses and coincidental values both show up
/// — but it reliably *contains* the real call chain, which is what answers
/// "who called the thing that died".
#[cfg(target_arch = "x86")]
fn stack_trail(esp: usize) -> Vec<String> {
    let mut frames = Vec::new();
    if esp == 0 || !readable(esp, STACK_DWORDS * 4) {
        return frames;
    }
    for slot in 0..STACK_DWORDS {
        // Safety: `readable` confirmed the whole span is committed.
        let value = unsafe { std::ptr::read_unaligned((esp + slot * 4) as *const u32) } as usize;
        if let Some((module, rva)) = locate(value) {
            frames.push(format!("[esp+{:#05x}] {module}+{rva:#x}", slot * 4));
            if frames.len() == MAX_FRAMES {
                break;
            }
        }
    }
    frames
}

/// Whether `len` bytes at `address` are committed, so reading them cannot
/// fault. Checked before the stack scan rather than leaving the re-entrancy
/// flag to catch the fallout, and used by [`crate::deathmsg`] to sanity-check
/// a pointer the game is about to dereference without checking it itself.
pub(crate) fn readable(address: usize, len: usize) -> bool {
    // Safety: as in `locate` — writes a struct we own, never faults.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let written = unsafe {
        VirtualQuery(
            address as *const c_void,
            &mut info,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 || info.State != MEM_COMMIT {
        return false;
    }
    let end = info.BaseAddress as usize + info.RegionSize;
    address + len <= end
}

/// The registers worth having: what the faulting instruction was working with.
///
/// `x86`-only because the field names are architecture-specific and the
/// workspace is linted on the 64-bit host even though this DLL only ever ships
/// 32-bit.
#[cfg(target_arch = "x86")]
fn registers(info: *mut EXCEPTION_POINTERS) -> (String, usize) {
    // Safety: the caller checked `info`; the OS hands us a valid context.
    let context = unsafe { (*info).ContextRecord };
    if context.is_null() {
        return (String::new(), 0);
    }
    let c = unsafe { &*context };
    (
        format!(
            "eip={:#010x} eax={:#010x} ebx={:#010x} ecx={:#010x} edx={:#010x} \
             esi={:#010x} edi={:#010x} esp={:#010x} ebp={:#010x}",
            c.Eip, c.Eax, c.Ebx, c.Ecx, c.Edx, c.Esi, c.Edi, c.Esp, c.Ebp
        ),
        c.Esp as usize,
    )
}

#[cfg(not(target_arch = "x86"))]
fn registers(_info: *mut EXCEPTION_POINTERS) -> (String, usize) {
    (String::new(), 0)
}

#[cfg(not(target_arch = "x86"))]
fn stack_trail(_esp: usize) -> Vec<String> {
    Vec::new()
}

/// Runs on the faulting thread, first-chance, before the engine's own filter.
unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    if info.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let record = unsafe { (*info).ExceptionRecord };
    if record.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let code = unsafe { (*record).ExceptionCode } as u32;
    if code == BREAKPOINT {
        let address = unsafe { (*record).ExceptionAddress } as usize;
        if first_sighting(&BREAKPOINT_SITES, address) && !INSIDE.swap(true, Ordering::Acquire) {
            unsafe {
                crate::debug::report(&format!(
                    "BREAKPOINT: int3 ({BREAKPOINT:#010x}) at {} -- usually harmless; if the game \
                     exits right after with code 0x80000003, this is where it died",
                    describe_address(address)
                ))
            };
            record_context("BREAKPOINT", info);
            INSIDE.store(false, Ordering::Release);
        }
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let Some(what) = describe(code) else {
        return EXCEPTION_CONTINUE_SEARCH;
    };
    if REPORTS.fetch_add(1, Ordering::Relaxed) >= MAX_REPORTS {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    if INSIDE.swap(true, Ordering::Acquire) {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let address = unsafe { (*record).ExceptionAddress } as usize;
    let mut line = format!(
        "CRASH: {what} ({code:#010x}) at {}",
        describe_address(address)
    );

    // For an access violation the two parameters are the operation and the
    // address it was aimed at — usually the whole story on its own.
    if code == 0xc000_0005 && unsafe { (*record).NumberParameters } >= 2 {
        let params = unsafe { (*record).ExceptionInformation };
        let operation = match params[0] {
            0 => "reading",
            1 => "writing",
            8 => "executing (DEP)",
            _ => "accessing",
        };
        line.push_str(&format!(" -- {operation} {:#x}", params[1]));
    }
    unsafe { crate::debug::report(&line) };
    record_context("CRASH", info);

    INSIDE.store(false, Ordering::Release);
    EXCEPTION_CONTINUE_SEARCH
}

/// The registers and the return addresses on the stack, each line under
/// `prefix` and indented, as `tools/crash_report.py` reads a `CRASH:` block.
fn record_context(prefix: &str, info: *mut EXCEPTION_POINTERS) {
    let (register_dump, esp) = registers(info);
    if !register_dump.is_empty() {
        unsafe { crate::debug::report(&format!("{prefix}:   {register_dump}")) };
    }
    for frame in stack_trail(esp) {
        unsafe { crate::debug::report(&format!("{prefix}:   {frame}")) };
    }
}

/// Whether `address` is new to `seen`, which it then holds; false once every
/// slot holds another address. Lock-free: a slot is claimed by swapping 0 for
/// the address, so two threads hitting the same new address log it once.
fn first_sighting(seen: &[AtomicUsize], address: usize) -> bool {
    if address == 0 {
        return false;
    }
    for slot in seen {
        match slot.compare_exchange(0, address, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(held) if held == address => return false,
            Err(_) => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_breakpoint_address_is_logged_once_up_to_the_cap() {
        let seen: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
        assert!(first_sighting(&seen, 0x1000));
        assert!(!first_sighting(&seen, 0x1000), "a repeat is quiet");
        assert!(first_sighting(&seen, 0x2000));
        assert!(first_sighting(&seen, 0x3000));
        assert!(!first_sighting(&seen, 0x4000), "past the cap, nothing new");
        assert!(!first_sighting(&seen, 0x2000));
    }

    #[test]
    fn a_null_address_is_never_logged() {
        let seen: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
        assert!(!first_sighting(&seen, 0));
        assert!(first_sighting(&seen, 0x10));
    }

    #[test]
    fn breakpoints_stay_out_of_the_crash_list() {
        assert_eq!(describe(BREAKPOINT), None);
    }
}
