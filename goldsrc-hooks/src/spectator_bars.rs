//! R&D for a `dodtools_hide_spectator_bars` cvar to hide the top and bottom
//! spectator UI without needing `mirv_recordmovie_start` running. **Parked,
//! not wired into the crate** (`lib.rs` doesn't declare this as a `mod`) --
//! two live-tested approaches both turned out to be dead ends; see the
//! bottom two sections for exactly what's ruled out and what the real next
//! step is (a live memory watch, not more static analysis). Kept on disk
//! rather than deleted so the next attempt doesn't re-derive any of this
//! from scratch -- the class hierarchy, the ownership chain and the
//! `SetVisible` slot are all still correct, they just don't explain the
//! bars' actual visibility.
//!
//! ## The problem this exists for
//!
//! HLAE's `mirv_movie_hidepanels`/`mirv_disable_specmenu` only hide panels
//! from the *capture* -- they stay on screen, and `mirv_disable_specmenu`
//! does not support `dod` at all (its own "Supported modifications" list is
//! `tfc`/`valve`). Editing `resource/ui/Spectator.res` does hide the top
//! bar, but the same edit to `resource/ui/BottomSpectator.res` does nothing
//! -- the bottom bar ignores it. Full investigation in
//! `docs/goldsrc_spectator_bars.md`; this module is its ending.
//!
//! ## Why the bottom bar ignores `.res`
//!
//! Both bars turned out to be `vgui2::Frame` (the newer VGUI2 hierarchy,
//! statically linked into `client.dll` -- not the `vgui.dll` VGUI1 runtime
//! their RTTI names suggest). Neither overrides `SetVisible`; both inherit
//! `Frame`'s own stock implementation unmodified. There is no DoD-authored
//! "propagate visibility" hook to patch, and no obvious per-instance flag to
//! flip -- so this does not try to read the `.res` scheme or find a "is
//! visible" byte. Instead it intercepts the one call every visibility
//! change already goes through: it redirects each class' *own* vtable slot
//! for `SetVisible` to a small trampoline that forces the boolean argument
//! to `false` and tails into the untouched stock function -- so every
//! internal side effect `Frame::SetVisible` normally has (there is an
//! animation controller it notifies) still happens, just always told to
//! hide.
//!
//! ## Why the vtable and not the function
//!
//! `crosshair.rs`/`scoreboard.rs` patch a function's own bytes, which works
//! when the function is safe to change for *everyone* who calls it.
//! `Frame::SetVisible` is not: every VGUI2 `Frame` in the client shares it
//! (menus, dialogs, the scoreboard's own dialog chrome), so stubbing its
//! code would hide all of them. `CDoDSpectatorGUI` and `CBottomBar` each
//! have their *own* vtable array in `.rdata` even though the function
//! pointer they currently hold is identical (both simply inherit `Frame`'s,
//! unoverridden) -- so redirecting one class' array, like
//! `hudelement.rs` already does for `Draw`, touches only that class.
//!
//! ## Reaching the two objects
//!
//! `gViewPort` (`+0x19d564`, the same global `scoreboard.rs`'s
//! `+showscores` patch already reads through) holds `DoDViewport*`.
//! `CDoDSpectatorGUI` is a direct member, `DoDViewport::CDoDSpectatorGUI*
//! m_pSpectatorGUI` at `+0x740` -- confirmed by disassembling the `new` +
//! constructor pair that builds it. `CBottomBar` is in turn a member of
//! `CSpectatorGUI`, `CDoDSpectatorGUI`'s own base class, at `+0x114`
//! (`CDoDSpectatorGUI`'s constructor calls `CSpectatorGUI`'s at
//! `client+0x1da42`, and `CSpectatorGUI::CSpectatorGUI` -- `client+0x82b50`
//! -- is the one and only place in the image that constructs a `CBottomBar`
//! and stores it there). Since `CSpectatorGUI` is `CDoDSpectatorGUI`'s
//! primary, offset-0 base, both offsets apply directly off the same
//! `CDoDSpectatorGUI*`.
//!
//! ## Live-tested, and it did not work (2026-09-21)
//!
//! The redirect installs with no error (RTTI and stock-value checks all
//! pass), but both bars stayed visible. The vtable-slot identity is
//! re-confirmed correct by that same test -- three of `CDoDSpectatorGUI`'s
//! and `CBottomBar`'s other "overridden" slots (27, 29, 30 -- 28 is the
//! destructor) turned out to be either a cached-string getter or pure
//! pass-through adjustor thunks wrapping the exact unmodified `Frame`
//! function, not a `Paint` override -- so stubbing a render function instead
//! is not an available fallback; these container classes apparently draw
//! nothing of their own, only their (separately-classed) children do.
//!
//! That leaves one real open question: whether `SetVisible` is ever called
//! on these two objects at all during ordinary play. A virtual call site
//! can't be enumerated by scanning for a fixed address the way a direct
//! `call rel32` can, so [`HIT_COUNT`] answers it empirically instead -- the
//! trampoline increments it on every invocation, and `dodtools_debug_status`
//! reports it.
//!
//! ## Confirmed dead: the hit count is 0 (2026-09-21, same session)
//!
//! A second live test, with the redirect on, never once incremented
//! [`HIT_COUNT`]. `SetVisible` is not the mechanism, full stop -- whatever
//! applies `Spectator.res`'s initial value does it once, at construction,
//! through some path this vtable slot is never on.
//!
//! Chased one step further anyway: `SetVisible`'s own first call,
//! `call 0x1957b40`, is *not* the low-level setter it looks like. It
//! disassembles to a bare `ret 4` -- an empty stub, byte-identical to (and
//! very likely COMDAT-folded together with) dozens of other unrelated
//! no-op stubs across the binary, including `Panel@vgui2`'s own
//! never-overridden base version of this same slot. Reading the rest of the
//! function's body with that ruled out: it never writes a persisted flag on
//! `this` anywhere. It reads `this`'s *current* visibility (a call through
//! `this`'s own vtable, `[eax+0x68]`), then hands both the old and new
//! state to a *separate* animation-controller object
//! (`call 0x1967370` looks like `GetAnimationController()`) via a call to
//! `[ebp+0xdc]` on it. `Frame::SetVisible`, in other words, does not
//! synchronously set anything here -- it queues an animated transition on
//! another object, which is presumably what eventually writes the real
//! flag, later, driven by its own per-frame ticking. So even if something
//! is found that does call this slot, clamping its argument would not
//! reliably force an immediate hide the way this module assumed.
//!
//! ## The actual next step
//!
//! Static analysis has now produced two wrong guesses in a row for this
//! specific question (the vtable slot itself, then this helper). The
//! reliable way to find what really sets a `CBottomBar`/`CDoDSpectatorGUI`
//! instance's visible flag is a live memory watch: attach a debugger to a
//! running session, set a hardware write-breakpoint on the field, and
//! toggle the already-working `Spectator.res` edit to see what writes it.
//! Not something this offline `pefile`/`capstone` toolchain can do.
//!
//! ## Sharper symptom, same day: it's the background, not the whole bar
//!
//! The user can already edit individual *items* on the bottom bar (the
//! mode/player/view comboboxes) via `BottomSpectator.res` -- those respond
//! fine. What doesn't respond is the black background/frame itself. That
//! changes what "the bottom bar ignores `.res`" actually means: the
//! children's own `visible`/`enabled` keys clearly do reach them, so
//! `BottomSpectator.res` is being read and applied to *something* -- just
//! not to whatever draws the black backdrop. Two real possibilities worth
//! checking first, next time, before touching any code:
//!
//! - The backdrop is `CBottomBar`'s own `Frame::PaintBackground` (inherited,
//!   generic -- reads a scheme border/color resource, not a `visible` key at
//!   all), separate from whatever visibility mechanism gates the children.
//! - The backdrop isn't `CBottomBar` at this point in the tree -- it could
//!   be a parent/sibling panel `BottomSpectator.res` doesn't even declare a
//!   section for, drawn unconditionally by something else entirely.
//!
//! ## Narrower goal for next time (2026-09-21): fix the `.res` file, not the game
//!
//! The user already has a working, no-code fix for the top bar --
//! `Spectator.res`'s `visible`/`enabled` keys, edited by hand. They'd be
//! fine with the same thing for the bottom bar instead of a
//! `dodtools_hide_spectator_bars` command, which changes the actual
//! question worth investigating next: not "how do we force `CBottomBar`
//! hidden at runtime" but "why does `BottomSpectator.res`'s `visible` key
//! not reach `CBottomBar` the way `Spectator.res`'s reaches
//! `CDoDSpectatorGUI`". That's a *narrower*, more answerable question --
//! likely something in how `CSpectatorGUI::CSpectatorGUI` (`+0x82b50`, see
//! above) constructs `CBottomBar`: does it even pass `BottomSpectator.res`'s
//! path/section to `CBottomBar`'s own `LoadControlSettings`-equivalent, or
//! does that call happen with the wrong resource name, get skipped, or get
//! overwritten by something right after? Worth tracing the construction
//! call site itself (`+0x82c66`) forward, rather than continuing to chase
//! `SetVisible`, next time this is picked up.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc};

