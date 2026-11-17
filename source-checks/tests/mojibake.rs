//! A file saved through a tool that read its UTF-8 as Windows-1252 and wrote it
//! back as UTF-8 turns every non-ASCII character into two or three others:
//! `—` becomes `â€”`, `▸` becomes `â–¸`. Nothing fails to build, so it slips
//! through: a merge in #1376 did it to `styles.css` (What's New's ▸/▾ drew as
//! junk) and `scan_manager.rs`, and only a review caught it. This scans every
//! tracked text file for that pattern and fails with `file:line` for each.
//!
//! The pattern: a run of characters that Windows-1252 maps to bytes 0x80-0xFF
//! and that, as those bytes, spell a whole multi-byte UTF-8 character. Real
//! text almost never does that (`é` alone, or `·` after a space, is fine).
//!
//! Skipped: binary files, `local/` (gitignored), lockfiles. A deliberate hit
//! (a test of this very thing) goes in `ALLOWED` with the reason.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration test: its helpers sit outside a #[test], where clippy.toml's test allowance does not reach (#1179)"
)]

mod source_tree;

use source_tree::{read, relative, workspace_root};
use std::path::PathBuf;
use std::process::Command;

/// `(file, reason)` pairs whose hits are deliberate. Keep it short.
const ALLOWED: &[(&str, &str)] = &[];

/// Windows-1252's characters at bytes 0x80-0x9F (0 where the code page has none).
const CP1252_80_9F: [char; 32] = [
    '\u{20AC}', '\0', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\0', '\u{017D}', '\0', '\0',
    '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}',
    '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\0', '\u{017E}', '\u{0178}',
];

/// The byte Windows-1252 writes `c` as, when it is one of 0x80-0xFF. Also takes
/// the C1 controls (U+0080-U+009F), which a lenient decoder passes through for
/// the five bytes the code page leaves undefined.
fn cp1252_high_byte(c: char) -> Option<u8> {
    let u = c as u32;
    if (0xA0..=0xFF).contains(&u) || (0x80..=0x9F).contains(&u) {
        return u8::try_from(u).ok();
    }
    CP1252_80_9F
        .iter()
        .position(|&x| x == c && x != '\0')
        .and_then(|i| u8::try_from(0x80 + i).ok())
}

/// Whether `bytes` holds a whole multi-byte UTF-8 character somewhere.
fn spells_utf8(bytes: &[u8]) -> bool {
    (0..bytes.len()).any(|i| {
        let need = match bytes[i] {
            0xC2..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF4 => 3,
            _ => return false,
        };
        let tail = bytes.get(i + 1..i + 1 + need);
        tail.is_some_and(|t| t.iter().all(|b| (0x80..=0xBF).contains(b)))
            && std::str::from_utf8(&bytes[i..=i + need]).is_ok()
    })
}

/// Line numbers (1-based) of `text` that hold double-encoded UTF-8.
fn find_hits(text: &str) -> Vec<usize> {
    let mut hits = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut run = Vec::new();
        let mut found = false;
        for c in line.chars().chain(std::iter::once('\n')) {
            match cp1252_high_byte(c) {
                Some(b) => run.push(b),
                None => {
                    if run.len() >= 2 && spells_utf8(&run) {
                        found = true;
                    }
                    run.clear();
                }
            }
        }
        if found {
            hits.push(i + 1);
        }
    }
    hits
}

fn tracked_files() -> Vec<PathBuf> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(workspace_root())
        .output()
        .expect("running git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| !p.starts_with("local/"))
        .filter(|p| !p.ends_with(".lock") && !p.ends_with("package-lock.json"))
        .map(|p| workspace_root().join(p))
        .collect()
}

#[test]
fn no_double_encoded_text_in_tracked_files() {
    let mut report = Vec::new();
    for path in tracked_files() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue; // deleted in the working tree, or a submodule
        };
        if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            continue; // binary
        }
        let rel = relative(&path);
        if ALLOWED.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        for line in find_hits(&read(&path)) {
            report.push(format!("{rel}:{line}"));
        }
    }
    assert!(
        report.is_empty(),
        "double-encoded UTF-8 (a file re-saved after being read as Windows-1252; restore the lines from the \
         file's last good version, or re-encode each run of them as Windows-1252 and decode it as UTF-8):\n{}",
        report.join("\n")
    );
}

#[test]
fn catches_double_encoding_and_passes_real_text() {
    // Built by double-encoding at run time, so this file holds no hit itself.
    let double = |s: &str| -> String {
        s.bytes()
            .map(|b| match b {
                0x80..=0x9F => match CP1252_80_9F[usize::from(b - 0x80)] {
                    '\0' => char::from(b),
                    c => c,
                },
                _ => char::from(b),
            })
            .collect()
    };
    for good in [
        "a — b",
        "▸ open",
        "x → y",
        "2 × 3",
        "Settings › Paths",
        "🤖 done",
        "café",
    ] {
        assert!(find_hits(good).is_empty(), "{good}");
        let bad = format!("ok\n{}", double(good));
        assert_eq!(find_hits(&bad), vec![2], "{good} double-encoded");
    }
    for fine in [
        "naïve résumé",
        "Größe",
        "£5 · €3",
        "« quoted »",
        "Ça va",
        "ÿ",
    ] {
        assert!(find_hits(fine).is_empty(), "{fine}");
    }
}
