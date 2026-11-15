//! The header of the review's on-screen box (`review_keys`, #660): the lines
//! above its kill list, each made of labels and values in two colours so it
//! reads at a glance (#1437).
//!
//! ```text
//! Highlight 3 of 12 | 4 kills | answered yes
//! Player m00cat
//! Kills 2-3 (was 1-4)
//! Note: the whole note, wrapped onto as many lines as it
//!       needs, up to NOTE_LINES_MAX
//! Now 5:41 | Speed 2x
//! ```
//!
//! The answer is coloured by what it says ([`Tone::Yes`], [`Tone::Maybe`],
//! [`Tone::No`]); the scan's kill range beside a changed one (#1399) and the
//! separators are quieter. This file decides the words and their tones; the
//! box's draw picks the colours.

use crate::review::Verdict;

/// What a run of text is, which sets its colour on the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// `Highlight`, `of`, `kills`, `answered`, `Player`, `Kills`, `Note:`,
    /// `Now`, `Speed`.
    Label,
    /// The numbers, the player's name, the note, the clock.
    Value,
    /// Separators and the scan's range beside a changed one.
    Quiet,
    /// The answer, by what it says.
    Yes,
    Maybe,
    No,
}

/// One run of text in one tone.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Span {
    pub text: String,
    pub tone: Tone,
}

/// One line of the header.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Line {
    pub spans: Vec<Span>,
    /// A note line after the first: drawn starting under the note's text, not
    /// under its label.
    pub under_note: bool,
}

impl Line {
    fn of(spans: Vec<Span>) -> Line {
        Line {
            spans,
            under_note: false,
        }
    }

