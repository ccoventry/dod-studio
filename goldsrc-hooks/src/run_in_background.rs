//! `dodstudio_run_in_background`: the game runs at full speed while another
//! window has the foreground, so a capture batch doesn't need the screen.
//!
//! ## What the engine does
//!
//! Every frame, `CEngine::Frame` (pre-Anniversary `hw.dll+0xad2f0`, 25th
//! Anniversary `+0x221140`) asks the game window whether it is the active
//! app, and when it isn't, waits for input before the frame:
//!
//! ```text
//!     call [game->IsActiveApp]          ; vtable +0x28
//!     test al, al
//!     jne  .run_frame                   ; <- 75 xx, made EB xx
//!     push (state == 2) ? 50 : 20       ; milliseconds
//!     call [game->SleepUntilInput]      ; vtable +0x10: SDL_WaitEventTimeout
//! .run_frame:
//! ```
//!
//! So with another window in front, the game runs at most about 45 frames a
//! second. Measured 2026-10-10 on both builds: fast-forward (`host_framerate
//! 0.05`) went from 11.7x real time to 2.2x (PRE) or 1.5x (25th Anniversary)
//! as soon as another window took the foreground. HLAE's own recording was
//! not slowed, and its frames were right whether the game was visible,
//! covered or minimised (PRE); on the 25th Anniversary build a minimised game
//! records at about an eighth of the speed, so cover it instead.
//!
//! With the `jne` made a `jmp`, the wait never happens: the game runs as if
//! it had the foreground. It also means a game left at its menu in the
//! background keeps drawing as fast as it can, which is why this is a
//! setting, default 0, for batches to turn on.
//!
//! Found by a byte pattern per build, each unique in its own `hw.dll`;
//! `tools/verify_run_in_background_offsets.py` re-derives both offline.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("run_in_background");

/// One build's view of the inactive-window wait.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offset of the `jne rel8` that skips the wait while active.
    jne_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         call [edx+28] test al,al jne   mov edx,[esi+8] mov ecx,[game]   sub edx,2 mov eax,[ecx] neg edx sbb edx,edx and edx,-1e add edx,32 push edx call [eax+10]
        pattern: "FF 52 28 84 C0 75 ?? 8B 56 08 8B 0D ?? ?? ?? ?? 83 EA 02 8B 01 F7 DA 1B D2 83 E2 E2 83 C2 32 52 FF 50 10",
        jne_at: 5,
    },
    Site {
        build: "25th Anniversary",
        //         mov eax,[eax+28] call eax test al,al jne   mov ecx,[game]   mov eax,14       cmp [esi+8],2 push edi mov edi,32       mov edx,[ecx] cmove eax,edi push eax call [edx+10]
        pattern: "8B 40 28 FF D0 84 C0 75 ?? 8B 0D ?? ?? ?? ?? B8 14 00 00 00 83 7E 08 02 57 BF 32 00 00 00 8B 11 0F 44 C7 50 FF 52 10",
        jne_at: 7,
    },
];

const JNE: u8 = 0x75;
const JMP: u8 = 0xEB;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static BUILD: AtomicUsize = AtomicUsize::new(usize::MAX);
static ON: AtomicBool = AtomicBool::new(false);
static COMPLAINED: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// The `jne`, found once per session (hw.dll never reloads).
fn jne_address() -> Result<usize, String> {
    let cached = ADDRESS.load(Ordering::Acquire);
    if cached != 0 {
        return Ok(cached);
    }
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let mut errors = Vec::new();
    for (i, site) in SITES.iter().enumerate() {
        // Safety: `base` is a module handle the loader gave us.
        match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(found) => {
                let address = found + site.jne_at;
                ADDRESS.store(address, Ordering::Release);
                BUILD.store(i, Ordering::Release);
                return Ok(address);
            }
            Err(why) => errors.push(format!("{}: {why}", site.build)),
        }
    }
    Err(format!(
        "the inactive-window wait wasn't found ({})",
        errors.join("; ")
    ))
}

/// Applies or removes the change, returning whether a byte was written.
/// Decides from the byte, so it is cheap to call every frame.
pub fn set_on(on: bool) -> Result<bool, String> {
    let address = jne_address()?;
    let want = if on { JMP } else { JNE };
    // Safety: the scan proved this byte is mapped code.
    let present = unsafe { *(address as *const u8) };
    if present == want {
        ON.store(on, Ordering::Release);
        return Ok(false);
    }
    if present != JNE && present != JMP {
        return Err(format!(
            "the inactive-window wait holds {present:#04x}, neither {JNE:#04x} nor {JMP:#04x} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        return Err("could not make the inactive-window wait writable".to_string());
    }
    ON.store(on, Ordering::Release);
    Ok(true)
}

fn describe(on: bool) -> String {
    let build = SITES
        .get(BUILD.load(Ordering::Acquire))
        .map_or("", |s| s.build);
    if on {
        format!("1: full speed with another window in front ({build} hw.dll)")
    } else {
        format!("0: the engine waits for input while another window is in front ({build} hw.dll)")
    }
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    // Nothing to do until it's first turned on: the scan waits until then.
    if !wanted && !ON.load(Ordering::Acquire) && ADDRESS.load(Ordering::Acquire) == 0 {
        return;
    }
    match set_on(wanted) {
        Ok(false) => COMPLAINED.store(false, Ordering::Relaxed),
        Ok(true) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!("run_in_background: {NAME} = {}", describe(wanted)))
            };
        }
        Err(why) => {
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("run_in_background: {NAME} not applied -- {why}"))
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pattern's `jne` offset points at a `75` in the pattern itself.
    #[test]
    fn each_jne_offset_lands_on_the_jne() {
        for site in SITES {
            let bytes: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(bytes[site.jne_at], "75", "{}", site.build);
            assert_eq!(bytes[site.jne_at + 1], "??", "{}", site.build);
        }
    }
}
