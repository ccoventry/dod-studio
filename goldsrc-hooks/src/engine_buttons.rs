//! Buttons inside GameUI's windows can run console commands: a button whose
//! command is `engine <console command>` runs it, as the ESC menu's entries
//! already do (issue #408).
//!
//! ## Why a window's button can't today
//!
//! A button sends its command to the window it sits in. The ESC menu's
//! entries go straight to `CBasePanel::OnCommand`, which hands anything
//! starting with `engine ` to the engine -- that is how a `GameMenu.res` entry
//! runs a console command (tested live). A window's own `OnCommand` handles
//! what it knows (`load`, say) and passes the rest to `Frame::OnCommand`,
//! which knows `Close`, `CloseModal`, `Minimize` and a couple more, and passes
//! the rest to `Panel::OnCommand`. In this vgui2 that is an empty function,
//! `ret 4`. So `engine echo hello` on a button added in build mode does
//! nothing, and the saved `.res` shows the command was stored correctly
//! (tested live, pre-Anniversary, 2026-09-26). Source's vgui passes unknown
//! commands up to the parent instead, which is why guides written for Source
//! say it works.
//!
//! ## The fix: one call
//!
//! The empty function can't be patched: the linker folded every empty
//! one-argument method into that one `ret 4`, which 76 vftable entries share
//! on the pre-Anniversary build and 629 on the Anniversary one. What is
//! patched instead is the one call to it at the end of `Frame::OnCommand` --
//! the last stop for every window's unknown command. It now calls
//! [`on_command`], which runs `engine <command>` through the engine's
//! `pfnClientCmd` and passes everything else on to the empty function as
//! before. Only commands that were being dropped change.
//!
//! Every GameUI window whose own `OnCommand` hands unknown commands to
//! `Frame`'s -- the Load Demo window does -- gets this, and so will a DoD
//! Studio window built on GameUI's `Frame`. A window that swallows commands
//! itself won't.
//!
//! Unlike `CBasePanel`, which looks for `engine ` anywhere in the command,
//! this takes it only at the start.
//!
//! It is on by default, since it only acts on commands that went nowhere;
//! `GOLDSRC_HOOKS_ENGINE_BUTTONS=0` turns it off. [`GAMEUI_BUILDS`] names each
//! GameUI by PE timestamp and image size, and before writing it checks the
//! call still goes to the empty function. `tools/verify_engine_buttons.py`
//! checks every address against both movie installs.

// The addresses are only used by the 32-bit build; a host build compiles them
// for the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::AtomicBool;

/// Whether to install at all -- `GOLDSRC_HOOKS_ENGINE_BUTTONS=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// What a command must start with to go to the engine, as in `CBasePanel`.
const PREFIX: &[u8] = b"engine ";

/// One `GameUI.dll` build: its identity and the call being redirected.
pub struct GameUiBuild {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
    /// `Frame::OnCommand`'s `call Panel::OnCommand`, the `E8` byte.
    pub call_site: usize,
    /// `Panel::OnCommand`: `ret 4`.
    pub panel_on_command: usize,
}

pub const GAMEUI_BUILDS: [GameUiBuild; 2] = [
    GameUiBuild {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cefc,
        size_of_image: 0xe_3000,
        call_site: 0x4_d0ed,
        panel_on_command: 0x4_6b10,
    },
    GameUiBuild {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_99b2,
        size_of_image: 0xd_f000,
        call_site: 0x5_4832,
        panel_on_command: 0x1_c570,
    },
];

/// `pfnClientCmd`'s slot in `cl_enginefunc_t` (`engine.rs` has the same).
const ENGFUNCS_SLOT_CLIENT_CMD: usize = 20;

