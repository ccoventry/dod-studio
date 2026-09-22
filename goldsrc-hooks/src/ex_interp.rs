//! `dodstudio_ex_interp_max`: raise the engine's interpolation-window ceiling.
//!
//! ## `ex_interp` is engine-managed, which is the whole problem
//!
//! A clamp at `hw+0x18ee0` runs every frame, forces `ex_interp` into a range,
//! and writes the clamped result back through `Cvar_Set` -- printing
//! `ex_interp forced up to %i msec` or `forced down to %i msec` as it goes.
//! Setting the cvar by hand and expecting it to stay does not work, and that is
//! not a DoD quirk: it is the engine.
//!
//! ```text
//!     hw+0x18ef3  mov edi, 0x32          ; floor, 50 ms
//!     hw+0x18ef8  mov ebx, 0x64          ; ceiling, 100 ms  <- this
//!     hw+0x18f5f  mov eax, [0x2d5df84]   ; a flag
//!     hw+0x18f64  test eax, eax
//!     hw+0x18f68  mov ebx, 0xc8          ; ceiling, 200 ms, when it is set
//!     hw+0x18f82  fld [1000.0]
//!     hw+0x18f88  fdiv [cl_updaterate]   ; the real floor, at least 1
//!                 ...clamp into [edi, ebx], print, Cvar_Set it back
//! ```
//!
//! ## The 200 ms path is dead, so this is worth what it looks like
//!
//! #271 asked what the flag at `0x2d5df84` is, and reasoned that if it means
//! "we are playing a demo" then demos already get 200 ms and the work is worth
//! half.
//!
//! They do not. Within `hw.dll` the flag has exactly **four** write sites --
//! every addressing form checked, not just the obvious one:
//!
//! ```text
//!     hw+0x10a58  mov dword [flag], 0        in the demo reader
//!     hw+0x1087a  mov [flag], ebx            in the demo reader, ebx zeroed
//!     hw+0x18537  mov [flag], ebx            in a block zeroing a dozen fields
//!     hw+0x1d791  mov dword [flag], 1        <- unreachable
//! ```
//!
//! The only site that writes 1 has **no caller, no jump to it, no absolute
//! reference anywhere in the image, and no fall-through** -- the instruction
//! before it is an unconditional `jmp`. So the engine never takes the 200 ms
//! branch, the ceiling is a hard 100, and raising it is the only way up.
//!
//! ## And the flag is not the lever to pull
//!
//! Setting it would be one dword and would look tempting. It is read from 25
//! sites, including inside `CL_ParseServerMessage`, the `svc_*` handlers and
//! `CL_CheckCRCs`. Flipping a feature switch the retail build never sets, to
//! find out what else it turns on, is not a thing to do inside someone's
//! capture run. The immediate is the narrow change: it affects the clamp and
//! nothing else.
//!
//! ## What still bounds it
//!
//! `cl_updaterate` sets the floor as `1000 / cl_updaterate`, so a demo recorded
//! at a low update rate cannot be interpolated below what it captured. Raising
//! the ceiling does not make a 20-tick recording smooth; it stops the engine
//! from shortening a window that was already long enough.
//!
//! ## Engine-wide, deliberately
//!
//! `docs/goldsrc_hw_dll_survey.md` sets the standing preference: do it in
//! `client.dll` where an equivalent exists, because an engine change affects
//! the menu and every mod. There is no client-side equivalent here -- the clamp
//! is the engine's -- and this install exists only to render demos.

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The cvar name. Registered in `commands.rs`.
pub const NAME: &str = console_name!("ex_interp_max");

/// The clamp's floor and ceiling, with the ceiling's immediate wildcarded so
/// one pattern matches both the stock and a raised value. Unique in `hw.dll`.
const PATTERN: &str = "BF 32 00 00 00 BB ?? ?? ?? ?? DF E0 F6 C4 05 7A";

/// Where the ceiling's `imm32` sits inside a match.
const CEILING_AT: usize = 6;

/// `mov edi, 0x32` -- the engine's own floor, in milliseconds. Not patched:
/// `cl_updaterate` overrides it a few instructions later anyway.
pub const FLOOR_MS: i32 = 0x32;

/// What the engine ships as the ceiling.
pub const STOCK_MS: i32 = 100;

/// The highest this will write. Not a hard engine limit -- the field is an
/// `int` -- but an interpolation window longer than a second stops being
/// smoothing and starts being a rewrite of when things happened.
pub const MAX_MS: i32 = 1000;

static SPAN_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);

/// The ceiling currently written into the engine, or 0 before the first apply.
static ACTIVE_MS: AtomicI32 = AtomicI32::new(0);

