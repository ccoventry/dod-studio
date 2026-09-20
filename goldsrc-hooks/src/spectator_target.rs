//! `dodtools_log_spectator_target` — logs `CHudSpectator`'s own idea of who
//! is being followed side by side with the entity the engine actually
//! renders a first-person viewmodel for, whenever either one changes.
//!
//! ## Why two numbers, not one
//!
//! Issue #206 ("the camera wanders off the spectated player") was originally
//! measured from `anim_fix`'s per-frame `GetViewModel()` read -- the entity
//! index the *engine* is currently rendering a first-person view for. That
//! is a different thing from `CHudSpectator`'s own spectator-target globals,
//! `g_iUser1`/`g_iUser2` (interface mode / followed-player index,
//! `+0xe88d4`/`+0xe88d8`), which is what the spectator UI's `follow`,
//! `spec_next`/`spec_prev` and the auto-director's `DRC_CMD_EVENT` handling
//! all read and write.
//!
//! A from-scratch static pass tonight (chasing #206 after #269's survey
//! found the mode global's real writer, `CHudSpectator::SetMode`)
//! exhaustively found **every** site in `client.dll` that writes
//! `g_iUser2` -- a whole-image byte search, not a guess -- and every one of
//! the seven is either gated on the auto-director cvar (`DRC_CMD_EVENT`,
//! confirmed absent from every local HLTV demo, per the earlier #206/#222
//! investigation) or driven by an explicit keypress
//! (`CHudSpectator::HandleButtonsDown`'s next/prev cycling), or is
//! `CHudSpectator::Reset` zeroing it at a handful of discrete transitions
//! (new demo, entering spectator UI) -- never a periodic, unprompted
//! switch. So whatever is actually moving the rendered view during ordinary
//! playback may not be `g_iUser2` changing at all, which would mean the fix
//! belongs in the engine's own view-entity selection (`hw.dll`, an
//! established harder target -- no RTTI, span-patched by HLAE, see
//! `docs/goldsrc_hw_dll_survey.md`) rather than in `CHudSpectator`.
//!
//! This module doesn't fix anything -- it settles which of those two it is,
//! cheaply, the next time someone can reproduce the wander live: if
//! `g_iUser2` stays constant while the viewmodel entity keeps changing, the
//! engine hypothesis is confirmed and `client.dll` patching is the wrong
//! layer to work in.
//!
//! ## Caveats
//!
//! The viewmodel-entity half piggybacks on `anim_fix`'s own per-frame
//! `GetViewModel()` read rather than duplicating that engine call, so it
//! only has fresh data while `dodtools_hltv_show_viewmodel_animations` is
//! on (any level). Turn that on too when using this to investigate #206 --
//! `g_iUser1`/`g_iUser2` are read directly here either way.
//!
//! This module's `poll` runs from `commands.rs`'s per-frame *prologue*,
//! which fires before `anim_fix::apply()` (the crate's one per-frame
//! callback slot) on the same frame -- so the viewmodel-entity value it logs
//! is `apply()`'s *previous* frame's result, up to one frame stale. Good
//! enough for spotting "did the rendered view change without the target
//! changing" across dozens of frames, not for frame-exact correlation.
//!
//! Analysis subject: `dod/cl_dlls/client.dll`, 977,816 bytes, byte-identical
//! across the stock, pre-Anniversary and post-Anniversary installs.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::engine;

/// `g_iUser1` -- the spectator interface mode (1..4). See #269's survey,
/// `docs/goldsrc_client_dll_survey.md` §10.
const MODE_RVA: usize = 0xe8_8d4;
/// `g_iUser2` -- the followed player's entity index, or 0 when none is set.
/// Exhaustively confirmed tonight as the only global the seven writers in
/// `client.dll` ever touch for this purpose.
const TARGET_RVA: usize = 0xe8_8d8;

pub static LOG: AtomicBool = AtomicBool::new(false);

static LAST_MODE: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_TARGET: AtomicI32 = AtomicI32::new(i32::MIN);
static LAST_VIEWMODEL: AtomicI32 = AtomicI32::new(i32::MIN);

fn read_i32(base: usize, rva: usize) -> Option<i32> {
    // Safety: rva is a fixed, confirmed offset into client.dll's own
    // .data section, read-only here.
    Some(unsafe { *((base + rva) as *const i32) })
}

/// Called once per frame from `commands.rs`'s per-frame prologue. Cheap: two
/// pointer reads and three atomic loads when off, which is the common case.
pub fn poll() {
    if !LOG.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::client_module_base() else { return };
    let Some(mode) = read_i32(base, MODE_RVA) else { return };
    let Some(target) = read_i32(base, TARGET_RVA) else { return };
    let viewmodel = crate::anim_fix::current_viewmodel_entity();

    let mode_changed = LAST_MODE.swap(mode, Ordering::Relaxed) != mode;
    let target_changed = LAST_TARGET.swap(target, Ordering::Relaxed) != target;
    let viewmodel_changed = LAST_VIEWMODEL.swap(viewmodel, Ordering::Relaxed) != viewmodel;

    if mode_changed || target_changed || viewmodel_changed {
        unsafe {
            crate::debug::report(&format!(
                "spectator_target: g_iUser1 (mode) = {mode}, g_iUser2 (CHudSpectator target) = {target}, \
                 GetViewModel() entity = {viewmodel} -- {}",
                if target_changed && !viewmodel_changed {
                    "CHudSpectator target changed but the rendered view didn't -- ordinary, e.g. a spec_next press"
                } else if !target_changed && viewmodel_changed {
                    "rendered view changed WITHOUT CHudSpectator's target changing -- the #206 engine hypothesis"
                } else if target_changed && viewmodel_changed {
                    "both changed together -- consistent with CHudSpectator driving the switch"
                } else {
                    "mode changed, target and view unchanged"
                }
            ))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_four_bytes_apart() {
        // g_iUser1 and g_iUser2 are adjacent dwords -- confirmed independently
        // across CHudSpectator::Reset, ::SetMode and the DRC_CMD_EVENT
        // handler tonight, not just asserted once.
        assert_eq!(TARGET_RVA - MODE_RVA, 4);
    }
}
