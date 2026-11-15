// messages/menu_scale.rs
// Errors of the Anniversary menu-size setting (`menu_scale.rs`, #237). Each
// function returns the text only.

use std::fmt::Display;

pub fn could_not_read(path: impl Display, err: impl Display) -> String {
    format!("could not read {path}: {err}")
}

pub fn scheme_is_utf16(path: impl Display) -> String {
    format!("{path} is UTF-16 text, which DoD Studio doesn't edit")
}

pub fn no_base_settings_block(path: impl Display) -> String {
    format!("{path} has no BaseSettings block to change")
}

pub fn scheme_not_ours(path: impl Display) -> String {
    format!("{path} is not DoD Studio's, so it was left alone; move it away to use this setting")
}

pub fn could_not_remove(path: impl Display, err: impl Display) -> String {
    format!("could not remove {path}: {err}")
}

/// `file` is the scheme's name, `resource` the game's `resource` folder.
pub fn no_scheme_to_copy(file: &str, resource: impl Display) -> String {
    format!("found no {file} to copy in {resource} or valve\\resource")
}

pub fn could_not_create(path: impl Display, err: impl Display) -> String {
    format!("could not create {path}: {err}")
}

pub fn could_not_write(path: impl Display, err: impl Display) -> String {
    format!("could not write {path}: {err}")
}

pub fn not_an_anniversary_install(exe: impl Display) -> String {
    format!(
        "{exe} is not a 25th Anniversary install: its engine already draws the menus at this size"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_scale_text_matches_its_original_inline_text() {
        assert_eq!(could_not_read("p", "e"), "could not read p: e");
        assert_eq!(
            scheme_is_utf16("p"),
            "p is UTF-16 text, which DoD Studio doesn't edit"
        );
        assert_eq!(
            no_base_settings_block("p"),
            "p has no BaseSettings block to change"
        );
        assert_eq!(
            scheme_not_ours("p"),
            "p is not DoD Studio's, so it was left alone; move it away to use this setting"
        );
        assert_eq!(could_not_remove("p", "e"), "could not remove p: e");
        assert_eq!(
            no_scheme_to_copy("ClientScheme.res", "D:/hl/resource"),
            "found no ClientScheme.res to copy in D:/hl/resource or valve\\resource"
        );
        assert_eq!(could_not_create("p", "e"), "could not create p: e");
        assert_eq!(could_not_write("p", "e"), "could not write p: e");
        assert_eq!(
            not_an_anniversary_install("hl.exe"),
            "hl.exe is not a 25th Anniversary install: its engine already draws the menus at this size"
        );
    }
}
