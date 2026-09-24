//! `dodstudio_log_texture_loads`: observe (read-only) every wad texture the
//! engine loads, by name, dimensions and source pointer -- the groundwork for
//! substituting higher-resolution replacements without touching any BSP.
//!
//! ## Why this exists
//!
//! Texture-upscale R&D (see memory `texture-upscale-rnd.md`, Track D) found
//! that a same-resolution AI upscale of a map's `.wad` (Track A) has a real
//! ceiling: a wad's `miptex_t` header carries both the texture's pixel
//! dimensions *and* the pixel data in one file, and the BSP's texinfo vectors
//! were computed by the map compiler against the original dimensions. Growing
//! one without the other tiles wrong on screen.
//!
//! Cross-checked against Xash3D's open-source renderer
//! (`engine/client/gl_rsurf.c`, `GL_BuildPolygonFromSurface`): the UV divide
//! (`s /= texinfo->texture->width`) reads `width`/`height` **live, from the
//! texture's in-memory struct**, not a value frozen into the BSP. That means a
//! hook can leave the *logical* dimensions the engine's own bookkeeping sees
//! unchanged (so tiling stays correct) while substituting different pixel data
//! for what actually reaches the GPU -- no BSP edit required. This module is
//! step one of that: prove the hook point exists and reads real data, before
//! anything writes back.
//!
//! ## Why `Draw_MiptexTexture`, not `GL_LoadTexture`
//!
//! `GL_LoadTexture` (further down the same call chain) receives an
//! already-expanded RGBA pixel buffer with no name attached anywhere in its
//! parameters -- confirmed by static analysis, and the reason an earlier pass
//! of this research chased a dead end (a `gltexture_t` name-cache that turned
//! out to only ever run for one hardcoded built-in texture). `Draw_MiptexTexture`
//! is the function that still has the texture's *name*, because it is the one
//! reading the `miptex_t` header out of the wad's mapped data in the first
//! place.
//!
//! ## What the hook actually does
//!
//! Nothing that changes behaviour. It detours the function's own entry point,
//! reads the same two bytes of pointer arithmetic the original code is about
//! to perform anyway (`miptex_ptr = wad_base + texinfo->miptex_offset`), logs
//! the name/width/height when asked to, then reproduces the stolen prologue
//! bytes and resumes the real function completely unmodified. If this read
//! were unsafe, the original function's *own* next few instructions --
//! unmodified, right after the stub returns -- would already be unsafe too:
//! it performs the identical address computation and immediately `rep movsd`s
//! 40 bytes from it. This hook cannot be less safe than the function it
//! detours.
//!
//! ## Signature, not a hardcoded address
//!
//! Verified against the pre-Anniversary movies-install `hw.dll` (the only
//! build dod-tools ever launches -- see `docs/two_dod_installs` /
//! [`crate::engine`]'s module docs) by extracting the real bytes and
//! independently confirming the pattern matches exactly once in the whole
//! file (`tools/verify_texture_hires_offsets.py`). The four wildcarded spans
//! are: the "Draw_MiptexTexture" debug string's own address (an error-path
//! argument, never reached on the normal path), the error-log call's
//! displacement, a helper-copy call's displacement, and three `call dword
//! ptr [addr]` indirect GL-wrapper calls -- all build-specific, none needed to
//! identify the function.
//!
//! ## Not wired into the default fix set yet
//!
//! Deliberately not called from `lib.rs::install_fixes` until it has been
//! live-tested at least once -- everything else in this crate is proven in a
//! real session; this is the first hook in unproven territory (detouring
//! `hw.dll`'s own internal code, not just an IAT/`GetProcAddress` seam). Call
//! [`install`] explicitly (currently from a `GOLDSRC_HOOKS_TEXTURE_HIRES`
//! env-flag check, matching `sound_fix`/`anim_fix`'s own opt-in convention)
//! once it has been.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::detour;
use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The command name that toggles verbose per-load logging. Registered in
/// `commands.rs`.
pub const NAME: &str = console_name!("log_texture_loads");