use crate::engine;
use crate::names::console_name;

/// The cvar name, for status and error text. Registered in `commands.rs`.
pub const NAME: &str = console_name!("hide_spectator_bars");

/// A vtable is per-*class*, not per-instance, so patching it needs no live
/// object at all -- unlike the ownership chain that proves these classes are
/// real and constructed (`gViewPort` -> `DoDViewport::m_pSpectatorGUI` at
/// `+0x740` -> `CSpectatorGUI::m_pBottomBar` at `+0x114`, see the module
/// doc), which is why none of those three offsets appear as code here.
///
/// `SetVisible`'s position in `vgui2::Frame`'s vtable -- empirically
/// derived (not from a public header): `Panel@vgui2`'s own slot 8 is a
/// trivial 3-byte stub, `Frame` replaces it with a real 77-byte, one-bool
/// (`ret 4`) function, and both `CDoDSpectatorGUI` and `CBottomBar` inherit
/// that replacement unchanged.
const SET_VISIBLE_SLOT: usize = 8;

/// `.?AVCDoDSpectatorGUI@@`'s own vtable (the primary of its two -- offset
/// 0, not the `+0x10c` `ISpectatorInterface` one).
const TOP_BAR_VFTABLE_RVA: usize = 0xaab84;

/// `.?AVCBottomBar@@`'s vtable.
const BOTTOM_BAR_VFTABLE_RVA: usize = 0xb51b4;

