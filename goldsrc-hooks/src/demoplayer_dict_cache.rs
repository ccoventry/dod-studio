//! Stops `DemoPlayer.dll` reading through a stale lookup cache after one of
//! its dictionaries grows: the 25th Anniversary crash at `DemoPlayer.dll+0x48e0`
//! on a second `viewdemo` of an HLTV demo (part of #1392).
//!
//! ## The bug
//!
//! The demo player keeps its director events in an `ObjectDictionary`: a
//! sorted array of `{object, key}` pairs, plus a 32-entry cache of recent
//! lookups that holds *pointers into that array*:
//!
//! ```text
//! this+0x0c   entries      the array
//! this+0x10   cache[32]    {entry pointer, key}, 8 bytes each
//! this+0x110  cache index  where the next lookup is cached
//! this+0x114  size
//! this+0x118  capacity
//! ```
//!
//! `AddToDictionary` (pre-Anniversary `+0x6bb0`, 25th Anniversary `+0x4860`)
//! first calls `CheckSize` (`+0x6f50` / `+0x4a40`) when the array is full.
//! `CheckSize` allocates a bigger array, copies, frees the old one, and stores
//! the new pointer, but leaves the cache alone. Then, for an event earlier
//! than the last one, `AddToDictionary` asks `FindClosestAsIndex` (`+0x6c80` /
//! `+0x4c20`) where it goes. That scans the cache first, and on a key match
//! returns `(cached pointer - entries) / 8`, refusing only a negative result.
//! A pointer into the freed array gives an index that is garbage: when it is
//! past the end, the insert's shift loop walks down from the end towards it
//! through memory until it faults, at `+0x48e0` on the Anniversary build.
//!
//! Its sign is the sign of "old array minus new array", which is wherever the
//! heap put each, so the same bug is usually harmless (negative: a normal
//! binary search) and sometimes a crash. A garbage index that lands inside the
//! array instead inserts the event out of order. HLTV demos carry many
//! director events and the crash needs one at a key just looked up, so it has
//! only been seen there, on a second load (2 of 2 on `scrim_1787106047-CHI1`).
//! The engine itself clears the cache at the end of every insert; it misses
//! only the grow in between.
//!
//! ## The fix: clear the cache when the array moves
//!
//! One detour per build, on `CheckSize`'s two stores of the new array and its
//! capacity, which run only when it has reallocated. The stub makes the same
//! two stores, then zeroes the cache and its index, as the pre-Anniversary
//! build's own `ClearCache` (`+0x6d80`) does:
//!
//! ```asm
//!         mov  [this+0xc], <new entries>     ; the stolen bytes
//!         mov  [this+0x118], <new capacity>
//!         lea  edx, [this+0x10]
//!         xor  eax, eax
//!         mov  ecx, 0x41                      ; cache and index: 0x104 bytes
//! .zero:  mov  [edx], eax
//!         add  edx, 4
//!         dec  ecx
//!         jnz  .zero
//!         lock inc dword ptr [CLEARS]
//!         jmp  dword ptr [RESUME]             ; the pops and `mov al, 1; ret`
//! ```
//!
//! An empty cache entry (null pointer) is one the lookup skips, so a cleared
//! cache sends every lookup to the binary search over the new array. Only
//! `eax`, `ecx` and `edx` change: the code after the span pops the saved
//! registers and returns `al = 1` without reading any of them, and all three
//! are a caller's to lose under `thiscall`.
//!
//! It is on by default, like `hull_trace_guard`, because it changes nothing
//! but a cache the engine would have had to rebuild anyway.
//! `GOLDSRC_HOOKS_DEMOPLAYER_CACHE_FIX=0` turns it off for a session.
//!
//! ## When it installs
//!
//! `DemoPlayer.dll` loads at the first `viewdemo`. [`poll`] looks it up each
//! frame, as `fast_load` does for `Core.dll`, and the `viewdemo` wrapper
//! (`demo_reload`) asks again straight after the command, so the patch is in
//! before the demo's first events are read. A `DemoPlayer.dll` loaded again
//! gets it again.
//!
//! ## Both engines
//!
//! The pre-Anniversary (138 KB) and 25th Anniversary (48 KB) `DemoPlayer.dll`
//! are different compiles of the same code: the same object layout, the same
//! missing clear. They keep `this` in different registers (`ebx`, `edi`), so
//! each build has its own signature, stolen bytes and register ([`BUILDS`]).
//! `tools/verify_demoplayer_dict_cache_offsets.py` checks every fact above
//! against each DLL.

