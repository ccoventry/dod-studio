//! The review mode's two messages (issue #623): the queue Studio hands the
//! game, and the lines the game sends back on its events pipe.
//!
//! The game side is `goldsrc-hooks/src/review.rs` (its answers, and the kill
//! range and note its form saves, #1139). Neither crate depends on
//! the other, so each pins the same strings in its tests.

use serde::{Deserialize, Serialize};

/// The queue file's first line. Must match `review::QUEUE_HEADER`.
pub const QUEUE_HEADER: &str = "dodstudio-review 1";

/// What the events pipe's lines carry in front of every marker.
const TAG: &str = "[dod-studio]";

/// One highlight to review.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReviewHighlight {
    /// The demo's full path.
    pub demo: String,
    /// The frontend's key for the row, handed back with the answer.
    pub key: String,
    #[serde(default)]
    pub player: String,
    /// Each kill's time on the demo player's (VCR bar) clock.
    pub kill_times: Vec<f64>,
    /// The kill range, from 1.
    pub from: usize,
    pub to: usize,
    /// `yes`, `maybe` or `no` when answered before.
    #[serde(default)]
    pub answered: Option<String>,
    #[serde(default)]
    pub note: String,
    /// HLTV: the player number the game's camera goes on (#822); `None` for
    /// a POV demo.
    #[serde(default)]
    pub spectate: Option<u32>,
    /// Each kill's victim and weapon, in `kill_times`' order, for the list
    /// the game shows while the highlight plays (#1140). Empty, or not one
    /// per kill, sends none.
    #[serde(default)]
    pub kills: Vec<ReviewKill>,
}

/// One kill of a highlight, as Highlight Details lists it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ReviewKill {
    #[serde(default)]
    pub victim: String,
    #[serde(default)]
    pub weapon: String,
}

/// Between two kills in the queue's kill-details field, and between a
/// kill's victim and weapon. Control characters, so no name can hold one:
/// [`field`] turns every control character in a name into a space. Must
/// match `review::KILL_SEP` and `review::PART_SEP`.
pub const KILL_SEP: char = '\u{1f}';
pub const PART_SEP: char = '\u{1e}';

/// The tenth field: each kill's victim and weapon, or `None` when there is
/// not one per kill.
fn kill_details(h: &ReviewHighlight) -> Option<String> {
    if h.kills.is_empty() || h.kills.len() != h.kill_times.len() {
        return None;
    }
    let kills: Vec<String> = h
        .kills
        .iter()
        .map(|k| format!("{}{PART_SEP}{}", field(&k.victim), field(&k.weapon)))
        .collect();
    Some(kills.join(&KILL_SEP.to_string()))
}

/// A field as one tab-free line.
fn field(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// How fast a gap between kills plays when it is fast-forwarded (#665).
pub const FAST_FORWARD_SPEED: f32 = 4.0;

/// The queue file's text. `fast_forward_gap`: gaps between kills longer than
/// this many seconds play at [`FAST_FORWARD_SPEED`]; `None` plays them all at
/// normal speed. It goes on the header line as `gap=` and `speed=`, which an
/// older game ignores. `start_from`: the review starts at the first
/// highlight whose first kill is at or after this time on the demo player's
/// clock, rather than the first (#1142's From here); `from=` on the header,
/// which an older game ignores too.
pub fn format_queue(
    highlights: &[ReviewHighlight],
    fast_forward_gap: Option<f64>,
    start_from: Option<f64>,
) -> String {
    let mut out = String::from(QUEUE_HEADER);
    if let Some(gap) = fast_forward_gap.filter(|g| g.is_finite() && *g > 0.0) {
        out.push_str(&format!("\tgap={gap}\tspeed={FAST_FORWARD_SPEED}"));
    }
    if let Some(from) = start_from.filter(|f| f.is_finite() && *f >= 0.0) {
        out.push_str(&format!("\tfrom={from:.3}"));
    }
    out.push('\n');
    for h in highlights {
        let times: Vec<String> = h.kill_times.iter().map(|t| format!("{t:.3}")).collect();
        let answered = match h.answered.as_deref() {
            Some("yes") => "yes",
            Some("maybe") => "maybe",
            Some("no") => "no",
            _ => "-",
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            field(&h.demo),
            field(&h.key),
            field(&h.player),
            times.join(","),
            h.from,
            h.to,
            answered,
            field(&h.note)
        ));
        // A ninth field for an HLTV highlight (`-` for a POV one when a tenth
        // follows), and a tenth with the kills' details (#1140); an older
        // game ignores both.
        let details = kill_details(h);
        match (h.spectate, &details) {
            (Some(n), _) => out.push_str(&format!("\t{n}")),
            (None, Some(_)) => out.push_str("\t-"),
            (None, None) => {}
        }
        if let Some(details) = details {
            out.push('\t');
            out.push_str(&details);
        }
        out.push('\n');
    }
    out
}

