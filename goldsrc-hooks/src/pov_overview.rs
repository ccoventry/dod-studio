//! `ds_debug_pov_overview`: R&D prototype that lets DoD's overview map (the
//! minimap, `_cl_minimap 2`, and the full map, `_cl_minimap 1`) draw in POV
//! demo playback (#897). Off by default.
//!
//! ## Why a POV demo shows no map
//!
//! `client.dll` draws the map in a second render view. Its `V_CalcRefdef`
//! sets `ref_params_s.nextView` to 1 whenever `CHud::OverviewMapMode` (that
//! is `_cl_minimap`, while the renderer is hardware and the FOV is 90) is
//! non-zero, for a living player (`V_CalcNormalRefdef`, `client+0x4f47f`) and
//! a spectator (`client+0x51365`) alike. On the second call it shrinks the
//! viewport to the map's rect and `HUD_DrawTransparentTriangles`
//! (`client+0x4c830`) draws the map in that view. Nothing in `client.dll`
//! asks whether the player is a spectator or HLTV.
//!
//! The gate is in `hw.dll`'s `V_RenderView`. In POV playback, right after the
//! first `ClientDLL_CalcRefdef`, it calls the demo view override (see
//! `pov_live_bob.rs`), which copies the recorded `ref_params_s` over the live
//! one, `nextView` included, and then stores **0** into `nextView`:
//!
//! ```text
//!     pre-Anniversary   hw.dll+0xc20c3   mov [ebp-0x14], esi      ; esi = 0
//!     25th Anniversary  hw.dll+0x231c86  mov dword [ebp-0xc], 0
//! ```
//!
//! (`ref_params_s` sits at `ebp-0xf4` / `ebp-0xec`; `nextView` is its
//! `+0xe0`.) So the loop never makes the map view. HLTV playback skips the
//! override (the engine's spectator flag) and live play records instead, so
//! both keep the client's `nextView` and draw the map.
//!
//! ## The change
//!
//! With the cvar on, that store writes the `nextView` the live
//! `V_CalcRefdef` worked out this frame ([`after_calc_refdef`]) instead of 0:
//! the viewer's `_cl_minimap` decides, not the recording. The span becomes a
//! jump to a stub that reproduces it with that value. Installed the first time
//! the cvar is on, and it stays: with the cvar back at 0 the stub stores 0, as
//! the game does. `tools/verify_pov_overview_offsets.py` re-derives both
//! builds' spans.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial, RefParamsPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("debug_pov_overview");

/// `ref_params_s.nextView`.
const NEXT_VIEW_AT: usize = 0xe0;

/// One build's store of 0 into `nextView` after the demo view override.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Where the stolen span starts in the pattern.
    span_at: usize,
    span_len: usize,
    /// Leading bytes of the span the stub copies as they are (instructions
    /// with no relative operand), before its own `nextView` store.
    keep: usize,
    /// `nextView`'s `ebp` displacement, as the span's store has it.
    frame_disp: u8,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         mov [imm],ecx      mov [imm],edx      mov [ebp-14],esi lea edx,[ebp-F4]  push call
        pattern: "89 0D ?? ?? ?? ?? 89 15 ?? ?? ?? ?? 89 75 EC 8D 95 0C FF FF FF 52 E8",
        // `mov [imm], edx` and the store: the store alone is 3 bytes, and the
        // instruction after it is a branch target.
        span_at: 6,
        span_len: 9,
        keep: 6,
        frame_disp: 0xEC,
    },
    Site {
        build: "25th Anniversary",
        //         movss xmm3,[ebp-E0]        add esp,0C movss xmm1,[ebp-DC]        movss xmm2,[ebp-D8]        cvtps2pd mov dword [ebp-C],0     movss [imm],xmm1
        pattern: "F3 0F 10 9D 20 FF FF FF 83 C4 0C F3 0F 10 8D 24 FF FF FF F3 0F 10 95 28 FF FF FF 0F 5A C3 C7 45 F4 00 00 00 00 F3 0F 11 0D",
        span_at: 30,
        span_len: 7,
        keep: 0,
        frame_disp: 0xF4,
    },
];

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// What the stub stores into `nextView`: the live one while the cvar is on,
/// else 0. The stub reads it by address.
static NEXT_VIEW: AtomicI32 = AtomicI32::new(0);
/// The `hw.dll` base the detour went into, 0 before.
static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);
static GAVE_UP: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn on() -> bool {
    let cvar = CVAR.load(Ordering::Acquire);
    // Safety: the engine owns the cvar for the session.
    !cvar.is_null() && unsafe { (*cvar).value } != 0.0
}

