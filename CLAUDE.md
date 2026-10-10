# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working in this repository.

## Project Overview

`dod-studio` is a high-performance pipeline for capturing, patching, and analyzing **Day of Defeat 1.3** (GoldSrc engine) demo files (`.dem`). It drives **HLAE** (Half-Life Advanced Effects) and `hl.exe` headlessly to batch-record highlight clips out of recorded matches, transcodes the results via FFmpeg, and parses demos for match analytics (scoreboards, kills, chat, rounds). The active desktop application is a Tauri + Vite frontend (`studio/`).

---

## Workspace Layout & Module Boundaries

Cargo workspace (`Cargo.toml`, resolver "3", edition 2024) containing the following members:

- **`dod/`** — Low-level `nom`-based binary parsers/types for DoD 1.3 demo message structures. Pure parsing primitives (no I/O); consumed by `analysis` and `native`.
- **`analysis/`** — Turns parsed demo data into match analytics (`scoreboard.rs`, `kill.rs`, `chat.rs`, `round.rs`, `mortality.rs`, `player.rs`, `clan_match.rs`, `localization.rs`). Entry point: `Analysis::try_from_bytes_with_progress`.
- **`dem-patch/`** — Vendored fork of the `dem` crate (`[patch.crates-io]`). Low-level demo bit/byte reading/writing (`bit.rs`, `byte_writer.rs`, `demo_parser.rs`, `demo_writer.rs`, `delta.rs`).
- **`native/`** — Core engine crate containing almost all non-UI logic:
  - `capture_engine.rs` — Orchestrates capture batches: spawns HLAE/`hl.exe`, drives playback, injects console commands at scheduled ticks.
  - `patch/` — Demo-file binary patcher (`engine.rs`, `builder.rs`, `scanner.rs`, `highlevel.rs`, `types.rs`). Scans, injects (bookmarks, director commands, `DRC_CMD_INEYE`), and rewrites GoldSrc frames.
  - `hlcr/` — Take management & FFmpeg transcoding (`renderer.rs`, `scanner.rs`, `config.rs`, `autosave.rs`).
  - `shared/`, `sys/`, `utils/` — Path resolution, disk-space queries, demo hashing.
  - `src/bin/preview_cli/main.rs` → `preview_cli` binary: Headless entry point with drag-and-drop support. (`src/bin/cli.rs` is a different binary, `dod-studio-cli`.) `autobins = false`: every binary is listed in `native/Cargo.toml`, and one-off R&D probes live in `native/examples/`.
- **`hl-demo-auditor/`** — Standalone duplicate-demo detector using size + header hash (`fnv1a_hash`).
- **`benchmark/`** — Performance benchmarking binary for the parsing/patching pipeline.
- **`studio/`** — Active Tauri v2 + Vite/JS frontend workspace (`src-tauri/` backend and `src/*.js` frontend modules).
- **`goldsrc-hooks/`** — 32-bit companion DLL injected into `hl.exe` alongside HLAE's: HLTV sound/viewmodel fixes, HUD/scoreboard/crosshair/kill-feed control, the decal clear, HD textures, and the `dodstudio_*` console surface (`docs/dodstudio_commands.md`). Builds only for `i686-pc-windows-msvc`: `cargo build -p goldsrc-hooks --release --target i686-pc-windows-msvc --lib --bins`. See `goldsrc-hooks/README.md`.
- **`web-analyzer/`** — `analysis` compiled to `wasm32-unknown-unknown`, deployed to GitHub Pages on every push to `main` (`.github/workflows/deploy_web.yml`). Static frontend lives in `www/`.

> The GoldSrc HLDEMO → Xash3D IDEM transcoder for the browser preview viewer moved out of this repo entirely: it now lives at `ccoventry/dod-web-demo-viewer` (`xash-transcode/` + a hand-synced copy of `dem-patch/`), since that's the repo the browser preview viewer itself lives in. dod-tools' old `experimental/xash-transcode` branch is kept only as a historical record — do not add new commits to it, and do not open PRs against it. See `docs/web_preview_viewer.md` in the new repo before touching that code.

