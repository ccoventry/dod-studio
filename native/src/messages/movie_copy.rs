// messages/movie_copy.rs
// Errors of "Make a movie copy" (`shared/movie_copy.rs`, #237): a name the
// folder cannot take, and what stopped the copy. Each function returns the
// text only.

use std::fmt::Display;

pub const MOVIE_COPY_NEEDS_A_NAME: &str = "The movie copy needs a name";

pub fn name_can_only_hold(name: &str) -> String {
    format!("\"{name}\" can only hold letters, digits and _")
}

pub fn name_reserved_by_windows(name: &str) -> String {
    format!("Windows doesn't allow a folder named \"{name}\"")
}

pub fn name_already_exists(name: &str) -> String {
    format!("There's already a \"{name}\" in the Half-Life folder")
}

pub fn no_dod_folder_to_copy(source: impl Display) -> String {
    format!("There's no dod folder to copy at {source}")
}

pub fn could_not_list_dod_folder(err: impl Display) -> String {
    format!("Couldn't list the dod folder to copy it: {err}")
}

/// `needed` and `free` are the sizes already rendered (`1.5 GB`).
pub fn copy_needs_more_space(
    needed: impl Display,
    drive_of: impl Display,
    free: impl Display,
) -> String {
    format!(
        "The copy needs {needed} and the drive holding {drive_of} has {free} free (DoD Studio leaves 1 GB spare)"
    )
}

pub fn left_in_place_tail(target: impl Display) -> String {
    format!("; something else wrote into {target} meanwhile, so it was left")
}

pub const REMOVED_COPIED_TAIL: &str = "; what it had copied was removed";

pub fn appeared_during_copy(path: impl Display) -> String {
    format!("{path} appeared during the copy")
}

pub fn could_not_make(path: impl Display, err: impl Display) -> String {
    format!("couldn't make {path}: {err}")
}

pub fn could_not_read(path: impl Display, err: impl Display) -> String {
    format!("couldn't read {path}: {err}")
}

pub fn could_not_write(path: impl Display, err: impl Display) -> String {
    format!("couldn't write {path}: {err}")
}

/// Listing a folder for the plan failed; `kind` is kept by the caller.
pub fn could_not_read_dir(dir: impl Display, err: impl Display) -> String {
    format!("can't read {dir}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_copy_text_matches_its_original_inline_text() {
        assert_eq!(MOVIE_COPY_NEEDS_A_NAME, "The movie copy needs a name");
        assert_eq!(
            name_can_only_hold("a b"),
            "\"a b\" can only hold letters, digits and _"
        );
        assert_eq!(
            name_reserved_by_windows("con"),
            "Windows doesn't allow a folder named \"con\""
        );
        assert_eq!(
            name_already_exists("x"),
            "There's already a \"x\" in the Half-Life folder"
        );
        assert_eq!(
            no_dod_folder_to_copy("D:/hl/dod"),
            "There's no dod folder to copy at D:/hl/dod"
        );
        assert_eq!(
            could_not_list_dod_folder("denied"),
            "Couldn't list the dod folder to copy it: denied"
        );
        assert_eq!(
            copy_needs_more_space("5.0 GB", "D:/hl", "1.0 GB"),
            "The copy needs 5.0 GB and the drive holding D:/hl has 1.0 GB free (DoD Studio leaves 1 GB spare)"
        );
        assert_eq!(
            left_in_place_tail("D:/x"),
            "; something else wrote into D:/x meanwhile, so it was left"
        );
        assert_eq!(REMOVED_COPIED_TAIL, "; what it had copied was removed");
        assert_eq!(
            appeared_during_copy("D:/x"),
            "D:/x appeared during the copy"
        );
        assert_eq!(could_not_make("D:/x", "e"), "couldn't make D:/x: e");
        assert_eq!(could_not_read("D:/x", "e"), "couldn't read D:/x: e");
        assert_eq!(could_not_write("D:/x", "e"), "couldn't write D:/x: e");
        assert_eq!(could_not_read_dir("D:/x", "e"), "can't read D:/x: e");
    }
}
