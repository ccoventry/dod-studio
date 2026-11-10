// messages/killfeed_layout.rs
// What Studio says when the kill feed's even layout (`ds_hud_deathmsg layout
// even`) can't draw and the game's own layout shows (#1339): each reason the
// hook sends, with the step that fixes it, for the activity log and the
// warning a capture batch shows (`capture_engine/markers/killfeed_layout.rs`).

/// The atlas's sprite or table didn't load; `why` is the hook's own reason,
/// which names the file and, for a missing one, the build step.
pub fn files(why: &str) -> String {
    if why.is_empty() {
        "the kill-feed atlas didn't load. Build it with killfeed_icons.py --even".to_string()
    } else {
        format!("the kill-feed atlas didn't load: {why}")
    }
}

pub const HD_OFF: &str = "HD textures are off (ds_hd_enabled 0), and the even layout draws its icons from them. \
     Turn HD textures on, then start the game again: the atlas loads once, when the game starts";

/// The atlas went up without its HD frames for `style`.
pub fn no_hd(style: &str) -> String {
    let style = if style.is_empty() {
        "the style in use"
    } else {
        style
    };
    format!(
        "the kill-feed atlas has no HD frames for the {style} HD style. If HD textures were \
         off when the game started, just start it again. Otherwise build them with \
         killfeed_icons.py --even with HD_STYLE={style}, then start the game again"
    )
}

pub const NO_NAMES: &str = "none of the game's kill-feed icons (hud.txt) is in the atlas's \
     table. Build the atlas again with killfeed_icons.py --even";

pub const NO_HOOKS: &str = "the hook couldn't take over the kill feed's icon draw in this \
     game build. `ds_hud_deathmsg status` and the hook's log say which part is missing";

/// A reason this Studio doesn't know yet, from a newer hook DLL.
pub fn unknown(code: &str) -> String {
    if code.is_empty() {
        "the hook gave no reason".to_string()
    } else {
        format!("the hook gave a reason this Studio doesn't know ({code})")
    }
}

/// The warning a capture batch shows; `reason` is one of the above.
pub fn warning(reason: &str) -> String {
    format!("Kill feed: the game's own layout is showing, not the even one: {reason}.")
}

/// The activity log's line for the same, with the hook's own words.
pub fn waiting_log_line(reason: &str, style: &str, why: &str) -> String {
    let mut line = format!(
        "⚠️ [HLAE] **Kill feed: the game's own layout is showing, not the even one** \
         (`ds_hud_deathmsg layout even` can't draw): {reason}."
    );
    if !why.is_empty() {
        line.push_str(&format!(" The hook says: {why}"));
    }
    if !style.is_empty() {
        line.push_str(&format!(" (HD style {style})"));
    }
    line
}

pub const EVEN_DRAWS_LOG_LINE: &str =
    "[HLAE] Kill feed: the even layout draws (`ds_hud_deathmsg layout even`)";

pub const DEFAULT_LAYOUT_LOG_LINE: &str =
    "[HLAE] Kill feed: the game's own layout (`ds_hud_deathmsg layout default`)";

#[cfg(test)]
mod tests {
    use super::*;

    /// The HD-files case names the build step and the style to build for.
    #[test]
    fn the_hd_files_fix_names_the_step_and_the_style() {
        let text = no_hd("ultrasharp");
        assert!(text.contains("killfeed_icons.py --even with HD_STYLE=ultrasharp"));
        assert!(no_hd("").contains("HD_STYLE=the style in use"));
    }

    #[test]
    fn the_warning_and_the_log_line_carry_the_reason() {
        assert_eq!(
            warning(HD_OFF),
            format!("Kill feed: the game's own layout is showing, not the even one: {HD_OFF}.")
        );
        let line = waiting_log_line(&files("no sprites"), "anime", "no sprites");
        assert!(line.starts_with("⚠️ [HLAE] **Kill feed"));
        assert!(line.contains("the kill-feed atlas didn't load: no sprites."));
        assert!(line.contains("The hook says: no sprites (HD style anime)"));
        assert!(!waiting_log_line(NO_NAMES, "", "").contains("The hook says"));
    }
}
