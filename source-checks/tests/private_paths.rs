//! The repo is public, so a real Windows user folder (`C:\Users\<someone>\...`)
//! in a tracked text file leaks a name. This scans every tracked text file for
//! `C:\Users\<segment>\` and `C:/Users/<segment>/` (also the doubled-backslash
//! form of string literals and JSON), case-insensitive, and fails with
//! `file:line` for each segment that is not a placeholder (CLAUDE.md, "SteamIDs
//! and player names are fine ...").
//!
//! Skipped: binary files, `local/` (gitignored), lockfiles. A real hit that
//! must stay goes in `ALLOWED` with the reason.

mod source_tree;

use source_tree::{read, relative, workspace_root};
use std::path::PathBuf;
use std::process::Command;

/// Segments that are placeholders or system profiles, not a person.
const PLACEHOLDERS: &[&str] = &[
    "name",
    "user",
    "you",
    "example",
    "username",
    "%username%",
    "public",
    "default",
    "runneradmin",
    // Obviously made-up names used in existing tests and docs.
    "me",
    "x",
    "someone",
];

/// `(file, reason)` pairs whose hits are deliberate. Keep it short.
const ALLOWED: &[(&str, &str)] = &[];

#[derive(Debug, PartialEq)]
struct Hit {
    line: usize,
    segment: String,
}

fn is_placeholder(segment: &str) -> bool {
    let s = segment.to_lowercase();
    (s.starts_with('<') && s.ends_with('>')) || PLACEHOLDERS.contains(&s.as_str())
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// Every non-placeholder user folder named in `text`.
fn find_hits(text: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let lower = line.to_lowercase();
        let mut from = 0;
        while let Some(p) = lower[from..].find("c:") {
            let at = from + p;
            from = at + 2;
            let rest: Vec<char> = line[at + 2..].chars().collect();
            let mut k = 0;
            let skip_seps = |k: &mut usize| {
                let start = *k;
                while *k < rest.len() && is_sep(rest[*k]) && *k - start < 2 {
                    *k += 1;
                }
                *k > start
            };
            if !skip_seps(&mut k) {
                continue;
            }
            let word: String = rest[k..].iter().take(5).collect();
            if !word.eq_ignore_ascii_case("users") {
                continue;
            }
            k += 5;
            if !skip_seps(&mut k) {
                continue;
            }
            let segment: String = rest[k..].iter().take_while(|c| !is_sep(**c)).collect();
            // A segment ends at a separator; one cut off by the line end or by
            // a quote or space is still a name, so only trim trailing quotes.
            let segment = segment
                .trim_end_matches(['"', '\'', '`', ')', ']', '}', ',', ';'])
                .to_string();
            if !segment.is_empty() && !is_placeholder(&segment) {
                hits.push(Hit {
                    line: i + 1,
                    segment,
                });
            }
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
fn no_real_user_folder_paths_in_tracked_files() {
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
        for hit in find_hits(&read(&path)) {
            report.push(format!("{rel}:{}: user folder `{}`", hit.line, hit.segment));
        }
    }
    assert!(
        report.is_empty(),
        "real user-folder paths in a public repo (use a made-up one such as C:/Users/<name>/...):\n{}",
        report.join("\n")
    );
}

#[test]
fn catches_a_real_looking_path_and_passes_placeholders() {
    // Built from parts so this file does not contain the pattern itself.
    let users = ["C:", "Users"];
    let made_up = "jdoe";
    for sep in ["\\", "/", "\\\\"] {
        let path = format!("{}{sep}{}{sep}{made_up}{sep}Documents", users[0], users[1]);
        let lower = path.to_lowercase();
        for text in [path.clone(), lower, path.to_uppercase()] {
            let hits = find_hits(&format!("ok\nsee `{text}` here"));
            assert_eq!(hits.len(), 1, "{text}");
            assert_eq!(hits[0].line, 2);
            assert!(hits[0].segment.eq_ignore_ascii_case(made_up));
        }
    }
    for seg in [
        "<name>",
        "<user>",
        "name",
        "user",
        "you",
        "example",
        "USERNAME",
        "%USERNAME%",
        "Public",
        "Default",
        "runneradmin",
    ] {
        for sep in ["\\", "/"] {
            let path = format!("{}{sep}{}{sep}{seg}{sep}x", users[0], users[1]);
            assert!(find_hits(&path).is_empty(), "{path}");
        }
    }
    assert!(find_hits("C:/Program Files/x and D:/Users/jdoe/y").is_empty());
}
