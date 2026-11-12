# Audit #1107: compliance plan against docs/coding_standards.md

> **Status (2026-11-11): archived findings of the #1107 codebase audit.** The audit ran against `dev` at 648b6963;
> line numbers and file sizes below are from then and have since moved. Keep this as the record of why the
> follow-up issues exist, not as the current state of the tree.
>
> **Fixed** (all "Part of #1107", no behaviour change): #1108 (the coding standards doc and CLAUDE.md rules),
> #1115 and #1219 (free-space bugs), #1161 and #1164 (duplicated helpers), #1167 (`capture_manager.rs` split),
> #1168 and #1197 (config report moved to `native`, then split into tested steps), #1172 and #1199 (Demo Auditor
> split), #1173 (`analyzer_pane.js`, first half), #1189 (`capture_pane.js`), #1190 (in-game panel Demos tab),
> #1193, #1196 and #1210 (the giant capture and render functions), #1208 (hook `commands.rs`), #1211 (analyzer POV
> stats), #1215 (SteamID helpers) and #1217 (one hook setting type).
>
> **Still open:** #1174 (Tauri `lib.rs` split), #1178 (render autosave and the audit delete rules), #1179
> (checks and lints, needs the owner's OK), #1180 (open questions in the standards doc), #604 (decoupling
> refresh), #634 (the unchecked `wasm32` gates in `native`) and #1377 (one shared poisoned-lock helper).
>
> Two findings were corrected after the audit: the "garbled" comment in `scan_game_configs` (finding 15) is a
> complete sentence, and the two `unquote` helpers (finding 13) are not duplicates, since `launch_settings` trims
> spaces inside quotes and `cfg_scan` keeps them.

Read-only audit of origin/dev (648b6963 plus 2 later merges). Line counts are current. Every claim below was checked against the code (file and line range given); items I only skimmed are marked "not examined in depth". Method: structure listings of the largest files, a script that measures function length (SCRATCH\audit1107\longfn.py, approximate: brace-by-indent), and greps for each standards section.

## Summary

The codebase is healthier than its biggest-file list suggests. Many of the large files have one job (ipc_bridge.js is 163 thin invoke wrappers; texture_hires.rs is one hook with a long, good module doc; strings.js is data). The real problems are concentrated in a handful of places:

1. `studio/src-tauri/src/capture_manager.rs` (5,111 lines) is at least five modules: capture batch start, demo scanning, demo preview launching, game/OBS/Steam process control, orphan-preview sweep, plus 1,200 lines of tests. Other modules already import scan helpers from it (`audit_manager.rs` uses `capture_manager::progress_emit_due`). Contains a 763-line function.
2. `studio/src-tauri/src/map_manager.rs` (2,053) is misnamed: ~300 lines are a map check, ~300 are `scan_game_configs`, a single command handler that builds the whole warnings report (decision logic in the Tauri layer, the exact thing standards section 1 forbids), and ~1,100 lines are tests of that report.
3. Three giant functions in `native`: `spawn_capture_engine` (1,056 lines, 11 parameters, one unused `_hlae_path`), `build_batch_queue` (969), `run_render_job` (810). These are the biggest single complexity risk, but also the riskiest to touch.
4. `goldsrc-hooks/src/studio_panel.rs` (5,375) holds the Demos tab (list, filters, columns, place box, ~1,300 lines across the top level and `mod hook`), settings persistence, the game-menu `.res` writer, console/viewdemo toggles and a 2,750-line `mod hook`, though it already has sub-modules for the other tabs.
5. `studio/src/analyzer_pane.js` (2,180) is two panes: a demo file browser and folder tree (lines ~325-1060) and the report viewer tabs (~1198-2180). Its pure logic has no unit tests.
6. Real duplication: byte formatting written 10+ times with different rounding, `fileName` 4 definitions plus 10 inline copies although `path_display.js` exports `fileNameOf`, `fnv1a` 64-bit identical in 4 crates, and two pool-free-space functions that disagree (one dedupes drives by volume, the other double-counts a shared drive).

Everything else (comments, errors, unwraps, tests) is mostly fine; findings there are small. The standard itself needs three clarifications (section "Where the standard is unclear").

## Ranked findings table (value for effort)

| # | Finding | Where | Fix size / risk | Value |
|---|---|---|---|---|
| 1 | Pool free-space duplicated, behaviour differs (render tab double-counts drives sharing a volume) | `lib.rs` 438-456 vs `render_manager.rs` 1794-1803 | S, low | Real bug-class + dedupe |
| 2 | Byte/size formatter duplicated 10+ times, `fileName` 4+10 times, `formatDuration` twice | `studio/src/*.js` (list in detail) | S, low | Dedupe, consistent UI |
| 3 | `fnv1a` 64-bit copy-pasted in 4 live places | `analysis/src/cache.rs:618`, `studio/src-tauri/src/open_files.rs:95`, `native/src/demo_bar_res.rs:239`, `hl-demo-auditor/src/lib.rs:98` | S, low | Dedupe; the auditor one is already `pub` |
| 4 | `scan_game_configs` (300-line handler) and the whole CfgReport live in the Tauri layer; ~1,100 test lines run only under the Tauri crate | `map_manager.rs` 100-723, tests 883-2053 | M, low-med (pure move, then tests follow) | High: moves logic to `native`, `cargo test -p native` reaches it |
| 5 | `capture_manager.rs` is five modules | see detail | L, medium (split in pure-move PRs) | Highest; also a hot file for feature PRs |
| 6 | `spawn_capture_engine` 1,056 lines / 11 params / unused param; `build_batch_queue` 969; `run_render_job` 810 | `native/src/capture_engine.rs:285`, `patch/builder/mod.rs:376`, `hlcr/renderer.rs:246` | L each, high | High value, high risk: do last, with tests first |
| 7 | `analyzer_pane.js` two panes, pure logic private and untested | `analyzer_pane.js` | M-L, low-med | Medium |
| 8 | `studio_panel.rs` Demos tab and settings persistence inside the god file | `studio_panel.rs` | L, medium (game-only testing) | Medium; hot file |
| 9 | `hl-demo-auditor/src/lib.rs` (2,643): hashing, drive/lane planning, folder walk, zip listing, duplicate grouping in one file; 1,190 lines of tests | `lib.rs` | M, low (pure Rust, well tested) | Medium |
| 10 | `capture_pane.js` (1,613) holds timing diagram, OBS test UI, orphan-preview modal, cfg import, standalone launch | `capture_pane.js` | M, low-med | Medium; hot file |
| 11 | 4 `invoke()` calls outside `ipc_bridge.js` | `config_wiring.js` (obs_check_orphan, obs_recover_orphan), `project_session.js` (save/load_project_session) | S, low | Small, enforces a rule |
| 12 | Dead-code allows: 3 `#[allow(dead_code)]` in `messages.rs` (macOS arms, documented), stale `.claude/worktrees/` (3 full repo copies, gitignored) | `messages.rs` 523-533 | XS | Small |
| 13 | `unquote` written twice with different results | `cfg_scan.rs:1236`, `launch_settings.rs:250` | XS | Small |
| 14 | Lint config allows `too_many_arguments` workspace-wide; comment says 8 and 12 params but `spawn_capture_engine` has 11 and `build_batch_queue`-style long functions are unflagged | `Cargo.toml:85` | XS (but "lint rules change only with owner's OK") | Prevents drift |
| 15 | Stale/garbled comment in `scan_game_configs` | `map_manager.rs` ~455-470 ("see *other* is a real ... question, left alone", "decal_flush_is_noop below" which is defined above) | XS | Folded into #4 |
| 16 | `native` wasm gates (152 `wasm32` mentions) nothing compiles | `native/src` | M, medium (read each `#[cfg]`) | Already tracked in #634 |
| 17 | Modules over 250 lines with no `#[test]` | list in Tests section | varies | Mostly fine (parsers covered through other tests) |