/// Called by `engine.rs` right before `client.dll`'s `V_CalcRefdef`: a call
/// that arrives with `nextView` already 1 is the engine's second (map) view.
pub fn before_calc_refdef(params: *const RefParamsPartial) {
    if params.is_null() || INSTALLED_BASE.load(Ordering::Relaxed) == 0 {
        return;
    }
    // Safety: as in `after_calc_refdef`.
    let next =
        unsafe { std::ptr::read_unaligned((params as *const u8).add(NEXT_VIEW_AT) as *const i32) };
    let counter = if next != 0 {
        &SECOND_VIEWS
    } else {
        &FIRST_VIEWS
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

/// Views counted since the detour went in, for `ds_debug_status`.
static FIRST_VIEWS: AtomicUsize = AtomicUsize::new(0);
static SECOND_VIEWS: AtomicUsize = AtomicUsize::new(0);

/// One line for `ds_debug_status`, once the detour is in.
pub fn status_line() -> Option<String> {
    (INSTALLED_BASE.load(Ordering::Relaxed) != 0).then(|| {
        format!(
            "{NAME} = {}: {} main views and {} map views rendered since it went in",
            on() as i32,
            FIRST_VIEWS.load(Ordering::Relaxed),
            SECOND_VIEWS.load(Ordering::Relaxed)
        )
    })
}

/// Called by `engine.rs` right after `client.dll`'s `V_CalcRefdef`. The demo
/// view override runs only after the first view's call, so what this keeps
/// when the stub reads it is that view's request.
pub fn after_calc_refdef(params: *const RefParamsPartial) {
    if params.is_null() || INSTALLED_BASE.load(Ordering::Relaxed) == 0 {
        return;
    }
    let value = if on() {
        // Safety: the engine's whole `ref_params_s`, valid for the call.
        unsafe { std::ptr::read_unaligned((params as *const u8).add(NEXT_VIEW_AT) as *const i32) }
    } else {
        0
    };
    NEXT_VIEW.store(value, Ordering::Relaxed);
}

/// The stub: the kept bytes, `nextView = [value]`, then back past the span.
fn stub_code(kept: &[u8], frame_disp: u8, value: usize, back: usize) -> Vec<u8> {
    let mut code = kept.to_vec();
    code.push(0x50); // push eax
    code.push(0xA1); // mov  eax, [value]
    code.extend_from_slice(&(value as u32).to_le_bytes());
    code.extend_from_slice(&[0x89, 0x45, frame_disp]); // mov [ebp+disp8], eax
    code.push(0x58); // pop  eax
    code.push(0x68); // push back; ret
    code.extend_from_slice(&(back as u32).to_le_bytes());
    code.push(0xC3);
    code
}

fn install() -> Result<&'static str, String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let mut errors = Vec::new();
    for site in SITES {
        // Safety: `base` is a module handle the loader gave us.
        let found = match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(found) => found,
            Err(why) => {
                errors.push(format!("{}: {why}", site.build));
                continue;
            }
        };
        let span = found + site.span_at;
        // Safety: the scan matched these bytes.
        let kept = unsafe { std::slice::from_raw_parts(span as *const u8, site.keep) }.to_vec();
        let code = stub_code(
            &kept,
            site.frame_disp,
            &NEXT_VIEW as *const AtomicI32 as usize,
            span + site.span_len,
        );
        // Safety: the pattern pins the span to whole instructions, and nothing
        // branches into it (checked by the verify script on both builds).
        let detour = unsafe { crate::detour::install(span, site.span_len, &code) }?;
        unsafe {
            crate::debug::report(&format!(
                "pov_overview: nextView store at hw.dll+{:#x} now asks {NAME} (stub at {:#x})",
                span - base,
                detour.stub_address()
            ))
        };
        INSTALLED_BASE.store(base, Ordering::Release);
        return Ok(site.build);
    }
    Err(format!(
        "the nextView store wasn't found ({})",
        errors.join("; ")
    ))
}

/// Installs the detour the first time the cvar is on. Every frame.
pub fn poll() {
    if INSTALLED_BASE.load(Ordering::Acquire) != 0 || GAVE_UP.load(Ordering::Relaxed) || !on() {
        return;
    }
    let line = match install() {
        Ok(build) => format!("{NAME}: on ({build} hw.dll)"),
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            format!("{NAME}: {why}")
        }
    };
    unsafe { crate::debug::report(&line) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(site: &Site) -> Vec<&'static str> {
        site.pattern.split_whitespace().collect()
    }

    #[test]
    fn each_span_ends_on_the_next_view_store() {
        for site in SITES {
            let t = tokens(site);
            let store = &t[site.span_at + site.keep..site.span_at + site.span_len];
            let disp = format!("{:02X}", site.frame_disp);
            match store {
                ["89", "75", d] => assert_eq!(*d, disp, "{}", site.build),
                ["C7", "45", d, "00", "00", "00", "00"] => assert_eq!(*d, disp, "{}", site.build),
                other => panic!("{}: {other:?} is not a store of 0", site.build),
            }
            assert!(site.span_len >= 5, "{}: room for a jump", site.build);
            assert!(t.len() > site.span_at + site.span_len, "{}", site.build);
        }
    }

    #[test]
    fn the_kept_bytes_have_no_wildcard_operand_of_a_relative_branch() {
        for site in SITES {
            let t = tokens(site);
            let kept = &t[site.span_at..site.span_at + site.keep];
            assert!(
                !matches!(kept.first(), Some(&"E8") | Some(&"E9") | Some(&"EB")),
                "{}",
                site.build
            );
        }
    }

    #[test]
    fn the_stub_stores_the_value_and_jumps_back() {
        let code = stub_code(&[0x89, 0x15, 1, 2, 3, 4], 0xEC, 0x1111_1111, 0x2222_2222);
        assert_eq!(&code[..6], &[0x89, 0x15, 1, 2, 3, 4]);
        assert_eq!(&code[6..12], &[0x50, 0xA1, 0x11, 0x11, 0x11, 0x11]);
        assert_eq!(&code[12..16], &[0x89, 0x45, 0xEC, 0x58]);
        assert_eq!(&code[16..], &[0x68, 0x22, 0x22, 0x22, 0x22, 0xC3]);
    }
}