/// `Draw_MiptexTexture`'s own entry point in the pre-Anniversary `hw.dll`.
///
/// The pattern starts at the function's first byte (`push ebp`), so a match's
/// address *is* the function's address -- no separate `_AT` offset needed,
/// unlike `deathmsg.rs`'s convergence-point detour.
///
/// Wildcards, in order: the "Draw_MiptexTexture" string's own address (error
/// path only), the error-log call's displacement, a helper-copy call's
/// displacement, and three indirect `call dword ptr [addr]` GL-wrapper calls.
const DRAW_MIPTEX_TEXTURE: &str = "55 8B EC 83 EC 28 53 56 8B 75 08 57 83 7E 18 20 74 10 8B 06 \
    50 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 08 8B 76 18 8B 5D 0C 03 F3 B9 0A 00 00 00 8D 7D D8 \
    6A 10 F3 A5 8D 4D D8 51 53 E8 ?? ?? ?? ?? 8B 55 E8 52 FF 15 ?? ?? ?? ?? 89 43 10 8B 45 EC \
    50 FF 15 ?? ?? ?? ?? 83 C4 14 33 F6 89 43 14 89 73 28 89 73 24 89 73 20 89 73 30 89 73 2C \
    8D 7B 34 8B 4C B5 F0 51 FF 15 ?? ?? ?? ?? 8B 55 08 83 C4 04";

/// `push ebp; mov ebp, esp; sub esp, 0x28; push ebx; push esi` -- the
/// instructions the jump overwrites, reproduced verbatim at the end of the
/// stub. 8 bytes, enough for the 5-byte jump with 3 to spare (folded into the
/// stolen span rather than left as trailing `nop`s, since all 8 land on
/// [`DRAW_MIPTEX_TEXTURE`]'s own unwildcarded prefix).
const STOLEN: &[u8] = &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x28, 0x53, 0x56];

/// Total wad textures observed since this hook installed, whether or not
/// verbose logging is on -- cheap, and what `dodstudio_debug_status` reports.
static SEEN_COUNT: AtomicU32 = AtomicU32::new(0);

/// The most recent texture name/dimensions seen, for `status()`.
static LAST_SEEN: Mutex<Option<(String, u32, u32)>> = Mutex::new(None);

/// Whether to write a debug-log line for every load. Off by default -- a busy
/// map loads well over a hundred textures, and a permanent line per texture
/// would flood the log the same way an unconditional per-frame line would.
pub static LOG_TEXTURE_LOADS: AtomicBool = AtomicBool::new(false);

/// Where the stub jumps back to: the instruction after [`STOLEN`].
static RESUME: AtomicUsize = AtomicUsize::new(0);

/// `observe_miptex`'s own address, read by the stub via an indirect call --
/// see `stub()`'s doc for why this can't be a direct `call rel32`.
static OBSERVE_FN: AtomicUsize = AtomicUsize::new(0);

/// Installed once per process; see [`detour::Detour`] on why it is never
/// undone.
static INSTALLED: Mutex<Option<detour::Detour>> = Mutex::new(None);

/// Reads the wad's `miptex_t` name/width/height the same way the original
/// function's very next (unmodified) instructions do, and records/logs it.
///
/// `texinfo_like` is `Draw_MiptexTexture`'s first parameter -- a struct whose
/// `+0x18` field is a byte offset from `wad_base` to the `miptex_t`. Not fully
/// identified (its error-path use, `param[+0]` in an error-log call, was not
/// needed here and is not read), but the offset computation itself is
/// confirmed by disassembly: the original code performs the identical
/// `wad_base + [texinfo_like+0x18]` add immediately after where this hook
/// resumes it.
///
/// # Safety
///
/// Called only from the installed stub, with the exact two parameters
/// `Draw_MiptexTexture` itself was called with. See the module docs for why
/// the read this performs cannot be less safe than the original function's
/// own next instructions.
unsafe extern "C" fn observe_miptex(texinfo_like: *const u8, wad_base: *const u8) {
    if texinfo_like.is_null() || wad_base.is_null() {
        return;
    }

    let miptex_offset = unsafe { (texinfo_like.add(0x18) as *const u32).read_unaligned() };
    let miptex_ptr = wad_base.wrapping_add(miptex_offset as usize);

    // Guards against a wildly implausible offset without pretending to fully
    // validate it -- the same non-guarantee the original code itself has
    // (its own `cmp` a few bytes earlier is a soft warning, not a bounds
    // check). This is strictly extra caution beyond what the game itself does.
    if (miptex_ptr as usize) < 0x1000 {
        return;
    }

    // miptex_t: char name[16], uint32 width, height -- see wad3.py / the
    // texture-upscale-rnd memory for the format, reverse-engineered
    // independently and now confirmed from this exact engine code path.
    let name_bytes = unsafe { std::slice::from_raw_parts(miptex_ptr, 16) };
    let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(16);
    let name = String::from_utf8_lossy(&name_bytes[..end]).into_owned();
    let width = unsafe { (miptex_ptr.add(16) as *const u32).read_unaligned() };
    let height = unsafe { (miptex_ptr.add(20) as *const u32).read_unaligned() };

    SEEN_COUNT.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = LAST_SEEN.lock() {
        *last = Some((name.clone(), width, height));
    }

    if LOG_TEXTURE_LOADS.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: Draw_MiptexTexture name={name:?} {width}x{height} (miptex @ {miptex_ptr:p})"
            ))
        };
    }
}