/// The stock `SetVisible` both classes currently share, inherited from
/// `Frame` -- an ILT jump thunk (`client.dll`'s vgui2-controls code is
/// incrementally linked), not the function body itself. Tailing into the
/// thunk rather than resolving its target keeps this independent of where
/// the linker happened to place the real body.
const STOCK_SET_VISIBLE_RVA: usize = 0x61420;

/// Whether the bars are currently asked to be hidden.
static HIDDEN_NOW: AtomicBool = AtomicBool::new(false);

/// The trampoline's address, or 0 before the first successful build.
static TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);

/// The module base [`TRAMPOLINE`] was built against. Same guard
/// `crosshair.rs`/`scoreboard.rs` keep: a `client.dll` reloaded at a
/// different address (not observed for a plain demo change,
/// `docs/goldsrc_dod_quirks.md`) gets a freshly built trampoline rather
/// than one pointing at a stale address.
static BUILT_BASE: AtomicUsize = AtomicUsize::new(0);

/// How many times the trampoline has actually run. A first live test
/// (2026-09-21) installed the redirect with no error and left both bars
/// visible -- this settles the open question that leaves, cheaply: whether
/// `SetVisible` is ever called on these two objects at all during ordinary
/// play. Incremented by the trampoline itself (`inc dword ptr [addr]`,
/// baked in at build time), not by anything on the Rust side.
static HIT_COUNT: AtomicU32 = AtomicU32::new(0);

/// How many times the trampoline has run, for `dodtools_debug_status`.
pub fn hit_count() -> u32 {
    HIT_COUNT.load(Ordering::Relaxed)
}

unsafe fn read_u32(address: usize) -> u32 {
    unsafe { (address as *const u32).read_unaligned() }
}

