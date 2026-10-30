# Coding standards

How code in this repository is written, for people and coding agents. These apply to every crate and every language here (Rust, JavaScript, Python tools), to new code and to any code a change touches. Engine and capture rules live in [CLAUDE.md](../CLAUDE.md); module boundaries in [app_architecture.md](app_architecture.md).

The aim is code that stays clean while it is written, so no file needs a cleanup pass every few weeks. Each rule says why, so a case it doesn't fit can be argued on its merits. Where a rule and the surrounding code disagree, follow the rule in new code and fix the old code when you are already changing it.

The rules were checked against current guidance (sources at the end): the Rust API Guidelines, Google's engineering practices and TypeScript guide, Ousterhout's *A Philosophy of Software Design* and his 2024-25 exchange with Robert Martin over *Clean Code*, and what the defect-density research does and doesn't show. Where that guidance disagrees with the older "small functions, few comments" school, the rule here says which side it takes and why.

## 1. Organisation

- **One area per file.** A file holds one area of the app: one pane, one parser, one manager, one hook module. Code that belongs to another area goes in that area's file, or a new module, when it is written, never in whichever file is open. No file is a dumping ground for everything loosely related to it.
- **Size is a prompt, not a limit.** A big file with one job is fine; a small file with three jobs isn't. When a file passes about 1,000 lines (Rust) or 600 (JavaScript), check that it still has one job. Count code, not tests: a Rust file's inline `#[cfg(test)]` module, a file that is only tests (`tests.rs`, `*_tests.rs`) and a table of data don't count, because tests grow with the logic they cover and say nothing about how many jobs a file has (#1180). Of 56 Rust files over 1,000 lines, about 22 are over it without their tests; those are the ones to look at. The same goes for a module's interface: a file whose exports keep growing is usually two areas.
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
- **A long function that runs a pipeline in order is split by stage, with a struct for what the stages share.** When a function does stage A, then B, then C, and the stages pass many local variables along, those locals are a struct. Name it for what the stages build up (`CaptureRun`: paths, the schedule, the cancel flag), make each stage a function that takes it and does that stage's work, and leave the original as the short function that calls the stages in order and returns at the first failure. A stage earns its own function when it has a name, one input and a result you can test without running the others; don't peel off a stage that only makes sense next to the following one's variables. Pin the current behaviour with tests first, split in a pure-move PR (section 10), and change behaviour only after (#1180; the #1107 audit's `spawn_capture_engine`, `build_batch_queue` and `run_render_job`).
- **Prefer pure functions** for anything with rules in it: input in, result out, no I/O. They are the ones worth unit-testing.

## 3. Names

