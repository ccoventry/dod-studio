//! `dodstudio_hide_asset` for DoD's own particles: the impact debris, the
//! grenade dirt cloud (`sprites/effects/adrian/dirt_puff.spr`), smoke trails,
//! splashes, snow (#614).
//!
//! ## Why `HUD_AddEntity` never sees them
//!
//! World entities and the engine's temporary effects both pass
//! `HUD_AddEntity`, which is where `hide_asset` filters (the engine's tempent
//! callback calls it with type 2 on both builds: PRE `hw.dll+0x26748`,
//! Anniversary `+0x1ad457`). DoD's particles don't: its client carries Valve's
//! particle manager, which draws them with triangles at the end of the frame.
//! Nearly every impact and explosion effect in DoD is one.
//!
//! ## Where they are made
//!
//! Every particle comes from one of four `Create` functions in `client.dll`
//! (one file on both installs), each found by the "called with a null sprite"
//! message it prints:
//!
//! | `Create` | at | makes |
//! | --- | --- | --- |
//! | `CDoDParticle` | `+0x4c9b0` | most effects (25 callers) |
//! | weather | `+0x52ac0` | snow, rain |
//! | `CDoDRocketTrail` | `+0x4e140` | rocket and dirt trail smoke |
//! | `CDoDDirtExploDust` | `+0x4e2f0` | the grenade dirt cloud |
//!
//! All four are plain cdecl, read no register arguments, take the sprite
//! (`model_s *`, whose name is the path) at `[esp+0x1c]` on entry, and nothing
//! branches into their first 6 or 7 bytes (checked against the binary).
//!
//! ## The hook: return NULL, as `cl_particlefx 0` does
//!
//! Each `Create` already returns NULL when `cl_particlefx` is 0 or the effect
//! is too far away, so every caller handles NULL. A detour at each entry asks
//! [`crate::hide_asset::should_hide`] about the sprite and, for a listed one,
//! returns NULL straight away: the `cl_particlefx 0` path, for that one sprite.
//!
//! Installed the first time `dodstudio_hide_asset add` runs, not at startup:
//! nobody who never hides anything gets `client.dll` patched for it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::detour;
use crate::engine::ModelSPartial;
use crate::hide_asset::DrawnAs;

struct Site {
    /// For the log.
    what: &'static str,
    /// The message the function prints for a null sprite; its address,
    /// pushed right after the null check, is what makes each pattern unique.
    message: &'static str,
    /// The bytes from the entry up to that `push imm32`.
    prologue: &'static str,
    /// The instructions the jump overwrites, reproduced in the stub. None is
    /// relative.
    stolen: &'static [u8],
}

/// `push ebx; push esi; push edi; mov edi, [esp+0x28]; test edi, edi; jnz`.
const THREE_PUSHES: &str = "53 56 57 8B 7C 24 28 85 FF 75 ??";
const THREE_PUSHES_STOLEN: &[u8] = &[0x53, 0x56, 0x57, 0x8b, 0x7c, 0x24, 0x28];

const SITES: [Site; 4] = [
    // mov eax, fs:[0]; push -1; push <handler>; push eax; mov fs:[0], esp
    // push ebp; push esi; push edi; mov edi, [esp+0x34]; test edi, edi; jnz
    Site {
        what: "particle",
        message: "CDoDParticle::Create called with a null sprite\n",
        prologue: "64 A1 00 00 00 00 6A FF 68 ?? ?? ?? ?? 50 64 89 25 00 00 00 00 \
                   55 56 57 8B 7C 24 34 85 FF 75 ??",
        stolen: &[0x64, 0xa1, 0x00, 0x00, 0x00, 0x00],
    },
    // The weather's own Create prints CDoDParticle's message.
    Site {
        what: "snow/rain",
        message: "CDoDParticle::Create called with a null sprite\n",
        prologue: THREE_PUSHES,
        stolen: THREE_PUSHES_STOLEN,
    },
    Site {
        what: "trail smoke",
        message: "CDoDRocketTrail::Create called with a null sprite\n",
        prologue: THREE_PUSHES,
        stolen: THREE_PUSHES_STOLEN,
    },
    Site {
        what: "explosion dust",
        message: "CDoDDirtExploDust::Create called with a null sprite\n",
        prologue: THREE_PUSHES,
        stolen: THREE_PUSHES_STOLEN,
    },
];

