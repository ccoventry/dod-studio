//! Grows wasm memory in large steps instead of one allocation at a time.
//!
//! Rust's wasm32 allocator (dlmalloc) asks for more memory with
//! `memory.grow` sized to the one allocation it cannot fit, rounded up to a
//! 64 KB page. Parsing a demo builds a frame tree 5-13x the demo's size, so
//! a first analysis made thousands of grows, and in V8 each grow costs more
//! as the memory gets bigger: a 46 MB demo took 6 s instead of 1 s, a 200 MB
//! one 70-300 s instead of 4 s. The same demo a second time was fast, because
//! dlmalloc reuses memory it already has.
//!
//! This wrapper watches for dlmalloc growing memory itself. When it does, the
//! wrapper allocates one big block and frees it at once: dlmalloc grows
//! memory once for the block and keeps it as free space for the allocations
//! that follow. Memory never shrinks in wasm, so the cost is the slack above
//! the peak, at most one step.

use std::alloc::{GlobalAlloc, Layout, System};

const PAGE: usize = 64 * 1024;
/// Smallest step: below this the per-grow cost doesn't matter.
const MIN_STEP: usize = 16 << 20;
/// Largest step, so a big demo's slack above its peak stays bounded.
const MAX_STEP: usize = 256 << 20;

pub struct GrowInSteps;

fn memory_bytes() -> usize {
    core::arch::wasm32::memory_size::<0>() * PAGE
}

/// Gives dlmalloc a step of free space: half the memory's size, clamped.
///
/// A failed grow (the browser's memory limit) is not an error: allocations
/// then grow memory themselves, as they would without this wrapper.
fn grow_one_step() {
    let mut step = (memory_bytes() / 2).clamp(MIN_STEP, MAX_STEP);
    while step >= MIN_STEP {
        if let Ok(layout) = Layout::from_size_align(step, 16) {
            // SAFETY: non-zero size; the block is freed straight away with
            // the same layout, and nothing reads it.
            unsafe {
                let p = System.alloc(layout);
                if !p.is_null() {
                    System.dealloc(p, layout);
                    return;
                }
            }
        }
        step /= 2;
    }
}

/// Runs `f` (one call into `System`) and adds a step if it grew memory.
fn then_stay_ahead(f: impl FnOnce() -> *mut u8) -> *mut u8 {
    let before = memory_bytes();
    let p = f();
    if memory_bytes() != before {
        grow_one_step();
    }
    p
}

// SAFETY: every call is forwarded to `System` with the caller's arguments;
// `grow_one_step` only adds an alloc/free pair of its own.
unsafe impl GlobalAlloc for GrowInSteps {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        then_stay_ahead(|| unsafe { System.alloc(layout) })
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        then_stay_ahead(|| unsafe { System.alloc_zeroed(layout) })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        then_stay_ahead(|| unsafe { System.realloc(ptr, layout, new_size) })
    }
}
