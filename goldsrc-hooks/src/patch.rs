//! Writing to a loaded module's memory, and putting the page back.
//!
//! Lifted out of `deathmsg` when `detour` needed the same three steps. Issue
//! #204 had recorded `client.dll` behaving as though it were hardened against
//! exactly this; a live session settled that it is not.
//!
//! Every write this DLL makes to a page it doesn't own goes through here,
//! code and pointer slots alike, because two things go wrong around a
//! protect-write-restore when another writer is on the same page (#1059):
//!
//! - **Two of our own threads.** One thread's restore lands between the
//!   other's protect and its write, and the write faults on a read-only page.
//!   That crashed the game at start-up: the start-up watch hooking `hw.dll`'s
//!   `USER32` imports while the worker hooked its `KERNEL32` ones, all on one
//!   import-table page. [`WRITE_LOCK`] takes our writers one at a time.
//! - **Someone else's thread.** HLAE patches import tables too, with its own
//!   protect-write-restore, and we can't take a lock with it. So a write that
//!   faults because the page went read-only under it is made writable again
//!   and re-run, by a vectored handler that only acts on the span the writing
//!   thread has declared ([`rearm_on_fault`]). Release builds abort on a
//!   panic, but that is about Rust panics: an access violation is a Windows
//!   exception, and a vectored handler can resume the faulting instruction.
//!
//! Afterwards, code pages get their protection back ([`After::Restore`]), as
//! the game shipped them. Import-table slots don't ([`After::LeaveWritable`]):
//! HLAE hooks imports on those same pages, and putting a page back to
//! read-only while its writer is between its own protect and write is what
//! crashed us; we can't see whether it is, so we never make the page stricter
//! than writable.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, TryLockError};

use windows_sys::Win32::Foundation::EXCEPTION_ACCESS_VIOLATION;
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::Memory::{
    PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, PAGE_READONLY,
    PAGE_READWRITE, VirtualProtect,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

/// Held across every protect-write-restore, so two of our threads never
/// interleave on one page. Held for two `VirtualProtect` calls and a copy;
/// only start-up threads (the worker, the start-up watch) write off the
/// engine's thread, so the per-frame callers find it free.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// The span the thread holding [`WRITE_LOCK`] is writing, `[start, end)`, and
/// that thread's id; zero when nobody is writing. Read by the fault handler.
static PENDING_START: AtomicUsize = AtomicUsize::new(0);
static PENDING_END: AtomicUsize = AtomicUsize::new(0);
static PENDING_THREAD: AtomicU32 = AtomicU32::new(0);
/// How many times the fault handler made the pending span writable again
/// during the current write.
static REARMS: AtomicU32 = AtomicU32::new(0);
/// A write that keeps faulting after this many re-arms is let through to the
/// crash it would have been, rather than hang the game. Generous on purpose:
/// a re-arm costs a few tens of microseconds, and a writer flipping the page
/// in a tight loop (the unit test's) beat 64 in a row about once in eight
/// runs on 32-bit, where exception dispatch is slower. A real writer's burst
/// (HLAE hooking a few dozen imports) ends long before 4096.
const MAX_REARMS: u32 = 4096;
/// Re-arms since the DLL loaded, so a test can tell the handler really ran.
static REARMS_EVER: AtomicU32 = AtomicU32::new(0);

static HANDLER: Once = Once::new();

const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
/// `ExceptionInformation[0]` of an access violation that was a write.
const WRITE_FAULT: usize = 1;

/// What happens to the page's protection once the write is done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    /// Put back what was found: code and data the game shipped protected.
    Restore,
    /// Leave it writable (only as strict as [`writable_equivalent`]), for
    /// pages another module's writer may be inside -- see the module doc.
    LeaveWritable,
}

/// Repoints one vftable slot to `ours`, saving what it held into `original`
/// **before** the slot changes. The other order leaves a window where the
/// slot already points at our thunk but `original` still reads 0, and a
/// call landing in that window (any thread) would jump to address 0.
///
/// Safety: `vftable_slot` must be the address of a slot in a loaded module's
/// vftable, and `ours` a function with that slot's signature.
// Its callers are the 32-bit hook modules; a host build has none.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub unsafe fn swap_vftable_slot(
    vftable_slot: usize,
    ours: usize,
    original: &AtomicUsize,
) -> Result<(), String> {
    let old = unsafe { *(vftable_slot as *const usize) };
    original.store(old, Ordering::Release);
    if unsafe {
        write_pointer(
            vftable_slot as *mut *mut c_void,
            ours as *mut c_void,
            After::Restore,
        )
    } {
        Ok(())
    } else {
        Err(format!(
            "could not make the vftable at {vftable_slot:#x} writable"
        ))
    }
}

