//! The machine gun's "Deploy your machine gun to Reload!" prompt, hidden under
//! `ds_hud_hide_hints` (#342): like the hint panel, it is the game teaching
//! the recording player, and tells a viewer nothing about the footage.
//! `hint_panel.rs` owns the cvar and calls [`poll`].
//!
//! ## Where it comes from
//!
//! A `client.dll` function that checks the player's buttons, on `+reload`
//! (`test dh, 0x20`) with an undeployed machine gun (weapon ids `0x11`,
//! `0x12`, `0x15`), queues the text on the centre-print object
//! (`+0x175c6c`), with the call at `client+0x3c011`:
//!
//! ```text
//!     push 1.0; push 1.0; push 1.0   ; colour
//!     push -1; push -1               ; position: centred
//!     push "Dod_mg_reload"           ; the localised key
//!     mov  ecx, <centre-print object>
//!     call +0x3a990                  ; E8 rel32   <- call filtered_mg_reload
//!     mov  al, bl
//! ```
//!
//! `+0x3a990` is a `__thiscall` that ends `ret 0x18`: it looks the key up,
//! copies the text, colour and position into the object and sets it to
//! expire 4 s later; `CHudVGUI2Print::Draw` draws it until then. It is the
//! only call that queues `Dod_mg_reload`. Centre text the server sends goes
//! through `+0x3a830` instead, so it is not touched (that is
//! `ds_hud_hide_center_text`'s).
//!
//! ## The patch
//!
//! While the cvar is on, the call goes to [`filtered_mg_reload`], which takes
//! the same arguments and pops the same 0x18 bytes, counts the prompt and
//! drops it. At 0 the stock call is written back. `client.dll` is one file,
//! byte-identical in every install, so one pattern covers both engine builds;
//! it pins the call's `rel32` (relative within the module, so not relocated),
//! and the pushed key is checked to read `Dod_mg_reload` before anything is
//! written. `tools/verify_mg_reload_hint_offsets.py` checks the pattern,
//! the callee's `ret 0x18`, and that nothing branches into the call.
//!
//! A prompt already queued when the cvar turns on runs out its 4 s.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine;
use crate::hw_call_site::{CALL_LEN, call_bytes, call_target};
use crate::scan;

/// The three colour pushes, the two positions, the key, the object and the
/// call, with its `rel32` pinned, then `mov al, bl; pop esi; pop ebx; ret`.
const PATTERN: &str = "68 00 00 80 3F 68 00 00 80 3F 68 00 00 80 3F 6A FF 6A FF \
                       68 ?? ?? ?? ?? B9 ?? ?? ?? ?? E8 7A E9 FF FF 8A C3 5E 5B C3";
/// Where the key's address sits in [`PATTERN`].
const KEY_AT: usize = 20;
/// Where the call is in [`PATTERN`].
const CALL_AT: usize = 29;
/// The key the call queues.
const KEY: &[u8] = b"Dod_mg_reload\0";

/// The module base the cached address belongs to.
static BASE: AtomicUsize = AtomicUsize::new(0);
/// The call's address, 0 before a successful scan.
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The function the stock call goes to.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// A base the scan already failed on, so it isn't repeated every frame.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);
/// Set the first time the cvar is on, so a session that never asks for it
/// never scans.
static TOUCHED: AtomicBool = AtomicBool::new(false);
/// Whether the call is ours right now.
static HIDDEN_NOW: AtomicBool = AtomicBool::new(false);
/// Whether a failure has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);
/// Prompts dropped this session.
static DROPPED: AtomicU32 = AtomicU32::new(0);

/// [`PATTERN`] with the stock call's bytes, as tokens.
fn stock_call() -> [u8; CALL_LEN] {
    let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
    let mut out = [0u8; CALL_LEN];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(tokens[CALL_AT + i], 16).unwrap_or(0);
    }
    out
}

/// [`PATTERN`] with its call's five bytes left open, which also finds a
/// call an earlier load of this DLL left pointing at its own filter.
fn open_pattern() -> String {
    let mut tokens: Vec<&str> = PATTERN.split_whitespace().collect();
    for token in &mut tokens[CALL_AT..CALL_AT + CALL_LEN] {
        *token = "??";
    }
    tokens.join(" ")
}

/// The call's address in the module at `base`, scanning once per module.
fn resolve(base: usize) -> Result<usize, String> {
    if BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    if FAILED_BASE.load(Ordering::Acquire) == base {
        return Err("the MG reload prompt's call was not found in this client.dll".into());
    }
    let fail = |why: String| {
        FAILED_BASE.store(base, Ordering::Release);
        Err(why)
    };
    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let found = match unsafe { scan::find_unique(base, &open_pattern()) } {
        Ok(found) => found,
        Err(why) => {
            return fail(format!(
                "could not find the MG reload prompt's call -- {why}"
            ));
        }
    };
    // Safety: the pattern matched, so the push's operand is mapped code.
    let key = unsafe { ((found + KEY_AT) as *const u32).read_unaligned() } as usize;
    if !(base..base + 0x0100_0000).contains(&key)
        || unsafe { std::slice::from_raw_parts(key as *const u8, KEY.len()) } != KEY
    {
        return fail(format!(
            "the call at +{:#x} does not queue Dod_mg_reload",
            found + CALL_AT - base
        ));
    }
    let address = found + CALL_AT;
    // The stock target, from the pinned bytes rather than what is there now,
    // which may be our own filter from an earlier load.
    ORIGINAL.store(call_target(address, &stock_call()), Ordering::Release);
    ADDRESS.store(address, Ordering::Release);
    BASE.store(base, Ordering::Release);
    Ok(address)
}

