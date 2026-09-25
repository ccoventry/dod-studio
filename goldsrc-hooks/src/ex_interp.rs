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
//! ## The 25th Anniversary build
//!
//! Its clamp (`hw+0x1a3cde`) has the same shape in SSE: ceiling
//! `mov edx, 100`, and `mov eax, 200` / `cmovne edx, eax` on the same kind of
//! flag. But there the flag is **live**: `CL_Parse_HLTV` sets it to 1 on
//! `svc_hltv`'s mode 0, so an HLTV demo gets the 200 ms ceiling and a POV demo
//! 100. [`BUILDS`] lists each build's ceiling immediates with what the engine
//! ships in them.
//!
//! The setting means the same on both builds: at its default, 100
//! ([`STOCK_MS`]), every immediate holds the engine's own value, so neither
//! engine is touched. Any other value becomes the ceiling for every demo --
//! on the Anniversary build that is both paths, so a value under 200 lowers an
//! HLTV demo's ceiling there.
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

/// One engine build's clamp.
struct Build {
    name: &'static str,
    /// The clamp, with each ceiling immediate wildcarded so the pattern still
    /// matches after this has written one. Unique in that build's `hw.dll`,
    /// matching nothing in the other's.
    pattern: &'static str,
    /// Each ceiling `imm32`: its offset in a match, and what the engine ships
    /// there. The first is the one every demo gets on the pre-Anniversary
    /// build and a POV demo gets on the Anniversary one.
    ceilings: &'static [(usize, i32)],
}

/// The builds this knows, tried in order.
const BUILDS: [Build; 2] = [
    // `mov edi, 50` (the floor), `mov ebx, <ceiling>`, then the flag test.
    Build {
        name: "pre-Anniversary",
        pattern: "BF 32 00 00 00 BB ?? ?? ?? ?? DF E0 F6 C4 05 7A",
        ceilings: &[(6, 100)],
    },
    // `cmp [flag], 0`, `mov eax, <200 path>`, `movss xmm4, [1000.0]`,
    // `mov edx, <100 path>`, `cmovne edx, eax`.
    Build {
        name: "25th Anniversary",
        pattern: "83 3D ?? ?? ?? ?? 00 B8 ?? ?? ?? ?? F3 0F 10 25 ?? ?? ?? ?? BA ?? ?? ?? ?? 0F 45 D0",
        ceilings: &[(21, 100), (8, 200)],
    },
];

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
/// Which of [`BUILDS`] matched.
static BUILD: AtomicUsize = AtomicUsize::new(0);

/// The ceiling currently written into the engine, or 0 before the first apply.
static ACTIVE_MS: AtomicI32 = AtomicI32::new(0);

/// The clamp's address and which build it is, scanning once per module base.
fn span() -> Result<(usize, &'static Build), String> {
    let Some(base) = engine::engine_module_base() else {
        return Err("hw.dll is not loaded yet".to_string());
    };
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = SPAN_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok((cached, &BUILDS[BUILD.load(Ordering::Acquire)]));
        }
    }
    let mut misses = Vec::new();
    for (index, build) in BUILDS.iter().enumerate() {
        // Safety: `engine_module_base` only returns a base for a mapped
        // module, and hw.dll stays mapped for the session.
        match unsafe { scan::find_unique(base, build.pattern) } {
            Ok(address) => {
                SPAN_ADDRESS.store(address, Ordering::Release);
                BUILD.store(index, Ordering::Release);
                SCANNED_BASE.store(base, Ordering::Release);
                return Ok((address, build));
            }
            Err(why) => misses.push(format!("{}: {why}", build.name)),
        }
    }
    Err(format!(
        "could not find the ex_interp clamp -- {}",
        misses.join("; ")
    ))
}