/// Overwrites `bytes.len()` bytes at `address`, restoring the original page
/// protection afterwards. `false` means nothing was written.
///
/// Safety: `address` must be a valid, mapped span of at least `bytes.len()`
/// bytes, and the caller must know that overwriting it is meaningful -- this
/// does no checking of its own.
pub unsafe fn write_code_bytes(address: usize, bytes: &[u8]) -> bool {
    unsafe {
        write_span(address, bytes.len(), After::Restore, || {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
        })
    }
}

/// Points the function-pointer slot at `slot` to `value` with one interlocked
/// store, so a thread calling through the slot meanwhile sees the old pointer
/// or the new one, never half of each. `false` means nothing was written.
///
/// Safety: `slot` must be a mapped, pointer-aligned slot whose callers accept
/// `value` (a function with the slot's signature).
pub unsafe fn write_pointer(slot: *mut *mut c_void, value: *mut c_void, after: After) -> bool {
    if !(slot as usize).is_multiple_of(align_of::<usize>()) {
        return false;
    }
    unsafe {
        write_span(slot as usize, size_of::<usize>(), after, || {
            AtomicUsize::from_ptr(slot as *mut usize).store(value as usize, Ordering::SeqCst);
        })
    }
}

/// The protect-write-restore every writer shares: under [`WRITE_LOCK`], with
/// the span declared for [`rearm_on_fault`] while `write` runs.
unsafe fn write_span(address: usize, len: usize, after: After, write: impl FnOnce()) -> bool {
    HANDLER.call_once(|| {
        // First in the chain, so a fault we can mend never reaches the crash
        // recorder or the game's own filter (crash.rs also skips it, for the
        // case its handler went in after this one).
        unsafe { AddVectoredExceptionHandler(1, Some(rearm_on_fault)) };
    });
    let (turn, waited) = match WRITE_LOCK.try_lock() {
        Ok(turn) => (turn, false),
        Err(TryLockError::WouldBlock) => (
            WRITE_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            true,
        ),
        Err(TryLockError::Poisoned(poisoned)) => (poisoned.into_inner(), false),
    };

    let target = address as *mut c_void;
    let mut old: PAGE_PROTECTION_FLAGS = 0;
    if unsafe { VirtualProtect(target, len, PAGE_EXECUTE_READWRITE, &mut old) } == 0 {
        return false;
    }

    REARMS.store(0, Ordering::Relaxed);
    PENDING_START.store(address, Ordering::Relaxed);
    PENDING_END.store(address.saturating_add(len), Ordering::Relaxed);
    PENDING_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    write();
    PENDING_THREAD.store(0, Ordering::SeqCst);
    let rearms = REARMS.load(Ordering::Relaxed);

    let put_back = match after {
        After::Restore => old,
        After::LeaveWritable => writable_equivalent(old),
    };
    if put_back != PAGE_EXECUTE_READWRITE {
        let mut discard: PAGE_PROTECTION_FLAGS = 0;
        unsafe { VirtualProtect(target, len, put_back, &mut discard) };
    }
    // Before the log write, so a writer waiting on the frame path never waits
    // on the disk too.
    drop(turn);

    #[cfg(not(test))]
    if waited || rearms > 0 {
        unsafe {
            crate::debug::report(&format!(
                "patch: writing {len} byte(s) at {address:#x}: {}{}",
                if waited {
                    "waited for another of our threads' writes; "
                } else {
                    ""
                },
                if rearms > 0 {
                    format!(
                        "the page went read-only mid-write (another writer's restore), made it writable again {rearms} time(s)"
                    )
                } else {
                    "no other writer in the way".to_string()
                }
            ))
        };
    }
    #[cfg(test)]
    let _ = (waited, rearms);
    true
}

/// The least protection that still lets the page be written: `old` itself
/// when it already does, else its read-write twin, keeping any modifier bits
/// (guard, no-cache) as they were.
pub fn writable_equivalent(old: PAGE_PROTECTION_FLAGS) -> PAGE_PROTECTION_FLAGS {
    let modifiers = old & !0xff;
    let writable = match old & 0xff {
        PAGE_READONLY => PAGE_READWRITE,
        PAGE_EXECUTE | PAGE_EXECUTE_READ => PAGE_EXECUTE_READWRITE,
        // Already writable (read-write, write-copy and their execute twins),
        // or no-access, which we never wrote through to begin with.
        _ => return old,
    };
    writable | modifiers
}

