//! A `fn` in a `#[cfg(test)] mod` that nothing calls and that has no `#[test]`
//! is a test that silently stopped running. Resolving a merge conflict by
//! keeping both sides of a block whose `#[test]` line was shared context does
//! exactly that (CLAUDE.md, #743); this test fails with the list instead.
//!
//! The heuristic, per workspace `.rs` file (comments and string contents are
//! blanked first):
//!
//! 1. Test modules are `#[cfg(test)] mod name { .. }` blocks and the files of
//!    `#[cfg(test)] mod name;` (`name.rs` or `name/mod.rs` beside the file, or
//!    under `<stem>/` for a file that is not `mod.rs`/`lib.rs`/`main.rs`).
//!    `#[path]` modules are not followed.
//! 2. Candidates are `fn`s whose enclosing blocks are all `mod` blocks: methods
//!    of an `impl` or `trait`, `extern` blocks, nested `fn`s and macro bodies
//!    are skipped.
//! 3. A candidate passes if an attribute marks it as a test (`#[test]`,
//!    `#[tokio::test]`, `#[rstest]`, `#[test_case(..)]`, `#[x_test]`).
//! 4. Otherwise it passes if its name appears again anywhere else in the same
//!    test module (a helper some test calls), or, for a `pub` fn, anywhere else
//!    in its file or in another workspace file (a shared helper).
//! 5. Anything left is reported as `file:line: fn name`.
//!
//! What it can miss: an un-attributed test whose name is also used elsewhere in
//! its module, or (when `pub`) anywhere in the workspace. Zero false positives
//! on today's tree is the bar; widen the heuristic only with that kept.

mod source_tree;

use source_tree::{blank_comments_and_strings, read, relative, rust_files};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
struct Finding {
    line: usize,
    name: String,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// How many times `word` appears in `text` as a whole identifier.
fn word_count(text: &str, word: &str) -> usize {
    let mut count = 0;
    let mut from = 0;
    while let Some(p) = text[from..].find(word) {
        let start = from + p;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
            count += 1;
        }
        from = end;
    }
    count
}

/// Index just past the `}` matching the `{` at `open` in `text`.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn skip_ws(text: &str, mut i: usize) -> usize {
    while text[i..].starts_with(|c: char| c.is_whitespace()) {
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    i
}

/// What follows a `#[cfg(test)]` at `after`: `mod name {` gives the block's
/// span, `mod name;` gives the name for the external file.
enum TestMod {
    Inline(usize, usize),
    External(String),
}

fn test_mod_after(text: &str, after: usize) -> Option<TestMod> {
    let mut i = skip_ws(text, after);
    // Further attributes between `#[cfg(test)]` and `mod`.
    while text[i..].starts_with("#[") {
        let close = text[i..].find(']')? + i;
        i = skip_ws(text, close + 1);
    }
    for vis in ["pub(crate)", "pub(super)", "pub"] {
        if text[i..].starts_with(vis) {
            i = skip_ws(text, i + vis.len());
            break;
        }
    }
    if !text[i..].starts_with("mod ") {
        return None;
    }
    i = skip_ws(text, i + 3);
    let name_end = text[i..].find(|c: char| !is_ident(c))? + i;
    let name = text[i..name_end].to_string();
    let j = skip_ws(text, name_end);
    match text[j..].chars().next()? {
        '{' => Some(TestMod::Inline(j + 1, matching_brace(text, j)? - 1)),
        ';' => Some(TestMod::External(name)),
        _ => None,
    }
}

/// The text of every test module inside one file, with the offset it starts
/// at, plus the names of `#[cfg(test)] mod name;` files to read separately.
fn test_modules(text: &str) -> (Vec<(usize, usize)>, Vec<String>) {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut external = Vec::new();
    let mut from = 0;
    while let Some(p) = text[from..].find("#[cfg(test)]") {
        let at = from + p;
        from = at + "#[cfg(test)]".len();
        match test_mod_after(text, from) {
            Some(TestMod::Inline(s, e)) => {
                if !spans.iter().any(|&(a, b)| a <= s && e <= b) {
                    spans.push((s, e));
                }
            }
            Some(TestMod::External(name)) => external.push(name),
            None => {}
        }
    }
    (spans, external)
}

#[derive(PartialEq, Clone, Copy)]
enum Block {
    Mod,
    Other,
}

struct Candidate {
    name: String,
    offset: usize,
    is_pub: bool,
    is_test: bool,
}

fn is_test_attr(attr: &str) -> bool {
    let path = attr
        .trim()
        .split(|c: char| c == '(' || c == '=' || c.is_whitespace())
        .next()
        .unwrap_or("");
    let last = path.rsplit("::").next().unwrap_or("");
    matches!(last, "test" | "rstest" | "test_case") || last.ends_with("_test")
}

/// Splits a header (the text since the last `{`, `}` or `;`) into its
/// attributes and the item text after them.
fn split_attrs(header: &str) -> (Vec<String>, String) {
    let mut attrs = Vec::new();
    let mut rest = String::new();
    let chars: Vec<char> = header.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' && chars.get(i + 1) == Some(&'[') {
            let mut depth = 0;
            let start = i + 2;
            while i < chars.len() {
                match chars[i] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            attrs.push(chars[start..i.min(chars.len())].iter().collect());
            i += 1;
        } else {
            rest.push(chars[i]);
            i += 1;
        }
    }
    (attrs, rest)
}

