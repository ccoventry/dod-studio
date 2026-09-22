//! `dodstudio_clear_decals`: empty the engine's decal pool at runtime.
//!
//! ## What this replaces
//!
//! `r_decals` bounds a rotating index and **evicts nothing**, so lowering it
//! strands every decal sitting above the new limit rather than removing it.
//! That is why the pipeline's decal hygiene has to be baked into the demo: pin
//! the ring small at load time, then inject a full revolution's worth of
//! synthetic decals into the gap before each clip so the index walks past every
//! real one (`native/src/patch/decal_strip.rs`). It works, and it costs a
//! demo rewrite, a load-time-only constraint on `r_decals`, and a standing rule
//! that the cvar must never be injected mid-demo because an extra
//! `ConsoleCommand` frame shifts every later frame ordinal by +1.
//!
//! None of that is needed if the walls can simply be wiped on command.
//!
//! ## The engine already has every piece
//!
//! #290 named the safe route first -- "find what the engine does on level load
//! and reuse it" -- and it is there:
//!
//! ```text
//!   hw+0x49e80  R_DecalUnlink(decal_t *)   detaches one decal from its
//!                                          surface's list; a NULL psurface
//!                                          returns early, so a free slot is
//!                                          safe to pass
//!   hw+0x49da0  R_DecalInit()              memset the whole pool, gDecalCount
//!                                          = 0, and reset the 256-entry decal
//!                                          texture table
//!   hw+0x4a000  R_DecalRemoveAll<by flag>  the pattern this copies: for each
//!                                          slot, unlink then zero 28 bytes
//! ```
//!
//! **`R_DecalInit` alone would be a crash.** It wipes the pool without
//! unlinking, so every `msurface_t::pdecals` left pointing into it becomes a
//! chain of zeroed structures the renderer will still walk. It is only safe on
//! level load because the surfaces are being rebuilt anyway. What makes the
//! engine's own *remove* functions safe is that they unlink first, and that is
//! what is reproduced here -- over every slot rather than over the ones
//! matching a texture or a flag.
//!
//! Nothing is invented: the loop below is byte-for-byte what `hw+0x4a000`
//! does, with its `if (flags & mask)` dropped.
//!
//! ## Pre-Anniversary only, and loudly so
//!
//! The two signatures match the pre-Anniversary `hw.dll` exactly once each.
//! `R_DecalInit`'s also matches the 25th-Anniversary engine (at a different
//! address), but the remove-by-flag loop does **not** -- that build compiled it
//! differently, so `R_DecalUnlink` cannot be recovered from it this way. Since
//! dod-tools only ever launches the pre-Anniversary movies install
//! (`docs/` and the two-installs rule), this refuses rather than guesses.
//!
//! ## Everything is read out of the instruction stream
//!
//! No address here is written down. The pool's base and end, the decal size,
//! `gDecalCount` and `R_DecalUnlink` are all immediates or relative calls
//! inside the two matched functions, and they cross-check each other: both
//! signatures must agree on the pool base, and the span between base and end
//! must equal the `memset` length `R_DecalInit` passes.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The command name. Registered in `commands.rs`.
pub const NAME: &str = console_name!("clear_decals");

/// `hw+0x4a000`, the engine's remove-every-decal-with-this-flag loop. The
/// wildcards are the pool base, the two relative calls and the pool end.
const REMOVE_LOOP: &str = "55 8B EC 56 57 8B 7D 08 BE ?? ?? ?? ?? 0F BF 46 16 85 C7 74 13 \
                           56 E8 ?? ?? ?? ?? 6A 1C 6A 00 56 E8 ?? ?? ?? ?? 83 C4 10 83 C6 1C \
                           81 FE ?? ?? ?? ?? 7C DA";

/// Offsets into a [`REMOVE_LOOP`] match.
const POOL_BASE_AT: usize = 9;
const UNLINK_CALL_AT: usize = 22;
const POOL_END_AT: usize = 45;

/// `hw+0x49da0`, `R_DecalInit`. Wildcards: the pool base, the `memset` call and
/// `gDecalCount`.
const DECAL_INIT: &str =
    "68 00 C0 01 00 6A 00 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 0C C7 05 ?? ?? ?? ?? 00 00 00 00";

