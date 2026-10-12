//! `dodstudio_show_demo_scope`: draw the sniper scope while a demo plays
//! (#307).
//!
//! ## What the game does
//!
//! The scope is `CHudScope::DrawTriApiScope` (`client+0x46690`): the rifle's
//! `scope_*.spr` drawn as four TriAPI quads in front of the view. Its gates,
//! in order:
//!
//! ```text
//!     +0x466a4  mov  eax, [esi+0x14]        ; m_iWeaponId, kept for the sprite
//!     ...
//!     +0x466c7  mov  eax, [g_iUser1]
//!     +0x466cf  cmp  eax, 4                 ; in-eye?
//!     +0x466d2  jne  +0x466f8
//!     +0x466d4  mov  dword [esi+0x14], 9    ; C7 46 14 09 00 00 00 -- always
//!                                           ; the Springfield; the imm32 is
//!                                           ; rewritten here, see below
//!     ...
//!     +0x466f8  mov  edx, [pDemoAPI]
//!     +0x466fe  call [edx+4]                ; IsPlayingback()
//!     +0x46701  test eax, eax
//!     +0x46703  je   +0x46707               ; 74 02, made EB 02
//!     +0x46705  xor  bl, bl                 ; a demo: no scope
//!     +0x46707  ...                         ; FOV 1..89, IsHardware(),
//!                                           ; the zoom has settled
//! ```
//!
//! The demo gate is the only demo-specific one. With the `je` made a `jmp`,
//! a POV demo draws the scope as live play does. Its recorded `Scope`
//! messages all carry 0 (918 of them in `wsod25_grp3_h1_stealth`), so
//! `m_iWeaponId` stays 0 and the function takes the rifle from the HUD's
//! current weapon instead (`+0x46742`, `CHudAmmo`'s `m_pWeapon->iId`), which
//! the recorded `CurWeapon` messages set. The FOV gate still decides when:
//! `SetFOV` gives the recorder's zoom, so the scope appears only while he is
//! zoomed.
//!
//! In an HLTV demo the zoom comes from the `HLTV` message (player, FOV):
//! its handler (`+0x22270`) keeps a FOV per player, and `CHud::Think`
//! (`+0x36d7c`) uses the watched player's while in-eye and 90 otherwise. So
//! the scope can only draw in-eye.
//!
//! ## The rifle, in-eye
//!
//! Spectating in-eye, DoD writes 9 (`WEAPON_SPRING`) into `m_iWeaponId`
//! every frame, so every sniper would get the Springfield's scope. With the
//! cvar on, that instruction's immediate is set to the scoped weapon the
//! watched player holds, read from his `curstate.weaponmodel`. The byte is
//! compared every frame and written only when the weapon changes.
//!
//! The ids and sprites come from the sprite switch at `+0x46750` (jump
//! table `+0x470cc`, index table `+0x470e4`):
//!
//! | model | id written | sprite |
//! | --- | --- | --- |
//! | `p_k98s` | 6 `WEAPON_SCOPEDKAR` | `scope_k43` |
//! | `p_spring` | 9 `WEAPON_SPRING` | `scope_spring` |
//! | `p_fg42s`, `p_scopedfg42bu` | 6, not 23 | `scope_k43` |
//! | `p_enfields` | 25 `WEAPON_ENFIELD` | `scope_enfield` |
//!
//! The scoped FG42 is `WEAPON_FG42` (23) with the scoped state, and the
//! switch draws 23 only when `g_iWeaponFlags & 1` (`client+0x108024`). That
//! global is the *local* player's predicted weapon state (written at
//! `+0x12538`), not the watched player's, so an HLTV viewer has nothing in
//! it. Id 6 reads the same sprite member (`+0x2c`) with no flag test, so the
//! FG42 gets its scope either way. The Enfield has no such twin: 25 is the
//! only id that reaches `scope_enfield`, so it shows only when the flag is
//! set.
//!
//! When the watched player holds anything else the byte is left as it is:
//! no other weapon narrows the FOV, so the scope cannot draw, and a sniper
//! switching to his pistol and back costs no write. Binoculars have no
//! third-person model of their own (no `p_binoc*.mdl` ships), so they are
//! not told apart from what the player last held.
//!
//! ## Where the scope points, in a POV demo
//!
//! The quads are drawn in front of the view along `GetViewAngles()`: the
//! *client's* angles, which in a POV demo are the viewer's own mouse, not the
//! recorded view the frame is drawn from. So the overlay hung still in the
//! world while the recorded view swung past it, and bounced around the screen
//! (the user, 2026-10-11, PRE). Only in-eye does the function copy `v_angles`,
//! the angles the frame is drawn with, into its view:
//!
//! ```text
//!     +0x466d2  jne  +0x466f8                 ; 75 24: not in-eye, skip both
//!     +0x466d4  mov  dword [esi+0x14], 9      ; the in-eye rifle (7 bytes)
//!     +0x466db  ...  view = v_angles          ; 29 bytes
//!     +0x466f8  ...                           ; the demo gate
//! ```
//!
//! While a demo plays, with the setting on, the `jne` lands on the copy
//! instead (`75 07`), skipping only the rifle store. Outside a demo it stays
//! `75 24`, so live play still draws along the client's angles.
//!
//! `v_angles` alone still left the overlay swaying sideways as the recorded
//! player moved: ±80 px at 1280x720 in a rendered clip, so in a POV demo it
//! isn't the angles the frame ends up drawn with. So the copy's three
//! absolute operands are pointed at [`DRAWN_ANGLES`] as well, which
//! `engine::tramp_calc_refdef` fills from `V_CalcRefdef`'s result every frame:
//! the scope is then drawn square to the camera.
//!
//! The function reads `m_iWeaponId` at entry, before the in-eye store, so a
//! change shows one frame after it is written. `CHudScope::Think`'s black
//! blink fires only on a `Scope` message (`SetScope` sets its dirty flag at
//! `+0x4640e`); the store here does not, so no blink.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("show_demo_scope");