const SITE_COUNT: usize = SITES.len();

/// Where each stub jumps back to, read by the stubs themselves.
static RESUME: [AtomicUsize; SITE_COUNT] = [const { AtomicUsize::new(0) }; SITE_COUNT];

/// The `client.dll` base the detours were written into, or 0 before the first
/// install. See `tempent_fix`'s `INSTALLED_BASE` for why the base alone can't
/// say a reloaded copy is patched.
static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);

/// See [`detour::Detour`] on why these are never undone.
static DETOURS: Mutex<Vec<detour::Detour>> = Mutex::new(Vec::new());

/// Called by every stub with the sprite `Create` was given. Nonzero hides it.
unsafe extern "C" fn hide_particle(sprite: *const ModelSPartial) -> i32 {
    if sprite.is_null() {
        // `Create`'s own null check runs next.
        return 0;
    }
    // Safety: a sprite model the client just got from GetSpritePointer.
    let name = unsafe { (*sprite).name_str() };
    i32::from(crate::hide_asset::should_hide(&name, DrawnAs::Particle))
}

/// A site's stub, hand-assembled.
///
/// ```asm
/// mov  eax, [esp+0x1c]       ; the sprite
/// push eax
/// mov  eax, hide_particle
/// call eax
/// add  esp, 4
/// test eax, eax
/// jz   .game
/// xor  eax, eax              ; hidden: Create returns NULL
/// ret
/// .game:
/// <the stolen instructions>
/// jmp  dword ptr [RESUME[i]]
/// ```
///
/// `eax`, `ecx` and `edx` are free at the entry of a cdecl function that reads
/// none of them, and `hide_particle` keeps the rest.
fn stub(hide: usize, stolen: &[u8], resume: usize) -> Vec<u8> {
    let mut code = vec![0x8b, 0x44, 0x24, 0x1c, 0x50, 0xb8];
    code.extend_from_slice(&(hide as u32).to_le_bytes());
    code.extend_from_slice(&[
        0xff, 0xd0, // call eax
        0x83, 0xc4, 0x04, // add esp, 4
        0x85, 0xc0, // test eax, eax
        0x74, 0x03, // jz .game
        0x31, 0xc0, // xor eax, eax
        0xc3, // ret
    ]);
    code.extend_from_slice(stolen);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    code
}

