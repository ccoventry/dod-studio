//! Keeping the camera on one player in an HLTV demo (issue #206):
//!
//! - `dodstudio_spec_lock 1` stops the game moving the camera to the next
//!   player four seconds after the one being watched dies;
//! - `dodstudio_spec_target <player>` puts the camera on a player by number.
//!
//! ## What moves the camera
//!
//! Not the demo. An HLTV recording carries no camera commands (its
//! `svc_director` messages are `START`, a 16s `STATUS` heartbeat and title
//! cards), and `iuser1`/`iuser2` are not sent for the proxy. The target is a
//! plain global in `client.dll`, `g_iUser2`, and only the client's own code
//! writes it.
//!
//! One piece of that code runs unprompted. `CHudDoDCommon::Draw` (it draws
//! nothing; see `whamemer/dod13-client`'s `cl_dll/dod_common.cpp`) watches
//! the spectated player and, four seconds after he dies, issues the same
//! keypress a viewer would:
//!
//! ```text
//!     +0x2c759  mov eax, [g_iUser1]        ; spectating at all?
//!     +0x2c760  je   +0x2c7b9
//!     +0x2c762  mov eax, [g_iUser2]        ; <- the pattern starts here
//!     +0x2c767  test eax, eax
//!     +0x2c769  je   +0x2c7b9
//!     +0x2c76b  lea ecx, [eax+eax*2]
//!     +0x2c76e  shl ecx, 4
//!     +0x2c771  mov al, [ecx+g_PlayerExtraInfo.dead]
//!     +0x2c777  test al, al
//!     +0x2c779  je   +0x2c7af              ; alive: clear the timer   <- patched
//!     ...                                  ; dead: start it, or if 4s have passed:
//!     +0x2c7a1  push "+attack;wait;-attack"
//!     +0x2c7a6  call [pfnClientCmd]
//!     +0x2c7af  mov [fl_DeadTargetSwitch], 0
//! ```
//!
//! Seen live with `dodstudio_debug_log_spectator_target 1`: the watched
//! player dies at 587.78, `client.dll ClientCmd: "+attack;wait;-attack"` is
//! logged at 591.77, and `g_iUser2` goes from 2 to 3 on the next frame.
//!
//! ## The lock
//!
//! One byte: the `je` at `+0x2c779` becomes `jmp`, so the dead player is
//! treated as alive by this routine alone, the timer stays cleared and the
//! keypress is never issued. Nothing is corrected after the fact, so there
//! is no frame in which the camera has already moved. A viewer's own
//! `+attack` still changes player.
//!
//! While the watched player is dead the view is the game's own for a dead
//! target (`V_GetInEyePos` rolls the camera and lowers it by 8), and it is
//! his again when he respawns.
//!
//! ## The target
//!
//! `g_iUser2` is written directly. The in-eye camera, the viewmodel and the
//! spectator panel all read it every frame, and nothing in an HLTV recording
//! writes it back. Its address is taken from the routine above (the operand
//! of the `mov` the pattern starts on), not from a fixed offset; `g_iUser1`,
//! the spectator mode, is the dword before it.
//!
//! The number is the player's entity index, the same number
//! `dodstudio_deathmsg players` lists.

use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::commands::console_print;
use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

/// The cvar: 1 keeps the camera on a player through his death.
pub const LOCK_NAME: &str = console_name!("spec_lock");
/// The command: puts the camera on a player.
pub const TARGET_NAME: &str = console_name!("spec_target");

/// From the read of `g_iUser2` to the `je` that skips the death switch for a
/// living player. Unique in `client.dll`, which is the same file in both
/// builds.
const PATTERN: &str = "A1 ?? ?? ?? ?? 85 C0 74 4E 8D 0C 40 C1 E1 04 8A 81 ?? ?? ?? ?? \
                       84 C0 74 34";
/// Where `g_iUser2`'s address sits in the pattern.
const TARGET_OPERAND_AT: usize = 1;
/// Where the patched `je` sits.
const JE_AT: usize = 23;
const JE: u8 = 0x74;
const JMP: u8 = 0xEB;

/// The highest player number the game has.
const MAX_PLAYERS: i32 = 32;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// Resolved address of the pattern, or 0 before the first successful scan.
static SPAN_ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The module base it was resolved against (see `scoreboard::SCANNED_BASE`).
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// Whether the `je` is a `jmp` in the loaded module now.
static LOCKED: AtomicBool = AtomicBool::new(false);
/// Whether a failure to apply has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// Called once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn span_address() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = SPAN_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let address = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the switch away from a dead player -- {why}"))?;
    SPAN_ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Applies or removes the lock, returning whether a byte was written.
/// Idempotent and cheap to call every frame; it decides from the byte in the
/// code, not from a flag.
pub fn set_locked(locked: bool) -> Result<bool, String> {
    let address = span_address()? + JE_AT;
    let want = if locked { JMP } else { JE };
    // Safety: the scan proved the pattern's bytes are mapped code.
    let present = unsafe { *(address as *const u8) };
    if present == want {
        LOCKED.store(locked, Ordering::Release);
        return Ok(false);
    }
    if present != JE && present != JMP {
        return Err(format!(
            "the switch away from a dead player holds {present:#04x}, which is neither {JE:#04x} nor {JMP:#04x} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        return Err("could not make the switch away from a dead player writable".to_string());
    }
    LOCKED.store(locked, Ordering::Release);
    Ok(true)
}

/// Follows the cvar. Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    match set_locked(wanted) {
        Ok(false) => COMPLAINED.store(false, Ordering::Relaxed),
        Ok(true) => {
            COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "spectator_follow: {LOCK_NAME} = {}",
                    describe(wanted)
                ))
            };
        }
        Err(why) => {
            // Off and never applied is the normal state before client.dll
            // loads; only a lock that was asked for is worth a line.
            if wanted && !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "spectator_follow: {LOCK_NAME} not applied yet -- {why}"
                    ))
                };
            }
        }
    }
}

