//! `dodtools_hide_hudelement`: hide any one of DoD's HUD elements, by name.
//!
//! ## One dword, not a code patch
//!
//! `CHud::Redraw` walks a linked list and calls each element's **vftable slot
//! 3**, `Draw`. `CHudBase::Draw` is `xor eax, eax; ret 4` -- a complete no-op
//! with the right calling convention, which five of the twenty-two elements
//! already use unchanged because they never override it.
//!
//! So hiding an element is writing that one address into its vftable slot 3.
//! No signature scan, no detour, no stub, no relocation, and nothing left
//! mid-instruction. Showing it again is writing back the dword that was there.
//!
//! `crosshair.rs` and `scoreboard.rs` patch code instead, and still should:
//! both overwrite a *function*, which needs no vftable address and reverts to
//! bytes their own signature already proved. This is the general case they are
//! not -- see #265, which asked for exactly this and kept them separate.
//!
//! ## Why the vftable and not `m_iFlags`
//!
//! Clearing bit 0 of an element's `m_iFlags` is per-*instance*, which sounds
//! better. The game writes that field itself from `Init`, `VidInit`, `Reset`
//! and in some cases `Draw`, so it would have to be re-applied against the
//! game's own writes. A vftable is written once by the constructor and never
//! touched again.
//!
//! ## The table is fixed RVAs, verified by name
//!
//! `scan.rs` argues against fixed offsets, and is right: a wrong RVA is a
//! plausible number, where a pattern that does not match is an error. A
//! vftable has no code to sign, so there is nothing to scan for -- but DoD's
//! `client.dll` ships **MSVC RTTI**, which is better than a signature.
//! `vftable[-1]` is a complete object locator, whose type descriptor carries
//! the class' decorated name, so every entry below is checked against the
//! string `.?AVCHudAmmo@@` and friends before anything is written. A wrong
//! build fails loudly and by name.
//!
//! (All three of the user's Half-Life installs ship a byte-identical
//! `client.dll`, so this is insurance rather than portability.)
//!
//! ## What is not listed
//!
//! Five of the twenty-two elements -- `CClientEnvModel`, `CHudTextMessage`,
//! `CParticleShooter`, `CVoiceStatusHud`, `CWeatherManager` -- do not override
//! `Draw` at all. They are on the list to receive user messages and to be
//! ticked, not to draw, so hiding them is already true and offering it would
//! only invite the question of why it did nothing.
//!
//! `CVoiceStatusHud` is also the one element with **two** vftables (`+0xabb48`
//! and `+0xabb24`), because it inherits from both `IVoiceHud` and `CHudBase`;
//! only the second is the element's. It is not offered here, but the next
//! person to add an entry should know the trap exists.
//!
//! `CHudAmmo` overrides `Draw` and registers itself, so `verify_hudelements.py`
//! has to be told about it by name (`KNOWN_EXCLUDED`) or its completeness check
//! would fail. Disassembly of `client+0x28b00` shows every `FillRGBA`/`SPR_Draw`
//! pair in the function -- ammo counter and weapon-select menu alike -- lands
//! after one of its four `CHud::ShouldDraw(3)` calls; nothing draws before the
//! first one. So the stock `cl_hud_ammo` cvar already hides all of it, with no
//! forced-reset trap the way `crosshair`/`r_drawentities`/`cl_lw` have -- unlike
//! `objectives` and `icons` below, which both draw something *before* their own
//! `ShouldDraw` gate and so keep genuine reach a stock cvar does not have.
//!
//! Five more overrode `Draw` and registered themselves, so they too need
//! `KNOWN_EXCLUDED` entries -- but for a different reason than `CHudAmmo`: each
//! one's `Draw` turned out not to draw anything at all, in any context, hook or
//! no hook. Writing `CHudBase::Draw` over a function that already does nothing
//! changes nothing observable, so offering these would only mislead:
//!
//! - `CHudDoDCommon::Draw` (`client+0x2c6f0`) is not the "shared HUD backdrop"
//!   the name suggests -- confirmed against the real DoD 1.3 client source
//!   (`whamemer/dod13-client`, `cl_dll/dod_common.cpp`): it auto-presses
//!   `+attack;wait;-attack` four seconds after the player you're spectating
//!   dies (cycling the observer target), syncs a couple of per-team gameplay
//!   flags (paratrooper mode, infinite lives) other elements read, and clears
//!   the sniper scope after a server-driven camera-view event ends. No
//!   `FillRGBA`/`SPR_Draw` anywhere in it. Live testing this element showed
//!   nothing changing for exactly this reason -- there was nothing to see.
//! - `CHudDoDMap::Draw` (`client+0x2e560`) is `mov eax, 1; ret 4` -- eight
//!   bytes, no calls. The overview map is rendered some other way entirely
//!   (not yet found; likely VGUI2, like the scoreboard).
//! - `CMortarHud::Draw` (`client+0x3e720`) calls one `gHUD` helper that checks
//!   a flag and an observer sub-mode, then returns a plain boolean. Neither
//!   function contains a single `FillRGBA`/`SPR_Draw` call. There is no mortar
//!   aiming HUD in this build to hide.
//! - `CHudSpectator::Draw` (`client+0x38000`, 45 bytes -- capstone's linear
//!   scan runs past the `ret 4` into an unrelated adjacent function with no
//!   padding between them, so measure it carefully if re-checking) checks
//!   observer mode and conditionally calls a method on what looks like a
//!   VGUI2 interface pointer -- plausibly telling a panel to hide -- but never
//!   draws. Whatever the real "spectator overlay" is, it is not this vtable
//!   slot.
//! - `CHudScope::Draw` (`client+0x46590`, 22 bytes) reads one flag and one
//!   observer-mode global, then unconditionally returns 1 without drawing.
//!   The actual scope vignette is a `ScreenFade` engine call inside
//!   `CHudScope::Think` (vftable slot 4, not 3), gated on the *local*
//!   player's own current weapon -- which is never populated while
//!   spectating, live-confirmed: no scope overlay ever appears in a demo.
//!   Reaching the real effect would mean hooking `Think`/`ScreenFade`
//!   instead, a materially different job (issue #307/#308 territory).
//!
//! ## One more excluded, for a third reason: never observed to fire
//!
//! `CHudStatusIcons::Draw` (`client+0x471c0`) is not like the five above --
//! it genuinely draws, via `SPR_DrawAdditive`, whenever one of its four icon
//! slots holds a sprite handle. The `StatusIcon` user message that populates
//! those slots (`(enable byte, icon-name string, [r,g,b] if enabling)`,
//! confirmed against `client.dll` itself, not just source) never appeared in
//! 661 real demos checked -- the local test library, four demos purpose-
//! recorded to trigger it, and 622 more from a full install scan. The one
//! concrete lead (`EnableIcon`'s `strstr(name, "grenade")` hack that plays
//! `weapons/timer.wav`, suggesting a grenade-cook countdown icon) doesn't
//! hold up either: that sound file doesn't exist in any install, and
//! `sprites/hud.txt` has no sprite registered under the plain name
//! `"grenade"` for `GetSpriteIndex` to resolve. Excluded because there is
//! nothing to test against, not because it is proven dead -- unlike the five
//! above, this one could turn out to work the moment the right demo turns up.
//!
//! ## Re-applied every frame
//!
//! `client.dll` does **not** reload on a plain demo change
//! (`docs/goldsrc_dod_quirks.md`), so this has not been observed to matter in
//! practice. It is still cheap idempotent insurance against whatever
//! transition *would* reload the module (a mod change, returning to the menu;
//! neither tested) -- the same guard `scoreboard.rs` and `crosshair.rs` keep
//! for the same reason, not a mechanism unique to this module. `commands::poll`
//! drives [`poll`], which re-reads the module base, rescans if it moved, and
//! writes only what differs.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;

