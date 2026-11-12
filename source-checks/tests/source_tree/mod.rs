#![allow(dead_code)] // each test crate uses a subset of these helpers

//! Shared by the source-tree standards tests (`test_attribute_scan.rs`,
//! `file_ceilings.rs`): finds the workspace's `.rs` files and reads them
//! without comments and string contents, so brace counting and word searches
//! are not fooled by `"{"` or a `// fn helper` note.

use std::fs;
use std::path::{Path, PathBuf};

/// The workspace root (the folder holding the root `Cargo.toml`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("native/ sits inside the workspace root")
        .to_path_buf()
}

/// Every tracked-looking `.rs` file under the workspace, sorted. Skips build
/// output, dependencies, `local/` (gitignored review files) and worktrees.
pub fn rust_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(&workspace_root(), &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let skip = name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "target" | "node_modules" | "local" | "dist" | "pkg"
                )
                || name.starts_with("target-");
            if !skip {
                walk(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `path` relative to the workspace root, with forward slashes.
pub fn relative(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// `src` with every comment and the inside of every string / char literal
/// replaced by spaces (newlines kept), so offsets and line numbers still match.
pub fn blank_comments_and_strings(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = b.clone();
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in out.iter_mut().take(to).skip(from) {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            let end = b[i..]
                .iter()
                .position(|&x| x == '\n')
                .map_or(b.len(), |p| i + p);
            blank(&mut out, i, end);
            i = end;
        } else if c == '/' && next == Some('*') {
            let mut depth = 1;
            let mut j = i + 2;
            while j < b.len() && depth > 0 {
                if b[j] == '/' && b.get(j + 1) == Some(&'*') {
                    depth += 1;
                    j += 2;
                } else if b[j] == '*' && b.get(j + 1) == Some(&'/') {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            blank(&mut out, i, j);
            i = j;
        } else if c == 'r' && matches!(next, Some('"') | Some('#')) && !prev_is_ident(&b, i) {
            // Raw string: r"..", r#".."#, r##".."##.
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) != Some(&'"') {
                i += 1;
                continue;
            }
            j += 1;
            let close: Vec<char> = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            while j < b.len() && !b[j..].starts_with(&close) {
                j += 1;
            }
            let end = (j + close.len()).min(b.len());
            blank(&mut out, i + 1, end);
            i = end;
        } else if c == '"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != '"' {
                j += if b[j] == '\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(b.len());
            blank(&mut out, i + 1, end.saturating_sub(1).max(i + 1));
            i = end;
        } else if c == '\'' {
            // A char literal ('x', '\n', '\u{1F}') or a lifetime ('a).
            if next == Some('\\') {
                let mut j = i + 2;
                while j < b.len() && b[j] != '\'' {
                    j += 1;
                }
                blank(&mut out, i + 1, j);
                i = j + 1;
            } else if b.get(i + 2) == Some(&'\'') {
                blank(&mut out, i + 1, i + 2);
                i += 3;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// Whether the `r` at `i` continues an identifier. A `b` or `c` just before it
/// is a string prefix (`br#".."#`), not part of one.
fn prev_is_ident(b: &[char], i: usize) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    match i {
        0 => false,
        1 => ident(b[0]) && !matches!(b[0], 'b' | 'c'),
        _ => ident(b[i - 1]) && !(matches!(b[i - 1], 'b' | 'c') && !ident(b[i - 2])),
    }
}

/// Non-test lines of a file: the lines before the first top-level
/// `#[cfg(test)]` (inline test modules sit at the end), and none at all for a
/// file that is itself a test module (`tests.rs`, `*_tests.rs`, anything under
/// a `tests/` folder). The same counting rule `docs/coding_standards.md` gives
/// for the 1,000-line prompt.
pub fn non_test_lines(path: &Path, src: &str) -> usize {
    let rel = relative(path);
    let file = rel.rsplit('/').next().unwrap_or(&rel);
    if file == "tests.rs"
        || file.ends_with("_tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/examples/")
    {
        return 0;
    }
    let mut count = 0;
    for line in src.lines() {
        if line.trim_end() == "#[cfg(test)]" {
            break;
        }
        count += 1;
    }
    count
}