/// From the `g_iUser1` load to the `xor bl, bl` the demo gate skips. The
/// bytes this module writes are wildcards, so a rescan finds the site in
/// any state; `put_byte` checks them instead.
const PATTERN: &str = "A1 ?? ?? ?? ?? 83 C4 04 83 F8 04 75 ?? \
                       C7 46 14 ?? 00 00 00 \
                       8B 15 ?? ?? ?? ?? A1 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? \
                       89 54 24 54 89 44 24 58 89 4C 24 5C \
                       8B 15 ?? ?? ?? ?? FF 52 04 85 C0 ?? 02 32 DB";
/// The rel8 of the "not in-eye" `jne`.
const VIEW_JNE_REL_AT: usize = 12;
/// Stock: past the rifle store and the `v_angles` copy.
const VIEW_SKIP_BOTH: u8 = 0x24;
/// In a demo: past the 7-byte rifle store only, onto the `v_angles` copy.
const VIEW_SKIP_STORE: u8 = 0x07;
/// The absolute addresses the copy reads pitch, yaw and roll from
/// (`mov edx, [a]; mov eax, [a+4]; mov ecx, [a+8]`): `v_angles`, stock.
const ANGLE_OPERANDS_AT: [usize; 3] = [22, 27, 33];
/// The low byte of the in-eye store's imm32 (`mov dword [esi+0x14], imm32`).
const IN_EYE_ID_AT: usize = 16;
/// The `je` that skips the `xor bl, bl`.
const JE_AT: usize = 60;
const JE: u8 = 0x74;
const JMP: u8 = 0xEB;

const WEAPON_SCOPEDKAR: u8 = 6;
const WEAPON_SPRING: u8 = 9;
const WEAPON_FG42: u8 = 23;
const WEAPON_ENFIELD: u8 = 25;
const WEAPON_BINOC: u8 = 33;

/// What the game writes in-eye.
const STOCK_IN_EYE_ID: u8 = WEAPON_SPRING;

/// The ids the sprite switch draws a scope for. Anything else in the
/// immediate means another patch got there first.
const SCOPE_IDS: [u8; 5] = [
    WEAPON_SCOPEDKAR,
    WEAPON_SPRING,
    WEAPON_FG42,
    WEAPON_ENFIELD,
    WEAPON_BINOC,
];

