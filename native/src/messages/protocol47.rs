//! Text for converting protocol-47 demos to protocol 48 (#239): the
//! conversion's errors (`patch::protocol47`), the patcher's activity-log
//! lines (`patch::engine::StreamPatcher`), and `dod-studio-cli`'s lines for
//! `convert-47` and `patch-streak`.

use std::fmt::Display;

pub const NOT_A_DEMO: &str = "not a demo: the file has no HLDEMO header";

pub fn not_protocol_47(network_protocol: i32) -> String {
    if network_protocol == dem::protocol::NETWORK_PROTOCOL_48 {
        "already a protocol 48 demo: nothing to convert".to_string()
    } else {
        format!("a protocol {network_protocol} demo: only protocol 47 demos are converted")
    }
}

pub const NO_SERVER_INFO: &str = "no server info saying protocol 47 was found in the demo's \
     frames, so a copy would say 48 in its header and 47 where it loads the map: nothing was written";

pub fn output_is_input(path: impl Display) -> String {
    format!("{path} is the demo being converted: name another file for the copy")
}

pub fn cannot_read(path: impl Display, err: impl Display) -> String {
    format!("could not read {path}: {err}")
}

pub fn cannot_write(path: impl Display, err: impl Display) -> String {
    format!("could not write {path}: {err}")
}

/// Every part-file name beside the copy was taken.
pub const NO_FREE_PART_NAME: &str = "every temporary name beside the copy is taken by another file";

/// How many map loads a conversion rewrote: one server info each, and a
/// restart loads the same map again.
pub fn map_loads(server_infos: usize) -> String {
    if server_infos == 1 {
        "1 map load".to_string()
    } else {
        format!("{server_infos} map loads")
    }
}

/// The frames stop fitting before the directory (a damaged or cut copy):
/// the conversion searched the bytes for server infos all the same.
pub fn walk_stopped(input: impl Display, at: usize) -> String {
    format!(
        "{input}: the frames stop fitting at byte {at} (a damaged or cut copy); \
         every server info was still found and converted, but the game may not \
         play past that point"
    )
}

// ── patch/engine.rs (StreamPatcher) ────────────────────────────────────────

/// A patched copy of a protocol-47 demo was written as protocol 48.
pub fn patched_copy_converted(input: impl Display, server_infos: usize) -> String {
    format!(
        "[convert-47] {input} is a protocol 47 demo: its patched copy says protocol 48 ({})",
        map_loads(server_infos)
    )
}

/// A patched copy of a protocol-47 demo with no server info found stays 47.
pub fn patched_copy_left_at_47(input: impl Display) -> String {
    format!(
        "[convert-47] {input} is a protocol 47 demo with no server info found: its patched copy \
         stays protocol 47, which the game plays with an outdated-protocol warning"
    )
}

// ── bin/cli.rs (dod-studio-cli patch-streak) ───────────────────────────────

/// patch-streak's copy of a protocol-47 demo was written as protocol 48.
pub fn cli_patch_converted(input: impl Display, server_infos: usize) -> String {
    format!(
        "{input} is a protocol 47 demo: the patched copy says protocol 48 ({})",
        map_loads(server_infos)
    )
}

/// patch-streak's copy of a protocol-47 demo stays 47: `reason` says why.
pub fn cli_patch_left_at_47(input: impl Display, reason: impl Display) -> String {
    format!("{input} is a protocol 47 demo, left at protocol 47 in the patched copy: {reason}")
}

// ── bin/cli.rs (dod-studio-cli convert-47) ─────────────────────────────────

pub fn cli_converted(
    input: impl Display,
    output: impl Display,
    server_infos: usize,
    took: impl Display,
) -> String {
    format!(
        "{input}: converted to protocol 48 ({}) in {took}: {output}",
        map_loads(server_infos)
    )
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
    fn the_cli_line_names_both_files_and_counts_map_loads() {
        assert_eq!(
            cli_converted("a.dem", "a_p48.dem", 1, "0.2s"),
            "a.dem: converted to protocol 48 (1 map load) in 0.2s: a_p48.dem"
        );
        assert!(cli_converted("a.dem", "b.dem", 2, "1s").contains("(2 map loads)"));
    }

    #[test]
    fn a_stopped_walk_says_where() {
        assert!(walk_stopped("a.dem", 1234).contains("byte 1234"));
    }

    #[test]
    fn a_48_demo_is_said_to_need_nothing() {
        assert!(not_protocol_47(48).contains("already"));
        assert!(not_protocol_47(46).contains("protocol 46"));
    }
}
