//! When the scope draws in a POV demo: when the frame is drawn zoomed.
//!
//! `DrawTriApiScope` draws only once "the zoom has settled": the HUD's FOV
//! (`gHUD.m_iFOV`, `client+0x1082e4`, what the quads are sized for) must
//! equal `in_fov` (`client+0x18a85c`), which `CHud::UpdateClientData`
//! (`+0x3a2aa`) copies from the engine's client data at the start of the
//! frame, before `Think` works out this frame's FOV. In a POV demo neither
//! is reliably the FOV the frame is drawn with:
//!
//! - `2021draft-rd1-harrington-axis` (#933): the HUD has the zoom (20) a
//!   frame before the frame is drawn zoomed; `in_fov` follows the drawn FOV
//!   there, so the test holds the scope back, as it should.
//! - `wsod25_ply2_m1_h2_m00cat` (anzio, 300 fps capture): the frame is
//!   drawn zoomed 3 or 4 frames (one recorded frame) before `in_fov`
//!   catches up, so every scope-in starts with zoomed frames and no scope.
//!   At 60 fps that shows on the scope-ins whose zoom lands between two
//!   captured frames' worth of recorded frames, not on the others.
//!
//! So in a POV demo the test's `in_fov` read (`fld [in_fov]` at the
//! function's entry, `+0x46696`, its only reader) is pointed at
//! [`DRAWN_FOV`], which [`measure`] fills just before the scope is drawn:
//! the HUD's FOV when the frame is drawn zoomed, measured from the engine's
//! own projection (`WorldToScreen`), and 0 (no match) when it isn't. The
//! scope then draws on exactly the frames drawn zoomed, whichever of the
//! two values runs late. HLTV keeps the stock read: there the test is
//! removed outright (`skip_settle_check`).

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// `DrawTriApiScope`'s entry (`+0x46696`): `fld [in_fov]; push ebx; push
/// ebp; push esi; mov esi, ecx; push edi; mov bl, 1; mov eax, [esi+0x14];
/// mov [esp+0x10], eax; call ftol; mov edi, [gHUD.m_iFOV]; lea ecx,
/// [esp+0x54]`.
const ENTRY_PATTERN: &str = "D9 05 ?? ?? ?? ?? 53 55 56 8B F1 57 B3 01 8B 46 14 \
                             89 44 24 10 E8 ?? ?? ?? ?? 8B 3D ?? ?? ?? ?? 8D 4C 24 54";
/// The `fld [in_fov]` operand: the value the HUD's FOV is compared with.
const IN_FOV_OPERAND_AT: usize = 2;
/// The `mov edi, [gHUD.m_iFOV]` operand.
const HUD_FOV_OPERAND_AT: usize = 28;

/// The FOV the test compares the HUD's with, as f32 bits, read in place.
static DRAWN_FOV: [AtomicU32; 1] = [const { AtomicU32::new(0) }; 1];
/// The read's stock operand (`in_fov`), kept before the first patch.
static STOCK_IN_FOV: [AtomicU32; 1] = [const { AtomicU32::new(0) }; 1];
static ENTRY_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static ENTRY_SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// `gHUD.m_iFOV`'s address, from the entry's own operand.
static HUD_FOV_ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The last value [`measure`] wrote, for `status_line`.
static LAST: AtomicI32 = AtomicI32::new(-1);

/// The start of [`ENTRY_PATTERN`] in the loaded `client.dll`.
fn entry_site() -> Result<usize, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if ENTRY_SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ENTRY_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, ENTRY_PATTERN) }
        .map_err(|why| format!("could not find the scope's FOV reads -- {why}"))?;
    // Safety: inside the pattern just matched, so mapped code.
    let hud_fov =
        unsafe { std::ptr::read_unaligned((found + HUD_FOV_OPERAND_AT) as *const u32) } as usize;
    HUD_FOV_ADDRESS.store(hud_fov, Ordering::Release);
    ENTRY_ADDRESS.store(found, Ordering::Release);
    ENTRY_SCANNED_BASE.store(base, Ordering::Release);
    Ok(found)
}

/// Points the test's `in_fov` read at [`DRAWN_FOV`] (`drawn`, a POV demo)
/// or back at `in_fov`. Returns whether it wrote.
pub(super) fn point(drawn: bool) -> Result<bool, String> {
    super::point_operands(
        entry_site()?,
        &[IN_FOV_OPERAND_AT],
        &DRAWN_FOV,
        &STOCK_IN_FOV,
        drawn,
        "the scope's zoom-settled FOV read",
    )
}

/// Whether a frame whose projection reaches `drawn_tan` (`tan` of half its
/// horizontal FOV) is drawn zoomed to the HUD's `hud_fov`: nearer, in
/// tangent terms, to the zoom than to an unzoomed 90. A zoomed frame is
/// `hud_fov` wide, or a little wider when the engine widens it for the
/// screen (`gl_widescreen_yfov`); an unzoomed one is 90 or wider (HLAE's
/// `mirv_fov 105`).
fn drawn_zoomed(hud_fov: i32, drawn_tan: f32) -> bool {
    if !(1..90).contains(&hud_fov) || !drawn_tan.is_finite() || drawn_tan <= 0.0 {
        return false;
    }
    let hud_tan = (hud_fov as f32 / 2.0).to_radians().tan();
    // The geometric midpoint of hud_tan and tan(45) = 1.
    drawn_tan * drawn_tan < hud_tan
}