#[cfg(target_arch = "x86")]
type QueueFn = unsafe extern "thiscall" fn(
    this: *mut std::ffi::c_void,
    key: *const std::ffi::c_char,
    x: i32,
    y: i32,
    r: f32,
    g: f32,
    b: f32,
);

/// What the call goes to while the cvar is on: the stock call's arguments and
/// convention (`__thiscall`, six stack arguments, `ret 0x18`), and nothing
/// queued.
#[cfg(target_arch = "x86")]
unsafe extern "thiscall" fn filtered_mg_reload(
    _this: *mut std::ffi::c_void,
    _key: *const std::ffi::c_char,
    _x: i32,
    _y: i32,
    _r: f32,
    _g: f32,
    _b: f32,
) {
    if DROPPED.fetch_add(1, Ordering::Relaxed) == 0 {
        unsafe { crate::debug::report("mg_reload_hint: dropped the first MG reload prompt") };
    }
}

/// Our filter's address, for the call.
#[cfg(target_arch = "x86")]
fn filter_address() -> Option<usize> {
    Some(filtered_mg_reload as QueueFn as usize)
}

/// Only the 32-bit DLL patches; a host build keeps the rest for its tests.
#[cfg(not(target_arch = "x86"))]
fn filter_address() -> Option<usize> {
    None
}

/// Puts the call in the wanted state, returning whether anything was written.
fn apply(base: usize, hidden: bool) -> Result<bool, String> {
    let address = resolve(base)?;
    let filter = filter_address().ok_or("only the 32-bit build patches the game")?;
    let stock = call_bytes(address, ORIGINAL.load(Ordering::Acquire));
    let ours = call_bytes(address, filter);
    let want = if hidden { ours } else { stock };
    // Safety: `resolve` proved the span is mapped code.
    let present: [u8; CALL_LEN] = unsafe { *(address as *const [u8; CALL_LEN]) };
    if present == want {
        return Ok(false);
    }
    if present != stock && present != ours {
        // An earlier load of this DLL left its own filter here: that address
        // is gone, so the stock call is the only safe thing to write.
        if present[0] != 0xe8 {
            return Err(format!(
                "the MG reload prompt's call holds {present:02x?}, not a call -- something else has patched it"
            ));
        }
    }
    // Safety: as above; written from the game's own thread between frames.
    if !unsafe { crate::patch::write_code_bytes(address, &want) } {
        return Err("could not make the MG reload prompt's call writable".into());
    }
    Ok(true)
}

/// Follows `ds_hud_hide_hints`. Called every frame from `hint_panel::poll`.
pub fn poll(wanted: bool) {
    if wanted {
        TOUCHED.store(true, Ordering::Relaxed);
    } else if !TOUCHED.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::client_module_base() else {
        return;
    };
    match apply(base, wanted) {
        Ok(wrote) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            HIDDEN_NOW.store(wanted, Ordering::Relaxed);
            if wrote {
                unsafe {
                    crate::debug::report(&format!(
                        "mg_reload_hint: the MG reload prompt {}",
                        if wanted { "is hidden" } else { "shows again" }
                    ))
                };
            }
        }
        Err(why) => {
            HIDDEN_NOW.store(false, Ordering::Relaxed);
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("mg_reload_hint: not applied yet -- {why}"))
                };
            }
        }
    }
}

/// Part of `ds_hud_hide_hints`'s `ds_debug_status` line.
pub fn status() -> String {
    if HIDDEN_NOW.load(Ordering::Relaxed) {
        format!(
            "the MG reload prompt is hidden ({} dropped this session)",
            DROPPED.load(Ordering::Relaxed)
        )
    } else {
        "the MG reload prompt shows".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_parses_and_indexes_the_key_and_the_call() {
        assert!(scan::Pattern::parse(PATTERN).is_ok());
        assert!(scan::Pattern::parse(&open_pattern()).is_ok());
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(tokens[KEY_AT - 1], "68", "push imm32: the key");
        assert!(tokens[KEY_AT..KEY_AT + 4].iter().all(|t| *t == "??"));
        assert_eq!(tokens[CALL_AT - 5], "B9", "mov ecx, imm32: the object");
        assert_eq!(tokens[CALL_AT], "E8");
        assert!(
            tokens[CALL_AT + 1..CALL_AT + CALL_LEN]
                .iter()
                .all(|t| *t != "??"),
            "the call's rel32 is pinned"
        );
        assert_eq!(
            &tokens[CALL_AT + CALL_LEN..],
            &["8A", "C3", "5E", "5B", "C3"]
        );
    }

    /// Six dwords go on the stack before the call: the callee's `ret 0x18`.
    #[test]
    fn six_arguments_are_pushed() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        let pushes = tokens[..CALL_AT - 5]
            .iter()
            .filter(|t| **t == "68" || **t == "6A")
            .count();
        assert_eq!(pushes, 6);
    }

    #[test]
    fn the_open_pattern_leaves_only_the_call_open() {
        let open = open_pattern();
        let a: Vec<&str> = PATTERN.split_whitespace().collect();
        let b: Vec<&str> = open.split_whitespace().collect();
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if (CALL_AT..CALL_AT + CALL_LEN).contains(&i) {
                assert_eq!(*y, "??");
            } else {
                assert_eq!(x, y);
            }
        }
        assert_eq!(stock_call(), [0xe8, 0x7a, 0xe9, 0xff, 0xff]);
    }

    #[test]
    fn the_key_is_nul_terminated() {
        assert_eq!(KEY.last(), Some(&0));
        assert_eq!(&KEY[..KEY.len() - 1], b"Dod_mg_reload");
    }
}