/// One line the game sent about the review.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewEvent {
    Started {
        count: usize,
    },
    Answer {
        demo: String,
        key: String,
        verdict: String,
        from: usize,
        to: usize,
        note: String,
    },
    /// A kill range and note saved in the game's form without an answer
    /// (#1139): the row takes both and keeps its Review mark.
    Edit {
        demo: String,
        key: String,
        from: usize,
        to: usize,
        note: String,
    },
    /// `done`, `stopped`, or `failed: <why>`.
    Ended {
        reason: String,
    },
    /// The game refused a line Studio sent while the review runs (#880):
    /// Launch Preview's `viewdemo`, a seek. `command` is the first locked
    /// command in it (`review_lock::refused_label`).
    Refused {
        command: String,
    },
}

/// The review event an events-pipe line carries, if it carries one.
pub fn parse_event(line: &str) -> Option<ReviewEvent> {
    let (_, rest) = line.trim_end_matches(['\r', '\n']).split_once(TAG)?;
    let rest = rest.trim_start();
    let mut fields = rest.split('\t');
    match fields.next()? {
        "REVIEW_START" => Some(ReviewEvent::Started {
            count: fields.next()?.trim().parse().ok()?,
        }),
        "REVIEW" => {
            let demo = fields.next()?.to_string();
            let key = fields.next()?.to_string();
            let verdict = fields.next()?.to_string();
            if !matches!(verdict.as_str(), "yes" | "maybe" | "no") {
                return None;
            }
            let from = fields.next()?.trim().parse().ok()?;
            let to = fields.next()?.trim().parse().ok()?;
            // The game trims a line's end, so an empty note leaves no field.
            let note = fields.next().unwrap_or("").trim().to_string();
            Some(ReviewEvent::Answer {
                demo,
                key,
                verdict,
                from,
                to,
                note,
            })
        }
        "REVIEW_EDIT" => {
            let demo = fields.next()?.to_string();
            let key = fields.next()?.to_string();
            let from = fields.next()?.trim().parse().ok()?;
            let to = fields.next()?.trim().parse().ok()?;
            // As an answer's: an empty note leaves no field.
            let note = fields.next().unwrap_or("").trim().to_string();
            Some(ReviewEvent::Edit {
                demo,
                key,
                from,
                to,
                note,
            })
        }
        "REVIEW_END" => Some(ReviewEvent::Ended {
            reason: fields.next().unwrap_or("done").trim().to_string(),
        }),
        "REVIEW_REFUSED" => Some(ReviewEvent::Refused {
            command: fields.next()?.trim().to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight() -> ReviewHighlight {
        ReviewHighlight {
            demo: r"C:\demos\a b.dem".to_string(),
            key: "3".to_string(),
            player: "m00cat".to_string(),
            kill_times: vec![100.5, 104.0],
            from: 1,
            to: 2,
            answered: None,
            note: "nice\tflick\n".to_string(),
            spectate: None,
            kills: Vec::new(),
        }
    }

    fn kill(victim: &str, weapon: &str) -> ReviewKill {
        ReviewKill {
            victim: victim.to_string(),
            weapon: weapon.to_string(),
        }
    }

    #[test]
    fn a_highlights_kills_go_in_a_tenth_field_after_a_dash_for_a_pov_demo() {
        let mut h = highlight();
        h.note = String::new();
        h.kills = vec![kill("gskiLL", "K98"), kill("{$T}\tBrain", "Colt")];
        let line = format_queue(&[h.clone()], None, None)
            .lines()
            .nth(1)
            .unwrap()
            .to_string();
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 10);
        assert_eq!(fields[8], "-");
        // A tab in a name became a space, so the line still splits right.
        assert_eq!(
            fields[9],
            format!("gskiLL{PART_SEP}K98{KILL_SEP}{{$T}} Brain{PART_SEP}Colt")
        );
        h.spectate = Some(4);
        let line = format_queue(&[h], None, None)
            .lines()
            .nth(1)
            .unwrap()
            .to_string();
        assert_eq!(line.split('\t').nth(8), Some("4"));
    }

    #[test]
    fn from_here_goes_on_the_header_line() {
        let text = format_queue(&[highlight()], Some(8.0), Some(356.25));
        assert_eq!(
            text.lines().next(),
            Some("dodstudio-review 1\tgap=8\tspeed=4\tfrom=356.250")
        );
        let text = format_queue(&[highlight()], None, Some(f64::NAN));
        assert_eq!(text.lines().next(), Some("dodstudio-review 1"));
    }

    #[test]
    fn kills_that_are_not_one_per_kill_time_are_left_out() {
        let mut h = highlight();
        h.kills = vec![kill("a", "K98")];
        let line = format_queue(&[h], None, None)
            .lines()
            .nth(1)
            .unwrap()
            .to_string();
        assert_eq!(line.split('\t').count(), 8);
    }

    #[test]
    fn an_hltv_highlight_adds_the_camera_player_as_a_ninth_field() {
        let mut h = highlight();
        h.spectate = Some(7);
        h.note = String::new();
        assert_eq!(
            format_queue(&[h], None, None).lines().nth(1),
            Some("C:\\demos\\a b.dem\t3\tm00cat\t100.500,104.000\t1\t2\t-\t\t7")
        );
    }

    #[test]
    fn the_queue_is_the_header_then_a_line_per_highlight() {
        let mut second = highlight();
        second.answered = Some("yes".to_string());
        second.note = String::new();
        assert_eq!(
            format_queue(&[highlight(), second], None, None),
            "dodstudio-review 1\n\
             C:\\demos\\a b.dem\t3\tm00cat\t100.500,104.000\t1\t2\t-\tnice flick\n\
             C:\\demos\\a b.dem\t3\tm00cat\t100.500,104.000\t1\t2\tyes\t\n"
        );
    }

    #[test]
    fn fast_forward_goes_on_the_header_line() {
        let text = format_queue(&[highlight()], Some(8.0), None);
        assert_eq!(
            text.lines().next(),
            Some("dodstudio-review 1\tgap=8\tspeed=4")
        );
        // Off, zero or nonsense leave the header bare.
        for gap in [None, Some(0.0), Some(f64::NAN)] {
            assert_eq!(
                format_queue(&[highlight()], gap, None).lines().next(),
                Some(QUEUE_HEADER)
            );
        }
    }

    #[test]
    fn an_answer_line_is_parsed() {
        assert_eq!(
            parse_event("[dod-studio] REVIEW\tC:/d/a.dem\t4\tyes\t2\t3\ttwo lines here"),
            Some(ReviewEvent::Answer {
                demo: "C:/d/a.dem".to_string(),
                key: "4".to_string(),
                verdict: "yes".to_string(),
                from: 2,
                to: 3,
                note: "two lines here".to_string(),
            })
        );
    }

    #[test]
    fn an_answer_without_a_note_is_still_an_answer() {
        let Some(ReviewEvent::Answer { note, verdict, .. }) =
            parse_event("[dod-studio] REVIEW\tC:/d/a.dem\t4\tno\t1\t1")
        else {
            panic!("not parsed");
        };
        assert_eq!(note, "");
        assert_eq!(verdict, "no");
    }

    #[test]
    fn maybe_goes_both_ways() {
        let mut h = highlight();
        h.answered = Some("maybe".to_string());
        assert!(format_queue(&[h], None, None).contains("\tmaybe\t"));
        let Some(ReviewEvent::Answer { verdict, .. }) =
            parse_event("[dod-studio] REVIEW\tC:/d/a.dem\t4\tmaybe\t1\t1")
        else {
            panic!("not parsed");
        };
        assert_eq!(verdict, "maybe");
    }

    /// The game's `review::edit_label` line (#1139), and the JSON the
    /// frontend's review_mode.js reads (`kind`, `from`, `to`, `note`).
    #[test]
    fn a_form_save_is_parsed_as_an_edit() {
        let event =
            parse_event("[dod-studio] REVIEW_EDIT\tC:/d/a.dem\t4\t2\t3\tnice flick").unwrap();
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({
                "kind": "edit", "demo": "C:/d/a.dem", "key": "4",
                "from": 2, "to": 3, "note": "nice flick"
            })
        );
        let Some(ReviewEvent::Edit { note, .. }) =
            parse_event("[dod-studio] REVIEW_EDIT\tC:/d/a.dem\t4\t1\t1")
        else {
            panic!("not parsed");
        };
        assert_eq!(note, "");
        assert_eq!(parse_event("[dod-studio] REVIEW_EDIT\ta\tb\tx\t1"), None);
    }

    /// The game's `review_lock::refused_label` line, and the JSON the
    /// frontend's review_mode.js reads (`kind`, `command`).
    #[test]
    fn a_refused_line_is_parsed() {
        let event = parse_event("[dod-studio] REVIEW_REFUSED\tviewdemo").unwrap();
        assert_eq!(
            event,
            ReviewEvent::Refused {
                command: "viewdemo".to_string()
            }
        );
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({ "kind": "refused", "command": "viewdemo" })
        );
        assert_eq!(parse_event("[dod-studio] REVIEW_REFUSED"), None);
    }

    #[test]
    fn start_and_end_are_parsed_and_other_markers_ignored() {
        assert_eq!(
            parse_event("[dod-studio] REVIEW_START\t12"),
            Some(ReviewEvent::Started { count: 12 })
        );
        assert_eq!(
            parse_event("[dod-studio] REVIEW_END\tstopped"),
            Some(ReviewEvent::Ended {
                reason: "stopped".to_string()
            })
        );
        assert_eq!(parse_event("[dod-studio] DEMO_START 1 2 3"), None);
        assert_eq!(parse_event("dodstudio-hooks events 1"), None);
        assert_eq!(
            parse_event("[dod-studio] REVIEW\ta\tb\tperhaps\t1\t1"),
            None
        );
    }
}