// The detour and the module lookup are only built for the 32-bit DLL; a host
// build compiles the pure parts for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// The x86 register numbers a build may keep `this` in.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reg {
    Ebx = 3,
    Edi = 7,
}

/// One `DemoPlayer.dll` build's `ObjectDictionary::CheckSize`.
struct Build {
    name: &'static str,
    /// The free of the old array, the two stores, and the epilogue's start.
    /// Unique in that build's `DemoPlayer.dll`, and matching nothing in the
    /// other's.
    pattern: &'static str,
    /// Where the stolen bytes start within `pattern`.
    site: usize,
    /// `mov [this+0xc], <entries>; mov [this+0x118], <capacity>`.
    stolen: [u8; STOLEN_LEN],
    /// The register holding the dictionary.
    this: Reg,
}

/// Both builds steal the same two instructions: a 3-byte and a 6-byte store.
const STOLEN_LEN: usize = 9;
const _: () = assert!(STOLEN_LEN >= 5, "a jmp rel32 needs 5 bytes");

/// The builds the fix knows, tried in order.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        // push [ebx+0xc]; call free; mov edx, [esp+0x18]; add esp, 4;
        // the stores; pop edi; pop esi; pop ebp; mov al, 1
        pattern: "8B 4B 0C 51 E8 ?? ?? ?? ?? 8B 54 24 18 83 C4 04 \
                  89 53 0C 89 AB 18 01 00 00 5F 5E 5D B0 01",
        site: 16,
        stolen: [0x89, 0x53, 0x0c, 0x89, 0xab, 0x18, 0x01, 0x00, 0x00],
        this: Reg::Ebx,
    },
    Build {
        name: "25th Anniversary",
        // push [edi+0xc]; call [free]; add esp, 0x10; the stores;
        // pop edi; pop esi; mov al, 1
        pattern: "FF 77 0C FF 15 ?? ?? ?? ?? 83 C4 10 \
                  89 5F 0C 89 B7 18 01 00 00 5F 5E B0 01",
        site: 12,
        stolen: [0x89, 0x5f, 0x0c, 0x89, 0xb7, 0x18, 0x01, 0x00, 0x00],
        this: Reg::Edi,
    },
];

/// Where the cache starts in the dictionary.
const CACHE_OFFSET: u8 = 0x10;
/// Dwords the stub zeroes from [`CACHE_OFFSET`]: 32 entries of 8 bytes, then
/// the cache index at `+0x110`. Stops short of the size at `+0x114`.
const CLEAR_DWORDS: u32 = 0x41;

/// Whether to install at all -- `GOLDSRC_HOOKS_DEMOPLAYER_CACHE_FIX=0` turns
/// it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// How many times a dictionary's array moved and the stub cleared its cache.
static CLEARS: AtomicU32 = AtomicU32::new(0);

/// Where the stub jumps back to: the instruction after the stolen bytes.
static RESUME: AtomicUsize = AtomicUsize::new(0);
/// The `DemoPlayer.dll` the patch is in, and where its jump is; 0 when it
/// isn't.
static PATCHED_BASE: AtomicUsize = AtomicUsize::new(0);
static PATCHED_AT: AtomicUsize = AtomicUsize::new(0);
/// The `DemoPlayer.dll` the last failed install was on, so a failure is
/// logged once.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);
/// Set once the "off" line is logged.
static REPORTED_OFF: AtomicBool = AtomicBool::new(false);
static BUILD_NAME: Mutex<&str> = Mutex::new("");

/// The stub, hand-assembled; see the module doc for the listing.
fn stub(build: &Build, clears: usize, resume_slot: usize) -> Vec<u8> {
    const ZERO: usize = STOLEN_LEN + 10;
    let mut code = build.stolen.to_vec();
    // lea edx, [this+disp8]: ModRM mod 01, reg edx (2), rm this.
    code.extend_from_slice(&[0x8d, 0x50 | build.this as u8, CACHE_OFFSET]);
    code.extend_from_slice(&[0x31, 0xc0]); // xor eax, eax
    code.push(0xb9); // mov ecx, imm32
    code.extend_from_slice(&CLEAR_DWORDS.to_le_bytes());
    debug_assert_eq!(code.len(), ZERO);
    code.extend_from_slice(&[0x89, 0x02]); // mov [edx], eax
    code.extend_from_slice(&[0x83, 0xc2, 0x04]); // add edx, 4
    code.push(0x49); // dec ecx
    let back = ZERO as isize - (code.len() + 2) as isize;
    code.extend_from_slice(&[0x75, back as u8]); // jnz .zero
    code.extend_from_slice(&[0xf0, 0xff, 0x05]); // lock inc dword ptr [abs32]
    code.extend_from_slice(&(clears as u32).to_le_bytes());
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume_slot as u32).to_le_bytes());
    code
}

