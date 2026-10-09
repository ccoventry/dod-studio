//! The review mode's two messages (issue #623): the queue Studio hands the
//! game, and the lines the game sends back on its events pipe.
//!
//! The game side is `goldsrc-hooks/src/review.rs`. Neither crate depends on
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
    /// `yes` or `no` when answered before.
    #[serde(default)]
    pub answered: Option<String>,
    #[serde(default)]
    pub note: String,
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
/// older game ignores.
pub fn format_queue(highlights: &[ReviewHighlight], fast_forward_gap: Option<f64>) -> String {
    let mut out = String::from(QUEUE_HEADER);
    if let Some(gap) = fast_forward_gap.filter(|g| g.is_finite() && *g > 0.0) {
        out.push_str(&format!("\tgap={gap}\tspeed={FAST_FORWARD_SPEED}"));
    }
    out.push('\n');
    for h in highlights {
        let times: Vec<String> = h.kill_times.iter().map(|t| format!("{t:.3}")).collect();
        let answered = match h.answered.as_deref() {
            Some("yes") => "yes",
            Some("no") => "no",
            _ => "-",
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            field(&h.demo),
            field(&h.key),
            field(&h.player),
            times.join(","),
            h.from,
            h.to,
            answered,
            field(&h.note)
        ));
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
    /// `done`, `stopped`, or `failed: <why>`.
    Ended {
        reason: String,
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
            if verdict != "yes" && verdict != "no" {
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
        "REVIEW_END" => Some(ReviewEvent::Ended {
            reason: fields.next().unwrap_or("done").trim().to_string(),
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
        }
    }

    #[test]
    fn the_queue_is_the_header_then_a_line_per_highlight() {
        let mut second = highlight();
        second.answered = Some("yes".to_string());
        second.note = String::new();
        assert_eq!(
            format_queue(&[highlight(), second], None),
            "dodstudio-review 1\n\
             C:\\demos\\a b.dem\t3\tm00cat\t100.500,104.000\t1\t2\t-\tnice flick\n\
             C:\\demos\\a b.dem\t3\tm00cat\t100.500,104.000\t1\t2\tyes\t\n"
        );
    }

    #[test]
    fn fast_forward_goes_on_the_header_line() {
        let text = format_queue(&[highlight()], Some(8.0));
        assert_eq!(
            text.lines().next(),
            Some("dodstudio-review 1\tgap=8\tspeed=4")
        );
        // Off, zero or nonsense leave the header bare.
        for gap in [None, Some(0.0), Some(f64::NAN)] {
            assert_eq!(
                format_queue(&[highlight()], gap).lines().next(),
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
        assert_eq!(parse_event("[dod-studio] REVIEW\ta\tb\tmaybe\t1\t1"), None);
    }
}