/// Keyword and name of an item header: `pub(crate) unsafe fn foo<T>(` is
/// ("fn", Some("foo")).
fn item_kind(item: &str) -> (String, Option<String>, bool) {
    const QUALIFIERS: [&str; 11] = [
        "pub", "crate", "super", "self", "in", "unsafe", "async", "const", "extern", "default",
        "safe",
    ];
    let words: Vec<&str> = item
        .split(|c: char| !is_ident(c))
        .filter(|w| !w.is_empty())
        .collect();
    let is_pub = words.first() == Some(&"pub");
    let mut it = words.iter().filter(|w| !QUALIFIERS.contains(w));
    let keyword = it.next().map_or(String::new(), |w| w.to_string());
    let name = it.next().map(|w| w.to_string());
    (keyword, name, is_pub)
}

/// The `fn`s of one module's text (offsets relative to it) that sit directly
/// in the module or in nested `mod` blocks.
fn candidates(module: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut stack: Vec<Block> = Vec::new();
    let mut header_start = 0;
    for (i, c) in module.char_indices() {
        match c {
            '{' => {
                let header = &module[header_start..i];
                let (attrs, item) = split_attrs(header);
                let (keyword, name, is_pub) = item_kind(&item);
                let direct = stack.iter().all(|b| *b == Block::Mod);
                if keyword == "fn"
                    && direct
                    && let Some(name) = name
                {
                    out.push(Candidate {
                        name,
                        offset: header_start + header.len() - header.trim_start().len(),
                        is_pub,
                        is_test: attrs.iter().any(|a| is_test_attr(a)),
                    });
                }
                stack.push(if keyword == "mod" {
                    Block::Mod
                } else {
                    Block::Other
                });
                header_start = i + 1;
            }
            '}' => {
                stack.pop();
                header_start = i + 1;
            }
            ';' => header_start = i + 1,
            _ => {}
        }
    }
    out
}

/// Findings for one test module. `elsewhere` answers whether a name occurs in
/// another workspace file.
fn untested_fns(
    module: &str,
    line_of: impl Fn(usize) -> usize,
    elsewhere: impl Fn(&str) -> bool,
) -> Vec<Finding> {
    candidates(module)
        .into_iter()
        .filter(|c| !c.is_test)
        // The definition itself is one occurrence.
        .filter(|c| word_count(module, &c.name) <= 1)
        .filter(|c| !(c.is_pub && elsewhere(&c.name)))
        .map(|c| Finding {
            line: line_of(c.offset),
            name: c.name,
        })
        .collect()
}

fn external_module_files(file: &Path, name: &str) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let base = if matches!(stem, "mod" | "lib" | "main") {
        dir.to_path_buf()
    } else {
        dir.join(stem)
    };
    vec![
        base.join(format!("{name}.rs")),
        base.join(name).join("mod.rs"),
    ]
}

#[test]
fn every_fn_in_a_test_module_is_a_test_or_is_used() {
    let files = rust_files();
    let blanked: HashMap<PathBuf, String> = files
        .iter()
        .map(|p| (p.clone(), blank_comments_and_strings(&read(p))))
        .collect();

    let mut findings = Vec::new();
    for file in &files {
        let text = &blanked[file];
        let (spans, external) = test_modules(text);
        let elsewhere = |name: &str| {
            word_count(text, name) > 1
                || blanked
                    .iter()
                    .any(|(p, t)| p != file && word_count(t, name) > 0)
        };
        for (start, end) in spans {
            let module = &text[start..end];
            let line_of = |off: usize| text[..start + off].matches('\n').count() + 1;
            for f in untested_fns(module, line_of, elsewhere) {
                findings.push(format!("{}:{}: fn {}", relative(file), f.line, f.name));
            }
        }
        for name in external {
            let Some(path) = external_module_files(file, &name)
                .into_iter()
                .find(|p| blanked.contains_key(p))
            else {
                continue;
            };
            let module = &blanked[&path];
            let line_of = |off: usize| module[..off].matches('\n').count() + 1;
            let elsewhere = |n: &str| {
                blanked
                    .iter()
                    .any(|(p, t)| *p != path && word_count(t, n) > 0)
            };
            for f in untested_fns(module, line_of, elsewhere) {
                findings.push(format!("{}:{}: fn {}", relative(&path), f.line, f.name));
            }
        }
    }
    findings.sort();
    findings.dedup();
    assert!(
        findings.is_empty(),
        "fn(s) in a #[cfg(test)] module with no #[test] that nothing calls (a lost #[test] attribute? \
         add it, or delete the fn):\n{}",
        findings.join("\n")
    );
}

#[test]
fn flags_a_fn_that_lost_its_test_attribute() {
    let src = blank_comments_and_strings(
        r#"
        use super::*;
        fn helper() -> i32 { 1 }
        #[test]
        fn kept() { assert_eq!(helper(), 1); }
        fn lost() { assert!(true); }
        #[tokio::test]
        async fn also_kept() {}
        struct S;
        impl S { fn method(&self) {} }
        fn outer() { fn inner() {} }
        mod nested { fn lost_in_nested() {} }
        "#,
    );
    let found: Vec<String> = untested_fns(&src, |_| 0, |_| false)
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(found, vec!["lost", "outer", "lost_in_nested"]);
}

#[test]
fn finds_inline_and_external_test_modules() {
    let src = blank_comments_and_strings(
        "fn real() {}\n#[cfg(test)]\nmod tests { #[test] fn t() {} }\n#[cfg(test)]\nmod more;\n",
    );
    let (spans, external) = test_modules(&src);
    assert_eq!(spans.len(), 1);
    assert_eq!(external, vec!["more".to_string()]);
}

#[test]
fn ignores_braces_and_fn_words_in_strings_and_comments() {
    let src = blank_comments_and_strings(
        "#[test] fn a() { let s = \"} fn ghost() {\"; // fn ghost2() {\n }\n#[test] fn b() {}",
    );
    assert!(untested_fns(&src, |_| 0, |_| false).is_empty());
}
