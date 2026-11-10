//! What the hook DLL says the kill feed's layout shows (#1339), read off the
//! events pipe: when `ds_hud_deathmsg layout even` can't draw, the game's own
//! layout shows, and every clip the batch records has it.
//!
//! The hook sends `[dod-studio] KILLFEED_LAYOUT`, then tab-separated: the
//! state (`default`, `even`, `waiting`) and, for `waiting`, `reason=`,
//! `style=` and `why=` fields (`goldsrc-hooks`' `deathmsg/layout_report.rs`
//! documents them). It sends one per change, and again at each batch's
//! start. Unknown fields are skipped, so either side can add one; a hook DLL
//! from before #1339 never sends the line.

use crate::messages::killfeed_layout as msg;

/// What the line's label starts with.
const LABEL: &str = "KILLFEED_LAYOUT";

/// The layout the hook says shows.
#[derive(Debug, PartialEq)]
pub(super) enum KillfeedLayout {
    /// `layout default`: the game's own, as asked.
    Default,
    /// `layout even`, drawing.
    Even,
    /// `layout even` asked for, the game's own showing.
    Waiting {
        reason: String,
        style: String,
        why: String,
    },
}

/// The report in `label` (the text after the tag), if it is one.
pub(super) fn parse(label: &str) -> Option<KillfeedLayout> {
    let mut fields = label.split('\t');
    if fields.next()?.trim() != LABEL {
        return None;
    }
    match fields.next()?.trim() {
        "default" => Some(KillfeedLayout::Default),
        "even" => Some(KillfeedLayout::Even),
        "waiting" => {
            let (mut reason, mut style, mut why) = (String::new(), String::new(), String::new());
            for field in fields {
                let Some((key, value)) = field.split_once('=') else {
                    continue;
                };
                let value = value.trim().to_string();
                match key.trim() {
                    "reason" => reason = value,
                    "style" => style = value,
                    "why" => why = value,
                    _ => {}
                }
            }
            Some(KillfeedLayout::Waiting { reason, style, why })
        }
        _ => None,
    }
}

impl KillfeedLayout {
    /// What is wrong and the step that fixes it, for a `waiting` report.
    fn reason_text(reason: &str, style: &str, why: &str) -> String {
        match reason {
            "files" => msg::files(why),
            "hd_off" => msg::HD_OFF.to_string(),
            "no_hd" => msg::no_hd(style),
            "no_names" => msg::NO_NAMES.to_string(),
            "no_hooks" => msg::NO_HOOKS.to_string(),
            other => msg::unknown(other),
        }
    }

    /// The warning a batch shows while the game's layout shows, or `None`
    /// when there is nothing to warn about (and a shown one goes).
    pub(super) fn warning(&self) -> Option<String> {
        match self {
            KillfeedLayout::Waiting { reason, style, why } => {
                Some(msg::warning(&Self::reason_text(reason, style, why)))
            }
            _ => None,
        }
    }

    /// The activity log's line.
    pub(super) fn log_line(&self) -> String {
        match self {
            KillfeedLayout::Default => msg::DEFAULT_LAYOUT_LOG_LINE.to_string(),
            KillfeedLayout::Even => msg::EVEN_DRAWS_LOG_LINE.to_string(),
            KillfeedLayout::Waiting { reason, style, why } => {
                // The hook's words are the reason itself for `files`.
                let words = if reason == "files" { "" } else { why };
                msg::waiting_log_line(&Self::reason_text(reason, style, why), style, words)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_HD: &str = "KILLFEED_LAYOUT\twaiting\treason=no_hd\tstyle=ultrasharp\twhy=the atlas \
                         went up without its HD files: none for this ds_hd_style in \
                         dodstudio_hd\\sprites (run killfeed_icons.py --even)";

    #[test]
    fn a_waiting_report_reads_back_and_names_the_fix() {
        let layout = parse(NO_HD).expect("a report");
        let KillfeedLayout::Waiting { reason, style, .. } = &layout else {
            panic!("{layout:?}");
        };
        assert_eq!((reason.as_str(), style.as_str()), ("no_hd", "ultrasharp"));
        let warning = layout.warning().expect("a warning");
        assert!(warning.starts_with("Kill feed: the game's own layout is showing"));
        assert!(warning.contains("killfeed_icons.py --even with HD_STYLE=ultrasharp"));
        let line = layout.log_line();
        assert!(line.starts_with("⚠️ [HLAE] **Kill feed"));
        assert!(line.contains("The hook says: the atlas went up without its HD files"));
    }

    /// Every reason the hook sends gets its own fix, and one it doesn't know
    /// still warns.
    #[test]
    fn each_reason_warns_with_its_own_step() {
        let warn = |reason: &str| {
            parse(&format!(
                "KILLFEED_LAYOUT\twaiting\treason={reason}\tstyle=anime\twhy=no sprites\\dodstudio_killfeed.spr"
            ))
            .and_then(|l| l.warning())
            .unwrap()
        };
        assert!(warn("files").contains("didn't load: no sprites\\dodstudio_killfeed.spr"));
        assert!(warn("hd_off").contains("ds_hd_enabled 0"));
        assert!(warn("no_hd").contains("HD_STYLE=anime"));
        assert!(warn("no_names").contains("hud.txt"));
        assert!(warn("no_hooks").contains("icon draw"));
        assert!(warn("something_new").contains("(something_new)"));
        // The `files` reason is the hook's words already: not said twice.
        let files = parse("KILLFEED_LAYOUT\twaiting\treason=files\twhy=gone").unwrap();
        assert!(!files.log_line().contains("The hook says"));
    }

    #[test]
    fn the_layout_drawing_or_default_clears_the_warning() {
        let even = parse("KILLFEED_LAYOUT\teven").unwrap();
        assert_eq!(even, KillfeedLayout::Even);
        assert_eq!(even.warning(), None);
        assert!(even.log_line().contains("the even layout draws"));
        let default = parse("KILLFEED_LAYOUT\tdefault\tnew=1").unwrap();
        assert_eq!(default.warning(), None);
    }

    #[test]
    fn other_markers_are_not_reports() {
        assert!(parse("CLIP_SUMMARY\tclip=1").is_none());
        assert!(parse("KILLFEED_LAYOUTX\teven").is_none());
        assert!(parse("KILLFEED_LAYOUT").is_none());
        assert!(parse("KILLFEED_LAYOUT\tsideways").is_none());
    }
}