/// `prologue`, then `push imm32` of `message_va`.
fn pattern(prologue: &str, message_va: usize) -> String {
    let bytes = (message_va as u32).to_le_bytes();
    format!(
        "{prologue} 68 {:02X} {:02X} {:02X} {:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

/// The address of `text` and its NUL inside the loaded module, if present.
fn find_message(base: usize, text: &str) -> Option<usize> {
    // Safety: `base` is a mapped module; `image_size` reads its headers.
    let size = unsafe { crate::pe::image_size(base as *mut u8) }?;
    // Safety: the whole image is mapped for the life of the module.
    let image = unsafe { std::slice::from_raw_parts(base as *const u8, size) };
    let mut needle = text.as_bytes().to_vec();
    needle.push(0);
    image
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
        .map(|at| base + at)
}

fn already_guarded(index: usize, site: &Site) -> bool {
    let resume = RESUME[index].load(Ordering::Acquire);
    // Safety: only called at the base RESUME was computed against.
    resume != 0 && unsafe { *((resume - site.stolen.len()) as *const u8) } == 0xe9
}

fn install_site(index: usize, site: &Site, base: usize) -> Result<detour::Detour, String> {
    let message = find_message(base, site.message)
        .ok_or_else(|| format!("no \"{}\" in client.dll", site.message.trim_end()))?;
    // Safety: `base` is a module handle the loader gave us.
    let target = unsafe { crate::scan::find_unique(base, &pattern(site.prologue, message)) }?;
    // Safety: the pattern matched here, inside the code section.
    let present = unsafe { std::slice::from_raw_parts(target as *const u8, site.stolen.len()) };
    if present != site.stolen {
        return Err(format!(
            "expected {:02x?} at +{:#x}, found {present:02x?}",
            site.stolen,
            target - base
        ));
    }
    RESUME[index].store(target + site.stolen.len(), Ordering::Release);
    let code = stub(
        hide_particle as *const () as usize,
        site.stolen,
        RESUME[index].as_ptr() as usize,
    );
    // Safety: the span was checked byte for byte above, and nothing branches
    // into it (see the module doc).
    unsafe { detour::install(target, site.stolen.len(), &code) }
}

/// Patches the four `Create` functions in the loaded `client.dll`, once per
/// copy of it. Engine thread only: called from `hide_asset`'s command and
/// again when `client.dll` initialises, which does nothing until the first.
pub fn install() {
    let Some(base) = crate::engine::client_module_base() else {
        return;
    };
    let same_base = INSTALLED_BASE.load(Ordering::Acquire) == base;
    let Ok(mut detours) = DETOURS.lock() else {
        return;
    };
    let mut installed = 0;
    let mut already = 0;
    let mut failed = Vec::new();
    for (index, site) in SITES.iter().enumerate() {
        if same_base && already_guarded(index, site) {
            already += 1;
            continue;
        }
        match install_site(index, site, base) {
            Ok(detour) => {
                detours.push(detour);
                installed += 1;
            }
            Err(why) => failed.push(format!("{} -- {why}", site.what)),
        }
    }
    INSTALLED_BASE.store(base, Ordering::Release);
    if installed == 0 && failed.is_empty() {
        return;
    }
    let mut report = format!(
        "particle_hide: {} of {SITE_COUNT} particle makers hooked, so {} can hide particles",
        installed + already,
        crate::hide_asset::COMMAND_NAMES[0]
    );
    for failure in failed {
        report.push_str(&format!("; not hooked: {failure}"));
    }
    unsafe { crate::debug::report(&report) };
}

/// For `lib.rs`: re-patch a reloaded `client.dll`, but only once the hook has
/// been wanted this session.
pub fn reinstall_if_used() {
    if INSTALLED_BASE.load(Ordering::Acquire) != 0 {
        install();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        let code = stub(0x1111_1111, &[0xaa, 0xbb], 0x3333_3333);
        #[rustfmt::skip]
        assert_eq!(
            code,
            vec![
                0x8b, 0x44, 0x24, 0x1c,             // mov  eax, [esp+0x1c]
                0x50,                               // push eax
                0xb8, 0x11, 0x11, 0x11, 0x11,       // mov  eax, hide_particle
                0xff, 0xd0,                         // call eax
                0x83, 0xc4, 0x04,                   // add  esp, 4
                0x85, 0xc0,                         // test eax, eax
                0x74, 0x03,                         // jz   .game
                0x31, 0xc0,                         // xor  eax, eax
                0xc3,                               // ret
                0xaa, 0xbb,                         // .game: the stolen bytes
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33, // jmp  dword [RESUME]
            ]
        );
    }

    #[test]
    fn the_jz_lands_on_the_stolen_bytes() {
        for site in &SITES {
            let code = stub(0, site.stolen, 0);
            let jz = code.iter().position(|&b| b == 0x74).unwrap();
            let lands = jz + 2 + code[jz + 1] as usize;
            assert_eq!(&code[lands..lands + site.stolen.len()], site.stolen);
        }
    }

    /// The stolen bytes are the start of each prologue, and long enough for
    /// the jump.
    #[test]
    fn every_site_steals_the_start_of_its_prologue() {
        for site in &SITES {
            assert!(site.stolen.len() >= 5, "{}", site.what);
            let start: Vec<String> = site.stolen.iter().map(|b| format!("{b:02X}")).collect();
            assert!(
                site.prologue.starts_with(&start.join(" ")),
                "{}: {:?}",
                site.what,
                site.prologue
            );
        }
    }

    #[test]
    fn the_pattern_ends_with_the_message_push() {
        assert_eq!(
            pattern("53 56", 0x019d_0490),
            "53 56 68 90 04 9D 01",
            "push imm32, little-endian"
        );
    }

    /// The sprite sits at `[esp+0x1c]` on entry in all four: the prologue's
    /// own load of it, minus what it pushed first.
    #[test]
    fn the_sprite_offset_matches_each_prologue() {
        // SEH frame (3 pushes) + ebp, esi, edi: [esp+0x34] after 0x18 bytes.
        assert_eq!(0x34 - 6 * 4, 0x1c);
        // ebx, esi, edi: [esp+0x28] after 0xc bytes.
        assert_eq!(0x28 - 3 * 4, 0x1c);
        assert!(SITES[0].prologue.contains("8B 7C 24 34"));
        assert!(THREE_PUSHES.contains("8B 7C 24 28"));
    }
}
