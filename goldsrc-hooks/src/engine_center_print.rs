//! The engine's own centre print, `svc_centerprint`, hidden under
//! `ds_hud_hide_center_text` (#677) along with the `TextMsg` centre text
//! `center_text` drops.
//!
//! ## Why it needs `hw.dll`
//!
//! `svc_centerprint` is an engine message: `hw.dll` draws it itself and
//! `client.dll` never sees it, so no user-message hook reaches it. Plugins
//! send it with the engine's own centre print (`engclient_print(...,
//! engprint_center)`) rather than DoD's `TextMsg`.
//!
//! ## The handler
//!
//! The svc table's opcode 26 entry, the same 13 bytes in both builds:
//!
//! ```text
//!     call MSG_ReadString     ; the text, read out of the message
//!     push eax
//!     call SCR_CenterPrint    ; E8 rel32   <- five nops while hidden
//!     pop  ecx                ; the handler pops its own argument
//!     ret
//! ```
//!
//! | build | handler | `SCR_CenterPrint` |
//! | --- | --- | --- |
//! | pre-Anniversary | `hw.dll+0x1dc80` | `+0x4ba90` |
//! | 25th Anniversary | `+0x1a9cc0` | `+0x24cde0` |
//!
//! The text is still read, so the message stream stays in step, and the
//! `pop ecx` still balances the `push`. Only this call changes: the other
//! callers of `SCR_CenterPrint` (`svc_finale` and `svc_cutscene`, Half-Life's
//! single-player endings, and the client's `pfnCenterPrint`, which DoD's
//! `client.dll` never calls) are untouched. Nothing of HLAE's sits on either handler
//! (`survey_hw_dll.py collide`).
//!
//! A line already showing when the setting turns on stays for the rest of its
//! `scr_centertime` (2 s by default).
//!
//! `tools/verify_engine_text_offsets.py` checks each build's signature against
//! both installs' `hw.dll`: that it is the svc table's opcode 26 handler, that
//! the call goes to `SCR_CenterPrint` (`strncpy` of 1023 bytes into the
//! centre string), and that `pop ecx` follows.

use crate::hw_call_site::{Build, CALL_LEN, CallSite};

/// The whole handler and the padding after it, both calls pinned.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        pattern: "E8 2B CE 00 00 50 E8 05 DE 02 00 59 C3 90 90 90",
        at: 6,
    },
    Build {
        name: "25th Anniversary",
        pattern: "E8 EB 01 01 00 50 E8 15 31 0A 00 59 C3 CC CC CC",
        at: 6,
    },
];

/// What the call becomes while hidden.
const HIDDEN: [u8; CALL_LEN] = [0x90; CALL_LEN];

static SITE: CallSite = CallSite::new("svc_centerprint's call to SCR_CenterPrint", &BUILDS);

/// Hides (`on`) or restores the engine's centre print. Returns whether it
/// wrote anything.
pub(crate) fn apply(on: bool) -> Result<bool, String> {
    SITE.put(on, HIDDEN)
}

/// Which build the handler was found in, once it has been.
pub(crate) fn build_name() -> Option<&'static str> {
    SITE.build_name()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_build_pins_the_whole_handler() {
        crate::hw_call_site::check_builds(&BUILDS);
        for build in &BUILDS {
            let t: Vec<&str> = build.pattern.split_whitespace().collect();
            assert_eq!(t[0], "E8", "{}: call MSG_ReadString", build.name);
            assert_eq!(t[5], "50", "{}: push eax", build.name);
            assert_eq!(
                &t[11..13],
                &["59", "C3"],
                "{}: pop ecx; ret -- the handler pops the argument",
                build.name
            );
        }
    }

    #[test]
    fn hidden_is_as_wide_as_the_call() {
        assert_eq!(HIDDEN.len(), CALL_LEN);
        assert!(HIDDEN.iter().all(|&b| b == 0x90));
    }
}