/// The stub, hand-assembled.
///
/// ```asm
/// mov eax, [esp+4]              ; param1 (texinfo_like)
/// mov ecx, [esp+8]              ; param2 (wad_base)
/// push ecx
/// push eax
/// call dword ptr [OBSERVE_FN]   ; observe_miptex(param1, param2)
/// add esp, 8
/// push ebp / mov ebp, esp / sub esp, 0x28 / push ebx / push esi   ; STOLEN
/// jmp dword ptr [RESUME]
/// ```
///
/// Both the call and the final jump are indirect (`FF 15`/`FF 25` through a
/// static holding the real address) rather than a direct `rel32`, because a
/// direct `call`/`jmp` is relative to the instruction's own address -- which
/// lives inside the stub, whose final location `VirtualAlloc` only decides
/// *after* these bytes are built. `observe_miptex`'s own address is already
/// fixed at this point (it is compiled into this DLL), so `OBSERVE_FN` only
/// needs setting once, not per-install.
///
/// `eax`/`ecx`/`edx` and the flags are all safe to clobber here: this is the
/// function's true entry point, and every caller of a cdecl function already
/// treats those as caller-saved. `ebp`/`esp`/`ebx`/`esi`/`edi` are untouched
/// by the call (a normal Rust `extern "C" fn` preserves them via its own
/// compiler-generated prologue/epilogue), so the reproduced `STOLEN` bytes see
/// exactly the state the original prologue would have.
fn stub(resume_slot: usize, observe_fn_slot: usize) -> Vec<u8> {
    let mut code = vec![
        0x8B, 0x44, 0x24, 0x04, // mov eax, [esp+4]
        0x8B, 0x4C, 0x24, 0x08, // mov ecx, [esp+8]
        0x51, // push ecx
        0x50, // push eax
        0xFF, 0x15, // call dword ptr [abs32]
    ];
    // `observe_fn_slot` is the address of a static holding observe_miptex's
    // function pointer (`OBSERVE_FN.as_ptr()`), not the function's own
    // address directly -- `FF 15` dereferences the operand once to get the
    // call target, matching `RESUME`/`OFFSET_VALUE`'s own indirection below
    // and in `deathmsg.rs`.
    code.extend_from_slice(&(observe_fn_slot as u32).to_le_bytes());
    code.extend_from_slice(&[0x83, 0xC4, 0x08]); // add esp, 8
    code.extend_from_slice(STOLEN);
    code.extend_from_slice(&[0xFF, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume_slot as u32).to_le_bytes());
    code
}

