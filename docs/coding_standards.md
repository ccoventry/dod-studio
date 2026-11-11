# Coding standards

How code in this repository is written, for people and coding agents. These apply to every crate and every language here (Rust, JavaScript, Python tools), to new code and to any code a change touches. Engine and capture rules live in [CLAUDE.md](../CLAUDE.md); module boundaries in [app_architecture.md](app_architecture.md).

The aim is code that stays clean while it is written, so no file needs a cleanup pass every few weeks. Each rule says why, so a case it doesn't fit can be argued on its merits. Where a rule and the surrounding code disagree, follow the rule in new code and fix the old code when you are already changing it.

The rules were checked against current guidance (sources at the end): the Rust API Guidelines, Google's engineering practices and TypeScript guide, Ousterhout's *A Philosophy of Software Design* and his 2024-25 exchange with Robert Martin over *Clean Code*, and what the defect-density research does and doesn't show. Where that guidance disagrees with the older "small functions, few comments" school, the rule here says which side it takes and why.

## 1. Organisation

- **One area per file.** A file holds one area of the app: one pane, one parser, one manager, one hook module. Code that belongs to another area goes in that area's file, or a new module, when it is written, never in whichever file is open. No file is a dumping ground for everything loosely related to it.
- **Size is a prompt, not a limit.** A big file with one job is fine; a small file with three jobs isn't. When a file passes about 1,000 lines (Rust, counting inline tests) or 600 (JavaScript), check that it still has one job. The same goes for a module's interface: a file whose exports keep growing is usually two areas.
- **No grab-bag modules.** A file called `utils`, `helpers`, `misc` or `common` collects unrelated code. Name a module for what it holds (`paths.rs`, `number_field.js`).
- **Deep modules, not many shallow ones.** A good module hides a lot behind a small interface: a caller uses it without knowing how it works (`capture_room_manager.rs` decides Continue / Hold / CannotFit; the caller never sees the arithmetic). Splitting a file into several that each expose their internals, so a reader must read all of them to follow one path, adds interface without removing complexity (Ousterhout). The test for a split is "can the caller stop reading here?", not "is this file smaller?".
- **Logic in the layer that owns it.**
  - Decisions about demos, takes, capture and rendering live in `native` (or `analysis`/`dod`), where `cargo test` reaches them. A Tauri command parses its arguments, calls `native`, and shapes the reply.
  - In the frontend, logic with no DOM goes in a `_logic.js` module with Vitest tests (`obs_capture_test_logic.js`); the UI module wires it to the page.
  - `main.js` only wires (see CLAUDE.md).
- **Build features in their own module from the first line**, with a `createX({ ... })` / `startX` factory that takes what it needs as arguments rather than reaching for globals.

## 2. Functions

