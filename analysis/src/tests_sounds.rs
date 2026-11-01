//! The capture-queue scan's fast parse ([`Sounds::Skip`], #37) gives the
//! same [`Analysis`] as the Demo Analyzer's full one. It has to: the scan
//! saves its analysis to the analyzer cache, which the Demo Analyzer and the
//! in-game Highlights tab then read as if they had made it themselves.

use crate::*;

/// Analyses `bytes` both ways and asserts the two analyses are the same,
/// field for field, as the cache would save them.
pub(crate) fn assert_skipping_sounds_changes_nothing(name: &str, bytes: &[u8]) {
    let full = Analysis::try_from_bytes_decoding(bytes, Sounds::Decode, |_, _| {});
    let fast = Analysis::try_from_bytes_decoding(bytes, Sounds::Skip, |_, _| {});
    match (full, fast) {
        (Ok(full), Ok(fast)) => {
            let full = serde_json::to_value(&full).unwrap();
            let fast = serde_json::to_value(&fast).unwrap();
            assert!(full == fast, "{name}: skipping sounds changed the analysis");
        }
        (Err(full), Err(fast)) => assert_eq!(full, fast, "{name}"),
        (full, fast) => panic!(
            "{name}: the full parse {} but the fast one {}",
            outcome(&full),
            outcome(&fast)
        ),
    }
}

fn outcome(result: &Result<Analysis, String>) -> String {
    match result {
        Ok(_) => "succeeded".to_string(),
        Err(e) => format!("failed ({e})"),
    }
}

#[test]
fn the_fixture_analyses_the_same_without_its_sounds() {
    assert_skipping_sounds_changes_nothing("ci_fixture.dem", &crate::tests_fixture::fixture());
}

/// A demo cut short fails, or reads up to its break, at the same place
/// either way: the sounds are still read through.
#[test]
fn a_damaged_fixture_breaks_at_the_same_place_without_its_sounds() {
    let bytes = crate::tests_fixture::fixture();
    for cut in [bytes.len() / 3, bytes.len() / 2, bytes.len() - 1000] {
        assert_skipping_sounds_changes_nothing(
            &format!("ci_fixture.dem cut at {cut}"),
            &bytes[..cut],
        );
    }
}

/// [`assert_skipping_sounds_changes_nothing`] on more demos:
/// `DOD_ANALYSIS_DEMOS` (a folder, or one file), else a spread from the PRE
/// install's `dod` folder. Ignored because CI has no demos:
/// `cargo test -p analysis --release -- --ignored skipping_sounds`.
#[test]
#[ignore]
fn skipping_sounds_changes_no_demos_analysis() {
    let paths = crate::tests::comparison_demos();
    assert!(
        !paths.is_empty(),
        "no demos found: set DOD_ANALYSIS_DEMOS to a folder or a .dem file"
    );
    for path in &paths {
        println!(
            "Comparing the full and the fast parse of {}",
            path.display()
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_skipping_sounds_changes_nothing(&path.display().to_string(), &bytes);
    }
}
