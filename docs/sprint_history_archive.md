# Sprint History Archive

> Archived 2026-08-24. This is the full narrative content that used to live in
> `docs/active_sprint_state.md` before that file was trimmed back down to a
> lightweight current-state-only doc (it had grown into a long chronological
> log — everything below was preserved here rather than deleted). Nothing
> here is guaranteed current; read it as history, not status. For what's
> actually true right now, see `active_sprint_state.md`. For ongoing
> bug/feature tracking, see `engineering_backlog.md`.

---

> **Two independent work streams exist in this repo.** Everything below is the
> Capture/Render Studio (Tauri migration + capture-block-manifest) track. There is a
> second, unrelated stream — demo-derived match stats for the KTP league, living on
> `dev`/`main` only — tracked in `docs/demo_stats_feasibility.md`, not here. Check which
> one you're resuming before reading further.

## Web AI State
- **Overarching Goal:** Get `feature/tauri-migration` merged to `dev`. All High-Priority parity gaps are resolved; two Medium-Priority items (Clear Previews modal, Standalone Game Launch) are also now done. Since the last doc sync (`4b26ffd`, 2026-08-13) the branch picked up a run of side work — a standalone `xash-transcode` HLDEMO->IDEM browser-preview transcoder crate (own `[workspace]`, not built by `cargo build --workspace`) and Demo Analyzer load-performance Tiers 1-3 (on-disk cache, progress events, dead-code removal — see `docs/demo_analyzer_load_performance.md`) — the analyzer work was reflected in `engineering_backlog.md`/`milestones.md` this pass; the `xash-transcode` crate was split off entirely onto its own `experimental/xash-transcode` branch (2026-08-18, see that branch's `docs/web_preview_viewer.md`) rather than tracked here, since it's experimental and shouldn't ride into `dev`.
- **2026-08-16/17 user-testing pass (through `8641dba`):** User ran Capture Studio and Render Studio end-to-end and reported a run of real bugs found live, not from code review — most seriously, **batch capture was failing 100% of the time**: `build_batch_queue()` never actually wrote patched demo bytes (`StreamPatcher::patch()` was never called on the batch path, only on the separate Preview path), so every batch failed immediately trying to copy a file that never existed. Also fixed: Save/Load Project Session unusable outside AppData-scoped paths, Master Demo Queue highlight counts mixing every player in the match (root cause: POV filter gated on the unreliable `is_pov` flag instead of `local_player_index`), an opt-in/opt-out `selected` mismatch that could have captured unselected highlights, a Capture Studio Configuration auto-save audit (several fields never persisted at all), new persistence for Save Local Patched Copy/Allocation Strategy/all of Render Studio, and session-file UX (remembers the loaded/saved path, Ctrl+O consistency fix, filename indicator). Full detail in `engineering_backlog.md`'s Completed Tasks and `bugs.md`'s Completed/Resolved Bugs (the 4 most severe ones). **After the batch-capture fix, user confirmed a full Capture Studio + Render Studio run "working great."**
- **Branch relationship to `dev`:** `dev`'s tip (`80feaaf`) *is* the merge-base — `feature/tauri-migration` already contains all of `dev`'s history. There is no divergence/conflict to reconcile; the remaining work is closing out backlog polish items and deciding whether to merge now or after more of the Medium/Low priority list.
- **Verification (2026-08-16):** `cargo build --workspace` clean. `cargo test --workspace --no-fail-fast` clean across every crate except 4 pre-existing `analysis` failures (localization loader/test key-prefix mismatch, one fixture-dependent demo test) — confirmed byte-identical at the `dev` merge-base via diff, so not a regression from this branch. Every fix from the 2026-08-16/17 testing pass above was `cargo check`-verified individually as it landed; a full workspace `cargo build`/`test` re-run since then is still outstanding before merge.
- **Verification (2026-08-18):** Full re-run of `cargo build --workspace` (clean) and `cargo test --workspace --no-fail-fast` reconfirmed clean except the same 4 pre-existing `analysis` failures (3 localization key-prefix mismatches, 1 missing demo fixture) — byte-identical to the set already confirmed present at the `dev` merge-base. No regressions from the 2026-08-16/17 fixes or the `xash-transcode` branch split.
- **`feature/tauri-migration` merged to `dev` (2026-08-18, merge commit `00e540d`), pushed.** `xash-transcode` was split out onto `experimental/xash-transcode` beforehand and is no longer in this line of history. Goal achieved; work since then is post-merge.
- **Post-merge work (2026-08-18):** Two branches off `dev`, both **local only, not yet pushed or merged**:
  - `chore/capture-allocation-cleanup` — removed the dead Chronological drive-allocation strategy entirely and replaced naive arrival-order first-fit with First-Fit-Decreasing bin-packing (`allocate_blocks_first_fit_decreasing`), so a large clip can't be stranded by an earlier small one claiming the only drive with room. Plus first-ever test coverage for `config_from_payload`/`AppSettings`.
  - `feature/capture-block-manifest` (stacked on the above) — Phases 1-2 of the connected-workspace plan (`~/.claude/plans/splendid-shimmying-kahn.md`), plus several real bugs found along the way. See below.
- **✅ MANUAL TEST DONE (2026-08-18 21:53).** Real HLAE capture against `feature/capture-block-manifest` confirmed working: final run `captured=true renderable=true` for both blocks. Took three failed attempts first, each a real bug — full detail in `engineering_backlog.md`'s Pending Manual Verification entry:
  1. `hl.exe` closing within seconds with nothing logged → `capture_engine.rs` never checked child-process liveness at all; added `try_wait()` polling + `[HLAE]` timing logs to `crash_log.md`.
  2. That check then false-positived on the HLAE launcher's *normal* ~2.5s handoff exit, ran the abort-cleanup path, and deleted `primer.dem` out from under `hl.exe` mid-startup — which is what actually produced the in-game `ERROR: couldn't open` (the demo file itself was never corrupt). Fixed by tracking `hl.exe` liveness by process name instead of the launcher's exit status.
  3. `captured: true` + `renderable: false` on every block, but not from a `.wav`-flush race as suspected — HLAE nests each recording one level deeper than expected (`chain_01_b0/take0000/...`, `mirv_movie`'s own auto-take-numbering), which `hlcr::scanner::is_renderable_take` didn't account for. Render Studio's real scanner was unaffected (it recurses). Fixed + unit-tested against the exact on-disk layout.
  - Not exercised: `mirv_movie_separate_hud 0` (both runs used default `all`/`hudcolor` layout), and the merge-path check below (`MIN_TAKE_SEPARATION_SECONDS`) — no merged blocks occurred in either test batch.
  - **Merge-path check (still open, optional):** run `cargo run --release -p native --bin find_overlaps -- <folder> --pre-roll 25` to find demos whose highlights merge, then capture those rows and confirm one block reports two `source_streak_indices` and both highlights flip to Captured together. Also the only way to calibrate `MIN_TAKE_SEPARATION_SECONDS` (`native/src/patch/builder.rs`, currently 1.0s, still an unmeasured guess).
- **✅ PHASE 3 DONE, manually verified end-to-end with screenshots (2026-08-18/19).** Take index + auto-Rendered, plus a real Render Studio bug hunt that grew out of testing it — full detail in `engineering_backlog.md`. Short version: capture → save → full restart → reload → render → confirmed `Rendered` status resolved from the persisted `takeIndex` (proven via explicit before/after console logs, not inferred). Found and fixed one real correlation bug (`take_key()` didn't account for HLAE's `take0000` nesting, same root cause as the `is_renderable_take` bug above — capture-side and render-side keys would never have matched). Testing Phase 3 also surfaced an unrelated Render Studio bug (Start/Cancel buttons desyncing from actual state after a `reset_render_job`-triggered resume) and a design gap (settings changes silently not applying to reset jobs) — both fixed, the latter by moving to a VirtualDub-style per-job settings model (each job owns its own codec/fps; the panel is a template for new jobs only) plus a new Settings column. **Retesting that found two more real bugs**: `render_take_finished` never actually fired on a genuine successful render (a status-guard ordering quirk swallowed it every time), and H.264/NVENC renders had always been silently mislabeled — output was `.mov` despite the dropdown saying "MP4", from a dead/unwired extension-mapping function. Both fixed and re-verified with screenshots (correct toasts, `Rendered` status in the UI, genuine `.mp4` files in Explorer/VLC). See `engineering_backlog.md`'s Phase 3 entry for the complete breakdown.
- **Phase 4 (Workspace vs Quick-Clip modes) — implemented AND extensively manually verified now (2026-08-19/20).** User ran a real, long click-through pass via `npm run tauri dev` against the QA checklist (`docs/qa_checklists/`, its own new tooling — see `engineering_backlog.md`), finding and fixing a long tail of real bugs, several of them genuine crashes invisible without devtools open: (1) mode-toggle/nav-router collision, (2) a stack-overflow-causing infinite recursion between `renderMasterList`'s empty-queue notification and `detail_pane.js`'s `onSelectionChange` callback, (3) a second, differently-triggered null-deref in that same empty-queue path, (4) **app-wide**: every `confirm()` dialog (including two pre-existing ones in Demo Auditor and Capture Studio, not new code) silently never blocked on the user's choice at all, because the global `window.confirm` Tauri intercepts is asynchronous while every call site used it synchronously — fixed by switching to the properly-awaited `confirm()` from `@tauri-apps/plugin-dialog`, (5) three separate selection-preservation bugs (Clear Untracked/Selected/All all hard-reset the selected row to index 0 instead of preserving it by identity; row delete always visually dropped the highlight regardless of which row was deleted), plus assorted wording/toast/search-scoping fixes. Full detail across `bugs.md` and `engineering_backlog.md`'s Phase 4 entries. Added a global `error_reporter.js` safety net (uncaught errors → console + `crash_log.md` + capped toast) directly motivated by (2)/(3) being invisible without devtools. Discussed and backlogged (not started) automated testing — Vitest for logic-level regression coverage of tonight's bugs, `tauri-driver`+WebdriverIO for true e2e later.
- **Not yet exercised in this pass:** the "Merged Blocks" and "Settings Persistence" checklist sections (need a real capture batch / an app restart respectively) — everything else in `docs/qa_checklists/phase4-connected-workspace.json` has been clicked through at least once, several sections multiple times across bug-fix iterations.
- **Git state (2026-08-20 EOD):** Two commits landed on `feature/capture-block-manifest` this session — `3d7493f` (the Phase 4 feature + first bug-fix round) and `1aed999` (the selection-preservation fixes + doc updates). Working tree is clean, nothing uncommitted. Both commits are still local-only, not pushed. **Next steps:** finish the two unexercised checklist sections above (Merged Blocks, Settings Persistence), then decide whether to merge `chore/capture-allocation-cleanup` + `feature/capture-block-manifest` to `dev` together or separately.
- **Testing/bug-hunt session (2026-08-22/23), same `feature/capture-render-quick-wins` branch.** Started as `MIN_TAKE_SEPARATION_SECONDS` calibration (still not actually calibrated — see below) and turned into a long chain of real, unrelated bugs found along the way. Full detail in `engineering_backlog.md`; short version per topic:
  - **Crash fix, split onto its own branch off `dev` and already pushed** — `fix/dem-patch-delta-description-crash` (commit `ddb68a2`, cherry-picked from this branch's `69e8374`), **PR not yet opened, that's the first thing to do in a fresh session.** `SvcDeltaDescription::parse` (`dem-patch/src/netmsg_doer/delta_description.rs`) unwrapped fields that can legitimately be absent on some demos, causing a full process crash (not just a parse error) with no `catch_unwind` anywhere in the real app's folder-scan path either. Fixed to return a normal parse error instead. This one commit is verified fully independent of the rest of the stack (diffed `dem-patch/` against `dev` — zero other divergence) and builds/tests clean on its own off `dev`.
  - **Two real corrupted demos found while hunting for calibration candidates**, unrelated to any dod-tools bug: `monday-wsod25_r07_m1_h1.dem`/`_h2.dem`/`m3_h1-1.dem`/`m3_h2.dem` fail at the byte-parsing level (now gracefully skipped, thanks to the fix above); `wsod25-po_r3_sf-m00cat_ih_m3_armory_h2.dem` parses fine but crashes the real GoldSrc engine on playback — turned out to be a thumb-drive-corrupted copy missing ~62% of its frames (confirmed via `dod-tools-inspect` frame counts against a known-good `..._h2-1.dem` copy), not a map/engine/patcher issue. Spun off an R&D idea (demo delta-continuity checker, likely a Demo Auditor extension) from the "why didn't our parser catch this" discussion.
  - **Two real render-audio bugs found testing the good copy**, both fixed and user-verified in Vegas Pro (not just VLC): (1) Render Studio's FPS setting has no connection to the FPS a take was actually captured at — no source of truth links a take folder to its real capture FPS, so a stale/mismatched Render Studio FPS silently produces sped-up video with no error. Diagnosed but **not fixed in code** — still needs either capture-time FPS metadata or a UI mismatch warning. (2) Separately, MP4 output's `-c:a pcm_s16le` used an `ipcm` audio box that FFmpeg/VLC tolerate but Vegas Pro can't decode (garbled/static audio) — confirmed by remuxing the identical PCM bytes into `.mov` (works) vs `.mp4` (broken). **Fixed**: MP4-outputting presets (h264_nvenc, libx264) now use AAC; MOV-outputting ones (ProRes, DNxHD) unchanged.
  - **Logging rework, unrelated to the above, done same session**: `crash_log.md` → daily-rotating `activity_YYYYMMDD.md` (one file per calendar day, not per launch — this branch was heavy on restarts), midnight-crossover cross-references between the two days' files, 30-day retention, and a "View Logs" button (bottom-right of the pinned footer) so AppData/logs isn't something the user has to know to go find.
  - **`MIN_TAKE_SEPARATION_SECONDS` itself is still not calibrated** — the actual goal of the session. Corrected `find_overlaps`-verified Start Lead/Stop Trail values are ready (`7.64`/`7.39`/`6.89`/`5.89` for ~1.5/2.0/3.0/5.0s physical gaps, against the intact `wsod25-po_r3_sf-m00cat_ih_m3_armory_h2-1.dem` copy, rows #8/#9) — next session should pick this back up with a real capture + audio check on take #9, now that the AAC fix means a bad-sounding result would actually mean something.
  - **Bonus, unplanned confirmation**: a real capture using the (at-the-time-wrong) calibration values happened to trigger a genuine merge, closing the separate "merge-path check, still open, optional" item from this doc's 2026-08-18 entry above — both highlights correctly flipped to `Captured` sharing one `merged → chain_01_b0` badge in the UI.
  - **Git state**: `fix/dem-patch-delta-description-crash` pushed, PR pending. `feature/capture-render-quick-wins` has 5 new local commits this session (dem-patch fix, logging rework, docs, AAC audio fix, plus this doc update) — push status depends on when this note is read; check `git status`/`git log --oneline @{u}..HEAD` rather than trusting this line to still be accurate.
- **UI polish pass (2026-08-21/22), own branch `feature/capture-render-quick-wins` (stacked on `feature/capture-block-manifest`, not yet merged there or to `dev`).** A run of small user-reported fixes, none touching capture/render logic itself: added the app logo to the top-left nav brand (sized 2x — at the original size it wasn't legible), scoped the Quick-Clip/Workspace toggle + Save/Load Session controls to only show on Capture/Render Studio (Demo Auditor/Analyzer don't use the project file), and fixed the top nav bar overflowing/wrapping at the app's actual 800px minimum window width (removed a redundant always-visible caption, added responsive breakpoints). In Capture Studio: fixed "Total Streaks" summing *every* player's streaks per demo instead of just the recording player's (could read in the hundreds against a Highlights column summing to a fraction of that — renamed to "Total Highlights" and reused the existing `recordingPlayerStreaks()` helper instead of re-deriving it wrong), fixed Kill Range's number-input spinner arrows covering the value on hover, replaced the Master Queue's 🗑 emoji delete icon with `list_editor.js`'s existing SVG (the emoji renders as a flat monochrome glyph in WebView2 and was misread as a pause icon), and removed the redundant "(Select a Demo)" title text. **Also found and fixed a real bug while investigating Match Telemetry**: it was permanently stuck empty after Load Session (and every row-click for the rest of that window) because that path's `renderMasterList` callback never called `analyzeDemo`/`renderTelemetry` at all — traced through all 5 `renderMasterList` call sites and consolidated onto one shared `selectDemoAndRenderDetail` helper. That surfaced a second, deeper bug: the backend `analyze_demo` Tauri command was reading `scoreboard`/`chat_logs`/`mortality_metrics`/`round_chronologies` from JSON paths that don't exist in the current `Analysis`/`AnalyzerState` shape at all, and populated `file_info` from raw disk metadata instead of the parsed demo's map/tick data — every field silently resolved to null. Given the fix needed real backend rework for a feature that's mostly redundant with the `View Match Telemetry` button (already jumps straight to the full Demo Analyzer report), **removed it entirely** rather than fixing it — deleted `telemetry_pane.js`, the `analyze_demo`/`SerializedAnalysis` Rust command, and `analyzeDemo()`'s IPC wrapper; the button and its full-report jump are untouched. Last fix: the Highlight Details header (title + action buttons) could overlap at narrow widths — a first attempt at letting the title shrink instead collapsed it to zero width, caught via a real screenshot before shipping; the actual fix lets the title row and button row wrap onto separate lines instead of fighting for one, applied at the shared `.flex-between` class so it also fixed the same latent issue on the Master Demo Queue's header. **Reviewed (not yet built)**: which panels app-wide should get an Explorer-sidebar-style resize handle — see the new backlog entry in `engineering_backlog.md`'s R&D section. Two real candidates found (Capture Studio's Queue↔Details split, Demo Analyzer's Demos↔Report split); Render Studio and Demo Auditor have no panel-to-panel split to add one to. Commits: `d5e14ee` (navbar), `978c7b9` (Highlights/spinner/trash-icon/Match-Telemetry-removal) pushed to `origin/feature/capture-render-quick-wins`; the redundant-title/header-wrap fix plus this doc pass committed and pushed same session.

- **Testing session (2026-08-23), same `feature/capture-render-quick-wins` branch.** Two threads: (1) opened and merged the crash-fix PR from the prior session — `fix/dem-patch-delta-description-crash` merged to `dev` as `6e48086` (PR #1), local `dev` already in sync, no cleanup needed beyond the routine local branch delete. (2) Ran `capture-render-quick-wins-testing-checklist.md` start to finish for the first time — **all 7 sections now complete**, including the crash-recovery and zero-takes sub-items that had been left open. Found and fixed 6 real bugs along the way, none part of the checklist's original 6-fix scope — full writeups in `engineering_backlog.md`:
  1. `reveal_in_explorer_impl` silently opened Explorer's default location (not an error) on a missing take/output folder instead of erroring — fixed with an existence check.
  2. Render job failures (missing folder, FFmpeg errors, cancellation, etc.) never wrote anything to `activity_*.md`, only successes did — fixed with a parallel `[render-take-failed]` log line.
  3. The "Render Batch Interrupted" recovery prompt fired on every F5 reload during an actively-running render (F5 only reloads the WebView, not the Rust backend) — fixed by gating `check_render_autosave` on the live `is_rendering` flag.
  4. Closing the app while an Init/Custom Command field still had focus silently dropped the edit (list_editor.js's debounce-to-`'change'` fix from an earlier session) — fixed via `onCloseRequested` flushing settings before close; that fix's own `window.destroy()` call then hit a missing `core:window:allow-destroy` capability (app couldn't close via X at all until granted — same "not in `:default`" pattern as the `dialog:allow-confirm` gap from 2026-08-19).
  5. `get_available_bytes` (`native/src/sys/disk.rs`) trusted a bare mount-point string-prefix match with no check the actual path resolves to anything real — a malformed path like `"C:\real\folder|garbage"` still reported the whole C: drive's free space. Not just a UI bug: this same function backs real capture drive-allocation (`patch/builder.rs`) and render JIT export routing (`hlcr/renderer.rs`). Fixed by gating it on the new `diagnose_path` classification (reused from item 6 below).
  6. Follow-up inconsistency from fixing #5: a not-yet-created-but-otherwise-valid path (real parent drive, leaf folder just doesn't exist yet — normal for a fresh output dir) was flagged as a "won't be used" problem in one place while the footer correctly counted it as usable in another. Fixed by having both consumers key off the same `usable` boolean.
  Built new alongside this: Capture Output now gets full per-path validation (`not_absolute`/`malformed`/`not_found`/`not_a_directory` via `diagnose_capture_output_paths`) surfaced as three distinct banner states — red/blocking (pool fully unusable), yellow/non-blocking (some entries bad, at least one good), and a new calm blue informational one ("doesn't exist yet, will be created on capture") added per user request to avoid masking a typo'd-intended-existing-folder while not treating routine fresh-folder use as a problem. Two new Low/Medium-priority backlog items logged: an app-wide sweep for per-field inline validation (this Capture Output work is the first concrete case), and a low-priority idea to strip pre-existing `svc_director`/console commands from a source demo before patching in new ones. Committed as `f6f4d0e`.
  - **Also closed out the two remaining `feature/capture-block-manifest` gates this session**, both previously only tracked as unchecked boxes in `docs/archive/phase4_manual_test_checklist.md`: **Merged Blocks** was actually already verified back on 2026-08-22/23 (an incidental real capture triggered a genuine merge, badge confirmed in the UI) but the checkbox itself was never ticked — fixed the doc oversight. **Settings Persistence** (`studio_mode` round-trip) was verified fresh this session: F5 reload held the mode both directions (Quick-Clip and Workspace), then re-confirmed via a genuine full close+reopen — briefly flashes the `"quick-clip"` code default before the async `get_settings()` fetch resolves and flips to the saved value, which is expected startup sequencing, not a bug. Two minor sub-items left honestly unchecked (badge tooltip wording, project-file `mode` field write) — not gates, just unverified polish.
  - **Net effect: the full stack (`chore/capture-allocation-cleanup` → `feature/capture-block-manifest` → `feature/capture-render-quick-wins`) has no remaining known blockers for a PR to `dev`.** Every checklist this doc has referenced as a gate — this branch's own quick-wins checklist, plus block-manifest's Merged Blocks and Settings Persistence — is now actually run, not just theoretically ready.
  - **3 stacked PRs opened (2026-08-23), same session, awaiting review/merge:** [PR #2](https://github.com/ccoventry/dod-studio/pull/2) `chore/capture-allocation-cleanup` → `dev`, [PR #3](https://github.com/ccoventry/dod-studio/pull/3) `feature/capture-block-manifest` → `chore/capture-allocation-cleanup`, [PR #4](https://github.com/ccoventry/dod-studio/pull/4) `feature/capture-render-quick-wins` → `feature/capture-block-manifest`. Merge them in that order (#2 first) since each depends on the branch below it; PR #3's base will need retargeting to `dev` once #2 merges (its current base branch gets deleted), and same for #4 once #3 merges. Also created two GitHub rulesets this session: `main-protection` (blocks deletion/force-push, requires PRs) and `dev-ruleset` (blocks deletion only, direct pushes still allowed).

## Active Epics (as of 2026-08-13, superseded — everything below eventually shipped; see `engineering_backlog.md` for what came after)
- **Headless Preview CLI:** COMPLETED
  - Secondary binary target `preview_cli` built and tested. Supports interactive prompt fallback (`is_interactive = true`), drag-and-drop file/folder inputs, and automatic localized `previews/` folder generation.
- **Top Navigation & Functional Cancellation:** COMPLETED
  - Migrated vertical navigation to top bar, extracted Export Manager view, implemented non-destructive `INGESTION_CANCEL` thread interruption, and unified localized view footers.
- **Localization Infrastructure:** COMPLETED
  - Migrated hardcoded GUI/CLI/scanner strings to localizations. Updated `analysis::localization` to support transparent dual-key lookups (`#key` and `key`) for Valve KeyValues and AMXX files.
- **HLTV Active Frame Injection:** COMPLETED
  - Standalone `DRC_CMD_INEYE` frame injection implemented in `native/src/patch/engine.rs`.
- **Dynamic Drive Failover:** COMPLETED
  - AOT capture routing, duration math parity, JIT render routing, and UI/UX export pool indicators with dynamic vector list reordering.
- **Frontend Migration & Capture Studio Parity:** COMPLETED 2026-08-18 (this section originally said "IN PROGRESS" — stale, corrected during the 2026-08-24 archive pass)
  - Transitioning frontend stack to Tauri + Vite architecture in `desktop-studio/`, restoring parity with legacy `dev` branch.

## IDE AI State (frozen 2026-08-13, never updated again — superseded entirely by Web AI State above; kept here only as a record that this doc once tracked two separate agent-context logs)
- **Current Goal:** All High-Priority Capture Studio parity gaps in `engineering_backlog.md` are resolved — `PatcherConfig` Full Persistence, Pre-Flight Disk Allocation & Pre-Scan Estimator, and Running Process Guard & Detector Modal all closed 2026-08-13, on top of Custom Engine Commands Integration from earlier in the sprint. The three feature diffs (previously one uncommitted working-tree blob) have since been split into three scoped commits — `b3c47e1` (settings persistence), `f36196c` (disk estimator), `d9b9f10` (process guard) — on `feature/tauri-migration`. `feature/tauri-migration` is ready for end-to-end verification, testing, and merge to `dev`; only Medium/Low priority polish items remain in the backlog.
- **Last Evaluated:** 2026-08-13
- **Status:** Working tree is clean against `HEAD` except for `docs/staging_lessons.md` (three new engine-quirk/workflow bullets appended this pass, not yet committed) and this file being rewritten in the same pass. No open compile errors or IDE diagnostics; the last `cargo check -p desktop-studio` (pre-split) was clean. Immediate Sprint Focus is cleared (all items resolved, see below); all High Priority parity items in `engineering_backlog.md` are closed. Next step is branch verification/merge, not further feature work.

### Immediate Sprint Focus (Top Priorities)
*(Cleared 2026-08-13 — every item previously tracked here, and every High Priority item in `engineering_backlog.md`'s parity backlog, is resolved. Full detail lives in `engineering_backlog.md`'s Completed Tasks section. Remaining work is Medium/Low priority polish plus end-to-end branch verification before merge.)*

---

## Testing Checklist — Capture/Render Quick Wins (archived 2026-08-24, moved verbatim from `docs/capture-render-quick-wins-testing-checklist.md`)

> Moved here rather than left as its own file: fully complete (all 7 sections
> checked off 2026-08-23), the branch it covers (`feature/capture-render-quick-wins`)
> is confirmed merged to `dev` via PR #4, and its findings are already narrated
> in prose above (2026-08-23 entry) — this is the granular checkbox-level
> record behind that narrative. `docs/capture-render-ux-audit.md`'s status
> table cites this section by number (§1-§7) for which audit findings it verified.

Manual verification for commit `976034b` on `feature/capture-render-quick-wins`
(six fixes picked from `docs/capture-render-ux-audit.md`). Run via
`npm run tauri dev` from `desktop-studio/`. Check off each item and note any
deviation from the expected result.

**Status as of 2026-08-22: none of this has been run yet.** All 21 boxes below
are unchecked — deferred to a dedicated testing pass, not skipped. Branch is
pushed to `origin/feature/capture-render-quick-wins` (`aa5c272`) but **not
merged to `dev`** until this checklist (plus the two still-open
`feature/capture-block-manifest` sections it stacks on — Merged Blocks and
Settings Persistence, see `active_sprint_state.md`) actually gets run.
Item 4's "close the app while a field still has focus" case is worth
prioritizing — it's a real open question about whether an edit can get
silently dropped, not just a sanity check.

**Update 2026-08-23:** Testing pass started (`npm run tauri dev`). Section 1
(Reveal in Explorer) done except the crash-recovery sub-item, and found +
fixed two real bugs along the way — see that section for detail and
`engineering_backlog.md` for the full writeup of both (reveal-in-explorer
silently opening the wrong folder, and render job failures never writing
anything to the activity log).

**Final update 2026-08-23: entire checklist complete.** All 7 sections done
(Section 1's crash-recovery sub-item and Section 6's zero-takes item both
came back around and got done too, not left open). Six real bugs found and
fixed along the way, none part of the original 6-fix audit this checklist
covers — full writeups in `engineering_backlog.md`:
1. Reveal-in-Explorer silently opened the wrong (default) folder on a
   missing path instead of erroring.
2. Render job failures never wrote anything to the activity log.
3. Render-recovery prompt fired on every F5 reload during an active render,
   even though nothing was interrupted.
4. Closing the app while an Init/Custom Command field still had focus
   silently dropped the edit (plus a `core:window:allow-destroy` permission
   gap found fixing it, which briefly meant the app couldn't close via X
   at all).
5. Capture Output's disk-space checks trusted a mount-point prefix match
   with no check the actual path was usable — affects real capture/render
   drive allocation (`patch/builder.rs`, `hlcr/renderer.rs`), not just this
   UI.
6. A follow-up inconsistency from fixing #5: a not-yet-created-but-valid
   path was flagged as a problem in the warning list while the footer
   correctly counted it as usable — two signals disagreeing about the same
   path.

Plus the new Capture Output per-path validation feature itself (Section 7),
built mid-session in response to bug #5/#6, with its own yellow (partial
pool) and blue (will-be-created) informational states beyond the original
red blocking case. Branch is otherwise unchanged from before this pass —
still not merged to `dev` (see `active_sprint_state.md` for the two
remaining `feature/capture-block-manifest` sections — Merged Blocks and
Settings Persistence — this checklist doesn't cover).

---

### 1. Reveal in Explorer (render job rows)

- [x] Scan a render folder with a few takes, don't start rendering yet. Each
      **Queued** row shows an "📁 Open Take Folder" button. Click it — Explorer
      opens with that take's source folder selected. **2026-08-23: "Scan for
      Takes" alone doesn't populate the job table/Actions column — that's by
      design (scan only fills the lightweight preview list; the real job rows
      appear once "Start Render Batch" queues them). Verified via
      Start Render Batch instead — button worked correctly.**
- [x] Start a render batch, let one job finish. The **Finished** row's button
      now reads "📁 Open Output" — click it — Explorer opens with the actual
      rendered file selected (not just the folder). **2026-08-23: confirmed.**
- [x] Cancel a job or let one **Error** out. Its button should still read
      "Open Take Folder" (no `output_path` was ever set for it). **2026-08-23:
      confirmed on a Cancelled row — correct label, opened the right folder,
      plus a bonus "View Log" button showed "Cancelled by user".**
- [x] Crash-recovery path: start a batch, let one job finish, force-quit the
      app before the batch fully completes, relaunch, and use the render
      recovery prompt. Confirm the recovered **Finished** job still shows
      "Open Output" pointing at a real file — this exercises the
      `output_path` persistence fix in `write_autosave`/`recover_render_batch`,
      which didn't work before this change. **2026-08-23: confirmed. Force-quit
      (killed `desktop-studio.exe` directly) correctly took the whole
      `tauri dev` wrapper down with it (exit 0, expected — it watches the
      child). On relaunch, "Render Batch Interrupted" prompt showed accurate
      Completed:1/Pending:1 counts; Recover Render Batch restored both rows
      correctly, and the recovered Finished row's "Open Output" opened
      Explorer with the real rendered .mp4 highlighted (file played back fine
      in VLC, video+audio both good). Minor Explorer-scroll-position quirk
      noted — not app-controllable, not a bug.**
- [x] Edge case: manually delete or rename a take folder on disk, then click
      its "Open Take Folder" button. Should surface an error toast, not crash
      the app. **2026-08-23: found a real bug here — see engineering_backlog.md
      ("Open Take Folder silently opened the wrong folder...").** Fixed and
      reverified: now toasts "Could not open folder: Path no longer exists:
      ..." instead of silently opening Explorer's default location.**

### 2. Dead code removal (`export_manager.rs`)

- [x] App launches normally via `npm run tauri dev` with no missing-module or
      startup errors. (Build already verified clean; this just confirms
      nothing at runtime was quietly depending on the deleted file.)
      **2026-08-23: confirmed across multiple launches this session (initial
      start, two hot-reload rebuilds, and a post-crash relaunch) — all clean,
      no missing-module/startup errors.**

### 3. Deduped export-dir fallback (`capture_engine.rs`)

- [x] Run a capture batch with **Media Output Directory** left blank (uses the
      exe-relative fallback). Confirm the batch completes and files land in
      the same place they did before this change (should be identical
      behavior — this was a pure refactor, not a behavior change).
      **2026-08-23: this field no longer exists — "Media Output Directory"
      was removed 2026-08-17 and replaced by the mandatory "Capture Output"
      list (`capture_pane.js`/`capture_manager.rs` — no separate Primary
      Media Dir field anymore, per `test_config_from_payload_primary_media_dir_is_first_drive`).
      The exe-relative fallback code technically still exists in
      `capture_engine.rs` but is unreachable through the UI now — the Start
      button is blocked with an error toast if Capture Output is empty, so
      there's no way to leave it "blank." Superseded, not testable as written.**
- [x] Run a capture batch with **Media Output Directory** explicitly set.
      Confirm normal behavior, unaffected. **2026-08-23: covered — real HLAE
      capture batches ran successfully multiple times earlier this session
      with Capture Output populated (the render-job tests in Section 1 all
      depended on a prior successful capture batch).** While testing this
      item found and fixed a real bug in the disk-space warning banner (see
      `engineering_backlog.md`, "Capture Output's 'unusable pool' warning
      gave the same generic message...") — an invalid Capture Output entry
      showed the same "not configured" message as an empty list. Also chased
      what looked like a second bug (long list of invalid paths silently
      passing validation) that turned out to be a leftover valid drive still
      in the list, not a defect — the aggregate check correctly treats "any
      one drive has room" as passing, by design.

### 4. Settings debounce (`list_editor.js` — Init/Custom Commands, numeric fields)

- [x] Type a multi-character value into a Custom Commands or Init Commands
      text field. While typing, confirm the app stays responsive (no
      per-keystroke lag/hitching). **2026-08-23: confirmed, no lag.**
- [x] After typing, click elsewhere (blur) — reopen the settings/reload the
      pane and confirm the typed value actually persisted correctly.
      **2026-08-23: confirmed — blurred, pressed F5 to reload the whole
      webview, value was still there.**
- [x] **Edge case to specifically watch for:** type a value into one of these
      fields and close the app *without* clicking away or pressing Enter
      first (i.e. while the field still has focus). Since the save now fires
      on `'change'` instead of `'input'`, an unblurred edit may not be
      persisted. Confirm whether this matters in practice — if it does, we
      should add a blur-on-close flush. **2026-08-23: confirmed it matters —
      edited a row, closed via the window's X without blurring, reopened,
      edit was gone. Fixed same session: `main.js` now hooks
      `getCurrentWindow().onCloseRequested()` to flush `persistAppSettings()`
      before the window actually closes (see `engineering_backlog.md`).
      **Re-verified: first retest hit a second bug (`window.destroy` blocked
      by missing `core:window:allow-destroy` permission — app couldn't close
      via X at all until fixed). After that fix, confirmed directly against
      `settings.json` on disk: typed a value, closed without blurring,
      the value was there. Fully resolved.**
- [x] Add / remove / reorder rows in these lists (not just edit text) — confirm
      those still save immediately as before (unaffected code path).
      **2026-08-23: confirmed on both lists.** Init Commands: added 3 rows
      (AAA/BBB/CCC on a cleared list), moved AAA down, moved CCC up, deleted
      CCC, reloaded — persisted as exactly `BBB, AAA`. Custom Commands: added
      2 rows (`first cmd`/Before/1, `test cmd`/After/5 on a cleared list),
      moved `test cmd` up, reloaded — persisted as exactly `test cmd`
      (After, 5) then `first cmd` (Before, 1), all fields intact.

### 5. NVENC concurrency warning (Render Studio)

- [x] Set Codec to **H.264 (NVENC GPU, MP4)** and Max Concurrent Renders to
      **4 or higher** — a warning toast should appear. **2026-08-23: incidental
      confirmation while testing Section 1 (those settings were already active)
      — the orange "4 concurrent NVENC renders may exceed your GPU's encoder
      session limit..." toast fired correctly.**
- [x] Lower Max Concurrent Renders to **3 or below** with NVENC still
      selected — no warning. **2026-08-23: confirmed, no toast.**
- [x] Switch codec away from NVENC (ProRes/DNxHR/software H.264) with
      concurrency still at 4+ — no warning. **2026-08-23: confirmed for all
      3 other codec options; switching back to NVENC re-triggered the
      warning correctly too.**
- [x] Load the app with NVENC + concurrency >3 already saved from a previous
      session (don't touch either field), then click **Start Render Batch**
      directly — warning should still fire (tests the click-time check, not
      just the change-event one). **2026-08-23: confirmed — reloaded with
      NVENC/4 already set, clicked Start Render Batch without touching
      either field, warning toast fired at click time alongside the
      Initializing/Queued toasts.**

### 6. Live render-scan status

- [x] Click **Scan Render Directories** on a folder with several takes.
      Confirm the status line updates incrementally (e.g. "found 1 take(s)
      so far", then 2, then 3...) rather than staying static until the whole
      scan finishes. **2026-08-23: with 7 real takes on fast local storage
      the scan completes too quickly to visually perceive the increments —
      confirmed via code instead (`render_manager.rs:51-89`):
      `scan_folder_background` emits one `render_scan_status` event per take
      as it's found, drained concurrently on its own thread, genuinely
      incremental at the implementation level, not batched at the end.**
- [x] Scan a folder with no valid takes — status should settle on "0 take(s)
      found" without erroring. **2026-08-23: confirmed, using a nonexistent
      (well-formed) folder path rather than an empty existing one — arguably
      a better test, since it also exercises the scanner's existence guard.
      Verified in code too (`native/src/hlcr/scanner.rs:100-103`):
      `scan_folder_background` explicitly checks
      `!source_folder.exists() || !source_folder.is_dir()` and skips before
      ever attempting to walk it — settled cleanly on "0 take(s) found", no
      error, even under repeated rapid scanning.**
- [x] Scan, then scan again (a second click) — the counter should reset to 0
      at the start of the second scan, not continue accumulating from the
      first. **2026-08-23: confirmed robustly — user rapid-clicked Scan for
      Takes a dozen+ times in a row, every single scan correctly reported
      "Scanned 7 render take(s)", never accumulating (14, 21, etc.).**

### 7. Capture Output validation (found + fixed mid-session, 2026-08-23 — not part of the original 6-fix audit)

Not in scope when this checklist was written; added retroactively so this
specific regression is documented for future testing passes rather than only
living in `engineering_backlog.md`. All items below already verified once
this session via manual edge-case testing in Capture Studio → Configuration
→ Capture Output.

- [x] Capture Output list empty → red banner reads "No Capture Output
      directories configured..."; Start Capture Batch disabled.
- [x] Every configured entry unusable (e.g. all relative paths like `"a"`,
      or all nonexistent) → red banner names the *specific* reason per entry
      (not absolute / malformed / doesn't exist / not a directory), not the
      generic "not configured" message; Start Capture Batch disabled.
- [x] Mixed pool — at least one real, valid, spacious drive plus one or more
      bad entries → non-blocking **yellow** banner lists the bad entries and
      states capture will proceed using the valid one(s); Start Capture
      Batch stays enabled.
- [x] Long list of bad entries (8+) → reason list caps at 3 bullets with an
      "...and N more" tail, point-form (one bullet per line via
      `white-space: pre-line`), not a run-on wall of text.
- [x] A genuinely full-but-valid drive (0 bytes free, not an invalid path)
      still gets the generic "have any free space" message, not misattributed
      to a path problem. *(Verified via code path, not a live full-drive test
      — revisit if a real 0-free-space drive is ever available to test.)*

### General regression pass

- [x] Full capture batch → render batch → finished movie, end to end, still
      works with no new errors in the console or toasts. **2026-08-23:
      covered cumulatively rather than as one isolated pass — multiple real
      capture batches (including a fresh capture mid-session) and multiple
      real NVENC render batches all completed cleanly with correct toasts
      throughout tonight's testing; every unexpected console error that did
      surface got found and fixed (see the 6 bugs above), not left unhandled.**
- [x] Other Settings fields (not Custom/Init Commands) still save on their
      existing triggers — nothing else was changed, but worth a spot check
      since `persistAppSettings()` fan-out is shared. **2026-08-23: HLAE/
      Half-Life/FFmpeg paths, resolution, codec, concurrency, etc. all
      persisted correctly across the many app reloads/restarts this session
      — same `persistAppSettings()` fan-out Init/Custom Commands use.**