/// The command name. Registered in `commands.rs`.
pub const NAME: &str = console_name!("hide_hudelement");

/// `Draw` is the fourth virtual: destructor, `Init`, `VidInit`, `Draw`.
const DRAW_SLOT: usize = 3;

/// `CHudBase::Draw` -- `xor eax, eax; ret 4`, the do-nothing implementation.
const BASE_DRAW_CODE: &[u8] = &[0x33, 0xc0, 0xc2, 0x04, 0x00];

/// An element that does **not** override `Draw`, so its slot 3 already holds
/// `CHudBase::Draw`'s address. Reading it from here means that address is
/// never a number written down in this file -- it is whatever the loaded module
/// says, checked against [`BASE_DRAW_CODE`] before use.
const BASE_DRAW_DONOR: Element = Element {
    name: "weather",
    class: ".?AVCWeatherManager@@",
    vftable_rva: 0xac2c0,
    what: "",
};

/// One HUD element on `CHud`'s draw list.
pub struct Element {
    /// What to type. Short, lowercase, and not the C++ class name: nobody
    /// wants to type `CHudDoDCrossHair` into a console.
    pub name: &'static str,
    /// The decorated RTTI name, checked against the loaded module.
    pub class: &'static str,
    pub vftable_rva: usize,
    /// What disappears, in the words someone would use to ask for it.
    pub what: &'static str,
}