fn describe(locked: bool) -> &'static str {
    if locked {
        "1: the camera stays on the player being watched when he dies"
    } else {
        "0: the game moves the camera on four seconds after he dies"
    }
}

/// One line for `dodstudio_debug_status`, once the lock has ever been on.
pub fn status_line() -> Option<String> {
    LOCKED
        .load(Ordering::Relaxed)
        .then(|| format!("spectator lock: {LOCK_NAME} = {}", describe(true)))
}

/// `(g_iUser1, g_iUser2)`: the spectator mode and the player being watched.
fn globals() -> Result<(*mut i32, *mut i32), String> {
    let span = span_address()?;
    // Safety: the scan proved the operand's four bytes are mapped code.
    let target =
        unsafe { std::ptr::read_unaligned((span + TARGET_OPERAND_AT) as *const u32) } as usize;
    Ok(((target - 4) as *mut i32, target as *mut i32))
}

/// What `dodstudio_spec_target`'s argument asks for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// No argument: say who the camera is on.
    Report,
    Player(i32),
    Invalid,
}

fn parse(argument: Option<&str>) -> Request {
    match argument.map(str::trim) {
        None | Some("") => Request::Report,
        Some(text) => match text.parse::<i32>() {
            Ok(n) if (1..=MAX_PLAYERS).contains(&n) => Request::Player(n),
            _ => Request::Invalid,
        },
    }
}

/// `dodstudio_spec_target [player]`.
pub unsafe extern "C" fn target_command() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let argument = if unsafe { (engfuncs.cmd_argc)() } >= 2 {
        let arg = unsafe { (engfuncs.cmd_argv)(1) };
        (!arg.is_null()).then(|| {
            unsafe { CStr::from_ptr(arg as *const c_char) }
                .to_string_lossy()
                .into_owned()
        })
    } else {
        None
    };
    let say = |text: String| {
        console_print(&format!("{TARGET_NAME}: {text}\n"));
        unsafe { crate::debug::report(&format!("spectator_follow: {TARGET_NAME}: {text}")) };
    };

    let (mode, target) = match globals() {
        Ok(found) => found,
        Err(why) => return say(format!("not available -- {why}")),
    };
    // Safety: both point into client.dll's data, written by the game on this
    // same thread.
    let (mode_now, target_now) = unsafe { (*mode, *target) };
    let player = match parse(argument.as_deref()) {
        Request::Report => {
            return say(if mode_now == 0 || target_now == 0 {
                format!(
                    "the camera is on no player. usage: {TARGET_NAME} <1-{MAX_PLAYERS}> (dodstudio_deathmsg players lists the numbers)"
                )
            } else {
                format!("the camera is on player {target_now}")
            });
        }
        Request::Invalid => {
            return say(format!(
                "expected a player number, 1-{MAX_PLAYERS} (dodstudio_deathmsg players lists them)"
            ));
        }
        Request::Player(n) => n,
    };

    // In a first-person demo the view is the recording player's own, and in
    // a live game the server decides who a spectator follows.
    if unsafe { (engfuncs.is_spectate_only)() } == 0 {
        return say("only works while watching an HLTV demo".to_string());
    }
    if mode_now == 0 {
        return say(
            "the spectator view is not up yet; try again once the demo is playing".to_string(),
        );
    }
    let entity = unsafe { (engfuncs.get_entity_by_index)(player) };
    if entity.is_null() || unsafe { (*entity).player } == 0 {
        return say(format!("there is no player {player}"));
    }
    // Safety: see above.
    unsafe { *target = player };
    say(format!("the camera is on player {player}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both offsets are read off the disassembly in the module docs. Anchoring
    /// them to the pattern means a mistyped pattern cannot quietly move the
    /// patched byte onto something else.
    #[test]
    fn the_offsets_index_what_the_pattern_says() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(tokens[TARGET_OPERAND_AT - 1], "A1");
        assert!(
            tokens[TARGET_OPERAND_AT..TARGET_OPERAND_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        assert_eq!(tokens[JE_AT], "74");
        // The displacement survives the edit: that is why one byte is enough.
        assert_eq!(tokens[JE_AT + 1], "34");
        assert_eq!(tokens.len(), JE_AT + 2);
        // The byte before is the `test al, al` on the dead flag.
        assert_eq!(&tokens[JE_AT - 2..JE_AT], ["84", "C0"]);
    }

    #[test]
    fn the_target_is_a_player_number() {
        assert_eq!(parse(None), Request::Report);
        assert_eq!(parse(Some("")), Request::Report);
        assert_eq!(parse(Some("1")), Request::Player(1));
        assert_eq!(parse(Some(" 32 ")), Request::Player(32));
        for bad in ["0", "33", "-1", "two", "2.5"] {
            assert_eq!(parse(Some(bad)), Request::Invalid, "{bad}");
        }
    }

    #[test]
    fn neither_name_is_the_start_of_the_other() {
        assert!(!LOCK_NAME.starts_with(TARGET_NAME));
        assert!(!TARGET_NAME.starts_with(LOCK_NAME));
        for name in [LOCK_NAME, TARGET_NAME] {
            assert!(!name.starts_with(crate::names::SPEC_MATCH_POV), "{name}");
            assert!(!crate::names::SPEC_MATCH_POV.starts_with(name), "{name}");
        }
    }
}