/// The decorated class name `vftable[-1]`'s RTTI leads to. Mirrors
/// `hudelement.rs`'s own helper of the same name -- MSVC's 32-bit layout:
/// slot -1 is a `RTTICompleteObjectLocator*`, whose fourth dword is a
/// `TypeDescriptor*`, whose name starts eight bytes in.
///
/// Safety: `vftable` must be a mapped address inside the module.
unsafe fn rtti_class_name(vftable: usize) -> Option<String> {
    unsafe {
        let locator = read_u32(vftable - 4) as usize;
        if locator == 0 {
            return None;
        }
        if read_u32(locator) != 0 {
            return None;
        }
        let descriptor = read_u32(locator + 0x0c) as usize;
        if descriptor == 0 {
            return None;
        }
        let name = std::ffi::CStr::from_ptr((descriptor + 8) as *const std::ffi::c_char);
        name.to_str().ok().map(str::to_owned)
    }
}

/// `mov dword ptr [esp+4], 0` (8 bytes), `inc dword ptr [hit_count_address]`
/// (6 bytes), then `jmp rel32` (5 bytes) to the stock thunk. `[esp+4]` is the
/// caller's pushed bool argument at trampoline entry -- confirmed against the
/// real `SetVisible` body's own read of the same slot one push deeper
/// (`mov ebx, [esp+8]`, after its own `push ebx`). A `jmp`, not a `call`, so
/// the stack the trampoline was entered with -- return address included --
/// reaches the stock function completely unchanged; its own `ret 4` returns
/// straight to the original caller.
fn build_trampoline(hit_count_address: usize) -> Vec<u8> {
    let mut code = vec![0xc7, 0x44, 0x24, 0x04, 0x00, 0x00, 0x00, 0x00];
    code.push(0xff);
    code.push(0x05);
    code.extend_from_slice(&(hit_count_address as u32).to_le_bytes());
    code.push(0xe9);
    code.extend_from_slice(&0u32.to_le_bytes()); // patched by the caller once the stub's own address is known
    code
}

/// Resolves (building the trampoline once per module base) and returns its
/// address.
fn trampoline_address() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };

    if BUILT_BASE.load(Ordering::Acquire) == base {
        let cached = TRAMPOLINE.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }

    let top_vft = base + TOP_BAR_VFTABLE_RVA;
    let top_name = unsafe { rtti_class_name(top_vft) };
    if top_name.as_deref() != Some(".?AVCDoDSpectatorGUI@@") {
        return Err(format!(
            "+{TOP_BAR_VFTABLE_RVA:#x} identifies as {top_name:?}, not .?AVCDoDSpectatorGUI@@ -- this is not the client.dll this module describes"
        ));
    }
    let bottom_vft = base + BOTTOM_BAR_VFTABLE_RVA;
    let bottom_name = unsafe { rtti_class_name(bottom_vft) };
    if bottom_name.as_deref() != Some(".?AVCBottomBar@@") {
        return Err(format!(
            "+{BOTTOM_BAR_VFTABLE_RVA:#x} identifies as {bottom_name:?}, not .?AVCBottomBar@@ -- this is not the client.dll this module describes"
        ));
    }

    let stock_address = base + STOCK_SET_VISIBLE_RVA;
    for (name, vft) in [("CDoDSpectatorGUI", top_vft), ("CBottomBar", bottom_vft)] {
        let present = unsafe { read_u32(vft + SET_VISIBLE_SLOT * 4) } as usize;
        if present != stock_address {
            return Err(format!(
                "{name}'s SetVisible slot holds +{:#x}, not the expected stock +{STOCK_SET_VISIBLE_RVA:#x} -- something else has already patched it",
                present.wrapping_sub(base)
            ));
        }
    }

    let hit_count_address = &HIT_COUNT as *const AtomicU32 as usize;
    let mut code = build_trampoline(hit_count_address);

    // Safety: a fresh RWX page we own, sized for exactly the bytes copied in.
    let stub = unsafe {
        VirtualAlloc(std::ptr::null(), code.len(), MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE)
    };
    if stub.is_null() {
        return Err("could not allocate an executable page for the trampoline".to_string());
    }
    let stub_address = stub as usize;
    let displacement = stock_address.wrapping_sub(stub_address + code.len()) as u32;
    let jmp_immediate = code.len() - 4;
    code[jmp_immediate..].copy_from_slice(&displacement.to_le_bytes());
    // Safety: `stub` is a page of at least `code.len()` bytes.
    unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), stub as *mut u8, code.len()) };

    TRAMPOLINE.store(stub_address, Ordering::Release);
    BUILT_BASE.store(base, Ordering::Release);
    Ok(stub_address)
}