/// `tan` of half the drawn frame's horizontal FOV, from where the engine's
/// projection puts a point 45 degrees to the right of the view.
fn drawn_tan() -> Option<f32> {
    let engfuncs = engine::engfuncs()?;
    let studio = engine::engine_studio()?;
    let tri = engfuncs.p_tri_api;
    if tri.is_null() {
        return None;
    }
    let (mut origin, mut up, mut right, mut forward) = ([0f32; 3], [0f32; 3], [0f32; 3], [0f32; 3]);
    // Safety: the engine's own accessor, writing three floats into each.
    unsafe {
        (studio.get_view_info)(
            origin.as_mut_ptr(),
            up.as_mut_ptr(),
            right.as_mut_ptr(),
            forward.as_mut_ptr(),
        )
    };
    let point: [f32; 3] = std::array::from_fn(|k| origin[k] + forward[k] * 8.0 + right[k] * 8.0);
    let mut screen = [0f32; 3];
    // Safety: the engine's TriAPI, three floats in and out, on its thread.
    let behind = unsafe { ((*tri).world_to_screen)(point.as_ptr(), screen.as_mut_ptr()) } != 0;
    // The point is at x = 1 / tan(half FOV) on the screen (-1..1).
    (!behind && screen[0] > 0.0).then(|| 1.0 / screen[0])
}

/// Fills [`DRAWN_FOV`] for this frame, just before the client draws the
/// scope. Only while the read points at it (a POV demo, the setting on).
pub(super) fn measure() {
    let at = ENTRY_ADDRESS.load(Ordering::Acquire);
    let hud = HUD_FOV_ADDRESS.load(Ordering::Acquire);
    if at == 0 || hud == 0 {
        return;
    }
    // Safety: inside the scanned entry, mapped code.
    let present = unsafe { std::ptr::read_unaligned((at + IN_FOV_OPERAND_AT) as *const u32) };
    if present != &DRAWN_FOV[0] as *const AtomicU32 as usize as u32 {
        return;
    }
    // Safety: `gHUD.m_iFOV`, the client's own data, read on its thread.
    let hud_fov = unsafe { std::ptr::read_unaligned(hud as *const i32) };
    let fov = match drawn_tan() {
        Some(tan) if drawn_zoomed(hud_fov, tan) => hud_fov,
        Some(_) => 0,
        // No measure: what the stock test reads.
        None => {
            let stock = STOCK_IN_FOV[0].load(Ordering::Acquire) as usize;
            if stock == 0 {
                return;
            }
            // Safety: `in_fov`, the client's own float, kept from the stock
            // operand.
            unsafe { std::ptr::read_unaligned(stock as *const f32) as i32 }
        }
    };
    DRAWN_FOV[0].store((fov as f32).to_bits(), Ordering::Relaxed);
    LAST.store(fov, Ordering::Relaxed);
}

/// For `status_line`.
pub(super) fn describe_last() -> String {
    match LAST.load(Ordering::Relaxed) {
        -1 => "zoom read from the game".to_string(),
        0 => "last frame drawn unzoomed".to_string(),
        fov => format!("last frame drawn zoomed to {fov}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tan_of(fov: f32) -> f32 {
        (fov / 2.0).to_radians().tan()
    }

    /// The horizontal FOV a 4:3 `fov` becomes at 16:9 with the vertical
    /// one kept (`gl_widescreen_yfov 1`).
    fn widened(fov: f32) -> f32 {
        tan_of(fov) * (16.0 / 9.0) / (4.0 / 3.0)
    }

    #[test]
    fn the_operands_follow_their_opcodes() {
        let t: Vec<&str> = ENTRY_PATTERN.split_whitespace().collect();
        assert_eq!(&t[IN_FOV_OPERAND_AT - 2..IN_FOV_OPERAND_AT], &["D9", "05"]);
        assert_eq!(
            &t[HUD_FOV_OPERAND_AT - 2..HUD_FOV_OPERAND_AT],
            &["8B", "3D"]
        );
        for at in [IN_FOV_OPERAND_AT, HUD_FOV_OPERAND_AT] {
            assert!(t[at..at + 4].iter().all(|b| *b == "??"), "operand at {at}");
        }
        assert_ne!(t[0], "??", "find_unique refuses a leading wildcard");
    }

    #[test]
    fn a_zoomed_frame_draws_the_scope() {
        for hud in [20, 25, 40, 55] {
            let fov = hud as f32;
            assert!(drawn_zoomed(hud, tan_of(fov)), "{hud}, as is");
            assert!(drawn_zoomed(hud, widened(fov)), "{hud}, widened");
        }
    }

    #[test]
    fn an_unzoomed_frame_holds_it_back() {
        // #933: the HUD has the zoom while the frame is still drawn at 90,
        // at 105 under mirv_fov, or widened for 16:9.
        for hud in [20, 25, 40, 55] {
            for drawn in [tan_of(90.0), tan_of(105.0), widened(90.0)] {
                assert!(!drawn_zoomed(hud, drawn), "{hud} on {drawn}");
            }
        }
    }

    #[test]
    fn no_zoom_on_the_hud_draws_nothing() {
        for hud in [0, 90, 105, -3] {
            assert!(!drawn_zoomed(hud, tan_of(20.0)), "{hud}");
        }
        for bad in [0.0, -0.2, f32::NAN, f32::INFINITY] {
            assert!(!drawn_zoomed(20, bad), "{bad}");
        }
    }
}