/// Finds `CheckSize` with the first of [`BUILDS`] whose signature matches,
/// and patches it. Returns the build's name, the patched address and the
/// stub's.
fn install_at(base: usize) -> Result<(&'static str, usize, usize), String> {
    let mut misses = Vec::new();
    for build in &BUILDS {
        // Safety: `base` is a module handle the loader gave us.
        let found = match unsafe { crate::scan::find_unique(base, build.pattern) } {
            Ok(found) => found,
            Err(why) => {
                misses.push(format!("{} signature: {why}", build.name));
                continue;
            }
        };
        let target = found + build.site;
        // Safety: inside the match, which is longer than site + stolen.
        let present = unsafe { std::slice::from_raw_parts(target as *const u8, STOLEN_LEN) };
        if present != build.stolen {
            return Err(format!(
                "expected {:02x?} at +{:#x}, found {present:02x?}",
                build.stolen,
                target - base
            ));
        }
        RESUME.store(target + STOLEN_LEN, Ordering::Release);
        let code = stub(build, CLEARS.as_ptr() as usize, RESUME.as_ptr() as usize);
        // Safety: the span was checked byte for byte above, and nothing
        // branches into it -- checked by
        // tools/verify_demoplayer_dict_cache_offsets.py.
        let detour = unsafe { crate::detour::install(target, STOLEN_LEN, &code) }?;
        // Never undone (see `detour::Detour`): a `DemoPlayer.dll` unloaded
        // under it takes the jump with it, and the stub page stays,
        // unreachable.
        return Ok((build.name, target, detour.stub_address()));
    }
    Err(misses.join("; "))
}

#[cfg(target_arch = "x86")]
fn demoplayer_dll() -> usize {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
    // Safety: a plain lookup of a loaded module by name.
    unsafe { GetModuleHandleA(c"DemoPlayer.dll".as_ptr() as *const u8) as usize }
}

#[cfg(not(target_arch = "x86"))]
fn demoplayer_dll() -> usize {
    0
}

/// Installs the patch when `DemoPlayer.dll` is loaded and doesn't have it.
/// Runs every frame from `commands::poll` and after each `viewdemo`: a module
/// lookup, and a one-byte read once the patch is in.
pub fn keep_installed() {
    if !ENABLED.load(Ordering::Relaxed) {
        if !REPORTED_OFF.swap(true, Ordering::Relaxed) {
            unsafe {
                crate::debug::report(
                    "demoplayer_dict_cache: off (GOLDSRC_HOOKS_DEMOPLAYER_CACHE_FIX=0) -- a second viewdemo of an HLTV demo can crash the 25th Anniversary build (#1392)",
                )
            };
        }
        return;
    }
    let base = demoplayer_dll();
    if base == 0 {
        PATCHED_BASE.store(0, Ordering::Relaxed);
        return;
    }
    if PATCHED_BASE.load(Ordering::Relaxed) == base {
        let at = PATCHED_AT.load(Ordering::Relaxed);
        // Safety: `at` is inside the module just found loaded at `base`.
        if unsafe { *(at as *const u8) } == 0xe9 {
            return;
        }
    }
    if FAILED_BASE.load(Ordering::Relaxed) == base {
        return;
    }
    match install_at(base) {
        Ok((build, at, stub)) => {
            PATCHED_BASE.store(base, Ordering::Relaxed);
            PATCHED_AT.store(at, Ordering::Relaxed);
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = build;
            }
            unsafe {
                crate::debug::report(&format!(
                    "demoplayer_dict_cache: installed ({build} DemoPlayer.dll, ObjectDictionary::CheckSize clears its lookup cache at +{:#x}, stub {stub:#x}) (#1392)",
                    at - base
                ))
            };
        }
        Err(why) => {
            FAILED_BASE.store(base, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "demoplayer_dict_cache: not installed -- {why}; a second viewdemo of an HLTV demo can still crash the 25th Anniversary build (#1392)"
                ))
            };
        }
    }
}

