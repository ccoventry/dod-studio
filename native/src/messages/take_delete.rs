// messages/take_delete.rs
// Why a take folder is refused or fails to be deleted (`hlcr/take_delete.rs`,
// #237). Each function returns the text only; `folder` is the take folder's
// `.display()`.

use std::fmt::Display;

pub fn not_a_full_path(folder: impl Display) -> String {
    format!("{folder} isn't a full path, so it was not deleted")
}

pub fn cant_be_read(folder: impl Display, err: impl Display) -> String {
    format!("{folder} can't be read: {err}")
}

pub fn is_a_link(folder: impl Display) -> String {
    format!("{folder} is a link, not a take folder, so it was not deleted")
}

pub fn not_a_folder(folder: impl Display) -> String {
    format!("{folder} isn't a folder")
}

pub fn cant_be_resolved(folder: impl Display, err: impl Display) -> String {
    format!("{folder} can't be resolved: {err}")
}

pub fn outside_destinations(folder: impl Display) -> String {
    format!(
        "{folder} isn't inside a Destinations folder (Configuration → Destinations), so it was not deleted"
    )
}

pub fn no_captured_streams(folder: impl Display) -> String {
    format!("{folder} isn't a take folder (no captured streams in it), so it was not deleted")
}

pub fn cant_be_listed(folder: impl Display, err: impl Display) -> String {
    format!("{folder} can't be listed: {err}")
}

pub fn holds_other_takes(folder: impl Display) -> String {
    format!("{folder} holds other takes, so it was not deleted")
}

pub fn not_in_a_session_folder(folder: impl Display) -> String {
    format!("{folder} isn't inside a session folder, so it was not deleted")
}

pub fn could_not_recycle(folder: impl Display, errors: impl Display) -> String {
    format!("Couldn't move {folder} to the Recycle Bin: {errors}")
}

pub fn could_not_delete(path: impl Display, err: impl Display) -> String {
    format!("Couldn't delete {path}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_delete_text_matches_its_original_inline_text() {
        assert_eq!(
            not_a_full_path("t"),
            "t isn't a full path, so it was not deleted"
        );
        assert_eq!(cant_be_read("t", "e"), "t can't be read: e");
        assert_eq!(
            is_a_link("t"),
            "t is a link, not a take folder, so it was not deleted"
        );
        assert_eq!(not_a_folder("t"), "t isn't a folder");
        assert_eq!(cant_be_resolved("t", "e"), "t can't be resolved: e");
        assert_eq!(
            outside_destinations("t"),
            "t isn't inside a Destinations folder (Configuration → Destinations), so it was not deleted"
        );
        assert_eq!(
            no_captured_streams("t"),
            "t isn't a take folder (no captured streams in it), so it was not deleted"
        );
        assert_eq!(cant_be_listed("t", "e"), "t can't be listed: e");
        assert_eq!(
            holds_other_takes("t"),
            "t holds other takes, so it was not deleted"
        );
        assert_eq!(
            not_in_a_session_folder("t"),
            "t isn't inside a session folder, so it was not deleted"
        );
        assert_eq!(
            could_not_recycle("t", "a; b"),
            "Couldn't move t to the Recycle Bin: a; b"
        );
        assert_eq!(could_not_delete("p", "e"), "Couldn't delete p: e");
    }
}
