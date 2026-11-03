//! Reading a `<map>_light.txt`: one box or light per line.
//!
//! - `x1 y1 z1 x2 y2 z2 scale [fade]`: a box ([`super::boxes`]);
//! - `light x y z r g b strength radius`: a point light ([`super::lights`]).
//!
//! `//` starts a comment; blank lines are skipped. A line that is neither is
//! reported with its number and skipped, so one typo doesn't drop the rest.

use super::boxes::{self, LightBox};
use super::lights::{self, PointLight};

/// The word a light line starts with.
const LIGHT: &str = "light";

/// A line of the file that isn't a box or a light: its number (from 1) and
/// why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Problem {
    pub line: usize,
    pub why: String,
}

/// What a `<map>_light.txt` holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LightFile {
    pub boxes: Vec<LightBox>,
    pub lights: Vec<PointLight>,
    pub problems: Vec<Problem>,
}

impl LightFile {
    /// Whether the file changes nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.boxes.is_empty() && self.lights.is_empty()
    }
}

/// Reads a light file's text; never fails, see [`LightFile::problems`].
pub(crate) fn parse(text: &str) -> LightFile {
    let mut file = LightFile::default();
    // Notepad's "UTF-8 with BOM" puts one in front of the first line.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Err(why) = parse_line(line, &mut file) {
            file.problems.push(Problem {
                line: index + 1,
                why,
            });
        }
    }
    file
}

/// Adds one non-empty line's box or light to `file`.
fn parse_line(line: &str, file: &mut LightFile) -> Result<(), String> {
    let mut words = line.split_whitespace().peekable();
    let is_light = words.peek().is_some_and(|w| w.eq_ignore_ascii_case(LIGHT));
    if is_light {
        words.next();
    }
    let numbers = words.map(number).collect::<Result<Vec<f32>, String>>()?;
    if is_light {
        file.lights.push(lights::parse_light(&numbers)?);
    } else {
        file.boxes.push(boxes::parse_box(&numbers)?);
    }
    Ok(())
}

fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("\"{word}\" is not a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let file = parse("// the underpass\n\n0 0 0 1 1 1 2 // brighter\n   \n");
        assert_eq!(file.boxes.len(), 1);
        assert_eq!(file.problems, vec![]);
    }

    #[test]
    fn a_byte_order_mark_and_crlf_line_ends_are_read_past() {
        let file = parse("\u{feff}0 0 0 1 1 1 2\r\nlight 5 5 5 6 6 6 1 3 // two\r\n");
        assert_eq!(file.problems, vec![]);
        assert_eq!((file.boxes.len(), file.lights.len()), (1, 1));
    }

    #[test]
    fn boxes_and_lights_mix_in_one_file() {
        let file =
            parse("0 0 0 1 1 1 2\nlight 5 5 5 255 200 100 0.5 160\nLIGHT 1 1 1 10 10 10 1 32");
        assert_eq!(file.boxes.len(), 1);
        assert_eq!(file.lights.len(), 2);
        assert!(!file.is_empty());
    }

    #[test]
    fn a_bad_line_is_reported_by_number_and_the_rest_still_read() {
        let file = parse(
            "0 0 0 1 1 1 2\n0 0 0 1 1 x 2\n0 0 0 1 1\nlight 0 0 0 255 255 255 1\nlamp 0 0 0 1 1 1 1 1\n5 5 5 6 6 6 3",
        );
        assert_eq!(file.boxes.len(), 2);
        let lines: Vec<usize> = file.problems.iter().map(|p| p.line).collect();
        assert_eq!(lines, vec![2, 3, 4, 5]);
        assert!(
            file.problems[0].why.contains("\"x\""),
            "{:?}",
            file.problems[0]
        );
        assert!(
            file.problems[3].why.contains("\"lamp\""),
            "{:?}",
            file.problems[3]
        );
    }

    #[test]
    fn a_file_of_comments_changes_nothing() {
        assert!(parse("// nothing yet\n").is_empty());
    }
}