    /// The line's text, every run joined, for the tests.
    #[cfg(test)]
    pub(crate) fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

fn span(text: impl Into<String>, tone: Tone) -> Span {
    Span {
        text: text.into(),
        tone,
    }
}

const SEPARATOR: &str = " | ";

/// `Highlight 3 of 12 | 4 kills | answered yes`.
pub(crate) fn title_line(at: usize, total: usize, kills: usize, verdict: Option<Verdict>) -> Line {
    let mut spans = vec![
        span("Highlight ", Tone::Label),
        span(format!("{}", at + 1), Tone::Value),
        span(" of ", Tone::Label),
        span(format!("{total}"), Tone::Value),
        span(SEPARATOR, Tone::Quiet),
        span(format!("{kills}"), Tone::Value),
        span(if kills == 1 { " kill" } else { " kills" }, Tone::Label),
    ];
    if let Some(v) = verdict {
        let tone = match v {
            Verdict::Yes => Tone::Yes,
            Verdict::Maybe => Tone::Maybe,
            Verdict::No => Tone::No,
        };
        spans.push(span(SEPARATOR, Tone::Quiet));
        spans.push(span("answered ", Tone::Label));
        spans.push(span(v.word(), tone));
    }
    Line::of(spans)
}

/// `Player m00cat`: the player the highlight is of, the recorder of a POV
/// demo or the one an HLTV demo's camera follows, by the name Studio queued
/// (the in-game name). `None` when the queue names no one.
pub(crate) fn player_line(player: &str) -> Option<Line> {
    let player = player.trim();
    (!player.is_empty()).then(|| {
        Line::of(vec![
            span("Player ", Tone::Label),
            span(player, Tone::Value),
        ])
    })
}

/// The kill range line for kills `from..=to` (from 1) of a highlight with
/// `kills` kills (#1399): `Kills 2-3`, and ` (was 1-4)` beside it when the
/// range isn't the one the scan found. The scan always gives a highlight its
/// whole list of kills as its range (`native::patch::scanner::capture_streaks`),
/// and Studio counts a range as changed when it is narrower than that
/// (`take_index.js`'s `isRangeModified`), so the original is `1-<kills>`.
pub(crate) fn range_line(from: usize, to: usize, kills: usize) -> Line {
    let span_text = |a: usize, b: usize| {
        if a == b {
            format!("{a}")
        } else {
            format!("{a}-{b}")
        }
    };
    let mut spans = vec![
        span(if from == to { "Kill " } else { "Kills " }, Tone::Label),
        span(span_text(from, to), Tone::Value),
    ];
    if (from, to) != (1, kills) {
        spans.push(span(format!(" (was {})", span_text(1, kills)), Tone::Quiet));
    }
    Line::of(spans)
}

/// The word in front of the note.
pub(crate) const NOTE_LABEL: &str = "Note: ";
/// The most characters of the note on one line: about as wide as the keys'
/// line, so a long note doesn't widen the box over the view (#1399).
const NOTE_LINE_CHARS: usize = 60;
/// The most lines the note takes (#1437): a very long note is cut short on
/// the last with [`NOTE_ELLIPSIS`] rather than cover the view.
pub(crate) const NOTE_LINES_MAX: usize = 4;
/// Three dots, not `…`: the pre-Anniversary console font is ASCII.
const NOTE_ELLIPSIS: &str = "...";

/// The note's lines (#1437): the whole note, wrapped between words onto as
/// many lines as it needs, up to [`NOTE_LINES_MAX`]; a word longer than a
/// line is broken. None for no note.
pub(crate) fn note_lines(note: &str) -> Vec<Line> {
    let rows = wrap(note, NOTE_LINE_CHARS, NOTE_LINES_MAX);
    rows.into_iter()
        .enumerate()
        .map(|(i, row)| {
            if i == 0 {
                Line::of(vec![span(NOTE_LABEL, Tone::Label), span(row, Tone::Value)])
            } else {
                Line {
                    spans: vec![span(row, Tone::Value)],
                    under_note: true,
                }
            }
        })
        .collect()
}

/// `text` on one line per row, flattened (control characters as spaces),
/// each row at most `width` characters, broken between words, at most `max`
/// rows: the last one cut short with [`NOTE_ELLIPSIS`] when more is left.
fn wrap(text: &str, width: usize, max: usize) -> Vec<String> {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in flat.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let used = row.chars().count();
            let gap = usize::from(used > 0);
            if used + gap + word.len() <= width {
                if gap == 1 {
                    row.push(' ');
                }
                row.extend(&word);
                break;
            }
            if used > 0 {
                rows.push(std::mem::take(&mut row));
                continue;
            }
            // A word longer than a whole row: broken.
            let rest = word.split_off(width);
            rows.push(word.into_iter().collect());
            word = rest;
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    if rows.len() > max {
        rows.truncate(max);
        let last = &mut rows[max - 1];
        let keep: String = last.chars().take(width - NOTE_ELLIPSIS.len()).collect();
        *last = format!("{}{NOTE_ELLIPSIS}", keep.trim_end());
    }
    rows
}

/// `Now 5:41`, on the same clock as the kills' times, and `| Speed 2x` when
/// the demo plays at another speed than normal (#1437): the speed keys' or
/// the review's fast-forward over a gap (#665).
pub(crate) fn now_line(seconds: f64, speed: f32) -> Line {
    let mut spans = vec![
        span("Now ", Tone::Label),
        span(crate::review_keys::clock_text(seconds), Tone::Value),
    ];
    if !crate::review_speed::is_normal(speed) {
        spans.push(span(SEPARATOR, Tone::Quiet));
        spans.push(span("Speed ", Tone::Label));
        spans.push(span(crate::review_speed::text(speed), Tone::Value));
    }
    Line::of(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(line: &Line) -> Vec<Tone> {
        line.spans.iter().map(|s| s.tone).collect()
    }

    #[test]
    fn the_title_says_which_highlight_and_how_many_kills() {
        let line = title_line(2, 12, 4, None);
        assert_eq!(line.text(), "Highlight 3 of 12 | 4 kills");
        assert_eq!(
            tones(&line),
            vec![
                Tone::Label,
                Tone::Value,
                Tone::Label,
                Tone::Value,
                Tone::Quiet,
                Tone::Value,
                Tone::Label
            ]
        );
        assert_eq!(
            title_line(0, 1, 1, None).text(),
            "Highlight 1 of 1 | 1 kill"
        );
    }

    #[test]
    fn the_answer_is_coloured_by_what_it_says() {
        for (verdict, word, tone) in [
            (Verdict::Yes, "yes", Tone::Yes),
            (Verdict::Maybe, "maybe", Tone::Maybe),
            (Verdict::No, "no", Tone::No),
        ] {
            let line = title_line(0, 2, 3, Some(verdict));
            assert_eq!(
                line.text(),
                format!("Highlight 1 of 2 | 3 kills | answered {word}")
            );
            let last = line.spans.last().expect("the answer");
            assert_eq!((last.text.as_str(), last.tone), (word, tone));
        }
    }

    #[test]
    fn the_player_has_a_line_of_their_own() {
        let line = player_line(" m00cat ").expect("a line");
        assert_eq!(line.text(), "Player m00cat");
        assert_eq!(tones(&line), vec![Tone::Label, Tone::Value]);
        assert_eq!(player_line(""), None);
    }

    #[test]
    fn an_unchanged_range_shows_no_bracket() {
        assert_eq!(range_line(1, 4, 4).text(), "Kills 1-4");
        assert_eq!(range_line(1, 1, 1).text(), "Kill 1");
        assert_eq!(tones(&range_line(1, 4, 4)), vec![Tone::Label, Tone::Value]);
    }

    #[test]
    fn a_changed_range_shows_the_scans_beside_it_quieter() {
        let line = range_line(2, 3, 4);
        assert_eq!(line.text(), "Kills 2-3 (was 1-4)");
        assert_eq!(tones(&line), vec![Tone::Label, Tone::Value, Tone::Quiet]);
        assert_eq!(range_line(3, 3, 4).text(), "Kill 3 (was 1-4)");
        assert_eq!(range_line(1, 3, 4).text(), "Kills 1-3 (was 1-4)");
    }

    #[test]
    fn a_short_note_is_one_line_and_none_is_no_line() {
        let lines = note_lines("  nice flick ");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "Note: nice flick");
        assert_eq!(tones(&lines[0]), vec![Tone::Label, Tone::Value]);
        assert!(note_lines("").is_empty());
        assert!(note_lines(" \t ").is_empty());
        assert_eq!(note_lines("two\nlines")[0].text(), "Note: two lines");
    }

    #[test]
    fn a_long_note_wraps_between_words() {
        let note = "word ".repeat(30);
        let rows = wrap(&note, NOTE_LINE_CHARS, NOTE_LINES_MAX);
        // 30 four-letter words: 12 to a 60-character row, so 12, 12 and 6.
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.chars().count() <= NOTE_LINE_CHARS));
        assert_eq!(rows.join(" "), note.trim());
        let lines = note_lines(&note);
        assert!(!lines[0].under_note);
        assert!(lines[1..].iter().all(|l| l.under_note));
        assert!(lines[1..].iter().all(|l| tones(l) == vec![Tone::Value]));
    }

    #[test]
    fn a_very_long_note_stops_at_the_last_line_with_dots() {
        let note = "word ".repeat(100);
        let rows = wrap(&note, NOTE_LINE_CHARS, NOTE_LINES_MAX);
        assert_eq!(rows.len(), NOTE_LINES_MAX);
        let last = rows.last().expect("a row");
        assert!(last.ends_with("..."));
        assert!(last.chars().count() <= NOTE_LINE_CHARS);
        // Exactly the most that fits: whole, no dots.
        let fits = "word ".repeat(12 * NOTE_LINES_MAX);
        let rows = wrap(&fits, NOTE_LINE_CHARS, NOTE_LINES_MAX);
        assert!(!rows.last().expect("a row").ends_with("..."));
    }

    #[test]
    fn a_word_longer_than_a_line_is_broken_on_a_character() {
        let rows = wrap(
            &"é".repeat(NOTE_LINE_CHARS + 5),
            NOTE_LINE_CHARS,
            NOTE_LINES_MAX,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].chars().count(), NOTE_LINE_CHARS);
        assert_eq!(rows[1], "é".repeat(5));
    }

    #[test]
    fn the_clock_line_says_the_speed_when_it_isnt_normal() {
        assert_eq!(now_line(341.6, 1.0).text(), "Now 5:41");
        assert_eq!(now_line(341.6, 2.0).text(), "Now 5:41 | Speed 2x");
        assert_eq!(now_line(5.0, 0.25).text(), "Now 0:05 | Speed 0.25x");
        assert_eq!(tones(&now_line(5.0, 1.0)), vec![Tone::Label, Tone::Value]);
    }
}