---

## Common Development Commands

    # Build workspace binaries
    cargo build --workspace
    cargo build --release --workspace

    # Execute test suites
    cargo test --workspace
    cargo test -p analysis            # Single crate
    cargo test -p native patch::      # Single module

    # Run headless preview CLI
    cargo run -p native --bin preview_cli -- <path-to-demo-or-folder>

    # Desktop App Dev Loop (Run from studio/)
    cd studio
    npm install
    npm run tauri dev     # Launch Tauri window with Vite HMR
    npm run dev           # Vite dev server only
    npm run build         # Production Vite build

### Formatting

The tree is rustfmt-formatted and CI's Clippy job gates on `cargo fmt --all --check` (#235). Format with the CI-pinned toolchain, not your default one, since rustfmt output can shift between releases:

    rustup run 1.98.1 cargo fmt --all

The one-time whole-tree reformat is listed in `.git-blame-ignore-revs`; run `git config blame.ignoreRevsFile .git-blame-ignore-revs` once per clone so `git blame` skips it.

Clippy is pinned the same way; your default toolchain misses lints CI catches:

    rustup run 1.98.1 cargo clippy --workspace --all-targets -- -D warnings

### Build traps

- **Build the hook DLL first in a fresh checkout or worktree.** `studio`'s build script checks that `target\i686-pc-windows-msvc\release\dodstudio_goldsrc_hooks.dll` exists (a Tauri bundle resource); without it the whole workspace fails to build, far from any code. Give each worktree its own `CARGO_TARGET_DIR`: a shared one silently reuses another worktree's artifacts.
- **`npm run tauri dev` watches the workspace.** Any file write in that checkout restarts Studio, and every child process (Steam, HLAE, `hl.exe`) dies with it, because they run inside cargo's kill-on-close job object. While the app is running from a checkout, edit in a separate worktree.
- **JS unit tests:** `npm run test:unit` (Vitest, `studio/src/*.test.js`) runs in CI alongside the Playwright e2e suite.

---

## System Guardrails & Agent Directives

### Context & Execution Boundaries
- **Context Scope:** Don't scan `target/` or `Cargo.lock`. `local/` (gitignored) holds large demos, screenshots and review files: read only the files you're pointed at.
- **Locked Files:** Do not modify build/deployment configs, environment files, lint rules, or public APIs unless explicitly requested.
- **Code Edits:** Apply minimal changes directly to files. Never rewrite unchanged lines or entire files unnecessarily.
- **Ambiguity:** State critical technical assumptions once and proceed. Fail loudly on blocking errors.

