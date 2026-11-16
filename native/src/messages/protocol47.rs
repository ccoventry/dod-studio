//! Text for converting protocol-47 demos to protocol 48 (#239): the
//! conversion's errors (`patch::protocol47`) and the CLI's lines.

use std::fmt::Display;

/// What a protocol-47 demo needs before the game plays it.
pub const CONVERT_FIRST: &str = "protocol 47 demo: convert it first \
     (dod-studio-cli convert-47 <demo>); the game plays only protocol 48 demos";

pub const NOT_A_DEMO: &str = "not a demo: the file has no HLDEMO header";

pub fn not_protocol_47(network_protocol: i32) -> String {
    if network_protocol == 48 {
        "already a protocol 48 demo: nothing to convert".to_string()
    } else {
        format!("a protocol {network_protocol} demo: only protocol 47 demos are converted")
    }
}

pub const NO_SERVER_INFO: &str = "no server info saying protocol 47 was found in the demo's \
     frames, so the game would still refuse it: nothing was written";

pub fn output_is_input(path: impl Display) -> String {
    format!("{path} is the demo being converted: name another file for the copy")
}

pub fn cannot_read(path: impl Display, err: impl Display) -> String {
    format!("could not read {path}: {err}")
}

pub fn cannot_write(path: impl Display, err: impl Display) -> String {
    format!("could not write {path}: {err}")
}

// ── bin/cli.rs (dod-studio-cli convert-47) ─────────────────────────────────

pub fn cli_converted(
    input: impl Display,
    output: impl Display,
    server_infos: usize,
    took: impl Display,
) -> String {
    let maps = if server_infos == 1 {
        "1 map".to_string()
    } else {
        format!("{server_infos} maps")
    };
    format!("{input}: converted to protocol 48 ({maps}) in {took}: {output}")
}

pub fn cli_not_converted(input: impl Display, err: impl Display) -> String {
    format!("{input}: not converted: {err}")
}

pub fn cli_summary(converted: usize, failed: usize) -> String {
    format!("{converted} converted, {failed} not converted")
}

/// `--output` names one file, so it takes one demo.
pub const CLI_OUTPUT_NEEDS_ONE_DEMO: &str = "--output names one file: give one demo with it, or leave it out to write each copy beside its demo";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_line_names_both_files_and_counts_maps() {
        assert_eq!(
            cli_converted("a.dem", "a_p48.dem", 1, "0.2s"),
            "a.dem: converted to protocol 48 (1 map) in 0.2s: a_p48.dem"
        );
        assert!(cli_converted("a.dem", "b.dem", 2, "1s").contains("(2 maps)"));
    }

    #[test]
    fn a_48_demo_is_said_to_need_nothing() {
        assert!(not_protocol_47(48).contains("already"));
        assert!(not_protocol_47(46).contains("protocol 46"));
    }
}