- **Say what it is in the app's words.** The terms the UI uses: highlight, take, clip, batch, project. One name per idea everywhere: not `streak` in one file and `highlight` in the next for the same thing. When the UI renames something, the code follows (CLAUDE.md, renames).
- **A name that is written to disk or sent over IPC keeps its old spelling until a migration PR renames it.** Saved project files, the analyzer cache and the IPC payloads between the frontend and Tauri hold names as data, so renaming one breaks every existing file. Code that never touches the wire uses the new word at once. The one place that does keeps the old name (`#[serde(rename = "kill_streaks")]`, or a payload field left as it was) with a comment saying it is the stored spelling. The stored name changes in its own PR, which reads both spellings, writes the new one and bumps or migrates the cache as CLAUDE.md describes. About 1,000 `streak` identifiers remain; the ones on the wire stay until then (#1180).
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
- **No `unwrap()` / `expect()` outside tests**, except on an invariant the code just established, with a comment saying which. One exception needs no comment: `.lock().unwrap()` (and `.read()` / `.write()`) on a `Mutex` or `RwLock`. A lock is poisoned only after another thread panicked while holding it, and release builds abort on a panic, so there it cannot happen; a comment on each of ~100 sites would be noise (#1180). Debug and test builds do unwind, so there one panicking thread poisons the lock and every later `.lock().unwrap()` panics too, which buries the first failure under copies of it. Code whose lock outlives a test or a `tauri dev` session therefore recovers instead: `unwrap_or_else(PoisonError::into_inner)`, which the tree already writes ~150 times. Write it once as a named helper, `lock_or_recover(&mutex) -> MutexGuard<T>` (with `read` / `write` twins), in one small module of `native`, and call that. Recover only when the data behind the lock is still valid after a panic mid-update; if it might not be, unwrap and let the panic stand. The wider Rust view is "unwrap is fine where a panic means a bug" (BurntSushi); here release builds abort on a panic and the hook DLL takes the game down with it (CLAUDE.md), so the bar is higher.
- **New types for distinct ideas.** A tick, a frame count and a byte count are all integers; a `Tick(i64)` newtype can't be handed where a byte count goes (C-NEWTYPE). Use one where mixing two numbers up has bitten, or plausibly could.

## 5. Errors

CLAUDE.md has the rules: name what failed, print paths with `.display()`, no panics on bad input, `.catch()` on every `invoke()`. In addition:
- **Don't swallow an error.** `let _ =` and an empty `catch {}` say why the error doesn't matter, in a comment, or the error is logged where the user can see it (`log_markdown`). Two kinds need no comment, because the reason is the same every time and the call shows it (#1180):
  - **A send to a receiver that may be gone**: `let _ = tx.send(..)`, `let _ = app.emit(..)`. A closed channel means the listener finished or the window closed, and there is nobody left to tell. If a dropped message would leave the user waiting or the state wrong, it isn't this case: handle it.
  - **Best-effort cleanup of something we made**: `let _ = std::fs::remove_dir_all(&scratch)`. Failing to delete our own temp file doesn't change the result. Deleting something the user owns is not cleanup: report it.

  Every other `let _ =` (a write, a kill, a flush, any call whose result is a real outcome) keeps its comment or log. `let _ = unused;` to silence a warning isn't an ignored error: delete the unused name (section 7). Of 571 `let _ =` sites, about 130 are sends or emits and about 285 are file, process or write calls, so cleanup matters as much as channels.
- **One error message per failure**, written where it is detected; callers add context ("while checking take X"), not a second message. In Rust: typed errors (enums, `thiserror`-style) in `native`, `analysis` and `dod` wherever a caller decides something from which error it got (retry on one, skip on another, show different text); context added as the error rises through the Tauri layer, which turns it into the one string the user sees.
- **`Result<_, String>` is fine when nobody branches on the error.** It is shown to the user or logged, and callers only pass it up with `?` or `map_err`. That is most of `native` today (about 175 `String` errors against 3 error enums) and a reasonable shape, not a debt. Make it a typed enum when a caller matches on the text (a `contains("...")` on an error message is the sign), or one function has two failures its caller treats differently. Don't add an enum to wrap one string. At the public edge of a crate other crates call (`dod`, `analysis`, `dem-patch`), prefer the enum: those callers can't read a message to decide (#1180).
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
- **Leave code you touch a little better**, within limits: a clearer local name, a stale comment fixed, a dead line removed can ride along in the PR. A split function, a moved module or a renamed public item is its own commit, or its own PR when it touches more than the files the change already touches.
- **A pure move is exempt from the size guide.** Moving code to a new file or module with no edit to the code itself may be as large as the file it splits, if the PR says it is only a move, the diff shows it (git's move detection, or the same lines under a new path) and the tests pass unchanged. Any change beyond the move goes in another PR, so a reviewer reads either a move or a change, never both (#1180). Not a rewrite of what you didn't need to change. Something noticed but out of scope gets an issue (CLAUDE.md, issues).
- **Follow the surrounding code's style** where these rules are silent. `rustfmt` and Clippy (pinned toolchain, CLAUDE.md) settle Rust formatting.

## Checked by tools

Today: `cargo fmt --check`, Clippy with `-D warnings`, and `main_js_budget.test.js` (a guideline: CLAUDE.md). Candidates, not yet turned on (lint rules change only with the owner's OK), each enabled by name rather than as a group, since Clippy's `pedantic` group is known for false positives:
- Clippy `too_many_arguments` (on by default at 7; lower it), `cognitive_complexity`, `fn_params_excessive_bools`, `unwrap_used` / `expect_used` (off by default; allow in tests), set as warnings first to see what they flag.
- An ESLint pass with `max-params` (default 3), `no-unused-vars`, `no-magic-numbers` with the usual 0, 1, -1 and array-index exceptions. Not `max-lines-per-function`: length is the weak signal (section 2).

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
