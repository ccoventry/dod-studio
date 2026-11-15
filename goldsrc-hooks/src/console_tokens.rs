//! Putting back what GoldSrc's console tokenizer splits (#1205).
//!
//! The tokenizer makes `:` and `,` tokens of their own, so `STEAM_0:1:123`
//! arrives as five pieces and `a,b` as three (`goldsrc-misc-facts`:
//! hw+0x2b18b). A quoted argument arrives whole, so a name in quotes is never
//! touched.
//!
//! Two callers need different amounts of the fix, and the two functions are
//! kept apart because they give different answers when a `,` is a token of its
//! own: the kill feed's `block` and `replace` take SteamIDs and slots, where a
//! comma has no meaning and stays a token ([`rejoin_colons`]); the name rules
//! take free text, where a name can hold a comma, so `,` is glued too and each
//! glued token is marked ([`rejoin_colons_and_commas`]).

/// Puts `STEAM_0:1:1234567` back together. Typed unquoted it arrives as
/// `STEAM_0`, `:`, `1`, `:`, `1234567` (seen live 2026-09-30). Any token that
/// is `:`, or follows one, is glued onto the one before it. A `,` is left
/// alone.
pub(crate) fn rejoin_colons(tokens: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut glue_next = false;
    for token in tokens {
        match out.last_mut() {
            Some(last) if token == ":" => {
                last.push(':');
                glue_next = true;
                continue;
            }
            Some(last) if glue_next => last.push_str(token),
            _ => out.push(token.clone()),
        }
        glue_next = false;
    }
    out
}

/// Like [`rejoin_colons`], and a `,` is glued the same way: it is glued to the
/// token before it and the one after.
///
/// Beside each token: whether it was glued from several, so `add` can warn
/// that an unquoted name lost any spaces around its `:` or `,`.
pub(crate) fn rejoin_colons_and_commas(tokens: &[String]) -> (Vec<String>, Vec<bool>) {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut glued: Vec<bool> = Vec::with_capacity(tokens.len());
    let mut glue_next = false;
    for token in tokens {
        let separator = token == ":" || token == ",";
        match (out.last_mut(), glued.last_mut()) {
            (Some(last), Some(mark)) if separator => {
                last.push_str(token);
                *mark = true;
                glue_next = true;
                continue;
            }
            (Some(last), Some(_)) if glue_next => last.push_str(token),
            _ => {
                out.push(token.clone());
                glued.push(false);
            }
        }
        glue_next = false;
    }
    (out, glued)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_classic_steam_id_split_at_its_colons_is_put_back_together() {
        assert_eq!(
            rejoin_colons(&argv(&["!STEAM_0", ":", "1", ":", "1234567", "13"])),
            argv(&["!STEAM_0:1:1234567", "13"])
        );
        assert_eq!(
            rejoin_colons(&argv(&["!self", "!STEAM_0:1:1234567"])),
            argv(&["!self", "!STEAM_0:1:1234567"]),
            "quoted: already whole"
        );
        assert_eq!(
            rejoin_colons(&argv(&[":", "3"])),
            argv(&[":", "3"]),
            "nothing to glue onto"
        );
    }

    /// Inputs with no `,` token: both functions must agree, so the colon-only
    /// one is exactly the other's behaviour for everything a SteamID needs.
    #[test]
    fn both_agree_on_every_input_without_a_comma_token() {
        let cases: &[&[&str]] = &[
            &[],
            &[""],
            &["STEAM_0:1:6155141"],
            &["STEAM_0", ":", "1", ":", "6155141"],
            &["!STEAM_0", ":", "1", ":", "1234567", "13"],
            &["dicE[", ":", " ", ":", "]m00cat :D"],
            &["dicE[: :]m00cat :D"],
            &["a", ":"],
            &[":", "3"],
            &[":"],
            &[":", ":"],
            &["a", ":", ":", "b"],
            &["x", ":", "", "y"],
            &["quoted name", ":", "1"],
            &["id", "76561197960265729", "to", "x"],
        ];
        for case in cases {
            let tokens = argv(case);
            assert_eq!(
                rejoin_colons(&tokens),
                rejoin_colons_and_commas(&tokens).0,
                "{case:?}"
            );
        }
    }

    /// The reason there are two: a `,` token is glued by one and left by the
    /// other.
    #[test]
    fn they_differ_when_a_comma_is_a_token_of_its_own() {
        let tokens = argv(&["a", ",", "b"]);
        assert_eq!(rejoin_colons(&tokens), tokens);
        assert_eq!(
            rejoin_colons_and_commas(&tokens),
            (argv(&["a,b"]), vec![true])
        );
    }

    #[test]
    fn glued_tokens_are_marked() {
        assert_eq!(
            rejoin_colons_and_commas(&argv(&["x", "STEAM_0", ":", "1", ":", "5"])),
            (argv(&["x", "STEAM_0:1:5"]), vec![false, true])
        );
        assert_eq!(
            rejoin_colons_and_commas(&argv(&["a", "b"])),
            (argv(&["a", "b"]), vec![false, false])
        );
    }
}