/// Installs the `Draw_MiptexTexture` observation hook, once.
///
/// Read-only: nothing about texture loading changes. Safe to call whenever
/// `hw.dll` is loaded; a signature mismatch (wrong build) fails loudly and
/// changes nothing, matching `decals.rs`'s own refuse-rather-than-guess rule.
pub fn install() -> Result<(), String> {
    let mut slot = INSTALLED
        .lock()
        .map_err(|_| "the texture_hires detour lock is poisoned".to_string())?;
    if slot.is_some() {
        return Ok(());
    }

    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;

    // Safety: `base` is a module handle the loader gave us, and stays mapped
    // for the session (see `engine::engine_module_base`'s own docs).
    let target = unsafe { scan::find_unique(base, DRAW_MIPTEX_TEXTURE) }.map_err(|why| {
        format!(
            "could not locate Draw_MiptexTexture -- {why}. This signature is the pre-Anniversary \
             hw.dll's; dod-tools only ever launches that build, so a mismatch here means the \
             running hw.dll is not the one expected"
        )
    })?;

    // Extra verification beyond the signature match itself, matching
    // `deathmsg.rs`'s own convention: proves the bytes about to be
    // overwritten are exactly what was disassembled, not just that the
    // longer pattern around them matched.
    // Safety: `target` is inside the matched (and therefore mapped) span.
    let present = unsafe { std::slice::from_raw_parts(target as *const u8, STOLEN.len()) };
    if present != STOLEN {
        return Err(format!(
            "expected {STOLEN:02x?} at the matched address, found {present:02x?}"
        ));
    }

    RESUME.store(target + STOLEN.len(), Ordering::Release);
    OBSERVE_FN.store(observe_miptex as *const () as usize, Ordering::Release);

    let code = stub(RESUME.as_ptr() as usize, OBSERVE_FN.as_ptr() as usize);
    // Safety: the span was checked byte-for-byte above; nothing branches into
    // the middle of a function's own prologue.
    let detour = unsafe { detour::install(target, STOLEN.len(), &code) }?;
    unsafe {
        crate::debug::report(&format!(
            "texture_hires: Draw_MiptexTexture hook installed at +{:#x}, stub at {:#x}",
            target - base,
            detour.stub_address()
        ))
    };
    *slot = Some(detour);
    Ok(())
}

/// One line for `dodstudio_debug_status`.
pub fn status() -> String {
    let seen = SEEN_COUNT.load(Ordering::Relaxed);
    if seen == 0 {
        return format!("{NAME}: no wad textures observed yet this session");
    }
    let last = LAST_SEEN
        .lock()
        .ok()
        .and_then(|l| l.clone())
        .map(|(name, w, h)| format!(", last: {name:?} {w}x{h}"))
        .unwrap_or_default();
    format!(
        "{NAME}: {seen} wad texture load(s) observed{last} (logging {})",
        if LOG_TEXTURE_LOADS.load(Ordering::Relaxed) {
            "on"
        } else {
            "off"
        }
    )
}

/// Whether the hook has observed anything this session -- gates whether
/// `status_text()` includes this module's line at all, matching every other
/// off-by-default logger in this crate.
pub fn has_observed() -> bool {
    SEEN_COUNT.load(Ordering::Relaxed) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pattern: &str) -> Vec<String> {
        pattern.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_pattern_is_well_formed_and_unwildcarded_at_the_prefix() {
        let toks = tokens(DRAW_MIPTEX_TEXTURE);
        assert!(toks.len() > 40, "too short to be unique");
        assert_ne!(
            toks[0], "??",
            "a leading wildcard is rejected by the scanner"
        );
        for tok in &toks {
            assert!(
                tok == "??" || u8::from_str_radix(tok, 16).is_ok(),
                "{tok:?} is not a hex byte or a wildcard"
            );
        }
        // The stolen bytes are exactly the pattern's own first 8 bytes,
        // unwildcarded -- the detour target is the match address itself.
        for (i, &b) in STOLEN.iter().enumerate() {
            assert_eq!(
                u8::from_str_radix(&toks[i], 16).unwrap(),
                b,
                "byte {i} of the pattern should equal STOLEN[{i}]"
            );
        }
    }

    #[test]
    fn stub_bytes_are_the_expected_length() {
        let code = stub(0x1234_5678, 0x8765_4321);
        // 4 (mov eax) + 4 (mov ecx) + 1 (push ecx) + 1 (push eax) + 6 (call [abs32])
        // + 3 (add esp,8) + 8 (STOLEN) + 6 (jmp [abs32])
        assert_eq!(code.len(), 4 + 4 + 1 + 1 + 6 + 3 + STOLEN.len() + 6);
        // The stolen bytes appear verbatim, at the offset the comment says.
        let stolen_at = 4 + 4 + 1 + 1 + 6 + 3;
        assert_eq!(&code[stolen_at..stolen_at + STOLEN.len()], STOLEN);
    }

    #[test]
    fn status_reports_nothing_observed_before_any_hit() {
        // SEEN_COUNT is process-global; only assert the zero-case shape here
        // rather than mutating shared state a parallel test could observe.
        if SEEN_COUNT.load(Ordering::Relaxed) == 0 {
            assert!(status().contains("no wad textures observed"));
            assert!(!has_observed());
        }
    }
}