fn span_address() -> Result<usize, String> {
    let Some(base) = engine::engine_module_base() else {
        return Err("hw.dll is not loaded yet".to_string());
    };
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = SPAN_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: `engine_module_base` only returns a base for a mapped module, and
    // hw.dll stays mapped for the session.
    let address = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the ex_interp clamp -- {why}"))?;
    SPAN_ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// The ceiling the engine is using right now, read back from its own code.
pub fn current() -> Result<i32, String> {
    let address = span_address()?;
    // Safety: the scan proved these bytes are mapped code.
    Ok(unsafe { ((address + CEILING_AT) as *const i32).read_unaligned() })
}

/// Rejects a ceiling the clamp could not honour, or that would not be a
/// smoothing window any more.
///
/// At or below the floor the clamp would force every value to one number, and
/// the engine would say so once per frame in the console.
pub fn validate(ms: i32) -> Result<(), String> {
    if ms <= FLOOR_MS {
        return Err(format!(
            "{ms} is at or below the engine's own {FLOOR_MS} ms floor, which would pin ex_interp to a single value"
        ));
    }
    if ms > MAX_MS {
        return Err(format!("{ms} is above the {MAX_MS} ms this will write"));
    }
    Ok(())
}

/// Writes `ms` as the clamp's ceiling, returning whether anything changed.
///
/// Idempotent and cheap to call every frame, which is how `commands::poll` uses
/// it -- and unlike the `client.dll` patches it is not there to survive a
/// module reload, since `hw.dll` is loaded once. It is there so that changing
/// the cvar takes effect without a restart.
pub fn set_max(ms: i32) -> Result<bool, String> {
    validate(ms)?;
    let address = span_address()?;
    let present = current()?;
    if present == ms {
        ACTIVE_MS.store(ms, Ordering::Release);
        return Ok(false);
    }
    // Anything that is neither the shipped ceiling nor a value this module
    // would write is someone else's patch, and overwriting it would hide that.
    if present != STOCK_MS && validate(present).is_err() {
        return Err(format!(
            "the clamp's ceiling is {present} ms, which is neither the engine's {STOCK_MS} nor a value this could have written -- something else has patched it"
        ));
    }
    // Safety: four bytes of an immediate inside the span the scan matched,
    // written through the same protect/write/restore used everywhere here.
    if !unsafe { crate::patch::write_code_bytes(address + CEILING_AT, &ms.to_le_bytes()) } {
        return Err("could not make the ex_interp clamp writable".to_string());
    }
    ACTIVE_MS.store(ms, Ordering::Release);
    Ok(true)
}

/// The ceiling this module last wrote, or 0 if it has not written one.
pub fn active() -> i32 {
    ACTIVE_MS.load(Ordering::Relaxed)
}

/// One line for `dodstudio_status`.
pub fn status() -> String {
    match ACTIVE_MS.load(Ordering::Relaxed) {
        0 => format!(
            "the engine's interpolation ceiling is untouched ({STOCK_MS} ms; its own 200 ms path is unreachable in this build)"
        ),
        STOCK_MS => format!("the interpolation ceiling is back to the engine's {STOCK_MS} ms"),
        ms => format!(
            "the interpolation ceiling is {ms} ms instead of {STOCK_MS} -- cl_updaterate still sets the floor, so a low-tick recording is not smoothed by it"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_is_well_formed_and_the_ceiling_is_its_only_wildcard() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_ne!(tokens[0], "??", "a leading wildcard is rejected by the scanner");
        for (i, token) in tokens.iter().enumerate() {
            let wildcard = (CEILING_AT..CEILING_AT + 4).contains(&i);
            if wildcard {
                assert_eq!(*token, "??", "byte {i} is the ceiling immediate");
            } else {
                assert!(
                    u8::from_str_radix(token, 16).is_ok(),
                    "byte {i} is {token:?}; only the ceiling may be a wildcard"
                );
            }
        }
    }

    /// The two `mov r32, imm32` opcodes the span starts with. `BF` is
    /// `mov edi, imm32` (the floor) and `BB` is `mov ebx, imm32` (the ceiling);
    /// patching the wrong one would raise the floor instead, which the engine
    /// would then force every value up to.
    #[test]
    fn the_ceiling_offset_lands_on_the_right_instruction() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(u8::from_str_radix(tokens[0], 16).unwrap(), 0xbf, "mov edi, imm32");
        assert_eq!(
            i32::from_le_bytes([
                u8::from_str_radix(tokens[1], 16).unwrap(),
                u8::from_str_radix(tokens[2], 16).unwrap(),
                u8::from_str_radix(tokens[3], 16).unwrap(),
                u8::from_str_radix(tokens[4], 16).unwrap(),
            ]),
            FLOOR_MS,
            "the floor immediate is the one this does NOT touch"
        );
        assert_eq!(
            u8::from_str_radix(tokens[CEILING_AT - 1], 16).unwrap(),
            0xbb,
            "mov ebx, imm32"
        );
    }

    #[test]
    fn a_ceiling_at_or_below_the_floor_is_refused() {
        assert!(validate(FLOOR_MS).is_err());
        assert!(validate(FLOOR_MS - 1).is_err());
        assert!(validate(0).is_err());
        assert!(validate(-1).is_err());
        assert!(validate(FLOOR_MS + 1).is_ok());
    }

    #[test]
    fn the_engines_own_ceiling_is_a_value_this_would_write() {
        // Putting the stock value back has to be expressible, or there is no
        // way to undo the setting without restarting the game.
        assert!(validate(STOCK_MS).is_ok());
        assert!(validate(MAX_MS).is_ok());
        assert!(validate(MAX_MS + 1).is_err());
    }

    /// The engine's own unreachable 200 ms branch has to be inside the range
    /// this will write, or the module would be refusing to reproduce something
    /// the engine itself contemplates.
    #[test]
    fn the_engines_dead_200ms_ceiling_is_writable() {
        assert!(validate(200).is_ok());
    }

    #[test]
    fn status_distinguishes_untouched_from_restored() {
        let saved = ACTIVE_MS.load(Ordering::Acquire);

        ACTIVE_MS.store(0, Ordering::Release);
        assert!(status().contains("untouched"), "{}", status());
        assert_eq!(active(), 0);

        ACTIVE_MS.store(STOCK_MS, Ordering::Release);
        assert!(status().contains("back to"), "{}", status());

        ACTIVE_MS.store(250, Ordering::Release);
        let text = status();
        assert!(text.contains("250"), "{text}");
        assert!(text.contains("cl_updaterate"), "{text}");

        ACTIVE_MS.store(saved, Ordering::Release);
    }
}
