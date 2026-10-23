//! `ds_debug_log_gait <entity index> [frames]`: logs one player's model state
//! every frame (#927): the drawn origin and the last received one, the body
//! and gait sequences, and the renderer's own gait state (`gaityaw`,
//! `gaitframe`) from the engine's player record. `ds_debug_log_gait 0` stops.
//!
//! Logged from `HUD_AddEntity`, before the frame's render, so the gait
//! columns are the previous render's result.

use std::sync::atomic::{AtomicI32, Ordering};

use crate::engine::{self, ClEntityS};
use crate::names::console_name;

pub const NAME: &str = console_name!("debug_log_gait");

static TARGET: AtomicI32 = AtomicI32::new(0);
static TARGET2: AtomicI32 = AtomicI32::new(0);
static LEFT: AtomicI32 = AtomicI32::new(0);

/// The command.
pub unsafe extern "C" fn command() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    let arg = |n: i32| -> Option<i32> {
        let raw = unsafe { (engfuncs.cmd_argv)(n) };
        if raw.is_null() {
            return None;
        }
        unsafe { std::ffi::CStr::from_ptr(raw as *const std::ffi::c_char) }
            .to_string_lossy()
            .trim()
            .parse()
            .ok()
    };
    let target = if argc >= 2 { arg(1).unwrap_or(0) } else { 0 };
    let frames = if argc >= 3 { arg(2).unwrap_or(3000) } else { 3000 };
    let second = if argc >= 4 { arg(3).unwrap_or(0) } else { 0 };
    TARGET.store(target, Ordering::Relaxed);
    TARGET2.store(second, Ordering::Relaxed);
    LEFT.store(frames, Ordering::Relaxed);
    let line = format!("{NAME}: entity {target}, {frames} frames");
    crate::commands::console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&line) };
}

fn read_f32(base: *const u8, offset: usize) -> f32 {
    // Safety: base is the engine's player record, at least 0x250 bytes.
    unsafe { std::ptr::read_unaligned(base.add(offset) as *const f32) }
}

fn read_i32(base: *const u8, offset: usize) -> i32 {
    unsafe { std::ptr::read_unaligned(base.add(offset) as *const i32) }
}

/// From `HUD_AddEntity`, for every entity about to be drawn.
pub fn on_add_entity(entity_type: i32, ent: *const ClEntityS) {
    let target = TARGET.load(Ordering::Relaxed);
    if target == 0 || ent.is_null() {
        return;
    }
    // Safety: the engine's entity, valid for this call.
    let e = unsafe { &*ent };
    let second = TARGET2.load(Ordering::Relaxed);
    // -1: every player.
    let wanted = if target == -1 {
        (1..=32).contains(&e.index)
    } else {
        e.index == target || (second > 0 && e.index == second)
    };
    if !wanted {
        return;
    }
    let target = e.index;
    if LEFT.fetch_sub(1, Ordering::Relaxed) <= 0 {
        TARGET.store(0, Ordering::Relaxed);
        return;
    }
    let clock = crate::demo_seek::clock().map(|c| c.now).unwrap_or(-1.0);
    let cs = &e.curstate;
    let info = engine::studio_player_info(target - 1);
    let gait = if info.is_null() {
        "no player record".to_string()
    } else {
        format!(
            "pi_gaitseq {} gaitframe {:.2} gaityaw {:.1} prevgait {:.1} {:.1}",
            read_i32(info, 380),
            read_f32(info, 384),
            read_f32(info, 388),
            read_f32(info, 392),
            read_f32(info, 396),
        )
    };
    let line = format!(
        "gait: t {clock:.3} type {entity_type} ent {} org {:.2} {:.2} {:.2} ang {:.1} \
         cur {:.2} {:.2} msg {:.3} seq {} frame {:.1} fr {:.2} gaitseq {} vel {:.0} {:.0} ct {:.4} bl {} {} {}",
        e.index,
        e.origin.x,
        e.origin.y,
        e.origin.z,
        e.angles.y,
        cs.origin.x,
        cs.origin.y,
        cs.msg_time,
        cs.sequence,
        cs.frame,
        cs.framerate,
        cs.gaitsequence,
        cs.velocity.x,
        cs.velocity.y,
        engine::engine_client_time().unwrap_or(-1.0),
        cs.blending[0],
        cs.blending[1],
        gait,
    );
    unsafe { crate::debug::report(&line) };
}

/// R&D toggle for `gait_hold`: `ds_debug_gait_hold <0|1>`.
pub const HOLD_NAME: &str = console_name!("debug_gait_hold");

pub unsafe extern "C" fn hold_command() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let on = unsafe {
        let raw = (engfuncs.cmd_argv)(1);
        !raw.is_null() && std::ffi::CStr::from_ptr(raw as *const std::ffi::c_char).to_bytes() == b"1"
    };
    crate::gait_hold::ENABLED.store(on, Ordering::Relaxed);
    let line = format!("{HOLD_NAME}: {} (held so far {})", on as i32, crate::gait_hold::held());
    crate::commands::console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&line) };
}
