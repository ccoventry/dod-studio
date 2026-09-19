//! `dodtools_hide_sprite` — suppress specific map-placed `env_sprite`
//! entities by their model path (e.g. `sprites/mapsprites/caparea.spr`).
//!
//! ## Why `dodtools_hide_hudelement` can't reach this
//!
//! That module (issue #265) patches the classic 2D HUD element list —
//! `CHud::Redraw` walking `CHudBase`-derived elements' vftable slot 3. An
//! `env_sprite` is a completely different code path: server-replicated
//! entity data the map's own BSP entity lump points at, rendered through the
//! engine's normal entity renderer. There is no HUD element vftable to swap.
//! `client.dll` has **zero** references to the string `"caparea"` anywhere in
//! the binary (confirmed by a full binary string search) — it doesn't load
//! or draw this sprite by name at all, unlike `mapsprites/speakerIcon.spr`/
//! `voiceIcon.spr`, which are hardcoded HUD elements and already reachable
//! through `voice.rs`.
//!
//! ## The mechanism: `HUD_AddEntity`
//!
//! Slot 20 of the `cldll_func_t` table `engine.rs` already partially owns
//! (three of its 43 slots were already swapped for `Initialize`, `HUD_Frame`
//! and `HUD_GetStudioModelInterface` — see `docs/goldsrc_client_dll_internals.md`
//! §2). The slot index itself is checked the same rigorous way as the other
//! three, independently of the constant in `engine.rs`: disassemble `F`
//! directly and resolve what it actually writes into slot 20 back to its own
//! export name (`tools/verify_hide_sprite_slot.py`). The engine calls
//! `HUD_AddEntity(int type, cl_entity_t *ent, const
//! char *modelname)` once per entity it is about to add to the render list;
//! the client returns 0 to suppress that one entity, or nonzero to let it
//! through. This is the standard Half-Life SDK contract for that slot, and
//! is not established here by disassembling `hw.dll`'s own caller (a closed,
//! heavily obfuscated binary — see `docs/goldsrc_hw_dll_survey.md`) — it is
//! cross-checked the same way `engine.rs`'s own slot table is, against
//! Xash3D's open-source engine, whose `CL_AddVisibleEntity`-equivalent path
//! explicitly treats a zero return from this slot as "don't add". That is a
//! different, weaker standard of evidence than the byte-level verification
//! this crate holds a *patch* to (nothing here is patched), and is stated as
//! such rather than presented as disassembly-confirmed.
//!
//! ## Design: an allow-list, not a blanket toggle
//!
//! Deliberately not a `dodtools_hide_map_sprites 1` switch. Most of
//! `sprites/mapsprites/`'s neighbours are decorative and meaningful —
//! smoke, fire, tracers — and indiscriminately suppressing every map-placed
//! sprite would remove things nobody asked to have removed. This command
//! only ever hides model paths named explicitly, the same shape
//! `dodtools_deathmsg block <id>...` already uses for players.
//!
//! Analysis subject: `dod/cl_dlls/client.dll`, 977,816 bytes, byte-identical
//! across the stock, pre-Anniversary and post-Anniversary installs.
//!
//! ## Live test (2026-09-21)
//!
//! Confirmed against `dod_railroad2_s9a`: `sprites/mapsprites/flames.spr`
//! visibly disappeared and came back across a `clear`/re-set cycle, which
//! confirms `HUD_AddEntity`'s return-value contract (0 = suppress) actually
//! holds in this build, not only in Xash3D's open-source equivalent -- the
//! weaker-evidence caveat above no longer applies to the mechanism itself.
//!
//! The path was found by scanning the map's own BSP entity lump (lump 0, a
//! plain-text sequence of Quake-style `{ "key" "value" ... }` blocks) for
//! `env_sprite` classnames and their `model` key -- a pure offline text
//! parse, no engine involved. Two earlier attempts on the same session
//! (`sprites/mapsprites/caparea.spr`, `sprites/customxhair.spr`) never
//! matched anything, not because of a path typo but because neither is a
//! genuine `env_sprite`: the capture-area icon and the crosshair are both
//! ordinary 2D HUD elements (`CHudDodIcons`/`CHudDoDCrossHair`), which this
//! command was never able to reach in the first place -- see "Why
//! `dodtools_hide_hudelement` can't reach this" above, which turns out to cut
//! both ways.

use std::sync::Mutex;

use crate::names::console_name;

pub const COMMAND_NAMES: &[&str] = &[COMMAND];
const COMMAND: &str = console_name!("hide_sprite");