/// The angles the last frame was drawn with, as `V_CalcRefdef` left them, as
/// f32 bits. The patched copy reads these three cells in place.
static DRAWN_ANGLES: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
/// The copy's stock operands (`v_angles`), read before the first patch.
static STOCK_OPERANDS: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
static ON: AtomicBool = AtomicBool::new(false);
static COMPLAINED: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// The scope id for a third-person weapon model, or `None` for one without
/// a scope. See the module doc for why the FG42 maps to 6.
fn scope_id_for_model(name: &str) -> Option<u8> {
    let file = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    let stem = file.strip_suffix(".mdl").unwrap_or(&file);
    let stem = stem.strip_prefix("p_")?;
    let stem = stem.strip_suffix("_l").unwrap_or(stem);
    match stem {
        "k98s" => Some(WEAPON_SCOPEDKAR),
        "spring" => Some(WEAPON_SPRING),
        "fg42s" | "scopedfg42bu" => Some(WEAPON_SCOPEDKAR),
        "enfields" => Some(WEAPON_ENFIELD),
        _ => None,
    }
}

fn sprite_name(id: u8) -> &'static str {
    match id {
        WEAPON_SCOPEDKAR | WEAPON_FG42 => "scope_k43",
        WEAPON_SPRING => "scope_spring",
        WEAPON_ENFIELD => "scope_enfield",
        WEAPON_BINOC => "scope_binoc",
        _ => "no scope",
    }
}

/// The start of [`PATTERN`] in the loaded `client.dll`.
fn site() -> Result<usize, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find CHudScope::DrawTriApiScope's gates -- {why}"))?;
    ADDRESS.store(found, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(found)
}

/// Writes `want` at `address` unless it is already there, after checking
/// the byte holds one of `allowed`. Returns whether it wrote.
fn put_byte(address: usize, want: u8, allowed: &[u8], what: &str) -> Result<bool, String> {
    // Safety: `site()` proved the pattern, and so this byte, is mapped code.
    let present = unsafe { *(address as *const u8) };
    if present == want {
        return Ok(false);
    }
    if !allowed.contains(&present) {
        return Err(format!(
            "{what} holds {present:#04x}, none of {allowed:02x?} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        return Err(format!("could not make {what} writable"));
    }
    Ok(true)
}

/// The scope id for the player the in-eye camera is on, or `None` when the
/// camera isn't in-eye or he holds no scoped weapon.
fn watched_scope_id() -> Option<u8> {
    let target = crate::spectator_target::in_eye_target()?;
    let engfuncs = engine::engfuncs()?;
    let studio = engine::engine_studio()?;
    // Safety: the engine's own accessor, null for an empty slot.
    let entity = unsafe { (engfuncs.get_entity_by_index)(target) };
    if entity.is_null() {
        return None;
    }
    // Safety: a non-null `cl_entity_t` lives for the frame, on this thread.
    let weaponmodel = unsafe { (*entity).curstate.weaponmodel };
    // Safety: null for an index the engine has not precached.
    let model = unsafe { (studio.get_model_by_index)(weaponmodel) };
    if model.is_null() {
        return None;
    }
    // Safety: a non-null model from the engine's own lookup.
    scope_id_for_model(&unsafe { (*model).name_str() })
}

/// Keeps the angles the frame is drawn with. Called from
/// `engine::tramp_calc_refdef` after the client's `V_CalcRefdef`.
pub fn after_calc_refdef(pparams: &engine::RefParamsPartial) {
    for (cell, value) in DRAWN_ANGLES.iter().zip(pparams.viewangles) {
        cell.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// Where the patched copy reads angle `k` from.
fn drawn_operand(k: usize) -> u32 {
    // The DLL is 32-bit: a static's address fits.
    &DRAWN_ANGLES[k] as *const AtomicU32 as usize as u32
}

/// Points the copy at [`DRAWN_ANGLES`] (`drawn`) or back at `v_angles`.
/// Returns whether it wrote.
fn point_angles(at: usize, drawn: bool) -> Result<bool, String> {
    let mut wrote = false;
    for (k, offset) in ANGLE_OPERANDS_AT.iter().enumerate() {
        let address = at + offset;
        // Safety: inside the pattern `site()` matched, so mapped code.
        let present = unsafe { std::ptr::read_unaligned(address as *const u32) };
        let ours = drawn_operand(k);
        let mut stock = STOCK_OPERANDS[k].load(Ordering::Acquire);
        if stock == 0 {
            if present == ours {
                return Err(
                    "the scope's angle copy already reads ours, with no stock address kept".into(),
                );
            }
            STOCK_OPERANDS[k].store(present, Ordering::Release);
            stock = present;
        }
        let want = if drawn { ours } else { stock };
        if present == want {
            continue;
        }
        if present != stock && present != ours {
            return Err(format!(
                "the scope's angle copy reads {present:#x}, neither v_angles ({stock:#x}) nor ours -- something else has patched it"
            ));
        }
        if !unsafe { crate::patch::write_code_bytes(address, &want.to_le_bytes()) } {
            return Err("could not make the scope's angle copy writable".into());
        }
        wrote = true;
    }
    Ok(wrote)
}

fn playing_demo() -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    let api = engfuncs.p_demo_api;
    // Safety: the engine's own demo API table, for the session.
    !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
}

/// Applies or removes the patches, returning whether a byte was written.
/// Decides from the bytes, so it is cheap to call every frame.
pub fn set_on(on: bool) -> Result<bool, String> {
    let at = site()?;
    let gate = put_byte(
        at + JE_AT,
        if on { JMP } else { JE },
        &[JE, JMP],
        "the scope's demo gate",
    )?;
    let in_demo = on && playing_demo();
    let view = put_byte(
        at + VIEW_JNE_REL_AT,
        if in_demo {
            VIEW_SKIP_STORE
        } else {
            VIEW_SKIP_BOTH
        },
        &[VIEW_SKIP_BOTH, VIEW_SKIP_STORE],
        "the scope's view-angle branch",
    )?;
    let view = point_angles(at, in_demo)? || view;
    let id = if on {
        watched_scope_id()
    } else {
        Some(STOCK_IN_EYE_ID)
    };
    let rifle = match id {
        Some(id) => put_byte(at + IN_EYE_ID_AT, id, &SCOPE_IDS, "the in-eye scope id")?,
        None => false,
    };
    ON.store(on, Ordering::Release);
    Ok(gate || view || rifle)
}

fn describe(on: bool) -> &'static str {
    if on {
        "1: the sniper scope shows while a demo plays, in-eye with the watched player's rifle"
    } else {
        "0: the game hides the sniper scope while a demo plays"
    }
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    let was = ON.load(Ordering::Relaxed);
    match set_on(wanted) {
        Ok(wrote) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            // The rifle byte changes with every new watched sniper; only the
            // setting itself is worth a log line.
            if wrote && was != wanted {
                unsafe {
                    crate::debug::report(&format!("demo_scope: {NAME} = {}", describe(wanted)))
                };
            }
        }
        Err(why) => {
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("demo_scope: {NAME} not applied yet -- {why}"))
                };
            }
        }
    }
}