### Comments & Docs
- **Comments say why, in the present tense.** No session narrative ("the user asked…", "per user", "tonight") and no decision dates: cite the issue or PR (`#214`) and let git history hold the story. Keep a date only when it dates a measurement. (#649 removed ~40 such comments.)
- **Renaming a feature, tab, setting or command:** grep comments, `docs/`, READMEs, `goldsrc-hooks/ui/Commands.txt` and `goldsrc-hooks/tools/` for the old name in the same PR. Most wrong comments found by the 2026-10 audits were left behind by a rename (#653: "Killstreaks" → Highlights).
- **A new file in `docs/` gets a line in `docs/README.md`.** A plan that's done moves to `docs/archive/` (#638, #642).
- **SteamIDs and player names are fine in tests, fixtures and docs:** they are public, shown on every server a player joins, and every real demo carries them. Keep local filesystem paths out (`C:\Users\<name>\…`); use a made-up path.

### Terminal & Shell Rules
- **Diagnostics:** Never output raw compiler logs. Provide concise, single-sentence failure summaries and direct mechanical fixes.

### GitHub Issues & PRs
- **Every PR targets `dev`** — always pass `gh pr create -B dev`. `main` is the default branch, so omitting `-B` opens the PR against `main`, which is how #325 skipped `dev`. The only PR into `main` is a `dev` → `main` release; the `Main only from dev` check refuses anything else. When syncing `main` back into `dev`, use a merge commit, never squash (#340 → #341).
- **After creating a PR**, check whether a GitHub issue already exists for the same work. If one does, link it to the PR via GraphQL, not just a `Closes #NN` line in the PR body — that text alone does not populate the issue's `closedByPullRequestsReferences`, so a "yes there's a PR" check on the issue can miss it:
      PR_ID=$(gh api repos/<owner>/<repo>/pulls/<pr-number> --jq .node_id)
      ISSUE_ID=$(gh api repos/<owner>/<repo>/issues/<issue-number> --jq .node_id)
      gh api graphql -f query='
        mutation($prId: ID!, $issueId: ID!) {
          addCloseIssueReferences(input: {issueId: $issueId, pullRequestIds: [$prId]}) {
            clientMutationId
          }
        }' -f prId="$PR_ID" -f issueId="$ISSUE_ID"
  Still include `Closes #NN` in the PR body too — the GraphQL call is in addition to that, not a replacement for it.
- **Issues close when their PR merges into `dev`**, not at release — `close_issues_on_dev.yml` does it from that same link data, since GitHub's own keywords only fire on the default branch (`main`). `[R&D]` issues and bare `(#NN)` commit-subject matches get the `on-dev` label instead, for a human call. So only link a PR as closing an issue when it finishes it; for partial work write "Part of #NN", which links nothing.
- **Before starting an issue**, search open PRs for it (`gh pr list -S "#NN"`) and grep open PR bodies: a PR's title often hides which issues it closes.
- **Merging into `dev`:** the ruleset requires the branch to be up to date, so `gh pr update-branch N`, wait for CI, then merge, one PR at a time (each merge puts the rest behind). A PR stacked on another feature branch gets no CI (`ci.yml` only runs for PRs into `dev`/`main`): say so on the PR and post local results. Head branches auto-delete on merge and GitHub retargets stacked PRs, but a manual `git push --delete` of a base branch closes the PRs stacked on it.
- **Do not create an issue after every PR as a matter of habit.** A PR that fixes something noticed and resolved in the same pass needs no separate paper trail — the PR description already is that record, and an issue closed minutes later by the very PR that created it is noise. Only file one for work you are deliberately *not* doing right now: something noticed but out of scope for the current PR, or a fix knowingly deferred rather than made. That is the actual signal — deferral, not the mere absence of a pre-existing issue.

### Changing CI (`.github/workflows/`, rulesets)
Keep checks thorough and wall-clock short; the reasons live as comments in `ci.yml` (#346, #348, #334, #654).
- **New checks go in their own parallel job**, not appended to an existing one: CI time is the longest job (~3 min), not the sum.
- **Cache Rust with `Swatinem/rust-cache`, saved only from dev** (`save-if: github.ref == 'refs/heads/dev'`). A PR can only restore its base branch's cache, so PR-saved caches are dead weight that evicts the useful ones.
- **Every cargo command takes `--locked`.** Test builds set `CARGO_PROFILE_{DEV,TEST}_DEBUG=0`. Clippy and rustfmt stay pinned to the toolchain in `ci.yml`.
- **Prefer prebuilt tools to building them** (`taiki-e/install-action` over `cargo install`), and skip installers that are slow for no gain (Playwright's `--with-deps` on Windows). Don't cache a download that is faster than restoring the cache.
- **Use ubuntu for jobs that don't need Windows.** Anything touching the app, the hooks or `#[cfg(windows)]` code stays on windows.
- **No `paths-ignore` on a workflow with required checks.** A PR it skips never gets the check and can't merge.
- **A new required check:** add it to both rulesets (`dev-protection`, `main-protection`) only after the workflow that produces it is on dev, with the exact job `name:`. Put before/after job times in the PR body.

---

## Concurrency, Rust & Memory Constraints

- **WASM Protection:** `web-analyzer` compiles `analysis` (and the `dod`/`dem-patch` parsers under it) to `wasm32-unknown-unknown`, so in those crates keep threads, `std::fs` and `std::process` behind `#[cfg(not(target_arch = "wasm32"))]`. `native`'s many wasm gates are legacy from the old egui web build; nothing compiles `native` for wasm32, so they are unchecked. Before deleting anything that looks unused, read the `#[cfg]` lines around it.
- **Hot-Path Locking:** No blocking mutexes in code that runs every game frame (`goldsrc-hooks`: `HUD_Frame`, `HUD_AddEntity`, render detours) or in Tauri event handlers. Use `std::sync::RwLock` for shared lists and atomics/channels for cross-thread signaling.
- **Telemetry Throttling:** Background progress channels must throttle update traffic to ~30fps (~33ms) using an `Arc<AtomicU32>` debouncer to prevent event loop flooding.
- **Process Lifecycles:** Never block on an external process (HLAE, `hl.exe`, FFmpeg). Poll it (`child.try_wait()`, or the process list for `hl.exe`, which HLAE starts and Studio does not own) with a sleep matched to what you are waiting for: ~16 ms when acting on timing-critical signals (OBS mode's console markers), up to ~500 ms when only watching a process stay alive. Check the `Arc<AtomicBool>` cancellation token every cycle. Make sure a child cannot outlive Studio: `.kill_on_drop(true)` for tokio children (FFmpeg), the process-tree guard in `hd/build.rs` for build tools, and `taskkill` by PID for `hl.exe`.
- **Release builds use `panic = "abort"`.** `catch_unwind` never catches anything in a shipped build, and a panic in the hook DLL takes `hl.exe` down with it. Handle bad input with bounds checks at the read site, not by catching panics. That includes conversions of wire values: `Duration::from_secs_f32` panics on a negative or NaN float, so parsers use the `try_` form and return a parse error (#655).
- **`log::` macros go nowhere.** No logger backend is registered; only `log_markdown` (the activity log) is visible.
- **Tests that need a real demo:** use `test-fixtures/ci_fixture.dem` (a real POV recording with bots, tracked; see its README) so CI runs them. For more or longer demos, take the path from an env var with the PRE install's `dod\` folder as the fallback (`DOD_ANALYSIS_DEMOS`, `DOD_ROUNDTRIP_DEMO`), never a file in `local/` (it gets cleaned, and #652's test silently lost its four demos that way); such a test is `#[ignore]`d and fails loudly when it finds no demo. Don't add more large demos to the tree without asking.
- **Error text names what failed and prints paths with `.display()`,** not `{:?}`, which quotes them and doubles every backslash (#648).
- **Analyzer cache schema:** bump `analysis::cache::SCHEMA_VERSION` when the cached format changes. Two open PRs that both bump it do not conflict in git, so whichever merges second must renumber.

---

## Domain & Engine Quirks (GoldSrc & HLAE)

- **Frame Order:** `DemoStart` (Type 2) frames must be processed *before* any `ConsoleCommand` (Type 3) frames are written, or the GoldSrc engine reads uninitialized memory.
- **64-byte Command Frames:** Command strings injected per tick must stay strictly under 64 bytes, because a demo's Type-3 `ConsoleCommand` frame carries a fixed `char command[64]` (`dem-patch`'s `parse_console_command` takes exactly 64 bytes). Stagger long absolute paths across multiple ticks. **This is not a `Cbuf_AddTextToBuffer` limit**, as this file and some error strings used to say: GoldSrc's command buffer is 16,384 bytes (`Cbuf_Init`, `hw.dll+0x272b0`) and `hw.dll` contains no "Cbuf" string at all. The distinction matters because it means the limit is a file-format property and cannot be raised — see `docs/goldsrc_hw_dll_survey.md` §3.1.
- **Packet Integrity:** Never interleave injected frames inside existing `NetworkMessage` payloads. Injected bookmarks/director frames must be written as complete, standalone frames ahead of the original packet to prevent `svc_bad` buffer overflows.
- **Decal Ring:** `r_decals` bounds the rotating decal index and evicts nothing, so lowering it strands every decal above the new limit. Set it exactly once, at demo load, from `init_commands` — never mid-demo, never as an injected `ConsoleCommand` frame (that shifts every later frame ordinal by +1). See `docs/goldsrc_dod_quirks.md`.
- **`client.dll` does not reload between demos** (measured), and `hw.dll` never reloads. `goldsrc-hooks` modules still re-check every frame, which stays correct; only their "between demos" justification was wrong. Evidence and the log-reading trap: `docs/goldsrc_dod_quirks.md`.
- **Command Tiers:** every command a user can type into Initial or Scheduled Commands falls into one of five lists in `native::patch::cfg_scan` (refused everywhere, refused only when scheduled, warned, or reported as a no-op). Read `cfg_scan.rs` for the current set, never describe it from memory, and see `docs/command_tiers.md` for why each list exists. A new pipeline-internal command the user could reach needs a tier before it ships, mirrored in `studio/src/command_suggest.js`.
- **User Config Files:** The game's own `.cfg` files are the user's. **Detect and warn, never write.** They override nothing the app assumes — a `config.cfg` ending in `exec movie.cfg` can set `mirv_fov` or `r_decals` behind the pipeline entirely. `native/src/patch/cfg_scan.rs` is read-only by construction; keep it that way. Studio's own commands are the last word: configs are never blocked, and the app never flips `config.cfg`'s read-only attribute (#478). The game's `.res` files are the user's too; the app ships its own in `dod_addon` (needs `-addons`) or `dod\dodstudio_ui`, never over `dod\resource`.
- **Both engine builds:** DoD Studio supports the pre-Anniversary and the 25th Anniversary `hw.dll`. A `goldsrc-hooks` module that touches `hw.dll` carries a per-build table (signature, stolen bytes, offsets; the pattern is `hull_trace_guard.rs`'s `BUILDS`) and a `tools/verify_*_offsets.py` that checks both (`--anniversary`). `client.dll` is byte-identical across installs, so client.dll modules need one table. Never drop pre-Anniversary support while adding Anniversary.
- **Console names (`goldsrc-hooks`):** every name comes from `console_name!` (prefix `dodstudio_`). Settings are cvars, not commands, named after the action so `1` does what the name says, default `0`. No name may be the whole start of another (the console's autocomplete swaps it on space). Diagnostics go under `dodstudio_debug_`; any fix that makes the spectated view match POV joins `dodstudio_spec_match_pov` rather than adding a cvar. A PR that adds a name must also add it to `goldsrc-hooks/ui/Commands.txt` and regenerate `studio/src/console_commands_data.js` (`goldsrc-hooks/tools/console_names.py`), or tests fail once `dev` is merged in.
- **Tauri IPC:** Every frontend `invoke()` call in `ipc_bridge.js` must implement a `.catch()` block to prevent swallowed Rust backend errors.
- **Filesystem Picking:** Force the use of `@tauri-apps/plugin-dialog` native pickers instead of text input paths to prevent string escaping vulnerabilities.
- **Visible progress:** Anything a click starts that can take more than about half a second shows it is working the moment it starts, and keeps showing it until it ends — otherwise it looks frozen. Disable the control that started it and show a progress bar with real progress (bytes, frames or items, from backend events throttled to ~33 ms, like `split_progress` or `scan_progress`), with a line saying which step it's on. When the backend can't measure a step, still say which step it is. Never leave a button looking idle while work runs. (#217's Split now first shipped with only "Splitting…" for a whole demo parse.)