/// Whether a write fault at `fault`, on thread `thread`, is inside the span
/// that thread declared it is writing.
fn is_pending_write(fault: usize, thread: u32, start: usize, end: usize, owner: u32) -> bool {
    owner != 0 && owner == thread && fault >= start && fault < end
}

/// Whether this exception is a write fault on the span our writing thread
/// declared, and [`rearm_on_fault`] hasn't given up on that write: one it
/// mends rather than a crash. For the crash recorder, so it stays quiet about
/// it, and still records the one we give up on.
///
/// Safety: `info` is the handler's argument, as Windows passed it.
pub unsafe fn is_mendable_fault(info: *const EXCEPTION_POINTERS) -> bool {
    if info.is_null() {
        return false;
    }
    let record = unsafe { (*info).ExceptionRecord };
    if record.is_null()
        || unsafe { (*record).ExceptionCode } != EXCEPTION_ACCESS_VIOLATION
        || unsafe { (*record).NumberParameters } < 2
    {
        return false;
    }
    let params = unsafe { (*record).ExceptionInformation };
    params[0] == WRITE_FAULT
        && REARMS.load(Ordering::Relaxed) < MAX_REARMS
        && is_pending_write(
            params[1],
            unsafe { GetCurrentThreadId() },
            PENDING_START.load(Ordering::Relaxed),
            PENDING_END.load(Ordering::Relaxed),
            PENDING_THREAD.load(Ordering::SeqCst),
        )
}