/// Offsets into a [`DECAL_INIT`] match.
const INIT_POOL_SIZE_AT: usize = 1;
const INIT_POOL_BASE_AT: usize = 8;
const INIT_DECAL_COUNT_AT: usize = 22;

/// `sizeof(decal_t)`, as `push 0x1c` in the remove loop's own `memset` call.
const DECAL_SIZE: usize = 0x1c;

/// `decal_t::psurface`. A slot with none is free; this is only read to report
/// how many decals there actually were.
const PSURFACE_AT: usize = 4;

/// `void R_DecalUnlink(decal_t *)`, cdecl.
type UnlinkFn = unsafe extern "C" fn(*mut c_void);

/// What one resolve found. All addresses are absolute in the loaded `hw.dll`.
#[derive(Clone, Copy)]
pub struct Pool {
    pub base: usize,
    pub end: usize,
    pub decal_count: usize,
    pub unlink: usize,
}

impl Pool {
    pub fn slots(&self) -> usize {
        (self.end - self.base) / DECAL_SIZE
    }
}

static POOL_BASE: AtomicUsize = AtomicUsize::new(0);
static POOL_END: AtomicUsize = AtomicUsize::new(0);
static DECAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static UNLINK: AtomicUsize = AtomicUsize::new(0);
static RESOLVED_BASE: AtomicUsize = AtomicUsize::new(0);

/// How many decals the last successful clear removed, for `dodstudio_status`.
static LAST_CLEARED: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Reads a little-endian `u32` at `address`.
///
/// Safety: four mapped, readable bytes.
unsafe fn read_u32(address: usize) -> u32 {
    unsafe { (address as *const u32).read_unaligned() }
}

/// The target of the `E8 rel32` whose opcode byte is at `call_site`.
///
/// Safety: five mapped, readable bytes.
unsafe fn call_target(call_site: usize) -> usize {
    let displacement = unsafe { read_u32(call_site + 1) } as i32;
    call_site.wrapping_add(5).wrapping_add(displacement as usize)
}

/// Finds the pool and `R_DecalUnlink` in the loaded engine.
///
/// Every value comes out of the matched instructions, and the two signatures
/// check each other: they must name the same pool base, and the span the remove
/// loop walks must be the length `R_DecalInit` clears.
pub fn resolve() -> Result<Pool, String> {
    let Some(base) = engine::engine_module_base() else {
        return Err("hw.dll is not loaded yet".to_string());
    };
    if RESOLVED_BASE.load(Ordering::Acquire) == base {
        return Ok(Pool {
            base: POOL_BASE.load(Ordering::Acquire),
            end: POOL_END.load(Ordering::Acquire),
            decal_count: DECAL_COUNT.load(Ordering::Acquire),
            unlink: UNLINK.load(Ordering::Acquire),
        });
    }

    // Safety: `engine_module_base` only returns a base for a mapped module,
    // and hw.dll stays mapped for the session.
    let remove_loop = unsafe { scan::find_unique(base, REMOVE_LOOP) }.map_err(|why| {
        format!(
            "could not find the engine's decal remove loop -- {why}. This signature is the \
             pre-Anniversary hw.dll's; the 25th-Anniversary engine compiles that function \
             differently and is not supported here"
        )
    })?;
    let init = unsafe { scan::find_unique(base, DECAL_INIT) }
        .map_err(|why| format!("could not find R_DecalInit -- {why}"))?;

    // Safety: both addresses are inside the matched spans, which the scan
    // proved are mapped.
    let (pool_base, pool_end, unlink, decal_count, cleared_bytes) = unsafe {
        (
            read_u32(remove_loop + POOL_BASE_AT) as usize,
            read_u32(remove_loop + POOL_END_AT) as usize,
            call_target(remove_loop + UNLINK_CALL_AT),
            read_u32(init + INIT_DECAL_COUNT_AT) as usize,
            read_u32(init + INIT_POOL_SIZE_AT) as usize,
        )
    };
    let init_pool_base = unsafe { read_u32(init + INIT_POOL_BASE_AT) } as usize;

    if init_pool_base != pool_base {
        return Err(format!(
            "the two signatures disagree about the decal pool ({pool_base:#x} vs {init_pool_base:#x})"
        ));
    }
    if pool_end <= pool_base || pool_end - pool_base != cleared_bytes {
        return Err(format!(
            "the pool spans {} bytes but R_DecalInit clears {cleared_bytes}",
            pool_end.wrapping_sub(pool_base)
        ));
    }
    if cleared_bytes % DECAL_SIZE != 0 {
        return Err(format!(
            "{cleared_bytes} bytes is not a whole number of {DECAL_SIZE}-byte decals"
        ));
    }

    POOL_BASE.store(pool_base, Ordering::Release);
    POOL_END.store(pool_end, Ordering::Release);
    DECAL_COUNT.store(decal_count, Ordering::Release);
    UNLINK.store(unlink, Ordering::Release);
    RESOLVED_BASE.store(base, Ordering::Release);

    Ok(Pool { base: pool_base, end: pool_end, decal_count, unlink })
}

