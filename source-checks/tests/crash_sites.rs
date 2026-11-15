//! The crash sites DoD Studio knows (`native/src/crash_maps.rs`'s
//! `KNOWN_CRASHES`) are the ones `goldsrc-hooks/tools/crash_report.py` knows; a
//! site known to one and not the other is how the two drift apart. Reads both
//! files as text so it runs in the fast `Source checks` CI job.

mod source_tree;

use source_tree::{read, workspace_root};

#[test]
fn the_known_sites_match_crash_report_py() {
    let root = workspace_root();
    let script = read(&root.join("goldsrc-hooks/tools/crash_report.py"));
    let known = &script[script.find("KNOWN = {").unwrap()..];
    let known = &known[..known.find("\n}").unwrap()];
    let mut theirs: Vec<&str> = known
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"'))
        .filter_map(|l| l.split('"').next())
        .filter(|s| s.contains(".dll+0x"))
        .collect();
    theirs.sort();

    // A site is the first string of a `KNOWN_CRASHES` entry: a line holding
    // only `"module.dll+0x..."` and a comma.
    let rust = read(&root.join("native/src/crash_maps.rs"));
    let table = &rust[rust.find("pub const KNOWN_CRASHES").unwrap()..];
    let table = &table[..table.find("\n];").unwrap()];
    let mut ours: Vec<&str> = table
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\","))
        .filter(|s| s.contains(".dll+0x") && !s.contains(' '))
        .collect();
    ours.sort();
    assert!(!ours.is_empty(), "found no sites in crash_maps.rs");
    assert_eq!(ours, theirs);
}
