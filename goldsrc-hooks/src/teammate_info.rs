//! `ds_hud_hide_teammate_info`: hide the name and health line a POV demo
//! shows under the crosshair while its player aims at a teammate
//! (`Name (100)`, #342).
//!
//! ## What draws there
//!
//! `CHudStatusBar::Draw` (`client+0x477a0`) returns at once while the viewer
//! is spectating (the observer mode at `+0xe88d4`) or dead (`g_iAlive`), and
//! otherwise draws one line, `"%s (%d)"`, for the player its `Think` traced
//! under the crosshair. `Think` keeps a target only when that player is on the
//! viewer's team, or when the viewer is a spectator or dead, and `Draw` never
//! draws for those two. So the line is only ever a living POV player's
//! teammate: never an enemy, and never anything in an HLTV demo or while
//! spectating. `hud_centerid` moves the line and nothing hides it, which is
//! why this is not a stock cvar (#342's survey).
//!
//! ## Mechanism
//!
//! `hudelement`'s vftable stub on `statusbar`: `Draw`'s slot points at
//! `CHudBase::Draw` while the cvar is on and back at its own `Draw` at 0. This
//! sets the bit; `commands::poll` applies it every frame.
//!
//! The team icon over a teammate's head (`cl_identiconmode`) is a sprite
//! entity the same element adds in `CreateEntities`, not `Draw`, so it stays;
//! `cl_identiconmode 0` is the stock way to hide it.

use std::sync::atomic::{AtomicPtr, Ordering};

use crate::engine::CvarSPartial;
use crate::hudelement;
use crate::names::console_name;

pub const NAME: &str = console_name!("hud_hide_teammate_info");

/// The element in `hudelement::ELEMENTS` this hides.
const ELEMENT: &str = "statusbar";

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Called by `commands/install.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// Follows the cvar. Called every frame from `commands::poll`, before
/// `hudelement::apply`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    if let Some(index) = hudelement::find(ELEMENT)
        && hudelement::is_hidden(index) != wanted
    {
        hudelement::set_hidden(index, wanted);
        unsafe {
            crate::debug::report(&format!(
                "teammate_info: {NAME} = {}",
                if wanted {
                    "1 (the teammate name and health line is hidden)"
                } else {
                    "0 (normal)"
                }
            ))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_element_is_the_status_bar() {
        let index = hudelement::find(ELEMENT).expect("statusbar is in the table");
        assert_eq!(hudelement::ELEMENTS[index].class, ".?AVCHudStatusBar@@");
    }
}