/// One line for `dodstudio_debug_status`, while it is on: the setting, and
/// the rifle the in-eye camera currently draws.
pub fn status_line() -> Option<String> {
    if !ON.load(Ordering::Relaxed) {
        return None;
    }
    let id = ADDRESS.load(Ordering::Acquire);
    let rifle = (id != 0)
        // Safety: the scanned site, mapped code.
        .then(|| unsafe { *((id + IN_EYE_ID_AT) as *const u8) })
        .map_or("unknown".to_string(), |id| {
            format!("{} (id {id})", sprite_name(id))
        });
    Some(format!(
        "demo scope: {NAME} = {}; in-eye draws {rifle}",
        describe(true)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Vec<&'static str> {
        PATTERN.split_whitespace().collect()
    }

    #[test]
    fn in_eye_id_at_indexes_the_store_immediate() {
        let t = tokens();
        assert_eq!(
            &t[IN_EYE_ID_AT - 3..IN_EYE_ID_AT],
            &["C7", "46", "14"],
            "mov dword [esi+0x14], imm32"
        );
        assert_eq!(t[IN_EYE_ID_AT], "??", "the byte this module writes");
        assert_eq!(&t[IN_EYE_ID_AT + 1..IN_EYE_ID_AT + 4], &["00", "00", "00"]);
        // The in-eye test right before it.
        assert_eq!(&t[8..12], &["83", "F8", "04", "75"]);
    }

    #[test]
    fn the_view_branch_skips_the_store_or_both() {
        let t = tokens();
        assert_eq!(t[VIEW_JNE_REL_AT - 1], "75", "jne rel8");
        assert_eq!(t[VIEW_JNE_REL_AT], "??", "the byte this module writes");
        // The store is 7 bytes; the v_angles copy runs from just past it to
        // the demo gate's `mov edx, [pDemoAPI]`.
        let after_jne = VIEW_JNE_REL_AT + 1;
        assert_eq!(&t[after_jne..after_jne + 3], &["C7", "46", "14"]);
        assert_eq!(
            &t[after_jne + VIEW_SKIP_STORE as usize..][..2],
            &["8B", "15"],
            "lands on the v_angles copy"
        );
        assert_eq!(
            &t[after_jne + VIEW_SKIP_BOTH as usize..][..2],
            &["8B", "15"],
            "lands on the demo gate"
        );
        assert_eq!(
            &t[after_jne + VIEW_SKIP_BOTH as usize + 6..][..3],
            &["FF", "52", "04"],
            "call [edx+4], IsPlayingback"
        );
    }

    #[test]
    fn je_at_indexes_the_demo_gate() {
        let t = tokens();
        assert_eq!(t[JE_AT], "??", "the byte this module writes");
        assert_eq!(t[JE_AT + 1], "02", "skips exactly the two-byte xor");
        assert_eq!(&t[JE_AT + 2..JE_AT + 4], &["32", "DB"], "xor bl, bl");
        assert_eq!(&t[JE_AT - 5..JE_AT], &["FF", "52", "04", "85", "C0"]);
        assert_eq!(t.len(), JE_AT + 4, "the pattern ends at the gate");
    }

    #[test]
    fn the_pattern_starts_with_a_fixed_byte() {
        let t = tokens();
        assert_ne!(t[0], "??", "find_unique refuses a leading wildcard");
        for token in &t {
            assert!(*token == "??" || u8::from_str_radix(token, 16).is_ok());
        }
    }

    #[test]
    fn the_jump_keeps_the_branch_length() {
        // A short jmp and a short je take the same rel8, so the target stays
        // +0x46707 either way.
        assert_eq!((JE, JMP), (0x74, 0xEB));
    }

    #[test]
    fn scoped_models_map_to_the_ids_the_sprite_switch_draws() {
        assert_eq!(scope_id_for_model("models/p_k98s.mdl"), Some(6));
        assert_eq!(scope_id_for_model("models/p_k98s_l.mdl"), Some(6));
        assert_eq!(scope_id_for_model("models/p_spring.mdl"), Some(9));
        assert_eq!(scope_id_for_model("models/p_spring_l.mdl"), Some(9));
        assert_eq!(scope_id_for_model("models/p_enfields.mdl"), Some(25));
        assert_eq!(scope_id_for_model("models/p_enfields_l.mdl"), Some(25));
        assert_eq!(scope_id_for_model("models/p_fg42s.mdl"), Some(6));
        assert_eq!(scope_id_for_model("models/p_scopedfg42bu.mdl"), Some(6));
        assert_eq!(scope_id_for_model("models\\P_K98S.MDL"), Some(6));
        for id in [6, 9, 25] {
            assert!(SCOPE_IDS.contains(&id));
        }
    }

    #[test]
    fn unscoped_models_leave_the_byte_alone() {
        for name in [
            "models/p_k98.mdl",
            "models/p_enfield.mdl",
            "models/p_fg42bu.mdl",
            "models/p_fg42sr.mdl",
            "models/p_garand.mdl",
            "models/p_colt.mdl",
            "models/v_spring.mdl",
            "",
        ] {
            assert_eq!(scope_id_for_model(name), None, "{name}");
        }
    }

    #[test]
    fn the_fg42_borrows_the_scoped_kar_sprite() {
        assert_eq!(
            sprite_name(scope_id_for_model("models/p_fg42s.mdl").unwrap()),
            sprite_name(WEAPON_FG42)
        );
    }

    #[test]
    fn stock_id_is_the_springfield() {
        assert_eq!(STOCK_IN_EYE_ID, 9);
        assert_eq!(sprite_name(STOCK_IN_EYE_ID), "scope_spring");
    }

    #[test]
    fn one_shows_the_scope() {
        assert!(describe(true).starts_with('1'));
        assert!(describe(true).contains("shows"));
        assert!(describe(false).starts_with('0'));
    }
}