/// Every element that overrides `Draw`, so every element there is any point
/// hiding. Derived by `goldsrc-hooks/tools/survey_client_dll.py elements`.
pub const ELEMENTS: &[Element] = &[
    Element {
        name: "crosshair",
        class: ".?AVCHudDoDCrossHair@@",
        vftable_rva: 0xac134,
        what: "the crosshair, POV and spectator alike (dodtools_hide_crosshair does this too)",
    },
    Element {
        name: "deathnotice",
        class: ".?AVCHudDeathNotice@@",
        vftable_rva: 0xac1c4,
        what: "the kill feed",
    },
    Element {
        name: "icons",
        class: ".?AVCHudDodIcons@@",
        vftable_rva: 0xac350,
        what: "the MG-deploy icon, the capture-area icon, blood and bandage",
    },
    Element {
        name: "menu",
        class: ".?AVCHudMenu@@",
        vftable_rva: 0xac1a0,
        what: "the team and class menus",
    },
    Element {
        name: "message",
        class: ".?AVCHudMessage@@",
        vftable_rva: 0xac20c,
        what: "map text and the round result (HudText)",
    },
    Element {
        name: "objectives",
        class: ".?AVCObjectiveIcons@@",
        vftable_rva: 0xac32c,
        what: "the objective icons and capture progress bar",
    },
    Element {
        name: "saytext",
        class: ".?AVCHudSayText@@",
        vftable_rva: 0xac278,
        what: "chat",
    },
    Element {
        name: "statusbar",
        class: ".?AVCHudStatusBar@@",
        vftable_rva: 0xac1e8,
        what: "the name and health readout under the crosshair",
    },
    Element {
        name: "train",
        class: ".?AVCHudTrain@@",
        vftable_rva: 0xac230,
        what: "the tram controls",
    },
    Element {
        name: "vgui2print",
        class: ".?AVCHudVGUI2Print@@",
        vftable_rva: 0xac3e0,
        what: "the VGUI2 print panel",
    },
];

/// Bit per [`ELEMENTS`] index: set means the user asked for it to be hidden.
static HIDDEN: AtomicU32 = AtomicU32::new(0);

/// Each element's stock `Draw`, captured the first time the module resolves.
/// Restoring writes these back rather than anything computed.
static STOCK_DRAW: [AtomicUsize; 10] = [const { AtomicUsize::new(0) }; 10];

/// `CHudBase::Draw` in the loaded module.
static BASE_DRAW: AtomicUsize = AtomicUsize::new(0);