/// Unlinks and zeroes every decal in the pool, returning how many were in use.
///
/// "In use" is counted from `psurface` being non-NULL, which is only for the
/// report -- every slot is unlinked and zeroed either way, because
/// `R_DecalUnlink` returns immediately for a slot with no surface and that is
/// cheaper than a branch that has to be right.
///
/// Must be called on the engine's own thread, which is where a console command
/// and the per-frame callback both run.
pub fn clear() -> Result<usize, String> {
    let pool = resolve()?;
    // Safety: resolved out of the engine's own call site, so it is that
    // function with that signature.
    let unlink: UnlinkFn = unsafe { std::mem::transmute::<usize, UnlinkFn>(pool.unlink) };

    let mut in_use = 0usize;
    for index in 0..pool.slots() {
        let decal = pool.base + index * DECAL_SIZE;
        // Safety: inside the pool the engine itself walks with the same stride.
        unsafe {
            if read_u32(decal + PSURFACE_AT) != 0 {
                in_use += 1;
            }
            unlink(decal as *mut c_void);
            std::ptr::write_bytes(decal as *mut u8, 0, DECAL_SIZE);
        }
    }

    // The ring index only decides which slot is reused next, so leaving it
    // would not strand anything -- but the engine's own R_DecalInit resets it,
    // and a fresh pool starting from the front is one less thing that differs
    // between a cleared session and a freshly loaded one.
    // Safety: a writable dword in the engine's data section.
    unsafe { (pool.decal_count as *mut u32).write_unaligned(0) };

    LAST_CLEARED.store(in_use, Ordering::Release);
    Ok(in_use)
}

/// One line for `dodstudio_status`.
pub fn status() -> String {
    match LAST_CLEARED.load(Ordering::Relaxed) {
        usize::MAX => format!("no decal clear has run this session ({NAME} runs one)"),
        0 => "the last decal clear found the walls already clean".to_string(),
        n => format!("the last decal clear removed {n} decal(s)"),
    }
}

