# Coding standards

How code in this repository is written, for people and coding agents. These apply to every crate and every language here (Rust, JavaScript, Python tools), to new code and to any code a change touches. Engine and capture rules live in [CLAUDE.md](../CLAUDE.md); module boundaries in [app_architecture.md](app_architecture.md).

The aim is code that stays clean while it is written, so no file needs a cleanup pass every few weeks. Each rule says why, so a case it doesn't fit can be argued on its merits. Where a rule and the surrounding code disagree, follow the rule in new code and fix the old code when you are already changing it.

## 1. Organisation

- **One area per file.** A file holds one area of the app: one pane, one parser, one manager, one hook module. Code that belongs to another area goes in that area's file, or a new module, when it is written, never in whichever file is open. No file is a dumping ground for everything loosely related to it.
- **Size is a prompt, not a limit.** A big file with one job is fine; a small file with three jobs isn't. When a file passes about 1,000 lines (Rust, counting inline tests) or 600 (JavaScript), check that it still has one job.
- **No grab-bag modules.** A file called `utils`, `helpers`, `misc` or `common` collects unrelated code. Name a module for what it holds (`paths.rs`, `number_field.js`).
- **Logic in the layer that owns it.**
  - Decisions about demos, takes, capture and rendering live in `native` (or `analysis`/`dod`), where `cargo test` reaches them. A Tauri command parses its arguments, calls `native`, and shapes the reply.
  - In the frontend, logic with no DOM goes in a `_logic.js` module with Vitest tests (`obs_capture_test_logic.js`); the UI module wires it to the page.
  - `main.js` only wires (see CLAUDE.md).
- **Build features in their own module from the first line**, with a `createX({ ... })` / `startX` factory that takes what it needs as arguments rather than reaching for globals.

## 2. Functions

- **One task per function, named for it.** If the name needs "and", or a comment has to list its steps, split it. A function that reads, decides and writes is three functions; the one that runs them in order is a fourth, short one.
- **Length is a prompt too.** Past about 50 lines, look for the second task hiding in it.
- **No flag arguments that switch behaviour.** `render(clip, true)` reads as nothing. Use two functions, or an enum (`Mode::Preview`), or a named option (`{ preview: true }`).
- **Few parameters.** Past four, pass a struct or an options object with named fields.
- **No hidden side effects.** A function named `check_`, `get_`, `is_` or `find_` doesn't write files, change state or send events. One that does says so in its name (`save_`, `apply_`, `send_`).
- **Return early** on errors and edge cases instead of nesting the main path inside them.
- **Prefer pure functions** for anything with rules in it: input in, result out, no I/O. They are the ones worth unit-testing.

## 3. Names

- **Say what it is in the app's words.** The terms the UI uses: highlight, take, clip, batch, project. One name per idea everywhere: not `streak` in one file and `highlight` in the next for the same thing. When the UI renames something, the code follows (CLAUDE.md, renames).
- **Functions are verbs, data are nouns, booleans are yes/no questions**: `verify_takes`, `take_folder`, `is_running`, `has_audio`.
- **Units in the name** when a number has one: `delay_ms`, `start_tick`, `size_bytes`, `fps`.
- **No abbreviations** beyond the domain's own (`hl`, `obs`, `fps`, `dem`). `cnt`, `tmp2` and single letters outside short loops and closures don't say enough.

## 4. Data and types

- **Named constants, not magic numbers or strings.** `FAILOVER_THRESHOLD`, not `5 * 1024 * 1024 * 1024`; one constant per value, defined once and imported.
- **Enums over strings** for a fixed set of choices, in Rust and at the IPC boundary. A string compared in five places is an enum waiting to happen.
- **Parse at the boundary.** Wire values, settings and IPC payloads become typed values where they arrive. Code past that point doesn't re-check them.
- **`Option` / `Result`, not sentinel values** (`-1`, `""`, `0` meaning "none"). JavaScript uses `null` for "none", never a magic number.
- **No `unwrap()` / `expect()` outside tests**, except on an invariant the code just established, with a comment saying which. Release builds abort on a panic (CLAUDE.md).

## 5. Errors

CLAUDE.md has the rules: name what failed, print paths with `.display()`, no panics on bad input, `.catch()` on every `invoke()`. In addition:
- **Don't swallow an error.** `let _ =` and an empty `catch {}` say why the error doesn't matter, in a comment, or the error is logged where the user can see it (`log_markdown`).
- **One error message per failure**, written where it is detected; callers add context, not a second message.

## 6. Duplication

- **Look before writing.** Search for an existing helper before adding one; the second copy of a check is where the two start to drift.
- **The second time, consider it; the third time, share it.** Two similar blocks can stay if they may change apart. Three become one function.
- **One source of truth for shared values**: a constant the frontend and backend both need is defined once and checked by a test when it has to be mirrored (`HIGHLIGHTS_VERSION` and `scan_version.js` are the pattern).

## 7. Dead code

- **Delete it; don't comment it out.** Git keeps it.
- **No unused parameters, settings, flags or exports.** Before deleting what looks unused, read the `#[cfg]` lines around it (CLAUDE.md, WASM).
- **Temporary code** (a probe, a debug log) is removed in the same PR or lives in `native/examples/`.

## 8. Comments

CLAUDE.md has the rules: comments say why, in the present tense; cite the issue, not a date. In addition:
- **The code says what; the comment says why.** If a comment explains what the next lines do, a better name or a smaller function usually makes it unnecessary.
- **Every public Rust item and every exported JS function** has a one- or two-line doc comment: what it is for, and anything a caller must know.

## 9. Tests

- **Logic ships with its tests** in the same PR, beside it (Rust `#[cfg(test)]` module, `x.test.js` next to `x.js`).
- **A test's name states the behaviour**: `a_take_rendered_and_deleted_while_capturing_verifies_as_captured`, not `test_verify_2`.
- **One behaviour per test.** A failing test should say what broke without reading its body.
- Fixtures and real demos: CLAUDE.md.

## 10. Changes and PRs

- **One change per PR.** A refactor and a behaviour change go in separate PRs, or at least separate commits, so a reviewer can see that the refactor changed nothing.
- **Leave code you touch a little better**: a clearer name, a split function, a stale comment fixed. Not a rewrite of what you didn't need to change.
- **Follow the surrounding code's style** where these rules are silent. `rustfmt` and Clippy (pinned toolchain, CLAUDE.md) settle Rust formatting.

## Checked by tools

Today: `cargo fmt --check`, Clippy with `-D warnings`, and `main_js_budget.test.js` (a guideline: CLAUDE.md). Candidates, not yet turned on (lint rules change only with the owner's OK):
- Clippy `too_many_lines`, `too_many_arguments`, `cognitive_complexity`, `fn_params_excessive_bools`, set as warnings first to see what they flag.
- An ESLint pass with `max-lines-per-function`, `max-params`, `no-unused-vars`.

Everything else here is for review: the person writing the code checks it, and a reviewer (or a review agent) checks it again.