- **One task per function, named for it.** If the name needs "and", or a comment has to list its steps, split it. A function that reads, decides and writes is three functions; the one that runs them in order is a fourth, short one.
- **But don't split what belongs together.** A piece is worth its own function when a reader can trust its name and skip its body: it has a contract of its own, or a second caller, or a test of its own. Two functions that can only be understood together ("entangled": each reads the other's state, or one exists only to hold the other's middle) are worse than one, and a chain of five-line helpers that each hide nothing is a worse read than the forty lines they came from (Ousterhout; Martin agrees over-decomposition is possible). The research backs this: function length on its own barely predicts defects; branching and nesting do.
- **Complexity is the prompt, not length.** Past three levels of nesting, or a function where the branches outnumber the straight-line statements, look for the piece with its own name. A long function that reads top to bottom with no branches is fine.
- **No flag arguments that switch behaviour.** `render(clip, true)` reads as nothing. Use two functions, or an enum (`Mode::Preview`), or a named option (`{ preview: true }`). The Rust API Guidelines say the same (C-CUSTOM-TYPE: arguments convey meaning through types, not `bool` or `Option`).
- **Few parameters.** Past three or four, pass a struct or an options object with named fields (ESLint's `max-params` defaults to three).
- **No hidden side effects.** A function named `check_`, `get_`, `is_` or `find_` doesn't write files, change state or send events. One that does says so in its name (`save_`, `apply_`, `send_`).
- **Return early** on errors and edge cases instead of nesting the main path inside them.
- **Prefer pure functions** for anything with rules in it: input in, result out, no I/O. They are the ones worth unit-testing.

## 3. Names

- **Say what it is in the app's words.** The terms the UI uses: highlight, take, clip, batch, project. One name per idea everywhere: not `streak` in one file and `highlight` in the next for the same thing. When the UI renames something, the code follows (CLAUDE.md, renames).
- **Functions are verbs, data are nouns, booleans are yes/no questions**: `verify_takes`, `take_folder`, `is_running`, `has_audio`.
- **Each language's own conventions on top.** Rust: the API Guidelines' names (a getter is the field's name, `fn take_folder(&self)`, not `get_take_folder`; conversions are `as_` / `to_` / `into_` by cost; `iter` / `iter_mut` / `into_iter`; RFC 430 casing). JavaScript: `lowerCamelCase` for functions and variables, `UpperCamelCase` for classes, `CONSTANT_CASE` for module-level constants, `getX` is fine for a reader (Google's guide). Python tools: PEP 8.
- **Long names are not comments.** A name says what a thing is; what a caller must know to use it goes in its doc comment (section 8). A name that tries to carry the contract (`verifyTakesAndMarkCapturedUnlessSkipped`) is hard to read and still incomplete.
- **Units in the name** when a number has one: `delay_ms`, `start_tick`, `size_bytes`, `fps`.
- **No abbreviations** beyond the domain's own (`hl`, `obs`, `fps`, `dem`). `cnt`, `tmp2` and single letters outside short loops and closures don't say enough.

## 4. Data and types

- **Named constants, not magic numbers or strings.** `FAILOVER_THRESHOLD`, not `5 * 1024 * 1024 * 1024`; one constant per value, defined once and imported.
- **Enums over strings** for a fixed set of choices, in Rust and at the IPC boundary. A string compared in five places is an enum waiting to happen.
- **Parse at the boundary.** Wire values, settings and IPC payloads become typed values where they arrive. Code past that point doesn't re-check them.
- **`Option` / `Result`, not sentinel values** (`-1`, `""`, `0` meaning "none"). JavaScript uses `null` for "none", never a magic number.
- **No `unwrap()` / `expect()` outside tests**, except on an invariant the code just established, with a comment saying which. The wider Rust view is "unwrap is fine where a panic means a bug" (BurntSushi); here release builds abort on a panic and the hook DLL takes the game down with it (CLAUDE.md), so the bar is higher.
- **New types for distinct ideas.** A tick, a frame count and a byte count are all integers; a `Tick(i64)` newtype can't be handed where a byte count goes (C-NEWTYPE). Use one where mixing two numbers up has bitten, or plausibly could.

## 5. Errors

CLAUDE.md has the rules: name what failed, print paths with `.display()`, no panics on bad input, `.catch()` on every `invoke()`. In addition:
- **Don't swallow an error.** `let _ =` and an empty `catch {}` say why the error doesn't matter, in a comment, or the error is logged where the user can see it (`log_markdown`).
- **One error message per failure**, written where it is detected; callers add context ("while checking take X"), not a second message. In Rust: typed errors (`thiserror`-style enums) in `native`, `analysis` and `dod`, where callers match on them; context added as the error rises through the Tauri layer, which turns it into the one string the user sees.
- **Define errors out of existence where you can** (Ousterhout). An operation whose "error" case is a normal outcome shouldn't have one: deleting a file that is already gone is success, a lookup of a demo with no highlights returns an empty list, not `Err`. Fewer error paths means fewer branches for callers to get wrong.

## 6. Duplication

- **Look before writing.** Search for an existing helper before adding one; the second copy of a check is where the two start to drift.
- **The second time, consider it; the third time, share it.** Two similar blocks can stay if they may change apart. Three become one function.
- **One source of truth for shared values**: a constant the frontend and backend both need is defined once and checked by a test when it has to be mirrored (`HIGHLIGHTS_VERSION` and `scan_version.js` are the pattern).

## 7. Dead code

- **Delete it; don't comment it out.** Git keeps it.
- **No unused parameters, settings, flags or exports.** Before deleting what looks unused, read the `#[cfg]` lines around it (CLAUDE.md, WASM).
- **Temporary code** (a probe, a debug log) is removed in the same PR or lives in `native/examples/`.

## 8. Comments

CLAUDE.md has the rules: comments say why, in the present tense; cite the issue, not a date. In addition, two kinds of comment with different jobs:
- **Interface comments say the contract.** Every public Rust item and every exported JS function has a doc comment: what it does, what it needs, what it returns, what it can fail with, and any side effect (writes a file, sends an event). That is what the code can't express and a name can't carry (Ousterhout; Rust C-FAILURE: docs cover errors, panics and safety). A reader should be able to use the function from its comment without opening its body. The one-line summary is enough for a small helper; anything with a precondition says so.
- **Implementation comments say why, and only where the code isn't obvious.** Inside a function, a comment that restates the next lines ("increment the counter") is noise: a better name or a split fixes that. A comment that says why this way and not the obvious one, which invariant holds, or which bug this guards (with its number) is worth more than the code it sits on. Both sides of the *Clean Code* debate agree on this one.
- **Comments are kept with the code they describe.** Change the code, check the comment, in the same edit. A comment left behind by a rename is the commonest wrong comment here (CLAUDE.md, renames).

## 9. Tests

- **Logic ships with its tests** in the same PR, beside it (Rust `#[cfg(test)]` module, `x.test.js` next to `x.js`).
- **A test's name states the behaviour**: `a_take_rendered_and_deleted_while_capturing_verifies_as_captured`, not `test_verify_2`.
- **One behaviour per test.** A failing test should say what broke without reading its body.
- Fixtures and real demos: CLAUDE.md.

## 10. Changes and PRs

- **One change per PR.** A refactor and a behaviour change go in separate PRs, or at least separate commits, so a reviewer can see that the refactor changed nothing. Google's guidance for review: about 100 changed lines is a comfortable review, about 1,000 is too many; a reviewer may send a PR back to be split.
- **Leave code you touch a little better**, within limits: a clearer local name, a stale comment fixed, a dead line removed can ride along in the PR. A split function, a moved module or a renamed public item is its own commit, or its own PR when it touches more than the files the change already touches. Not a rewrite of what you didn't need to change. Something noticed but out of scope gets an issue (CLAUDE.md, issues).
- **Follow the surrounding code's style** where these rules are silent. `rustfmt` and Clippy (pinned toolchain, CLAUDE.md) settle Rust formatting.

## Checked by tools

Today, all in CI (#1179):
- `cargo fmt --check` and Clippy with `-D warnings`, which includes `too_many_arguments` at threshold 7 (`clippy.toml`). A function that still needs more carries `#[allow(clippy::too_many_arguments)]` with a one-line reason above it; there is no workspace-wide allow.
- `native/tests/test_attribute_scan.rs` (CLAUDE.md, #743): a `fn` in a `#[cfg(test)] mod` with no `#[test]` that nothing calls fails the build. The heuristic is written at the top of the file.
- Guideline tests for file size, like `main_js_budget.test.js`: `native/tests/file_ceilings.rs` and `studio/src/file_ceilings.test.js` hold a non-test line ceiling per hot file. A prompt to look, not a limit: raise a number with a reason in the PR.
- ESLint on `studio/src` (`npm run lint`, CI job "Frontend lint"): `no-unused-vars` is an error, `max-params` (5) a warning. A leading underscore marks a name taken only to be left out.

Measured, not gated (a warning would fail `-D warnings`): `cognitive_complexity` at Clippy's default threshold of 25 (`clippy.toml`). Run it when touching a long function, or before a refactor PR:

    rustup run 1.98.1 cargo clippy --workspace --all-targets -- -A clippy::all -W clippy::cognitive_complexity

On the day it was added it flagged 23 functions (12 outside `examples/`). Lower `cognitive-complexity-threshold` in `clippy.toml` to see more (15 flagged 81). Turn it on as a CI gate only once the list is short.

Candidates, not yet turned on (lint rules change only with the owner's OK), each enabled by name rather than as a group, since Clippy's `pedantic` group is known for false positives:
- Clippy `fn_params_excessive_bools`, and `unwrap_used` / `expect_used` (off by default; allow in tests; waits on the mutex-lock answer, #1180), set as warnings first to see what they flag.
- More ESLint rules: `max-params` as an error once its eight warnings are gone, `no-magic-numbers` with the usual 0, 1, -1 and array-index exceptions. Not `max-lines-per-function`: length is the weak signal (section 2).

Everything else here is for review: the person writing the code checks it, and a reviewer (or a review agent) checks it again. Rules that a tool can check should move to the tool over time: a 2026 study of agent instruction files found they help with the conventions an agent can't guess from the code and cost context for everything else, which is why this doc is separate from CLAUDE.md and CLAUDE.md only points here.

## Sources

- [A Philosophy of Software Design vs Clean Code](https://github.com/johnousterhout/aposd-vs-clean-code): Ousterhout and Martin's written exchange (Sept 2024 to Feb 2025) on function length, comments and TDD; sections 1, 2 and 8 take positions from it.
- [Rust API Guidelines checklist](https://rust-lang.github.io/api-guidelines/checklist.html): naming (C-CASE, C-CONV, C-GETTER, C-ITER), types over flags (C-CUSTOM-TYPE, C-NEWTYPE), docs cover failures (C-FAILURE), validate arguments (C-VALIDATE).
- [Google: small CLs](https://google.github.io/eng-practices/review/developer/small-cls.html): sizes, refactors apart from features, what may ride along.
- [Google TypeScript style guide](https://google.github.io/styleguide/tsguide.html) (the JavaScript guide is frozen and points here): casing, descriptive names, no abbreviations.
- [Using unwrap() in Rust is okay](https://burntsushi.net/unwrap/): the mainstream Rust view this doc is stricter than, and why.
- [ESLint `max-params`](https://oxc.rs/docs/guide/usage/linter/rules/eslint/max-params): the default of three.
- Defect-density research as summarised on [embedded.com](https://www.embedded.com/?p=4007519) (Coverity: no correlation between function length and defects) and the [SystemSafety list](https://lists.techfak.uni-bielefeld.de/pipermail/systemsafety/2013-June/000281.html) (complexity correlates, with no usable threshold).
- [Evaluating AGENTS.md](https://arxiviq.substack.com/p/evaluating-agentsmd-are-repository) (Gloaguen et al., arXiv 2602.11988, 2026): instruction files help with non-guessable conventions and otherwise cost more than they return.
