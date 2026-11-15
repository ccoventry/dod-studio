// messages/hook_dll.rs
// Activity-log warning when no hook DLL can be found for the game launch
// (`patch/types.rs`, #237). The function returns the line only.

use std::fmt::Display;

/// No `dodstudio_goldsrc_hooks.dll` at any of the three places looked in.
pub fn not_found(
    override_path: impl Display,
    bundled_default: impl Display,
    legacy_beside_hlae: impl Display,
) -> String {
    format!(
        "goldsrc-hooks: no dodstudio_goldsrc_hooks.dll found, so the sound/animation fixes and the \
         ds_* console commands will be absent this session. Tried: override={override_path}, \
         bundled default={bundled_default}, legacy spot beside hlae.exe={legacy_beside_hlae}. In a dev build only the \
         override is ever present -- set Studio -> Configuration -> Paths -> GoldSrc Hooks DLL."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_warning_keeps_its_wording() {
        assert_eq!(
            not_found("none", "D:/d.dll", "none"),
            "goldsrc-hooks: no dodstudio_goldsrc_hooks.dll found, so the sound/animation fixes and the ds_* console commands will be absent this session. Tried: override=none, bundled default=D:/d.dll, legacy spot beside hlae.exe=none. In a dev build only the override is ever present -- set Studio -> Configuration -> Paths -> GoldSrc Hooks DLL."
        );
    }
}
