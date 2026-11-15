//! The colour arguments of `ds_hud_deathmsg` (`outline`, `background`,
//! `background allies|axis`): three numbers `r g b`, each 0..255, or one hex
//! word, `#rrggbb` or `#rgb` as in CSS, or six hex digits without the `#`.
//!
//! The `#` reaches the command as it was typed, from the console and from a
//! cfg line alike: the engine's tokenizer splits only at whitespace, quotes,
//! `:` and `{ } ( ) ' ,`, and its cfg comments start with `//`. Three hex
//! digits without the `#` would read as a number (`255` is a channel), so
//! `#rgb` keeps its `#`; six digits never are one (a channel is at most three).

/// An `r g b` colour, each 0..=255.
pub(super) type Rgb = (i32, i32, i32);

/// What a bad colour says, whichever way it was typed.
const EXPECTED: &str = "a colour is r g b (each 0..255) or #rrggbb";

/// Whether `word` is a colour on its own: it starts with `#`, or it is six
/// hex digits. A `#` word that is not a good colour still counts, so it gets
/// the hex error rather than a number's.
pub(super) fn is_hex_word(word: &str) -> bool {
    word.starts_with('#') || (word.len() == 6 && word.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// One hex colour: `#rrggbb`, `#rgb` (each digit doubled, `#0af` is
/// `#00aaff`) or `rrggbb`, any case.
pub(super) fn parse_hex(word: &str) -> Result<Rgb, String> {
    let digits = word.strip_prefix('#').unwrap_or(word);
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "colour {word:?}: a hex colour takes only 0-9 and a-f ({EXPECTED})"
        ));
    }
    // Every byte is an ASCII hex digit, so slicing by bytes is safe.
    let channel = |s: &str| i32::from_str_radix(s, 16).unwrap_or(0);
    match (digits.len(), word.starts_with('#')) {
        (6, _) => Ok((
            channel(&digits[0..2]),
            channel(&digits[2..4]),
            channel(&digits[4..6]),
        )),
        (3, true) => {
            let doubled = |i: usize| channel(&digits[i..=i]) * 0x11;
            Ok((doubled(0), doubled(1), doubled(2)))
        }
        _ => Err(format!(
            "colour {word:?}: a hex colour is 6 digits (#0a286e) or 3 (#0af) ({EXPECTED})"
        )),
    }
}

/// One channel of an `r g b` colour.
fn parse_channel(word: &str) -> Result<i32, String> {
    match word.parse::<i32>() {
        Ok(v) if (0..=255).contains(&v) => Ok(v),
        _ => Err(format!(
            "colour: expected 0..255, got {word:?} ({EXPECTED})"
        )),
    }
}

/// Takes one colour off the front of `words`, one hex word or three numbers,
/// and returns it with the words after it (alpha, padding).
pub(super) fn take(words: &[String]) -> Result<(Rgb, &[String]), String> {
    match words {
        [hex, rest @ ..] if is_hex_word(hex) => Ok((parse_hex(hex)?, rest)),
        [r, g, b, rest @ ..] => Ok((
            (parse_channel(r)?, parse_channel(g)?, parse_channel(b)?),
            rest,
        )),
        _ => Err(EXPECTED.to_string()),
    }
}

/// Exactly one colour, nothing after it.
pub(super) fn exactly(words: &[String]) -> Result<Rgb, String> {
    match take(words)? {
        (rgb, []) => Ok(rgb),
        (_, extra) => Err(format!(
            "expected just a colour, then nothing; got {:?} after it",
            extra.join(" ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn hex_colours_in_every_form() {
        assert_eq!(parse_hex("#0a286e"), Ok((10, 40, 110)));
        assert_eq!(parse_hex("#0A286E"), Ok((10, 40, 110)), "any case");
        assert_eq!(parse_hex("0a286e"), Ok((10, 40, 110)), "no #");
        assert_eq!(parse_hex("#ffffff"), Ok((255, 255, 255)));
        assert_eq!(parse_hex("#000000"), Ok((0, 0, 0)));
        assert_eq!(parse_hex("#0af"), Ok((0, 0xaa, 0xff)), "short form");
        assert_eq!(parse_hex("#FFF"), Ok((255, 255, 255)));
    }

    #[test]
    fn hex_colours_of_the_wrong_length_are_refused() {
        for bad in ["#", "#0", "#0a", "#0a28", "#0a286", "#0a286e0", "#0a286e00"] {
            let err = parse_hex(bad).unwrap_err();
            assert!(err.contains("6 digits"), "{bad}: {err}");
        }
        // Three digits with no # is a number, not a colour.
        assert!(!is_hex_word("0af"));
        assert!(!is_hex_word("255"));
        assert!(is_hex_word("0a286e"));
        assert!(!is_hex_word("0a286"), "five digits");
    }

    #[test]
    fn hex_colours_with_bad_digits_are_refused() {
        for bad in ["#0a286g", "#zzz", "#0a 286", "#-12345", "#+0a286"] {
            let err = parse_hex(bad).unwrap_err();
            assert!(err.contains("0-9 and a-f"), "{bad}: {err}");
        }
        // Six characters that aren't all hex digits are not a hex word: they
        // read as a number, and fail as one.
        assert!(!is_hex_word("0a286g"));
        assert!(take(&argv(&["0a286g"])).is_err());
    }

    #[test]
    fn a_colour_is_taken_off_the_front_and_the_rest_is_left() {
        let words = argv(&["#0a286e", "128"]);
        assert_eq!(take(&words), Ok(((10, 40, 110), &words[1..])));
        let words = argv(&["10", "40", "110", "128"]);
        assert_eq!(take(&words), Ok(((10, 40, 110), &words[3..])));
        let words = argv(&["#fff"]);
        assert_eq!(take(&words), Ok(((255, 255, 255), &words[1..])));
        // A # word that is bad is a hex error, not "expected three numbers".
        assert!(
            take(&argv(&["#12", "128"]))
                .unwrap_err()
                .contains("6 digits")
        );
        assert!(take(&argv(&["255"])).is_err(), "half a colour");
        assert!(take(&argv(&["255", "0"])).is_err());
        assert!(take(&argv(&["0", "0", "256"])).is_err());
        assert!(take(&argv(&[])).is_err());
    }

    #[test]
    fn exactly_one_colour() {
        assert_eq!(exactly(&argv(&["#0a286e"])), Ok((10, 40, 110)));
        assert_eq!(exactly(&argv(&["0", "40", "0"])), Ok((0, 40, 0)));
        assert!(exactly(&argv(&["#0a286e", "5"])).is_err());
        assert!(exactly(&argv(&["0", "40", "0", "5"])).is_err());
        assert!(exactly(&argv(&["0", "40"])).is_err());
    }
}