/// The console command in `command`, if it is `engine <something>`.
fn console_command(command: &[u8]) -> Option<&[u8]> {
    let rest = command.strip_prefix(PREFIX)?;
    let rest = rest.trim_ascii();
    (!rest.is_empty()).then_some(rest)
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;

    use super::*;

    type OnCommandFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char);
    type ClientCmdFn = unsafe extern "C" fn(*const c_char);

    static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
    static DONE: AtomicBool = AtomicBool::new(false);

    fn run(command: &[u8]) {
        let Some(engfuncs) = crate::engine::engfuncs() else {
            return;
        };
        let mut line = command.to_vec();
        line.push(b'\n');
        let Ok(line) = CString::new(line) else {
            return;
        };
        // Safety: a slot of the engine's function table, as engine.rs reads it.
        unsafe {
            let slot = *(engfuncs as *const _ as *const usize).add(ENGFUNCS_SLOT_CLIENT_CMD);
            if slot == 0 {
                return;
            }
            let client_cmd: ClientCmdFn = std::mem::transmute(slot);
            client_cmd(line.as_ptr());
            crate::debug::report(&format!(
                "engine_buttons: a window's button ran \"{}\"",
                String::from_utf8_lossy(command).escape_debug()
            ));
        }
    }

    /// Stands in for `Panel::OnCommand` at the end of `Frame::OnCommand`.
    unsafe extern "thiscall" fn on_command(panel: *mut c_void, command: *const c_char) {
        unsafe {
            if !command.is_null()
                && let Some(console) = console_command(CStr::from_ptr(command).to_bytes())
            {
                run(console);
                return;
            }
            let original: OnCommandFn = std::mem::transmute(ORIGINAL.load(Ordering::Relaxed));
            original(panel, command)
        }
    }

    fn install(base: usize) -> Result<&'static str, String> {
        // Safety: a module handle the loader gave us.
        let identity = unsafe { crate::pe::image_identity(base as *mut u8) };
        let build = GAMEUI_BUILDS
            .iter()
            .find(|b| identity == Some((b.time_date_stamp, b.size_of_image)))
            .ok_or("GameUI.dll is a build this was not checked against")?;
        let site = base + build.call_site;
        let original = base + build.panel_on_command;
        // Safety: inside the identified image; checked before anything is written.
        unsafe {
            let rel = (site as *const u8).add(1).cast::<i32>().read_unaligned();
            if *(site as *const u8) != 0xe8 || (site + 5).wrapping_add(rel as usize) != original {
                return Err(format!(
                    "the call at +{:#x} doesn't go to Panel::OnCommand any more -- something else has patched it",
                    build.call_site
                ));
            }
            if std::slice::from_raw_parts(original as *const u8, 3) != [0xc2, 0x04, 0x00] {
                return Err("Panel::OnCommand isn't the empty `ret 4` it was".into());
            }
            ORIGINAL.store(original, Ordering::Relaxed);
            let ours = on_command as OnCommandFn as usize;
            let rel = (ours as i64 - (site + 5) as i64) as i32;
            if !crate::patch::write_code_bytes(site + 1, &rel.to_le_bytes()) {
                return Err("could not make Frame::OnCommand writable".into());
            }
        }
        Ok(build.name)
    }

    pub(super) fn poll() {
        if DONE.load(Ordering::Relaxed) {
            return;
        }
        if !ENABLED.load(Ordering::Relaxed) {
            DONE.store(true, Ordering::Relaxed);
            unsafe {
                crate::debug::report(
                    "engine_buttons: off (GOLDSRC_HOOKS_ENGINE_BUTTONS=0) -- `engine <cmd>` buttons in windows do nothing (#408)",
                )
            };
            return;
        }
        let handle = unsafe { GetModuleHandleA(c"GameUI.dll".as_ptr() as *const u8) };
        if handle.is_null() {
            return; // not loaded yet: try again next frame
        }
        DONE.store(true, Ordering::Relaxed);
        let message = match install(handle as usize) {
            Ok(build) => format!(
                "engine_buttons: `engine <cmd>` buttons in GameUI windows run their command ({build} GameUI) (#408)"
            ),
            Err(why) => format!("engine_buttons: not installed -- {why}"),
        };
        unsafe { crate::debug::report(&message) };
    }
}

/// Installs once `GameUI.dll` is loaded. Runs every frame from
/// `commands::poll`; one atomic load after.
pub fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_engine_commands_are_taken() {
        assert_eq!(
            console_command(b"engine echo hello"),
            Some(&b"echo hello"[..])
        );
        assert_eq!(
            console_command(b"engine  dodstudio_seek_by -10 "),
            Some(&b"dodstudio_seek_by -10"[..])
        );
        assert_eq!(console_command(b"engine "), None);
        assert_eq!(console_command(b"engine"), None);
        assert_eq!(console_command(b"load"), None);
        assert_eq!(console_command(b"Close"), None);
        // Only at the start, unlike CBasePanel's strstr.
        assert_eq!(console_command(b"xengine echo"), None);
    }

    #[test]
    fn the_builds_are_told_apart() {
        assert_ne!(
            GAMEUI_BUILDS[0].time_date_stamp,
            GAMEUI_BUILDS[1].time_date_stamp
        );
    }
}