/// The module base the two above were read from; 0 before the first resolve.
static RESOLVED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Reads a `u32` out of the loaded module.
///
/// Safety: `address` must be inside a mapped, readable page of the module.
unsafe fn read_u32(address: usize) -> u32 {
    unsafe { (address as *const u32).read_unaligned() }
}

/// The decorated class name `vftable[-1]`'s RTTI leads to.
///
/// MSVC's 32-bit layout: slot -1 of the vftable is a
/// `RTTICompleteObjectLocator*`; its fourth dword is a `TypeDescriptor*`; a
/// type descriptor's name starts eight bytes in. `signature` being 0 is what
/// says this is the 32-bit form, where those two are plain pointers rather than
/// image-relative.
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

/// Captures every stock `Draw` and `CHudBase::Draw` itself, checking each
/// vftable really belongs to the class the table names.
fn resolve() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    if RESOLVED_BASE.load(Ordering::Acquire) == base {
        return Ok(base);
    }

    // The no-op every hidden element is pointed at. Taken from an element that
    // does not override Draw rather than from an address written down here.
    let donor = base + BASE_DRAW_DONOR.vftable_rva;
    let donor_name = unsafe { rtti_class_name(donor) };
    if donor_name.as_deref() != Some(BASE_DRAW_DONOR.class) {
        return Err(format!(
            "+{:#x} identifies as {:?}, not {} -- this is not the client.dll this table describes",
            BASE_DRAW_DONOR.vftable_rva, donor_name, BASE_DRAW_DONOR.class
        ));
    }
    let base_draw = unsafe { read_u32(donor + DRAW_SLOT * 4) } as usize;
    let code = unsafe { std::slice::from_raw_parts(base_draw as *const u8, BASE_DRAW_CODE.len()) };
    if code != BASE_DRAW_CODE {
        return Err(format!(
            "CHudBase::Draw at +{:#x} starts {code:02x?}, not the expected {BASE_DRAW_CODE:02x?}",
            base_draw - base
        ));
    }

    for (index, element) in ELEMENTS.iter().enumerate() {
        let vftable = base + element.vftable_rva;
        let name = unsafe { rtti_class_name(vftable) };
        if name.as_deref() != Some(element.class) {
            return Err(format!(
                "+{:#x} identifies as {:?}, not {} ({})",
                element.vftable_rva, name, element.class, element.name
            ));
        }
        let draw = unsafe { read_u32(vftable + DRAW_SLOT * 4) } as usize;
        // A stock Draw that is already the no-op would mean this element does
        // not override it and should not be in the table at all.
        if draw == base_draw {
            return Err(format!(
                "{} does not override Draw, so it has nothing to hide",
                element.name
            ));
        }
        STOCK_DRAW[index].store(draw, Ordering::Release);
    }

    BASE_DRAW.store(base_draw, Ordering::Release);
    RESOLVED_BASE.store(base, Ordering::Release);
    Ok(base)
}

/// Whether `name` is an element this can hide.
pub fn find(name: &str) -> Option<usize> {
    ELEMENTS.iter().position(|e| e.name.eq_ignore_ascii_case(name))
}