/// Applies or removes the redirect on both classes' own vtables, returning
/// whether anything was actually written. `hidden` is the cvar's own sense:
/// `true` forces both bars invisible, `false` is the game's stock
/// behaviour.
///
/// Idempotent and cheap to call every frame: a short pointer compare once
/// the trampoline exists, which is how `crosshair.rs`/`scoreboard.rs`'s
/// equivalents are used.
pub fn set_hidden(hidden: bool) -> Result<bool, String> {
    let trampoline = trampoline_address()?;
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    let stock_address = base + STOCK_SET_VISIBLE_RVA;
    let want = if hidden { trampoline } else { stock_address };

    let mut written = false;
    for vftable_rva in [TOP_BAR_VFTABLE_RVA, BOTTOM_BAR_VFTABLE_RVA] {
        let slot = base + vftable_rva + SET_VISIBLE_SLOT * 4;
        let present = unsafe { read_u32(slot) } as usize;
        if present == want {
            continue;
        }
        if present != stock_address && present != trampoline {
            return Err(format!(
                "+{vftable_rva:#x}'s SetVisible slot holds +{:#x}, which is neither the stock function nor this module's trampoline -- something else has patched it",
                present.wrapping_sub(base)
            ));
        }
        if !unsafe { crate::patch::write_code_bytes(slot, &(want as u32).to_le_bytes()) } {
            return Err(format!("could not make +{vftable_rva:#x}'s vtable writable"));
        }
        written = true;
    }
    HIDDEN_NOW.store(hidden, Ordering::Release);
    Ok(written)
}

/// Whether the bars are currently asked to be hidden.
pub fn hidden() -> bool {
    HIDDEN_NOW.load(Ordering::Relaxed)
}

/// One line for `dodtools_debug_status`.
pub fn status() -> String {
    if !hidden() {
        return "both spectator bars draw normally".into();
    }
    format!(
        "CDoDSpectatorGUI and CBottomBar's own SetVisible is redirected to force them hidden \
         -- confirmed non-functional (docs/goldsrc_spectator_bars.md): trampoline hit count = {} \
         after a live session, meaning SetVisible is never called on either object during \
         ordinary play",
        hit_count()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trampoline_is_a_clamp_a_counter_then_a_tail_jump() {
        let code = build_trampoline(0x1234_5678);
        assert_eq!(code.len(), 19);
        // mov dword ptr [esp+4], 0
        assert_eq!(&code[0..8], &[0xc7, 0x44, 0x24, 0x04, 0x00, 0x00, 0x00, 0x00]);
        // inc dword ptr [hit_count_address]
        assert_eq!(&code[8..10], &[0xff, 0x05]);
        assert_eq!(&code[10..14], &0x1234_5678u32.to_le_bytes());
        // jmp rel32
        assert_eq!(code[14], 0xe9);
    }

    #[test]
    fn set_visible_slot_is_the_established_constant() {
        // Not a load-bearing assertion -- pins the one number the whole
        // module's reasoning depends on, so an accidental edit fails loudly
        // here rather than silently at runtime.
        assert_eq!(SET_VISIBLE_SLOT, 8);
    }

    #[test]
    fn status_names_the_two_classes_and_points_at_the_writeup() {
        HIDDEN_NOW.store(true, Ordering::Release);
        let text = status();
        assert!(text.contains("CDoDSpectatorGUI"));
        assert!(text.contains("CBottomBar"));
        assert!(text.contains("confirmed non-functional"));
        HIDDEN_NOW.store(false, Ordering::Release);
        assert!(status().contains("normally"));
    }
}