/// Model paths to suppress, exactly as typed (case-insensitive compare).
/// Empty by default -- the whole point is that nothing is hidden until
/// asked for by name.
static HIDDEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Pure matcher, so it can be unit-tested without touching the shared
/// `HIDDEN` static -- see the test module for why nothing here does that.
fn matches(hidden: &[String], model_name: &str) -> bool {
    hidden.iter().any(|h| h.eq_ignore_ascii_case(model_name))
}

/// Called from `engine::tramp_hud_add_entity` for every entity the engine is
/// about to add to the render list. `true` means suppress it.
///
/// A plain linear scan over what is expected to be a handful of entries, not
/// the 71-message table `msglog.rs` searches -- this runs once per entity
/// per frame, considerably hotter than a console command's own argument
/// parsing, so it stays a `Vec`, not a data structure sized for a table that
/// will never be large.
pub fn should_hide(model_name: &str) -> bool {
    match HIDDEN.lock() {
        Ok(list) => matches(&list, model_name),
        Err(_) => false,
    }
}

fn status() -> String {
    match HIDDEN.lock() {
        Ok(list) if list.is_empty() => format!("{COMMAND} = hiding nothing\n"),
        Ok(list) => format!("{COMMAND} = hiding {}\n", list.join(", ")),
        Err(_) => format!("{COMMAND} = (lock poisoned)\n"),
    }
}

fn usage() -> String {
    format!(
        "usage:\n\
         \x20 {COMMAND}                         what is being hidden\n\
         \x20 {COMMAND} <model-path>...          hide these entities by exact model path\n\
         \x20                                    e.g. {COMMAND} sprites/mapsprites/caparea.spr\n\
         \x20 {COMMAND} clear                    stop hiding anything\n\
         \x20 no \"all\" -- deliberately an allow-list, not a blanket toggle; see the module doc\n"
    )
}

fn args() -> Vec<String> {
    let Some(engfuncs) = crate::engine::engfuncs() else { return Vec::new() };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    (0..argc)
        .filter_map(|i| {
            let ptr = unsafe { (engfuncs.cmd_argv)(i) };
            if ptr.is_null() {
                return None;
            }
            Some(unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
        })
        .collect()
}

fn dispatch(argv: &[String]) -> String {
    let rest = &argv[1..];
    if rest.is_empty() {
        return format!("{}{}", status(), usage());
    }
    if rest.len() == 1 && rest[0].eq_ignore_ascii_case("clear") {
        if let Ok(mut list) = HIDDEN.lock() {
            list.clear();
        }
        return format!("{COMMAND}: hiding nothing\n");
    }
    // Anything else is a list of model paths, replacing whatever was hidden
    // before -- the same "each call restates the whole set" shape
    // `dodtools_deathmsg block <id>...` and `dodtools_msglog <name>...` use.
    if let Ok(mut list) = HIDDEN.lock() {
        *list = rest.to_vec();
    }
    format!("{COMMAND}: hiding {}\n", rest.join(", "))
}

pub unsafe extern "C" fn command() {
    let argv = args();
    let reply = dispatch(&argv);
    crate::commands::console_print(&reply);
    unsafe { crate::debug::report(&format!("hide_sprite: {} -> {}", argv.join(" "), reply.trim())) };
}

/// Folded into `dodtools_debug_status`, gated on being non-empty like
/// `msglog`'s own status line -- off by default, and a permanent "hiding
/// nothing" line would be noise in the overwhelmingly common case.
pub(crate) fn status_line() -> Option<String> {
    match HIDDEN.lock() {
        Ok(list) if !list.is_empty() => Some(format!("{COMMAND} = hiding {}", list.join(", "))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Nothing here touches the shared HIDDEN static: cargo test runs a
    // crate's tests in parallel by default with no isolation between them,
    // and dispatch()'s list-replacing subcommands mutate it -- the same
    // reasoning msglog.rs's test module documents for its own equivalent
    // WANTED/ACTIVE statics. matches() carries the actual comparison logic
    // and is pure, so it's what's tested; only dispatch()'s one
    // state-free path (bare invocation) is exercised directly.

    #[test]
    fn matching_is_case_insensitive() {
        let hidden = vec!["sprites/mapsprites/caparea.spr".to_string()];
        assert!(matches(&hidden, "sprites/mapsprites/caparea.spr"));
        assert!(matches(&hidden, "SPRITES/MAPSPRITES/CAPAREA.SPR"));
        assert!(!matches(&hidden, "sprites/mapsprites/speakerIcon.spr"));
    }

    #[test]
    fn an_empty_list_hides_nothing() {
        assert!(!matches(&[], "sprites/mapsprites/caparea.spr"));
    }

    #[test]
    fn bare_invocation_is_a_status_query_not_a_mutation() {
        let reply = dispatch(&["hide_sprite".to_string()]);
        assert!(reply.contains("usage"), "{reply}");
    }
}