/// The `ds_debug_status` line, or `None` while `DemoPlayer.dll` isn't
/// patched.
pub fn status_line() -> Option<String> {
    if PATCHED_BASE.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let build = BUILD_NAME.lock().map(|name| *name).unwrap_or_default();
    Some(format!(
        "demo player cache fix: on ({build} DemoPlayer.dll), cache cleared after {} array moves",
        CLEARS.load(Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_module_doc_claims() {
        #[rustfmt::skip]
        let pre = vec![
            0x89, 0x53, 0x0c,                         //  0 mov  [ebx+0xc], edx
            0x89, 0xab, 0x18, 0x01, 0x00, 0x00,       //  3 mov  [ebx+0x118], ebp
            0x8d, 0x53, 0x10,                         //  9 lea  edx, [ebx+0x10]
            0x31, 0xc0,                               // 12 xor  eax, eax
            0xb9, 0x41, 0x00, 0x00, 0x00,             // 14 mov  ecx, 0x41
            0x89, 0x02,                               // 19 .zero: mov [edx], eax
            0x83, 0xc2, 0x04,                         // 21 add  edx, 4
            0x49,                                     // 24 dec  ecx
            0x75, 0xf8,                               // 25 jnz  .zero (19)
            0xf0, 0xff, 0x05, 0x22, 0x22, 0x22, 0x22, // 27 lock inc [CLEARS]
            0xff, 0x25, 0x11, 0x11, 0x11, 0x11,       // 34 jmp  dword [RESUME]
        ];
        assert_eq!(stub(&BUILDS[0], 0x2222_2222, 0x1111_1111), pre);
    }

    /// Each build's stub is the same code but for its stolen bytes and the
    /// register in the `lea`.
    #[test]
    fn the_anniversary_stub_differs_only_in_its_stores_and_register() {
        let pre = stub(&BUILDS[0], 0x2222_2222, 0x1111_1111);
        let post = stub(&BUILDS[1], 0x2222_2222, 0x1111_1111);
        assert_eq!(pre.len(), post.len());
        assert_eq!(&post[..STOLEN_LEN], &BUILDS[1].stolen);
        assert_eq!(post[10], 0x57, "lea edx, [edi+0x10]");
        let differ: Vec<usize> = (STOLEN_LEN..pre.len())
            .filter(|&i| pre[i] != post[i])
            .collect();
        assert_eq!(differ, [10]);
    }

    #[test]
    fn each_build_steals_what_its_pattern_holds() {
        for build in &BUILDS {
            let bytes: Vec<Option<u8>> = build
                .pattern
                .split_whitespace()
                .map(|t| u8::from_str_radix(t, 16).ok())
                .collect();
            let stolen: Vec<Option<u8>> = build.stolen.iter().copied().map(Some).collect();
            assert_eq!(
                &bytes[build.site..build.site + STOLEN_LEN],
                &stolen[..],
                "{}",
                build.name
            );
            assert!(crate::scan::Pattern::parse(build.pattern).is_ok());
        }
    }

    /// The stolen stores are `mov [this+0xc], r` and `mov [this+0x118], r`
    /// through the register the build names, so the `lea` reads the same
    /// dictionary they write.
    #[test]
    fn the_stores_go_through_the_builds_register() {
        for build in &BUILDS {
            let rm = |modrm: u8| modrm & 7;
            assert_eq!(build.stolen[0], 0x89, "{}", build.name);
            assert_eq!(build.stolen[1] >> 6, 0b01, "disp8: {}", build.name);
            assert_eq!(rm(build.stolen[1]), build.this as u8, "{}", build.name);
            assert_eq!(build.stolen[2], 0x0c, "{}", build.name);
            assert_eq!(build.stolen[3], 0x89, "{}", build.name);
            assert_eq!(build.stolen[4] >> 6, 0b10, "disp32: {}", build.name);
            assert_eq!(rm(build.stolen[4]), build.this as u8, "{}", build.name);
            assert_eq!(
                &build.stolen[5..],
                &[0x18, 0x01, 0x00, 0x00],
                "{}",
                build.name
            );
        }
    }

    /// The zeroed span is the 32 cache entries and the cache index, and ends
    /// where the dictionary's size begins.
    #[test]
    fn the_clear_covers_the_cache_and_its_index_and_stops_at_the_size() {
        const ENTRIES: u32 = 32;
        const ENTRY_BYTES: u32 = 8;
        const INDEX_AT: u32 = 0x110;
        const SIZE_AT: u32 = 0x114;
        let start = u32::from(CACHE_OFFSET);
        assert_eq!(start + ENTRIES * ENTRY_BYTES, INDEX_AT);
        assert_eq!(start + CLEAR_DWORDS * 4, SIZE_AT);
    }
}