## Per-area detail

### Frontend (studio/src)

- `main.js`: 501 lines, budget 515. Fine. It is already close to the limit; the first batches must not add to it.
- `analyzer_pane.js` 2,180 lines, two jobs. Job 1, browser: `shortenPath`, `parentDirOf`, `ancestorChain`, quick links, `flattenVisibleTreeRows`, tree open/close/keyboard, `setCurrentDir`, `sortedFilteredDemos`, demo table (lines ~325-1060, ~730 lines). Job 2, report viewer: summary, scoreboard, player details, team, timeline, kill map, rounds and chat tabs (~1198-2180). Move job 1 to `analyzer_browser.js` with `createAnalyzerBrowser({...})` and its pure helpers (`passesDemoFilter`, `sortedFilteredDemos`, `demoDateISO`, path helpers, `mortalityLifespans`, `groupConsecutiveWeapons`, `formatGameTime`) into a `_logic.js` with Vitest tests. Only `demo_folders.test.js` imports this file today.
- Only one `_logic.js` exists in 300 files (`obs_capture_test_logic.js`). Many pane files have no test file of their own (capture_pane, hd_pane, render_pane, detail_pane, master_pane, queue_scan, project_demos, settings_sync, config_wiring). Playwright e2e covers the UI paths; the pure functions inside are untested. Not all are worth extracting; the analyzer ones above are the clearest.
- `capture_pane.js` 1,613: `renderTimingDiagram`, OBS connection test UI (`runObsConnectionTest`, `renderObsReport`, `launchObsAndRetry`), orphan-preview modal (`initClearPreviewsModal`), cfg import, standalone launch, plus `refreshLaunchGuard` (213 lines, lines 402-615). `obs_capture_test.js` already exists as the pattern. Extract `capture_obs_test_ui.js`, `orphan_previews_ui.js`, `timing_diagram.js`.
- Duplicates (verified):
  - Byte formatting: `batch_results.js:42`, `setup_checklist.js:138`, `hd_progress.js:11`, `analyzer_damage.js:14`, `audit_duplicates.js:251`, `auditor_pane.js:231`, `capture_pane.js:593,683,741`, `folder_pools.js:52`, `render_pane.js:291`, `render_while_capturing.js:119`, `take_cleanup.js:163`. They differ in decimals and TB handling, so UI shows the same size differently per tab. One `format_bytes.js` with the variants as options.
  - File name from a path: `path_display.js:fileNameOf` exists, yet `batch_catch_up.js:75`, `combine_clips.js:20`, `render_history.js:46`, `split_pane.js:23` redefine it and 10 more sites inline `split(/[\\/]/).pop()` (`audit_duplicates.js:132`, `crosshair_file.js:51`, `demo_rename_ui.js:334`, `detail_pane.js:64`, `project_demos.js:32`, `project_file_type.js:34`, `project_session.js:103`, `setup_checklist.js:41`).
  - `formatDuration`: `rescan_estimate.js:82` vs a wrapper in `analyzer_pane.js:169` over `time_format.js:durationText`. Different outputs, same name.
