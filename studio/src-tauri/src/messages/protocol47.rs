//! Activity-log lines of the Demo Analyzer's Convert to protocol 48 (#239):
//! `demo_convert.rs`.

pub fn convert_started(path: &str) -> String {
    format!("[convert-47] started: {path}")
}

pub fn convert_done(path: &str, written: &str, server_infos: usize, took: &str) -> String {
    format!(
        "[convert-47] done in {took}: {path} -> {written}, protocol 48 in the header and {}",
        native::messages::protocol47::map_loads(server_infos)
    )
}

/// The frames stop fitting before the directory: the copy is converted, but
/// a damaged demo may still not play past that point.
pub fn convert_walk_stopped(path: &str, at: usize) -> String {
    format!(
        "[convert-47] {}",
        native::messages::protocol47::walk_stopped(path, at)
    )
}

pub fn convert_failed(path: &str, err: &str, took: &str) -> String {
    format!("[convert-47] {path}: not converted after {took}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversion_logs_both_files_and_how_long_it_took() {
        let line = convert_done("a.dem", "a_p48.dem", 2, "0.3 s");
        assert!(line.contains("a.dem -> a_p48.dem"), "{line}");
        assert!(line.contains("0.3 s"), "{line}");
        assert!(convert_failed("a.dem", "boom", "1 s").contains("boom"));
    }
}
