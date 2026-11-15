//! The console half of the review lock (#855): wraps each command in
//! [`CONSOLE_DEMO`] and [`CONSOLE_CAMERA`] through the engine's own command
//! list ([`crate::cmd_list`]), and puts back a [`GUARDED_CVARS`] value that
//! changes while a review runs.
//!
//! One wrapper serves every command: a handler gets no arguments, but the
//! engine has already tokenised the line, so `argv[0]` names the command
//! that is running, and its index finds the handler the wrap kept. Commands
//! the engine, DemoPlayer.dll, HLAE or this DLL register later are wrapped
//! by [`poll`] as they appear.
//!
//! A cvar has no handler: the console sets it directly. So [`poll`] notes
//! each guarded cvar's value when a review starts, and when one differs it
//! prints the note and queues the noted value back (it applies at the next
//! frame's command buffer, so a typed value lasts one frame).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use super::{CONSOLE_CAMERA, CONSOLE_DEMO, GUARDED_CVARS};
use crate::cmd_list::{self, call_real, wrap};
use crate::engine::{self, CvarSPartial};

const COUNT: usize = CONSOLE_DEMO.len() + CONSOLE_CAMERA.len();

/// Each locked command's own handler, in [`name`] order; 0 until wrapped.
static REAL: [AtomicUsize; COUNT] = [const { AtomicUsize::new(0) }; COUNT];
/// Frames until the next look for commands not wrapped yet.
static NEXT_LOOK: AtomicU32 = AtomicU32::new(0);
/// Looks left before the missing ones are reported and the looking stops.
static LOOKS_LEFT: AtomicU32 = AtomicU32::new(LOOKS);
/// How often, and how many times, to look: every 30 frames for a few
/// minutes covers HLAE's and DemoPlayer.dll's late registrations.
const LOOK_EVERY: u32 = 30;
const LOOKS: u32 = 600;

fn name(index: usize) -> &'static str {
    CONSOLE_DEMO
        .iter()
        .chain(CONSOLE_CAMERA)
        .nth(index)
        .copied()
        .unwrap_or("")
}

fn index_of(command: &str) -> Option<usize> {
    CONSOLE_DEMO
        .iter()
        .chain(CONSOLE_CAMERA)
        .position(|n| n.eq_ignore_ascii_case(command))
}

/// Wraps every locked command the engine has registered so far. Called once
/// the engine's function table is captured; [`poll`] does the rest.
pub fn install() {
    wrap_found();
}

/// Wraps what it finds; true once every command is wrapped.
fn wrap_found() -> bool {
    cmd_list::for_each(|found, entry| {
        let Ok(found) = std::str::from_utf8(found) else {
            return;
        };
        if let Some(i) = index_of(found) {
            wrap(entry, &REAL[i], wrapped);
        }
    });
    REAL.iter().all(|r| r.load(Ordering::Relaxed) != 0)
}

/// Every locked command runs through here.
unsafe extern "C" fn wrapped() {
    let argv = cmd_list::argv();
    let Some(command) = argv.first() else {
        return;
    };
    let Some(i) = index_of(command) else {
        unsafe {
            crate::debug::report(&format!(
                "review_lock: a wrapped command ran as {command:?}, which is none of them; skipped"
            ))
        };
        return;
    };
    if super::refuses(command) {
        crate::commands::console_print(&super::console_note(command));
        unsafe {
            crate::debug::report(&format!(
                "review_lock: refused \"{}\" during a review",
                argv.join(" ")
            ))
        };
        return;
    }
    unsafe { call_real(&REAL[i]) };
    // The review's own command may change a guarded cvar on its way (the
    // camera line's `spec_mode`): that is the value to keep.
    if ARMED.load(Ordering::Relaxed) {
        note_cvars();
    }
}

/// Whether the cvar guard holds this review's values.
static ARMED: AtomicBool = AtomicBool::new(false);
/// Each guarded cvar, in [`GUARDED_CVARS`] order: its `cvar_t`, the value
/// to keep (`f32` bits), and whether a put-back is queued.
static CVARS: [AtomicUsize; GUARDED_CVARS.len()] =
    [const { AtomicUsize::new(0) }; GUARDED_CVARS.len()];