/// Whether a clear has run this session.
pub fn has_run() -> bool {
    LAST_CLEARED.load(Ordering::Relaxed) != usize::MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lowercased, because the patterns are written in the uppercase HLAE
    /// spelling and `scan::Pattern` is case-insensitive -- so a test that
    /// compared case-sensitively would be testing the spelling, not the bytes.
    fn tokens(pattern: &str) -> Vec<String> {
        pattern.split_whitespace().map(str::to_ascii_lowercase).collect()
    }

    /// Both patterns have to parse, and neither may start with a wildcard --
    /// `scan::Pattern` refuses that, and the failure would only show up in a
    /// running game.
    #[test]
    fn both_patterns_are_well_formed() {
        for pattern in [REMOVE_LOOP, DECAL_INIT] {
            let tokens = tokens(pattern);
            assert!(tokens.len() > 16, "too short to be unique");
            assert_ne!(tokens[0].as_str(), "??", "a leading wildcard is rejected by the scanner");
            for token in &tokens {
                assert!(
                    token == "??" || u8::from_str_radix(token, 16).is_ok(),
                    "{token:?} is not a hex byte or a wildcard"
                );
            }
        }
    }

    /// Each value is read from a specific operand. If an offset drifted onto an
    /// opcode the read would still succeed and return a plausible number, so
    /// the offsets are anchored to the wildcards instead.
    #[test]
    fn every_read_offset_lands_on_a_wildcard() {
        let wildcards = |pattern: &str, at: usize, count: usize, what: &str| {
            for (offset, token) in tokens(pattern).iter().skip(at).take(count).enumerate() {
                assert_eq!(token.as_str(), "??", "byte {} of {what}", at + offset);
            }
        };
        wildcards(REMOVE_LOOP, POOL_BASE_AT, 4, "the remove loop (pool base)");
        wildcards(REMOVE_LOOP, POOL_END_AT, 4, "the remove loop (pool end)");
        // The call's own opcode is fixed; its displacement is the wildcard.
        assert_eq!(tokens(REMOVE_LOOP)[UNLINK_CALL_AT].as_str(), "e8");
        wildcards(REMOVE_LOOP, UNLINK_CALL_AT + 1, 4, "the unlink call");
        wildcards(DECAL_INIT, INIT_POOL_BASE_AT, 4, "R_DecalInit (pool base)");
        wildcards(DECAL_INIT, INIT_DECAL_COUNT_AT, 4, "R_DecalInit (gDecalCount)");
    }

    /// `R_DecalInit`'s `memset` length is a fixed immediate in the pattern, and
    /// it is the number every cross-check is against. 0x1c000 is 4096 decals of
    /// 28 bytes -- `MAX_RENDER_DECALS`.
    #[test]
    fn the_pool_size_immediate_is_4096_decals() {
        let init = tokens(DECAL_INIT);
        let bytes: Vec<u8> = init[INIT_POOL_SIZE_AT..INIT_POOL_SIZE_AT + 4]
            .iter()
            .map(|t| u8::from_str_radix(t, 16).expect("a fixed byte"))
            .collect();
        let size = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(size, 0x1c000);
        assert_eq!(size % DECAL_SIZE, 0);
        assert_eq!(size / DECAL_SIZE, 4096);
    }

    /// The remove loop's own stride and `memset` length are the two places
    /// `sizeof(decal_t)` appears in the instruction stream. Both are fixed
    /// bytes in the pattern, so a wrong `DECAL_SIZE` here would disagree with
    /// the code it is copying.
    #[test]
    fn the_decal_size_matches_the_loop_it_copies() {
        let remove = tokens(REMOVE_LOOP);
        // `6A 1C` -- push sizeof(decal_t) to memset.
        let push = remove.iter().position(|t| t == "6a").expect("a push imm8");
        assert_eq!(u8::from_str_radix(&remove[push + 1], 16).unwrap() as usize, DECAL_SIZE);
        // `83 C6 1C` -- add esi, sizeof(decal_t).
        let add = remove
            .windows(2)
            .position(|w| w[0] == "83" && w[1] == "c6")
            .expect("the stride");
        assert_eq!(u8::from_str_radix(&remove[add + 2], 16).unwrap() as usize, DECAL_SIZE);
    }

    #[test]
    fn slots_are_counted_from_the_span() {
        let pool = Pool { base: 0x1000, end: 0x1000 + 0x1c000, decal_count: 0, unlink: 0 };
        assert_eq!(pool.slots(), 4096);
    }

    #[test]
    fn status_distinguishes_never_run_from_found_nothing() {
        let saved = LAST_CLEARED.load(Ordering::Acquire);

        LAST_CLEARED.store(usize::MAX, Ordering::Release);
        assert!(!has_run());
        assert!(status().contains("no decal clear"), "{}", status());

        LAST_CLEARED.store(0, Ordering::Release);
        assert!(has_run(), "a clear that found nothing still ran");
        assert!(status().contains("already clean"), "{}", status());

        LAST_CLEARED.store(37, Ordering::Release);
        assert!(status().contains("37"), "{}", status());

        LAST_CLEARED.store(saved, Ordering::Release);
    }
}
