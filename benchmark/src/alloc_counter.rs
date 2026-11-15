//! Counting global allocator, so the profiler can say how many heap blocks a
//! phase makes and how high the heap peaks, not just how long it takes (#37:
//! the decode and the drop of the decoded tree were both suspected of being
//! allocation-bound, and only a count can confirm it).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call forwards to `System` unchanged; the counters are only
// bookkeeping beside it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            grow(new_size);
        }
        p
    }
}

fn grow(size: usize) {
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

/// Allocations made so far (reallocations count as one each).
pub fn allocations() -> usize {
    ALLOCS.load(Ordering::Relaxed)
}

/// Bytes live on the heap right now.
pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

/// Waits until the heap stops shrinking: `Analysis::try_from_bytes` frees the
/// decoded tree on a thread of its own (#511), and a phase measured while that
/// is still running reads a low peak and pays for the free's CPU.
pub fn settle() {
    let mut last = live_bytes();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let now = live_bytes();
        if now >= last {
            return;
        }
        last = now;
    }
}

/// Restarts the peak from what is live now, so the next reading is one phase's.
pub fn reset_peak() {
    PEAK.store(live_bytes(), Ordering::Relaxed);
}

/// Highest live heap since the last `reset_peak`.
pub fn peak_bytes() -> usize {
    PEAK.load(Ordering::Relaxed)
}