/// Makes the pending span writable again when another writer's restore made
/// it read-only between our protect and our write, and resumes the write.
/// Anything else passes through untouched.
unsafe extern "system" fn rearm_on_fault(info: *mut EXCEPTION_POINTERS) -> i32 {
    if !unsafe { is_mendable_fault(info) } {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    REARMS.fetch_add(1, Ordering::Relaxed);
    REARMS_EVER.fetch_add(1, Ordering::Relaxed);
    let start = PENDING_START.load(Ordering::Relaxed);
    let len = PENDING_END.load(Ordering::Relaxed) - start;
    let mut discard: PAGE_PROTECTION_FLAGS = 0;
    if unsafe {
        VirtualProtect(
            start as *mut c_void,
            len,
            PAGE_EXECUTE_READWRITE,
            &mut discard,
        )
    } == 0
    {
        // Given up: the crash recorder, if it runs after us, records it.
        REARMS.store(MAX_REARMS, Ordering::Relaxed);
        return EXCEPTION_CONTINUE_SEARCH;
    }
    EXCEPTION_CONTINUE_EXECUTION
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_WRITECOPY,
        PAGE_GUARD, PAGE_NOACCESS, PAGE_WRITECOPY, VirtualAlloc, VirtualFree, VirtualQuery,
    };

    /// One page of our own, at `protect`, freed on drop.
    struct Page(usize);

    impl Page {
        fn new(protect: PAGE_PROTECTION_FLAGS) -> Self {
            let at = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    4096,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            } as usize;
            assert_ne!(at, 0);
            let mut old = 0;
            assert_ne!(
                unsafe { VirtualProtect(at as *mut c_void, 4096, protect, &mut old) },
                0
            );
            Self(at)
        }

        fn protection(&self) -> PAGE_PROTECTION_FLAGS {
            let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
            unsafe {
                VirtualQuery(
                    self.0 as *const c_void,
                    &mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            info.Protect
        }

        fn read(&self, offset: usize) -> usize {
            unsafe { std::ptr::read_volatile((self.0 + offset) as *const usize) }
        }
    }

    impl Drop for Page {
        fn drop(&mut self) {
            unsafe { VirtualFree(self.0 as *mut c_void, 0, MEM_RELEASE) };
        }
    }

    #[test]
    fn only_read_only_pages_get_a_writable_twin() {
        assert_eq!(writable_equivalent(PAGE_READONLY), PAGE_READWRITE);
        assert_eq!(
            writable_equivalent(PAGE_EXECUTE_READ),
            PAGE_EXECUTE_READWRITE
        );
        assert_eq!(writable_equivalent(PAGE_EXECUTE), PAGE_EXECUTE_READWRITE);
        for already in [
            PAGE_READWRITE,
            PAGE_WRITECOPY,
            PAGE_EXECUTE_READWRITE,
            PAGE_EXECUTE_WRITECOPY,
            PAGE_NOACCESS,
        ] {
            assert_eq!(writable_equivalent(already), already);
        }
        assert_eq!(
            writable_equivalent(PAGE_READONLY | PAGE_GUARD),
            PAGE_READWRITE | PAGE_GUARD
        );
    }

    #[test]
    fn a_pending_write_is_only_the_writers_own_span() {
        assert!(is_pending_write(0x1004, 7, 0x1004, 0x1008, 7));
        assert!(is_pending_write(0x1007, 7, 0x1004, 0x1008, 7));
        assert!(!is_pending_write(0x1008, 7, 0x1004, 0x1008, 7));
        assert!(!is_pending_write(0x1003, 7, 0x1004, 0x1008, 7));
        // Another thread faulting on the same bytes is not ours to mend.
        assert!(!is_pending_write(0x1004, 8, 0x1004, 0x1008, 7));
        // Nobody writing.
        assert!(!is_pending_write(0x1004, 0, 0x1004, 0x1008, 0));
    }

    #[test]
    fn code_pages_get_their_protection_back() {
        let page = Page::new(PAGE_EXECUTE_READ);
        assert!(unsafe { write_code_bytes(page.0 + 16, &0x1234_5678usize.to_ne_bytes()) });
        assert_eq!(page.read(16), 0x1234_5678);
        assert_eq!(page.protection(), PAGE_EXECUTE_READ);
    }

    #[test]
    fn import_slots_stay_writable_but_no_more() {
        let page = Page::new(PAGE_READONLY);
        let slot = (page.0 + 8) as *mut *mut c_void;
        assert!(unsafe {
            write_pointer(slot, 0xabcd_0000usize as *mut c_void, After::LeaveWritable)
        });
        assert_eq!(page.read(8), 0xabcd_0000);
        assert_eq!(page.protection(), PAGE_READWRITE);
    }

    #[test]
    fn a_misaligned_slot_is_refused() {
        let page = Page::new(PAGE_READWRITE);
        let slot = (page.0 + 2) as *mut *mut c_void;
        assert!(!unsafe { write_pointer(slot, 0x1000 as *mut c_void, After::Restore) });
    }

    /// #1059 as it happens, with the other writer outside our lock: a thread
    /// that keeps unprotecting and restoring the page, the way HLAE's import
    /// hooks do. Without the re-arm a write would land on a read-only page
    /// and the test process would die of an access violation.
    #[test]
    fn a_writer_outside_the_lock_cannot_make_ours_fault() {
        let page = Page::new(PAGE_READONLY);
        let at = page.0;
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    let mut old = 0;
                    unsafe {
                        VirtualProtect(at as *mut c_void, 4, PAGE_READWRITE, &mut old);
                        VirtualProtect(at as *mut c_void, 4, PAGE_READONLY, &mut old);
                    }
                    std::thread::yield_now();
                }
            });
            for i in 1..=20_000usize {
                let slot = (at + 64) as *mut *mut c_void;
                assert!(unsafe { write_pointer(slot, i as *mut c_void, After::Restore) });
                assert_eq!(page.read(64), i);
                assert!(unsafe { write_code_bytes(at + 128, &i.to_ne_bytes()) });
                assert_eq!(page.read(128), i);
            }
            stop.store(true, Ordering::Relaxed);
        });
        // Only this test makes a page go read-only mid-write.
        assert!(
            REARMS_EVER.load(Ordering::Relaxed) > 0,
            "the race never fired"
        );
    }

    /// Our own writers, two threads on one page, the way the start-up watch
    /// and the worker met: each one's write lands, and the page ends up as it
    /// started.
    #[test]
    fn two_of_our_writers_on_one_page_both_land() {
        let page = Page::new(PAGE_READONLY);
        let at = page.0;
        std::thread::scope(|scope| {
            for lane in 0..2usize {
                scope.spawn(move || {
                    for i in 1..=20_000usize {
                        let slot = (at + 256 + lane * 8) as *mut *mut c_void;
                        assert!(unsafe { write_pointer(slot, i as *mut c_void, After::Restore) });
                    }
                });
            }
        });
        assert_eq!(page.read(256), 20_000);
        assert_eq!(page.read(264), 20_000);
        assert_eq!(page.protection(), PAGE_READONLY);
    }
}