/// Reads one ceiling immediate.
///
/// Safety: `address + at` must be inside the span the scan matched.
unsafe fn read_ceiling(address: usize, at: usize) -> i32 {
    unsafe { ((address + at) as *const i32).read_unaligned() }
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
    let (address, build) = span()?;
    // The default leaves every immediate at the engine's own value; anything
    // else is the ceiling on every path.
    let wanted = |stock: i32| if ms == STOCK_MS { stock } else { ms };
    // Safety (every read and write below): immediates inside the matched span.
    let present: Vec<i32> = build
        .ceilings
        .iter()
        .map(|&(at, _)| unsafe { read_ceiling(address, at) })
        .collect();
    if build
        .ceilings
        .iter()
        .zip(&present)
        .all(|(&(_, stock), &now)| now == wanted(stock))
    {
        ACTIVE_MS.store(ms, Ordering::Release);
        return Ok(false);
    }
    // Anything that is neither the shipped value nor a value this module
    // would write is someone else's patch, and overwriting it would hide that.
    for (&(at, stock), &now) in build.ceilings.iter().zip(&present) {
        if now != stock && validate(now).is_err() {
            return Err(format!(
                "the clamp's ceiling at +{at} is {now} ms, which is neither the engine's {stock} nor a value this could have written -- something else has patched it"
            ));
        }
    }
    for &(at, stock) in build.ceilings {
        // Written through the same protect/write/restore used everywhere here.
        if !unsafe { crate::patch::write_code_bytes(address + at, &wanted(stock).to_le_bytes()) } {
            return Err("could not make the ex_interp clamp writable".to_string());
        }
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
    let anniversary =
        SCANNED_BASE.load(Ordering::Relaxed) != 0 && BUILD.load(Ordering::Relaxed) == 1;
    match ACTIVE_MS.load(Ordering::Relaxed) {
        0 if anniversary => format!(
            "the engine's interpolation ceiling is untouched ({STOCK_MS} ms for a POV demo, 200 ms for an HLTV demo on this build)"
        ),
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
    fn each_pattern_is_well_formed_and_every_ceiling_is_a_wildcard() {
        for build in &BUILDS {
            assert!(
                scan::Pattern::parse(build.pattern).is_ok(),
                "{}",
                build.name
            );
            let tokens: Vec<&str> = build.pattern.split_whitespace().collect();
            for &(at, stock) in build.ceilings {
                for (i, token) in tokens.iter().enumerate().skip(at).take(4) {
                    assert_eq!(
                        *token, "??",
                        "{}: byte {i} is a ceiling immediate",
                        build.name
                    );
                }
                // `mov r32, imm32` is B8+r: the byte before each ceiling.
                let opcode = u8::from_str_radix(tokens[at - 1], 16).unwrap();
                assert_eq!(
                    opcode & 0xf8,
                    0xb8,
                    "{}: +{at} follows a mov r32, imm32",
                    build.name
                );
                assert!(
                    validate(stock).is_ok(),
                    "{}: stock {stock} is writable",
                    build.name
                );
            }
        }
    }

    /// The Anniversary clamp's two ceilings: `mov edx` (the 100 ms path, the
    /// one kept) and `mov eax` (200 ms, moved into edx by `cmovne edx, eax`).
    #[test]
    fn the_anniversary_ceilings_are_the_right_registers() {
        let build = &BUILDS[1];
        let tokens: Vec<&str> = build.pattern.split_whitespace().collect();
        let (pov, hltv) = (build.ceilings[0], build.ceilings[1]);
        assert_eq!((tokens[pov.0 - 1], pov.1), ("BA", STOCK_MS), "mov edx, 100");
        assert_eq!((tokens[hltv.0 - 1], hltv.1), ("B8", 200), "mov eax, 200");
        assert_eq!(
            &tokens[tokens.len() - 3..],
            ["0F", "45", "D0"],
            "cmovne edx, eax"
        );
    }

    /// The two `mov r32, imm32` opcodes the pre-Anniversary span starts with.
    /// `BF` is `mov edi, imm32` (the floor) and `BB` is `mov ebx, imm32` (the
    /// ceiling); patching the wrong one would raise the floor instead, which
    /// the engine would then force every value up to.
    #[test]
    fn the_ceiling_offset_lands_on_the_right_instruction() {
        const CEILING_AT: usize = 6;
        assert_eq!(BUILDS[0].ceilings, &[(CEILING_AT, STOCK_MS)]);
        let tokens: Vec<&str> = BUILDS[0].pattern.split_whitespace().collect();
        assert_eq!(
            u8::from_str_radix(tokens[0], 16).unwrap(),
            0xbf,
            "mov edi, imm32"
        );
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