/// Records that `index` should be hidden or shown. The write itself happens in
/// [`poll`], which is also what re-applies it after a demo change.
pub fn set_hidden(index: usize, hidden: bool) {
    let bit = 1u32 << index;
    if hidden {
        HIDDEN.fetch_or(bit, Ordering::Relaxed);
    } else {
        HIDDEN.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// Shows every element again.
pub fn show_all() {
    HIDDEN.store(0, Ordering::Relaxed);
}

/// Whether `index` is currently asked to be hidden.
pub fn is_hidden(index: usize) -> bool {
    HIDDEN.load(Ordering::Relaxed) & (1u32 << index) != 0
}

/// How many elements are hidden.
pub fn hidden_count() -> usize {
    HIDDEN.load(Ordering::Relaxed).count_ones() as usize
}

/// Writes the vftable slots to match what was asked for, returning how many
/// changed.
///
/// Called every frame. After the first resolve this is twelve dword
/// comparisons -- cheap insurance against a `client.dll` reload (see the
/// module doc's "Re-applied every frame" section), not a cost a plain demo
/// change is known to trigger.
pub fn apply() -> Result<usize, String> {
    let base = resolve()?;
    let base_draw = BASE_DRAW.load(Ordering::Acquire);
    let mut written = 0;

    for (index, element) in ELEMENTS.iter().enumerate() {
        let stock = STOCK_DRAW[index].load(Ordering::Acquire);
        let want = if is_hidden(index) { base_draw } else { stock };
        let slot = base + element.vftable_rva + DRAW_SLOT * 4;
        // Safety: `resolve` proved this vftable is the class it claims, and a
        // vftable slot is four mapped bytes.
        let present = unsafe { read_u32(slot) } as usize;
        if present == want {
            continue;
        }
        if present != stock && present != base_draw {
            return Err(format!(
                "{}'s Draw slot holds +{:#x}, which is neither its own nor CHudBase::Draw -- something else has patched it",
                element.name,
                present.wrapping_sub(base)
            ));
        }
        // Safety: four bytes inside the module, made writable and restored.
        if !unsafe { crate::patch::write_code_bytes(slot, &(want as u32).to_le_bytes()) } {
            return Err(format!("could not make {}'s vftable writable", element.name));
        }
        written += 1;
    }
    Ok(written)
}

/// One line per element, for the bare command.
pub fn listing() -> String {
    let mut out = String::new();
    for (index, element) in ELEMENTS.iter().enumerate() {
        out.push_str(&format!(
            "  {:<12} {}  {}\n",
            element.name,
            if is_hidden(index) { "1" } else { "0" },
            element.what
        ));
    }
    out
}

/// One line for `dodtools_status`.
pub fn status() -> String {
    let hidden: Vec<&str> = ELEMENTS
        .iter()
        .enumerate()
        .filter(|(index, _)| is_hidden(*index))
        .map(|(_, element)| element.name)
        .collect();
    if hidden.is_empty() {
        return format!("every HUD element draws normally ({NAME} lists them)");
    }
    format!(
        "{} HUD element(s) stubbed at the vftable: {}",
        hidden.len(),
        hidden.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// `STOCK_DRAW` is a fixed-size array indexed by position in `ELEMENTS`.
    /// A table that outgrew it would index out of bounds on the eighteenth
    /// element, at runtime, inside the game.
    #[test]
    fn the_stock_draw_array_covers_the_table() {
        assert_eq!(STOCK_DRAW.len(), ELEMENTS.len());
    }

    /// `HIDDEN` is a bitmask, so the table cannot outgrow its width either.
    #[test]
    fn the_hidden_mask_has_a_bit_per_element() {
        assert!(ELEMENTS.len() <= u32::BITS as usize);
    }

    #[test]
    fn names_are_unique_and_typeable() {
        let mut seen = HashSet::new();
        for element in ELEMENTS {
            assert!(seen.insert(element.name), "{} is listed twice", element.name);
            assert!(!element.name.is_empty());
            assert!(
                element.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{:?} should be lowercase and digits only -- it gets typed into a console",
                element.name
            );
        }
    }

    /// Two entries pointing at the same vftable would mean one of them is
    /// wrong, and hiding either would hide both.
    #[test]
    fn every_vftable_is_listed_once() {
        let mut seen = HashSet::new();
        for element in ELEMENTS {
            assert!(
                seen.insert(element.vftable_rva),
                "{:#x} is listed twice",
                element.vftable_rva
            );
        }
    }

    /// The RTTI name is the whole verification story, so a malformed one would
    /// quietly never match and the feature would refuse to work with a
    /// confusing message. `.?AV` is MSVC's prefix for a class.
    #[test]
    fn every_class_is_a_decorated_msvc_class_name() {
        for element in ELEMENTS.iter().chain([&BASE_DRAW_DONOR]) {
            assert!(
                element.class.starts_with(".?AV") && element.class.ends_with("@@"),
                "{:?} is not a decorated class name",
                element.class
            );
        }
    }

    /// The donor is the source of `CHudBase::Draw`'s address, so it must not
    /// also be something this offers to hide -- hiding it would be writing its
    /// own `Draw` over its own `Draw`.
    #[test]
    fn the_donor_is_not_one_of_the_hideable_elements() {
        assert!(find(BASE_DRAW_DONOR.name).is_none());
        assert!(!ELEMENTS.iter().any(|e| e.class == BASE_DRAW_DONOR.class));
        assert!(!ELEMENTS.iter().any(|e| e.vftable_rva == BASE_DRAW_DONOR.vftable_rva));
    }

    #[test]
    fn lookup_is_case_insensitive_and_exact() {
        assert_eq!(find("crosshair"), find("CROSSHAIR"));
        assert!(find("crosshair").is_some());
        assert!(find("cross").is_none());
        assert!(find("crosshairr").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn hiding_and_showing_move_only_the_one_bit() {
        show_all();
        let crosshair = find("crosshair").unwrap();
        let chat = find("saytext").unwrap();

        set_hidden(crosshair, true);
        assert!(is_hidden(crosshair));
        assert!(!is_hidden(chat));
        assert_eq!(hidden_count(), 1);

        set_hidden(chat, true);
        assert_eq!(hidden_count(), 2);

        set_hidden(crosshair, false);
        assert!(!is_hidden(crosshair));
        assert!(is_hidden(chat));

        show_all();
        assert_eq!(hidden_count(), 0);
    }

    /// Setting the same state twice must not toggle it -- `fetch_or` and
    /// `fetch_and` are idempotent where a `fetch_xor` would not be, and this is
    /// the test that says that was deliberate.
    #[test]
    fn setting_the_same_state_twice_is_idempotent() {
        show_all();
        let train = find("train").unwrap();
        set_hidden(train, true);
        set_hidden(train, true);
        assert!(is_hidden(train));
        set_hidden(train, false);
        set_hidden(train, false);
        assert!(!is_hidden(train));
        show_all();
    }

    #[test]
    fn the_listing_names_every_element_and_its_state() {
        show_all();
        set_hidden(find("saytext").unwrap(), true);
        let listing = listing();
        for element in ELEMENTS {
            assert!(listing.contains(element.name), "{} is missing", element.name);
        }
        assert!(listing.lines().any(|l| l.contains("saytext") && l.contains(" 1 ")));
        assert!(listing.lines().any(|l| l.contains("crosshair") && l.contains(" 0 ")));
        show_all();
    }

    #[test]
    fn status_names_what_is_hidden() {
        show_all();
        assert!(status().contains("draws normally"), "{}", status());
        set_hidden(find("deathnotice").unwrap(), true);
        let text = status();
        assert!(text.contains("deathnotice"), "{text}");
        assert!(text.contains('1'), "{text}");
        show_all();
    }

    /// `CHudBase::Draw` has to be `xor eax, eax; ret 4`. Spelled out because a
    /// wrong `ret` operand would unbalance the stack on a `__thiscall` taking
    /// one float, and the symptom would be a crash some frames later rather
    /// than at the patch.
    #[test]
    fn the_no_op_draw_is_exactly_chudbase_draw() {
        assert_eq!(BASE_DRAW_CODE, &[0x33, 0xc0, 0xc2, 0x04, 0x00]);
        assert_eq!(u16::from_le_bytes([BASE_DRAW_CODE[3], BASE_DRAW_CODE[4]]), 4);
    }

    /// `Draw` is the fourth virtual, after the destructor, `Init` and
    /// `VidInit`. Writing slot 2 instead would stub `VidInit` and the element
    /// would draw from an uninitialised sprite handle.
    #[test]
    fn draw_is_the_fourth_virtual() {
        assert_eq!(DRAW_SLOT, 3);
    }
}