- IPC: 163 `invoke(` in `ipc_bridge.js`, 165 `.catch`. Compliant. Exceptions above (#11).
- `ipc_bridge.js` is one job (wire wrappers); no split needed. `strings.js` is data (#237).

### Tauri layer (studio/src-tauri/src)

- `capture_manager.rs` 5,111 lines (line ranges from the current file):
  - Batch start: payload types and `config_from_payload` (74-560), `check_takes`/`verify_capture_takes*`/`finish_batch` (846-1135), `start_capture_batch_impl` (1173-1936, 763 lines).
  - OBS connection/orphan commands: 549-770 (belongs with `obs_test_manager.rs` or a new `obs_commands.rs`).
  - Demo scanning: `SerializedDemo`, `ScanOutcome`, `ListScan`, `VisitedFolders`, `list_scan_demos`, `locate_missing_demos`, `skip_known`, `scan_directory_impl` (377 lines): 1950-3130 (~1,200 lines). Used by `lib.rs`, `audit_manager.rs` (progress throttle). New `scan_manager.rs`.
  - Demo preview and game control: `patch_bookmark_previews`, `launch_demo_preview`, `send_preview_to_running_game`, `watch_for_error_dialogs`, `running_game_pids`, `launch_standalone_game`, `check_running_game`, `close_running_game`, `launch_obs`, `start_steam`, `kill_engine_processes`: 3134-3780. New `game_process_manager.rs` (`preview_in_place.rs` and `batch_catch_up.rs` already import `running_game_pids` from here).
  - Orphaned preview sweep: 3785-3910. New or into the preview module.
  - Tests: 3915-5111, 57 tests, to travel with their code.
  - Pure candidates for `native`: `collapse_whitespace_outside_quotes` (3445), `folder_key`/`VisitedFolders` logic, `scan_status_line` family, `obs_drop_rate_payload`, `check_takes` decisions.
- `map_manager.rs` 2,053: rename and split. `check_demo_maps` and `download_map` stay (they are map commands; `native/src/patch/map_check.rs` and `map_fetch.rs` already hold the logic). `scan_game_configs` and the ten `*Row` structs (100-723) become `native::patch::cfg_report` (the logic: effective settings, unseen, banned, noop, hazards, value rules, exec follow) with the Tauri command a 20-line wrapper. `roll_floors` (749-826) builds a `PatcherConfig` and maps a report; it belongs in `native` next to `builder::roll_floors`. Tests move with it (about 1,100 lines). Stale comment fix included.
- `lib.rs` 1,178 lines: besides `run()` it holds command handlers of unrelated areas: settings, project session files, audit, scan, HLAE FFmpeg report (`hlae_ffmpeg_report` 539-600 is 60 lines of decisions: agrees_with_app, app_ffmpeg_problem, no_override; belongs in `native::shared::hlae_ffmpeg` with tests), export-pool space, capture-output diagnostics, analyzer (`index_demo_players`, `analyze_demo_full`), `load_map_overview`. The pattern `players_tab.rs`/`match_group_cmd.rs` shows the way: one `*_cmd.rs` per area. Mostly mechanical.
- `render_manager.rs` 2,240: queue/scheduler/autosave/recovery in one manager; `spawn_scheduler` 197 lines, `apply_render_update` 183, `scan_and_merge` 169. One area (rendering), several concerns. Candidates: `render_autosave.rs` (autosave_path, write/save_autosave, recovery_summary, recovered_*: 83-90, 623-684, 1806-2020). Medium priority; `scan_merge` is already a sub-module.
- `settings_manager.rs` 1,234: 50 `default_*` functions (352-500) beside `AppSettings`. Fine (serde needs them); could use a `defaults` sub-module; low value.
- `audit_manager.rs` 1,249: `run_demo_audit_impl`, delete-with-verification (`check_same_contents`, `compare_readers`, `delete_audit_files_impl`), `reveal_in_explorer_impl` (780: an OS shell helper in the auditor file). The comparison/deletion rules are decisions about demos and belong in `hl-demo-auditor` (testable there). Medium.
- `hd_manager.rs` 381 lines, no tests: thin command handlers; fine.

### native

- Long functions (approximate measure): `capture_engine::spawn_capture_engine` 285-1340 (1,056 lines, 11 parameters, `_hlae_path` unused and passed by its one caller, `capture_manager.rs:1920`; a `macro_rules!` inside a closure inside a thread spawn), `patch::builder::build_batch_queue` 376-1345 (969), `hlcr::renderer::run_render_job` 246-1056 (810), `patch::engine::patch_with_stages` 171 (447), `decal_probe::probe_decal_offsets` 1327 (396, R&D probe living in `src/patch/`; standard section 7 says probes go in `native/examples/`), `overview::scene::build_until` (328), `hlcr::scanner::scan_folder_background` (261), `patch::highlevel::patch_demo_highlights` (204).
  - `build_batch_queue` has a large test file (`builder/tests.rs`, 2,070 lines), so it is the safest of the three to split. `spawn_capture_engine` has fewer tests; extract pure decisions first (the pre-launch headroom check, the "how the end of a batch was noticed" logic, the log lines).
  - The workspace `Cargo.toml:85` comment justifying `too_many_arguments = "allow"` says "spawn_capture_engine (8) and the CLI entry point (12)": it has 11 now.
- `decal_probe.rs` 2,115 lines: it is a probe (offset discovery); check whether `src/` is the right home (not examined in depth).
- `patch/cfg_scan.rs` 2,451: one job (scan and classify config commands); the tier lists are documented. Fine as one file; if split, tier tables and parsing are the natural seam, but not recommended now.
- `native/src/sys/disk.rs` has `volume_key`/`group_by_volume`; the pool-space sum belongs here (see #1).
- Duplicates: `fnv1a` 64-bit (above); `unquote` (`cfg_scan.rs:1236` returns `String`, trims quotes only; `launch_settings.rs:250` trims twice). Not identical.
- `wasm32` gates: 152 mentions in `native/src` that nothing compiles (#634, CLAUDE.md says to read the `#[cfg]` before deleting). Not touched here.

### analysis / dod / dem-patch

- Files over 1,000 lines (`dem-patch/src/types.rs` 1,974, `demo_parser.rs` 1,109, `analysis/src/lib.rs` 1,905, `match_group.rs` 1,693, `dod/src/lib.rs` 1,696, `objective.rs` 1,241): parser/type files with one job each from the listings; `analysis/src/lib.rs` (1,905) holds `Analysis`, state and the entry point and is worth a look (not examined in depth). `dem-patch` is a vendored LGPL fork: leave alone.
- `dem-patch/src/types.rs` and `demo_parser.rs` have no `#[test]` in-file; covered by `dem-patch` round-trip probes and the CI fixture (checked: `mangle_probe` is the safety net per #225).
- `Duration::from_secs_f32/f64` (rule from #655): only 3 uses in non-test code scan, all inside `#[cfg(test)]` modules or test helpers (`clock_offset.rs:234`, `match_group.rs:955`, `objective.rs:958`). Fine.

### goldsrc-hooks

- `studio_panel.rs` 5,375. Order: constants and slot tables (1-715), pure Demos-tab helpers (`DemoInfo`, `civil`, `date_text`, `passes`, `folder_line`, `display_name`, `demo_count_text`: 719-1040), settings file logic (`settings_line`, `settings_with`, `migrated_settings`: 1073-1125; `apply_saved_settings`, `save_setting`, `reset_settings`, `ensure_res`, `fnv1a`: 1590-1760), viewdemo/console-in-panel toggles (1130-1300), game menu writer (1312-1380), `mod hook` (1765-4522) with the Demos tab (`filter_demo_list`, `add_demo_columns`, `stamp_demo_rows`, `fill_place_box`: 2023-2730, ~700 lines) beside the generic window plumbing (`build`, `borrow`, `fit_pages`, `give_back`), and 750 lines of tests. Seven other tabs already have their own file under `studio_panel/hook/`; the Demos tab does not. Suggested: `studio_panel/demos_tab.rs` (pure helpers + hook part), `studio_panel/saved_settings.rs`, `studio_panel/panel_toggles.rs`. The `civil` date function exists only here (no duplicate).
  - Risk: needs an in-game check on both PRE and POST. Pure-move PRs keep that to one smoke test each.
- `commands.rs` 2,233 (`install` 327 lines, `status_text` 227, `poll` 183), `deathmsg.rs` 2,050 (+ `deathmsg/font.rs` 1,990), `demo_list_folders.rs` 1,846, `engine.rs` 1,888: not examined in depth. `commands.rs` `install` at 327 lines is a registration list; likely fine.
- `texture_hires.rs` 4,848: one subject (the GL_LoadTexture2 swap), doc is excellent. It does contain several swap arms (world, models, sprites, HUD sprites, decals, sprays, skies, detail, stale-fix); an arm-per-file split would be possible but is not required by the standard. Not recommended now.
- `fnv1a` 32-bit exists in `map_text.rs:251` and `studio_panel.rs:1693`: the hook crate is a separate 32-bit target; a small `hash.rs` in the hook crate would remove one copy. Low value.
- `goldsrc-hooks/src/lib.rs` 589 and `studio_panel/hook/streaks_tab.rs` 468 have no tests; streaks_tab is game-UI code (fine).

### hl-demo-auditor

`lib.rs` 2,643 lines: hashing (`Fnv1a`, `head_hash`, `tail_hash`, `content_hashes`: 98-135, 730-810), drive/lane planning (`drive_of`, `seek_penalty`, `plan_lanes`, `Readers`: 134-365), folder walking (`scan_dir*`, `walk_roots`, `walk_pool`, `is_windows_folder*`: 364-720), zip listing (`list_zips`, `read_zip_demos`: 922-1095), duplicate grouping (`find_duplicates_with`, 299 lines, `split_by_full_hash`), plus 1,190 lines of tests (1455-2643). `archive/`, `backup.rs`, `key_cache.rs` are already split. Suggest `walk.rs`, `hash.rs` (key/hash types), `lanes.rs`, `zips.rs`, leaving `lib.rs` as the re-exporting entry and `find_duplicates_with`. Pure Rust, well tested: a safe pure-move.

## Per standards section

### 1 Organisation
Findings: capture_manager.rs, map_manager.rs, studio_panel.rs, analyzer_pane.js, capture_pane.js, hl-demo-auditor lib.rs, lib.rs (tauri), audit_manager.rs (above). Logic in the wrong layer: `scan_game_configs` and `roll_floors` (map_manager.rs), `hlae_ffmpeg_report`, `export_pool_space`, `capture_output_diagnostics` (lib.rs), audit delete/compare rules (audit_manager.rs). Frontend `_logic.js`: only 1 exists. No `utils`/`helpers`/`misc` files found (checked with `ls`); `messages.rs` is the one grab-bag-shaped file, but it is the Tauri error text module (a reasoned home, see #237). Checked fine: `ipc_bridge.js`, `strings.js`, `cfg_scan.rs`, `texture_hires.rs`, `main.js` (501/515).

### 2 Functions
- One task per function: the 8 long functions above. Tally of functions over 250 lines outside tests: about 14 in 7 crates (script output in SCRATCH\audit1107\longfn.py; top 30 were reviewed, the top ones confirmed by reading the opening and closing lines). The standard says length alone is weak; the real signal is nesting and branching, which I did not measure (no tool in the repo does).
- Parameters: `spawn_capture_engine` 11 (one unused), `build_batch_queue` 3, `start_capture_batch_impl` 3 but 763 lines. 8 explicit `allow(clippy::too_many_arguments)` in `hd/build.rs`, `behind_imports.rs`, `font.rs`, `map_sounds.rs`, `objicons/scale.rs`, `hl-demo-auditor/lib.rs`, and the workspace-wide allow.
- Flag arguments, hidden side effects: not surveyed systematically. Spot check: `check_takes` (capture_manager.rs:1007) and `verify_capture_takes_settling` (899) are named `check_`/`verify_` and `record_capture_settings` writes; `finish_batch` is named for what it does. Names mostly honest. `check_demo_maps` is read-only. `watch_for_error_dialogs` starts a thread (name says watch). Not exhaustively verified.

### 3 Names
Spot checks only. `map_manager.rs` holds config scanning, not just maps (misnamed). `capture_manager.rs` holds scanning, previews, process control. "streak" vs "highlight": the UI says highlight; Rust types still say `CaptureStreak`, `SerializedStreak`, `KillStreak`, `scan_demo_for_highlights_with_analysis`. A mixed vocabulary that the standard calls out; renaming `Streak` to `Highlight` across IPC is large and touches saved projects; recommend leaving until a deliberate decision (there is a `Streak` -> Highlights note in CLAUDE.md for strings). Unit suffixes in fields (`pre_roll_seconds`, `offset_seconds`) are good. JS: single-letter variables in short callbacks only (spot check).

### 4 Data and types
Strings as enums: `kind: "hazard".to_string()` in `CustomCommandWarning` (map_manager.rs, scan_game_configs), `status: &'static str` for "ok|not_absolute|..." (lib.rs `CaptureOutputDiagnostic`), `relation: String` compared to "After" in `roll_floors` and `scan_game_configs` (`c.relation == "After"`), `mode: 'workspace'` in project session. These cross the IPC boundary where the standard asks for enums. Small, per-struct fixes; fold into the batch that touches each file. Magic numbers: `1_073_741_824.0` in render_manager.rs `export_pool_free_gb`, `1024 * 1024 * 1024` in 8 JS files (covered by the formatter dedupe).

### 5 Errors
- `.display()` paths: grep for `{:?}` near path/dir/file format strings in native and src-tauri found none. Checked, fine.
- Wire-value panics: `Duration::from_secs_f*` only in tests (above). Checked, fine.
- `let _ =`: 439 sites in non-test Rust (native, src-tauri, hooks, analysis). Most are `tx.send(...)` on a channel whose receiver may be gone, which is the acceptable case, but the standard wants a comment saying why. Not practical to comment 439; see "unclear" below.
- Empty `catch {}` in JS: 0 empty catch blocks; 28 `.catch(() => {})`/`catch {}` forms, not individually checked.
- Defined-away errors: not surveyed.
- Typed errors in native: many functions return `Result<_, String>` (the Tauri layer, by design) and `std::io::Error` in `build_batch_queue`; the standard's `thiserror` enums in native were not found being used in the places I read (`build_batch_queue` returns `std::io::Error` for non-IO failures). Not examined in depth.

### 6 Duplication
`fnv1a` x4 (+2 32-bit, +2 in examples), pool free-space x2 with different behaviour, byte formatter x10+, `fileName` x4 + 10 inline, `formatDuration` x2, `unquote` x2, `civil`/date none. "Third time share it" applies to bytes, fileName, fnv1a.

### 7 Dead code
- 46 `allow(dead_code)`; most are `cfg_attr(not(target_arch = "x86"), allow(dead_code))` for the 32-bit hook crate and carry an explanatory comment (checked `hint_panel.rs`, `engine.rs`); `detail_cache.rs:54-58` keeps three documentation-only fields (`texture_at`, `hscale_at`, `vscale_at`) with a comment; `messages.rs:523-533` documented macOS arms. None found that was genuinely dead among the ones read; I did not read all 46.
- `.claude/worktrees/` holds 3 gitignored full repo copies (agent-a38615bea8e2ab2fb, agent-afdcf15ab8269b784, recursing-matsumoto-28dd8e). They pollute searches (grep for `fnv1a` returns them first) and the Glob/Grep tools; delete with `git worktree remove` when no longer needed.
- `_hlae_path` in `spawn_capture_engine` is a dead parameter.
- `decal_probe.rs` (2,115 lines, R&D) in `native/src/patch/` where the standard says probes belong in `native/examples/` (not examined in depth whether the module is still wired in).
- `native/examples/` probes 1,467 and 1,494 lines: allowed home.
- `wasm32` gates in native: #634.
- Commented-out code: no scan done beyond the greps above.

### 8 Comments
- Dates and narrative: 60 comment lines mention a year-month date or "the user asked". Most date a measurement (allowed). Narrative ones found: `native/src/capture_engine.rs:1104` ("2026-09-29, a cancel 1.0s in"), `sys/dialogs.rs:5,101` ("Seen on 2026-09-29", "Seen live"), `patch/decal_strip/flush_job.rs:497,889` and `decal_strip/mod.rs:458` use "the user asked" (that is behaviour language, not session narrative), `capture_engine.rs:1461`. A handful, not a pattern. `native/src/lib.rs:183` ("359 MB of it on one machine, 2026-09") is a measurement. Low value; batch into the file that is being touched anyway.
- Stale/garbled comment: `map_manager.rs` ~455-470 in `scan_game_configs` (sentence ends mid-thought "see *other* is a real ... question", and refers to `decal_flush_is_noop below` which is defined above it). Found by reading, not by name.
- Interface comments: spot checked `capture_manager.rs` (only `start_capture_batch_impl` has a one-line doc and the 90-line command payload has field docs), `spawn_capture_engine` has no doc comment on the function despite 11 parameters (comments on individual parameters instead). `scan_directory_impl`, `scan_game_configs` (comment above explains one clause, not the contract). Standard section 8 says every public item has a doc comment; this was not measured repo-wide, `missing_docs` would measure it (see tools).
- `BUDGET` comment in `main_js_budget.test.js` is accurate.

### 9 Tests
- Logic that lives where `cargo test -p native` cannot reach it: `scan_game_configs` report (1,100 test lines run only in the Tauri crate, which is slower to build), HLAE-FFmpeg report (untested), `export_pool_space`/`export_pool_free_gb` (untested), `capture_output_diagnostics` (checked, no test), `collapse_whitespace_outside_quotes` (it has a test in the file's tests mod: fine).
- Modules over 250 lines with no `#[test]` and no sibling test module: `dem-patch/src/types.rs`, `demo_parser.rs`, `netmsg_doer/mod.rs`, `temp_entity/mod.rs`, `delta.rs` (covered by round-trip probes and fixture), `analysis/src/player.rs` (330), `native/src/overview/level.rs` (394), `studio/src-tauri/src/hd_manager.rs` (381), `native/src/hlcr/scanner/media_probe.rs`, `bin/strip_decals.rs`, `analysis/src/bin/parse_demo.rs`, `goldsrc-hooks/src/lib.rs`. `overview/level.rs` and `player.rs` are the ones worth a closer look for logic.
- Frontend: 114 test files for ~190 modules; pane files covered by e2e (104 files in tests/e2e); pure logic inside analyzer_pane, capture_pane (`seeksBetweenClips`, `describeProblemPaths`, `formatPreviewSize`, `batchSettingsPayload`) and master_pane is untested.
- Lost `#[test]`: not scanned. A cheap scan: every `fn` in a `mod tests` without a preceding `#[test]`/helper naming. A script could do it (see tools).
- Test names: sampled map_manager tests; they state behaviour. Good.

### 10 Changes and PRs
Process, not code. Findings: none in code. For this plan: pure moves in their own PRs; refactor and behaviour change separate. The ~100-line comfortable review size cannot hold for file splits (a move is thousands of lines), so the plan splits by file and uses a "pure move" description plus `git diff --color-moved` / `.git-blame-ignore-revs`.

### "Checked by tools" list: what each candidate would catch today
- `clippy::too_many_arguments` at a lower threshold (5): would flag `spawn_capture_engine` (11, currently allowed workspace-wide at `Cargo.toml:85`), the 8 explicit allows (`hd/build.rs`, `font.rs`, `map_sounds.rs`, ...), likely 20-40 more. The workspace-wide allow must be replaced by per-site allows first: a one-PR change, but "lint rules change only with owner's OK".
- `clippy::cognitive_complexity`: would flag the 8 giants; threshold needs tuning (default 25 is likely to flag hundreds). Warn-only first.
- `clippy::fn_params_excessive_bools`: not measured.
- `clippy::unwrap_used`: would flag the 44 `lock().unwrap()` in `render_manager.rs` alone (71 across native+src-tauri non-test). With `panic = "abort"` in release, a poisoned mutex cannot occur: the standard's own reasoning does not apply to mutex locks. Needs a standard exception ("`lock().unwrap()` is fine because a panic aborts") or a `lock_or_recover` helper, else this lint produces noise. See "unclear".
- `clippy::expect_used`: same, plus the `.unwrap()` after `thread::Builder::spawn` (`capture_engine.rs:1340`).
- `clippy::missing_docs_in_private_items`: noisy; `missing_docs` on pub items of `native`, `analysis`, `dod` would measure section 8's contract rule. Not run (needs a build; read-only audit).
- ESLint `max-params`, `no-unused-vars`, `no-magic-numbers`: `no-unused-vars` is the one with certain value; there is no ESLint config in `studio/` (checked with `ls`: none found), so this is new infrastructure. `no-magic-numbers` would flag `1024 * 1024 * 1024` in 8 files.
- Cheap custom tests that already fit the repo's style (like `main_js_budget.test.js`): (a) a Vitest scan that fails on `invoke(` outside `ipc_bridge.js` (4 sites to fix first); (b) a Vitest scan that fails on a new `split(/[\\/]/).pop()` outside `path_display.js`; (c) a Rust test scanning for `fn` inside `mod tests` without `#[test]` (CLAUDE.md #743 rule); (d) per-file budget list as a guideline, not a gate.

### Where the standard is unclear or does not fit
1. `lock().unwrap()`: the unwrap rule says "except on an invariant the code just established, with a comment". Mutex locks under `panic = "abort"` cannot be poisoned, so 71 sites cannot fail; commenting each is silly. Add a one-line exception or a helper.
2. `let _ = tx.send(..)` ("say why in a comment"): 439 sites, almost all channel sends to a receiver that may be gone. Suggest the standard says "a send to a closed channel needs no comment" or provide `fn ignore_closed(...)`.
3. "Size is a prompt": the 1,000-line Rust prompt fires on tests-heavy files (map_manager is 1,100 test lines) and on generated-style tables; counting inline tests makes sense for the review, but the prompt should say "non-test lines" or the audit gets false alarms (e.g. `builder/tests.rs`, 2,070, is a test file). Also "~100 changed lines per PR" cannot hold for file splits; section 10 should say pure moves are exempt if the diff is mechanical and marked.
4. Section 2 says one task per function but gives no handle for 1,000-line orchestration functions (spawn_capture_engine) that run a pipeline in order; the "fourth, short one that runs them in order" advice applies, but there is no guidance on a state struct for the shared locals. A short worked example would help.
5. "Typed errors in native/analysis/dod" vs many `Result<_, String>` in `native` helper code: the standard does not say when a `String` error is acceptable inside `native` (e.g. patch stages that only log).
6. The "Names" section wants one name per idea and says streak vs highlight; the code has about a hundred `Streak` identifiers on the IPC boundary and in saved project files, so a rename needs a migration (analysis cache `SCHEMA_VERSION`, project files). The standard should say existing serialized names stay until a migration PR.

## Things checked and found fine

- `ipc_bridge.js`: 163 invokes, 165 `.catch`, one job.
- `main.js` 501/515 and wiring-only (I read the budget test, did not read every line of main.js).
- `strings.js` is data, tracked in #237.
- `cfg_scan.rs` (2,451): one job, tier tables documented, read-only by construction per CLAUDE.md.
- `texture_hires.rs` (4,848): one subject, module doc explains why and how; not a candidate.
- `render_manager.rs` has a `scan_merge` sub-module and `render_scan_merge.rs`; structure is reasonable though large.
- `hd_manager.rs`, `settings_manager.rs`: thin or declarative.
- Path debug-printing (`{:?}`) in error text: none found near path/file/dir strings.
- `Duration::from_secs_f*` panics on wire values: none outside tests.
- `#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]` in hooks: justified with comments; the `detail_cache.rs` and `messages.rs` allows carry reasons.
- `goldsrc-hooks` per-feature file organisation is good (one file per feature); only `studio_panel.rs` is oversized.
- No `utils.rs`/`helpers.rs`/`common.rs`/`misc.rs` files.
- Test names sampled in `map_manager.rs`, `capture_manager.rs`: behaviour-stating.

Not examined in depth (so no verdict): `goldsrc-hooks/src/commands.rs`, `deathmsg*.rs`, `demo_list_folders.rs`, `engine.rs`, `analysis/src/lib.rs`, `match_group.rs`, `native/src/hlcr/renderer.rs` beyond `run_render_job`, `decal_probe.rs`, `overview/*`, test names outside the two files sampled, magic numbers in Rust, nesting depth anywhere.

## Plan

Start point: the open PR list is empty, so the first batches can touch the hot files without conflicting. Batch estimates are my time plus CI, not review time (the owner's own game test is separate).

### Order and why

Hot files first (they are the ones feature PRs touch: `capture_manager.rs`, `studio_panel.rs`, `main.js`, `ipc_bridge.js`, `capture_pane.js`, `lib.rs`). Do their pure moves while there are no open PRs; each lands in under a day, so new feature branches start from the split code. Small dedupe batches go first because they are quick wins and make later moves smaller. The giant-function refactors go last because they change behaviour surface and need tests first.

| Batch | What | Rough time | Risk | Depends on |
|---|---|---|---|---|
| B1 | Quick fixes: pool free-space one function in `native::sys::disk` (volume dedupe), `_hlae_path` removal, fix stale comment, 3 JS `invoke()`s moved into `ipc_bridge.js`, `fnv1a` shared (one pub fn in `dem-patch` or `native`), `unquote`, delete `.claude/worktrees` | 2-3 h | low | none |
| B2 | Frontend dedupe: `format_bytes.js`, `fileNameOf` everywhere, `formatDuration` naming, with tests | 2-3 h | low | none |
| B3 | Tauri `capture_manager.rs` split, as separate pure-move PRs: (a) tests-with-code split, scan to `scan_manager.rs`; (b) game/OBS/Steam/preview to `game_process_manager.rs`; (c) OBS commands to `obs_commands.rs`. Each PR is `git mv` + `use` fixes, nothing else | 3 PRs x 2-3 h | low (compiler-checked) | B1 |
| B4 | `map_manager.rs`: move `scan_game_configs` report and `roll_floors` logic into `native::patch::cfg_report` (pure move, then commit the ten `*Row` structs; Tauri command becomes a wrapper); rename the file to what remains; fix the comment | 3-4 h | low-med | B3 not required; do right after |
| B5 | `lib.rs` (tauri): `hlae_ffmpeg_report` to `native::shared::hlae_ffmpeg` with tests; command groups to `*_cmd.rs` files (settings, analyzer, diagnostics) | 3 h | low | B1 |
| B6 | `hl-demo-auditor/src/lib.rs` split into `walk`, `hash`, `lanes`, `zips`; audit delete/compare rules from `audit_manager.rs` into the crate | 3-4 h | low (pure Rust, well tested) | B1 (fnv1a) |
| B7 | `analyzer_pane.js` split (browser vs report), pure helpers to `analyzer_logic.js` with Vitest; `capture_pane.js` three extractions | 4-5 h | low-med (e2e app-boot each step) | B2 |
| B8 | `studio_panel.rs`: Demos tab, saved settings, panel toggles into `studio_panel/`; requires a PRE and POST smoke test | 4-5 h + user test | medium (game-only) | none; do while the game-test queue is empty |
| B9 | `native` giants: `build_batch_queue` (largest test net) first, then `spawn_capture_engine`, then `run_render_job`: extract pure decisions with tests, keep the orchestration function short | 3 sessions of 4-6 h | high | B3, tests added first as step 1 of each |
| B10 | `render_manager.rs` autosave extraction; `audit_manager.rs` leftover; `settings_manager` defaults sub-module | 3 h | low | B3 |
| B11 | Tooling after the batches: see below | 2 h | low | B1-B9 |
| B12 | Optional/deferred: `Streak` vs `highlight` rename (needs migration), `wasm32` gates (#634), `decal_probe.rs` to `examples/`, `commands.rs`/`deathmsg` deeper audit | n/a | | decide later |

### How to avoid merge pain

1. One area at a time; one PR open per hot file. Never run B3, B5 and B8 in parallel: they touch different hot files but all call into `capture_manager`/`lib.rs`.
2. A pure move is its own commit/PR, with "pure move: no behaviour change; verify with `git diff -M --color-moved=dimmed-zebra`" in the PR body. Behaviour fixes (B1's disk bug, B4's comment fix) go in separate PRs from the moves.
3. Add each big mechanical move commit to `.git-blame-ignore-revs` (only after merging, the hash is the merge-commit's or squash commit's). Merge these PRs with a merge commit or a squash only after checking the hash is stable: squash merges produce one hash that is the right thing to list.
4. After each Tauri/native batch: `cargo test --workspace --locked --exclude goldsrc-hooks --exclude web-analyzer`, `rustup run 1.98.1 cargo clippy ... -D warnings`, and `cargo fmt --all --check`. After each frontend batch: `npm run test:unit` and `CI=1 npx playwright test tests/e2e/app-boot.spec.js` (plus the e2e spec of the pane touched).
5. Land the hot-file moves within a day each, and tell the owner not to branch features from `dev` mid-batch (or rebase them straight after: `git rebase` handles whole-file `git mv` with `-M` detection when no content changed; keep the move commit free of content edits so the rename detection works).
6. Keep `main.js` untouched except import lines; do not raise `BUDGET`.

### Enforce after each batch so it does not drift back

- After B2: Vitest scan failing on `split(/[\\/]/).pop()` and `1024 * 1024 * 1024` outside the shared helpers.
- After B1: Vitest scan failing on `invoke(` outside `ipc_bridge.js`.
- After B3/B4/B5: a Rust test that fails if `studio/src-tauri/src/*.rs` (excluding the tests module) exceeds a ceiling per file as a guideline-style test like `main_js_budget.test.js` (tell the reviewer it is a prompt, raise with a reason). A generic `file_budget.test.js` listing a ceiling per hot file, ratcheted down after each batch.
- After B9: `clippy::too_many_arguments` at 7 with the workspace-wide allow removed and per-site allows only where documented; `cognitive_complexity` as warn.
- After B7: ESLint `no-unused-vars` and `max-params` (new config; owner's OK needed).
- Add the "fn without #[test] in mod tests" scan as a Rust test (CLAUDE.md rule #743).
- Add the standards exceptions from "unclear" (mutex lock, channel send) before turning on `unwrap_used`.

### Start here (first session)

1. B1 (all small): the pool-space bug is the one finding that users can see.
2. B2 immediately after (frontend dedupe, with tests).
3. B3(a): move demo scanning out of `capture_manager.rs` as a pure move; it unblocks `audit_manager.rs` importing from a capture file.
4. B4 if time remains (decision logic out of the Tauri layer is the most standards-relevant fix).
5. Do not start B8 or B9 until the owner can run an in-game test or the tests-first step is done.

## Proposed follow-up issues (in plan order; titles in this repo's style; not created)

1. `[Chore] One function for the export-pool free space, and shared helpers instead of copies`
   Pool space exists twice (lib.rs dedupes drives by volume, render_manager.rs does not), plus fnv1a x4, `unquote` x2, an unused `_hlae_path` parameter and 4 `invoke()` calls outside ipc_bridge.js. One PR, with a test for the shared-volume case.
2. `[Chore] One byte-size formatter and one file-name helper in the frontend`
   10+ copies of the size formatter (different rounding per tab) and 4+10 copies of "file name from a path"; `path_display.js` already has `fileNameOf`. Add the shared formatter with tests and replace the copies; add a Vitest scan so new copies fail.
3. `[Chore] Split capture_manager.rs by area: scanning, game and OBS control, previews`
   5,111 lines holding five areas; `audit_manager.rs`, `batch_catch_up.rs`, `preview_in_place.rs` import from it. Three pure-move PRs (no behaviour change), tests travel with their code.
4. `[Chore] Move the config-warnings report out of map_manager.rs into native`
   `scan_game_configs` (300 lines of decisions), its ten row structs and `roll_floors` live in the Tauri layer; 1,100 lines of tests run only there. Move to `native::patch::cfg_report`, keep the command a wrapper, rename the file to what is left, fix the garbled comment.
5. `[Chore] Tidy lib.rs in the Tauri layer: HLAE FFmpeg report to native, command groups into their own files`
   `hlae_ffmpeg_report` is untested decision logic in lib.rs; settings, analyzer and diagnostics commands each get a `*_cmd.rs` like `players_tab.rs`.
6. `[Chore] Split the demo auditor's lib.rs: walking, hashing, drive lanes, zip listing`
   2,643 lines, 1,190 of them tests; pure Rust and well tested. Also move the audit delete/compare rules from `audit_manager.rs` into the crate.
7. `[Chore] Split analyzer_pane.js into the demo browser and the report tabs, and test the pure parts`
   Two panes in one 2,180-line file; path/filter/sort/lifespan helpers are private and untested. Also pull the OBS test UI, orphan-preview modal and timing diagram out of capture_pane.js.
8. `[Chore] Move the Demos tab and saved settings out of studio_panel.rs`
   The only tab without its own file under `studio_panel/`; about 1,500 lines. Pure moves; needs a PRE and POST smoke test.
9. `[Chore] Break up the three giant functions: build_batch_queue, spawn_capture_engine, run_render_job`
   969, 1,056 (11 parameters) and 810 lines. Tests first, then extract pure decisions; one PR per function, `build_batch_queue` first (best test net).
10. `[Chore] Move render autosave and the leftover audit rules out of their managers`
    `render_manager.rs` autosave/recovery (about 400 lines) and `audit_manager.rs` delete/compare rules each get a module.
11. `[Chore] Turn on the checks that keep these rules: file budgets, invoke scan, test-attribute scan, lint thresholds`
    After the batches: per-file ceilings as a guideline test, scans from the "tools" list, `too_many_arguments` without the workspace-wide allow, `unwrap_used` once the standards exception for mutex locks is written. Needs the owner's OK for lint rules.
12. `[Chore] Settle the open questions in docs/coding_standards.md`
    Mutex `lock().unwrap()` and `let _ = tx.send()` exceptions, "size" counting non-test lines, pure-move PRs exempt from the 100-line guide, when `Result<_, String>` is fine in native, serialized `Streak` names stay until a migration. Do this first if lint rules are going to follow; it is a docs-only PR.

## Beyond the standards

Noted in passing while reading, each spot-checked in the code; nothing was profiled and I did not hunt for these, so the list is short on purpose and absence here is not a clean bill. Only items that a planned move/split would not fix by itself.

| # | Where | Problem | Failure scenario | Severity | Fix size | When |
|---|---|---|---|---|---|---|
| X1 | `studio/src-tauri/src/render_manager.rs` 1794-1803 (`export_pool_free_gb`), called from `render_pane.js:569` via `getExportPoolFreeGb` | Sums `get_available_bytes` per folder without the volume dedupe that `lib.rs:439-456` (`export_pool_space`) does. Two export folders on the same drive count that drive's free space twice. | Render tab shows 2x the real free space with two folders on one drive, so the "enough room" gate and the free-space readout say there is space when there is not; a render can then fill the disk. Capture tab (which dedupes) shows the right number, so the two tabs disagree on the same folders. | Medium (wrong number drives a disk-space decision) | XS: call the deduping function (batch B1) | Before: it is the first item of B1 |
| X2 | `studio/src-tauri/src/lib.rs:306-308` `reveal_in_explorer` (-> `audit_manager.rs:780`) and `crash_maps_manager.rs:58-61` `forget_crash_map` | Plain `#[tauri::command] fn`, so they run on the main thread. `reveal_in_explorer_impl` calls `path.exists()` (a stat that can wait on a sleeping or network drive) and spawns a process; `forget_crash_map` reads and writes a file in the app data folder. CLAUDE.md: a command that touches the disk or other processes is `#[tauri::command(async)]` (#789). | Click "Reveal in Explorer" on a demo on a disconnected network share: the window freezes until the stat times out (the #789 symptom). | Low-medium | XS each: add `(async)` | During B1 (one line each) |
| X3 | `native/src/sys/disk.rs:104,215` `get_available_bytes` returns `u64::MAX` for "unknown" | A sentinel in a number that callers sum or compare. Both pool functions remember to skip it (`!= u64::MAX`); any new caller that forgets adds `u64::MAX` to a total (wraps in release) or passes a "fits" check. | A future caller does `free >= needed` and an unreadable drive reports "plenty of room". | Low today, fragile | S: return `Option<u64>` (standard section 4) with the callers (about 10 sites) updated | During B1/B5, with the pool-space dedupe |
| X4 | `native/src/capture_engine.rs` ~1340 `.unwrap()` on `thread::Builder::spawn` | A failed thread spawn (resource exhaustion) panics, and release builds abort the whole app, killing HLAE/hl.exe children with it, instead of reporting "could not start the capture". | Rare: low-memory machine at batch start. | Low | XS: map the error to `EngineEvent::Error` | During B9 (it is in the function being split) |
| X5 | `native/src/hlcr/renderer.rs`, `capture_engine.rs`, and elsewhere: 439 `let _ = tx.send(..)` | A failure to deliver `Finished`/`Error` events is silent. If the receiver is gone the job's result is lost (acceptable on shutdown), but there is no distinction between shutdown and a dropped receiver mid-batch. | A UI-side listener that is dropped early leaves a render job stuck on "Rendering" in the list with no log line. I did not reproduce this; it is a pattern, not a verified bug. | Low | M to audit; most are fine | After (standards doc decision on `let _`, issue 12) |

Not listed because I could not confirm them from a short read: whether `get_render_required_estimate_gb` (render_manager.rs:1772, a plain main-thread command that takes the `jobs` lock) can wait on a scheduler that holds the same lock during work; and whether `cache_demos` (`demo_cache_cmd.rs:18`) can leave `RUNNING` set if the worker thread dies before the final event (it cannot on the normal path: `native/src/demo_cache.rs:292-296` always emits a `finished` event after the workers join). I also checked `skip_known` (capture_manager.rs:2600-2695): it uses hash maps, so no O(n^2) over the demo list.

Only X1 is clearly a real bug. Draft issue, to run first (it is the same PR as plan batch B1):

- `[Chore] Render tab counts free space twice when two export folders share a drive`
  `get_export_pool_free_gb` (render_manager.rs) sums every folder; the capture tab's pool function skips folders on the same volume. Use one function in `native::sys::disk` with a test for two folders on one volume, and make X2's two commands `(async)` in the same PR.

Order against the standards plan: X1 and X2 go in B1 (before), X3 with B1/B5, X4 inside B9, X5 after the standards doc decision.