static KEPT: [AtomicU32; GUARDED_CVARS.len()] = [const { AtomicU32::new(0) }; GUARDED_CVARS.len()];
static PUTTING_BACK: [AtomicBool; GUARDED_CVARS.len()] =
    [const { AtomicBool::new(false) }; GUARDED_CVARS.len()];
/// Whether this review has printed the cvar's note already.
static NOTED: [AtomicBool; GUARDED_CVARS.len()] =
    [const { AtomicBool::new(false) }; GUARDED_CVARS.len()];

fn cvar(i: usize) -> *mut CvarSPartial {
    let known = CVARS[i].load(Ordering::Relaxed);
    if known != 0 {
        return known as *mut CvarSPartial;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return std::ptr::null_mut();
    };
    let Ok(c_name) = std::ffi::CString::new(GUARDED_CVARS[i]) else {
        return std::ptr::null_mut();
    };
    // Safety: the engine's own lookup; cvars live for the session.
    let found = unsafe { (engfuncs.pfn_get_cvar_pointer)(c_name.as_ptr()) };
    CVARS[i].store(found as usize, Ordering::Relaxed);
    found
}

/// Notes each guarded cvar's value now as the one to keep.
fn note_cvars() {
    for i in 0..GUARDED_CVARS.len() {
        let c = cvar(i);
        if !c.is_null() {
            // Safety: a live `cvar_t`, written on this thread.
            KEPT[i].store(unsafe { (*c).value }.to_bits(), Ordering::Relaxed);
            PUTTING_BACK[i].store(false, Ordering::Relaxed);
        }
    }
}

/// Puts back a guarded cvar that changed since the review started.
fn guard_cvars() {
    for (i, cvar_name) in GUARDED_CVARS.iter().enumerate() {
        let c = cvar(i);
        if c.is_null() {
            continue;
        }
        // Safety: as in `note_cvars`.
        let now = unsafe { (*c).value };
        let kept = f32::from_bits(KEPT[i].load(Ordering::Relaxed));
        if now.to_bits() == kept.to_bits() {
            PUTTING_BACK[i].store(false, Ordering::Relaxed);
            continue;
        }
        if PUTTING_BACK[i].swap(true, Ordering::Relaxed) {
            continue;
        }
        // Once per review: HLAE recording sets host_framerate every frame.
        if !NOTED[i].swap(true, Ordering::Relaxed) {
            crate::commands::console_print(&super::console_note(cvar_name));
            unsafe {
                crate::debug::report(&format!(
                    "review_lock: {cvar_name} changed to {now} during a review; put back to {kept}"
                ))
            };
        }
        if let Ok(line) = std::ffi::CString::new(format!("{cvar_name} \"{kept}\"\n")) {
            engine::client_cmd(&line);
        }
    }
}

/// Runs every frame: wraps commands registered late, and holds the guarded
/// cvars while a review runs.
pub fn poll() {
    if LOOKS_LEFT.load(Ordering::Relaxed) > 0 {
        let next = NEXT_LOOK.load(Ordering::Relaxed);
        if next > 0 {
            NEXT_LOOK.store(next - 1, Ordering::Relaxed);
        } else {
            NEXT_LOOK.store(LOOK_EVERY, Ordering::Relaxed);
            if wrap_found() {
                LOOKS_LEFT.store(0, Ordering::Relaxed);
                unsafe {
                    crate::debug::report(&format!(
                        "review_lock: wrapped all {COUNT} commands a review locks"
                    ))
                };
            } else if LOOKS_LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
                let missing: Vec<&str> = (0..COUNT)
                    .filter(|&i| REAL[i].load(Ordering::Relaxed) == 0)
                    .map(name)
                    .collect();
                unsafe {
                    crate::debug::report(&format!(
                        "review_lock: not registered, so not locked during a review: {}",
                        missing.join(", ")
                    ))
                };
            }
        }
    }

    let locked = super::locked();
    match (locked, ARMED.load(Ordering::Relaxed)) {
        (true, false) => {
            note_cvars();
            for noted in &NOTED {
                noted.store(false, Ordering::Relaxed);
            }
            ARMED.store(true, Ordering::Relaxed);
        }
        (true, true) => guard_cvars(),
        (false, true) => {
            ARMED.store(false, Ordering::Relaxed);
            super::forget_own();
        }
        (false, false) => {}
    }
}
