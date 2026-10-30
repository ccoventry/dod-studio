//! A line ceiling per hot Rust file (#1179), the Rust twin of
//! `studio/src/main_js_budget.test.js` and `file_ceilings.test.js`.
//!
//! These are prompts to look, not limits to design around: a file that crosses
//! its ceiling is asking whether it still has one job (`docs/coding_standards.md`
//! section 1). Raise a number here only with the reason in the PR, or better,
//! move the code that belongs to another area into a module of its own.
//!
//! Lines count non-test code, the rule the standard gives for the 1,000-line
//! prompt: everything before the file's first top-level `#[cfg(test)]`, and
//! nothing for `tests.rs` / `*_tests.rs` files. Each ceiling is the size on the
//! day it was set plus about 3%.

mod source_tree;

use source_tree::{non_test_lines, read, relative, workspace_root};

const CEILINGS: &[(&str, usize)] = &[
    // The Tauri managers: each wraps `native` for the UI; decisions belong in
    // `native` (docs/coding_standards.md, section 1).
    ("studio/src-tauri/src/capture_manager.rs", 2250),
    ("studio/src-tauri/src/render_manager.rs", 2100),
    ("studio/src-tauri/src/scan_manager.rs", 1300),
    ("studio/src-tauri/src/audit_manager.rs", 850),
    ("studio/src-tauri/src/settings_manager.rs", 800),
    // The hook DLL's biggest areas.
    ("goldsrc-hooks/src/studio_panel.rs", 3500),
    ("goldsrc-hooks/src/texture_hires.rs", 4050),
    ("goldsrc-hooks/src/deathmsg.rs", 1750),
    ("goldsrc-hooks/src/demo_list_folders.rs", 1600),
    ("goldsrc-hooks/src/review.rs", 1500),
    ("goldsrc-hooks/src/demo_seek.rs", 1350),
    // `native`.
    ("native/src/patch/cfg_scan.rs", 1300),
];

#[test]
fn hot_files_stay_under_their_ceiling() {
    let root = workspace_root();
    let mut over = Vec::new();
    for (rel, ceiling) in CEILINGS {
        let path = root.join(rel);
        assert!(
            path.is_file(),
            "{rel} is listed in file_ceilings.rs but does not exist: update the list (renamed or split?)"
        );
        let lines = non_test_lines(&path, &read(&path));
        if lines > *ceiling {
            over.push(format!(
                "{}: {lines} non-test lines, ceiling {ceiling}",
                relative(&path)
            ));
        }
    }
    assert!(
        over.is_empty(),
        "over the ceiling (a prompt to check the file still has one job; move code that belongs to \
         another area out, or raise the number with a reason in the PR):\n{}",
        over.join("\n")
    );
}
