# UI facelift build plan (draft)

For Chris, who decides and tests, and for the Claude sessions that build it. Checked against `origin/dev` e429382d on 2026-11-05. No code has changed. Settled decisions in the design notes aren't re-opened here. The words Clip, Variant, Take, Clip settings, Project profile are the settled ones.

**The short version:** make the project file safe first, then move settings into the project, then teach batches to capture one moment more than once, and only then swap the pages. The old screens keep working the whole way, because the data changes come first and add to what's there.

## Order of work

| # | Step | Needs first | My time | You see |
|---|---|---|---|---|
| 1 | Project-file safety: version number, unknown data kept on save, backup | Nothing | 1-1.5 h | Almost nothing |
| 2 | No project switch or app restart during a batch | Nothing | 0.5-1 h | A greyed item |
| 3 | The lambda spray fix (#1322) | Probe on POST first | 2-4 h | Sprays |
| 4 | Project settings block and the one place that resolves them | 1 | 2-3 h | Nothing yet |
| 5 | History with full settings | 4 | 1.5-2.5 h | More detail in History |
| 6 | Project settings page with tabs and OK, Cancel, Apply | 4 | 2-3 h | First new page |
| 7 | Players per project | 4, 2 | 1.5-2.5 h | Monday movie and WSOD25 apart |
| 8 | Clip settings rows and presets | 4, Q4 | 3-5 h | Clip settings page |
| 9 | Project profiles (#444), folders, Players and Teams included | 6, 7, 8 | 2.5-3.5 h | Pick, save, import, export |
| 10 | Batch planner, seek back in a batch, and a dry run (hook DLL) | 4, Q5 | 3.5-5 h | Nothing (same batches), Review's back jumps if Q9 is yes |
| 11 | Variant rows | 10, Q5 | 3-5 h | Variants on the old table |
| 12 | Per-clip commands and add-ons | 11, 8 | 2-4 h | Wallhack on one variant |
| 13 | A batch stage before the demo loads (hook DLL) | 12 | 1-2 h | HD style on a variant |
| 14 | Per-clip frame rate and Capture mode in one batch | 11, Q5 | 4-6 h | 300 fps on one variant |
| 15 | Shell, tokens, one switch for the new pages | Canvas fix list done | 2-3 h | New look, switch off |
| 16 | Clips page with tick bar | 15, 11 | 4-6 h | The main page |
| 17 | Capturing, rendering, batch finished | 16 | 3-4 h | |
| 18 | Demos page and Review | 16 | 3-5 h | |
| 19 | Settings · App, Setup, Help, pop-ups, tool pages | 15 | 4-6 h | |
| 20 | Turn the new pages on for everyone | 16-19 | 2-4 h (test rewrite) | The swap |
| 21 | Remove the old pages | One release after 20 | 1-2 h | Nothing |

**Total:** about 48-73 h of my time, about 2-3 h of CI (some 25 rounds at ~6 min), and about 5-7 h of your test time spread over the steps. No step is over a normal PR; the risk is engine surprises in 3, 13 and 14, named in each step.

**Stop points worth having:**
- After step 9, each movie has its own look and Players. About 15-22 h of my time.
- After step 14, variants and 300 fps work on today's screens. About 28-43 h.
- After step 21, the facelift is done.

**Can run side by side:** 2, 3 and 5 don't touch each other. 15-19 can run at once in separate worktrees once 14 is in.

## Decisions needed

**Decided in review (replaces Q1 and Q2, and #1085's "App default or This project" rule):**

1. **A new project is a copy of a profile, and owns its settings from then on.** There's no live link to an app default, so "App default", "Set by project" chips and "Use app default" links all go. Changing a profile later never touches a project.
2. **Project settings is one full page with tabs,** Project, Clip settings, and Players and sprays, with one OK, Cancel and Apply covering all three, then the project's own Save, like VEGAS's Project Properties. Apply is greyed unless there are changes. OK and Cancel are always enabled and return to where you came from. OK marks the project unsaved only if something changed. Cancel reverts what changed since the last Apply. Leaving any other way with pending edits asks Apply, Discard or Stay. App shortcuts are off while it's open; text boxes keep Ctrl+C, V, Z and A. On OK or Apply, a change that affects captured or rendered clips warns "This changes how 45 captured clips would look", with Apply anyway and Go back, and counts clips with no record separately. Only settings that affect a capture trigger it. Variants keep their drawer's Add variant and Cancel. Marks and notes apply at once. Settings · App keeps saving as you go.
3. **Profile row at the top of the page:** dropdown ("None: this project only", "FOTW (edited)"), Save, Delete, and ⋯ for Save as new, Import and Export, like VEGAS's Template row. These act at once on the saved profiles; Cancel only covers the project. Picking or importing a profile opens a window listing its sections: Picture, Clip length, Clip settings, Clip names, HD style, Folders, Players and Teams, each saying what would change. On a new project all are ticked. On an existing project Folders and Players and Teams aren't. Nothing reaches other projects: to pick up a profile's changes, you import it again there.
4. **Profiles hold everything, folders, Players and Teams included.** Teams (names and merges) are already in the project file, so a shared project carries them anyway; unknown team tags do no harm. Folders are per project and per PC: saved under a PC id, never exported, and a friend chooses their own before their first capture. Players import with names and SteamIDs; a spray or crosshair file that isn't on the PC comes in blank, so that player uses the fallback, and the import ends with a count ("3 sprays and 1 crosshair aren't on this PC"). Bundling the image files can come later.
5. **Settings · App has one row, "New projects start from":** a profile dropdown with a built-in default. First launch and Ctrl+N use it. A capture with no project saved, and the Analyzer's Launch Preview, use it too.
6. **An old project gets today's settings copied in once**, with the one-time summary. Every old project gets the same copy, so the order they're opened in doesn't matter, and Monday's are changed once.
7. **Players and sprays moves from Tools to its tab on Project settings.** The sprays and crosshairs to pick from stay shared on the PC. Read-only during a batch.

8. **Teams joins the Project group,** beside Project settings, as its own window, applying at once, because it can read demos. The button where it is today opens the same window.
9. **One rule: libraries hold templates, and using one copies it in.** There are two PC libraries, Project profiles and Clip settings presets. The project holds the real settings. A variant holds only what's different from the project, and follows the project for everything else; that's the only live link. A take is a frozen record.
10. **A variant has one list.** This replaces "a preset, then What's different". "+ Import from a preset…" adds only the rows that differ from the project, as plain rows with no link to the preset.
11. **Add-ons:** three routes onto a variant: built-in add-ons with their turn-offs (we ship many), your own (a local PC library, with "Save as add-on…" on a typed row), and "+ Add a command…". An add-on is copied into the variant. A table of known turn-offs, in one file, fills in After for typed commands (`mirv_matte_entities add 7` gets `mirv_matte_entities clear`), and warns about known gotchas. Anything else gets "Studio doesn't know how to undo this." A stopped batch still runs pending After lines.
12. **Batches and OBS:** if any clip uses OBS, OBS is started and checked at the start of the batch, as an all-OBS batch does today. Hardware limits warn and never grey an option; a warning is specific if the OBS test has run. A clip's `mirv_movie_fps` is set before its `mirv_recordmovie_start`, never while recording.

Still open: should a batch warn when the project is unsaved? Today it doesn't, and History records what was used, so the plan keeps today's behaviour.

None of the questions left hold up #1085. Each has a recommendation; a "yes to all" is a real answer.

| # | Question | Recommended | If yes | Blocks |
|---|---|---|---|---|
| Q1 | Answered: folders are per project and per PC (2 and 4 above) | | | |
| Q2 | Answered: copied in once (6 above) | | | |
| Q3 | Answered: an older build saves normally, with Save a copy only as the fallback when a normal save can't work | | | Step 1 |
| Q4 | Answered: decision 9 | | | Step 8 |
| Q5 | One playthrough with seek-back, or an extra pass per demo? | One playthrough, and OBS on the variant list. Tested 11-06 on PRE and POST, 4 POV demos from 4 players and 1 HLTV demo: a rewound take differs from a fresh one no more than two fresh loads differ. `-wavonly` changes nothing in `sound.wav` (8 pairs). Steps 10, 11 and 14 below assume yes; say no and they go back to an extra pass | About 2-3 s per variant instead of a 10-15 s reload, from the one demo tried with the first-frame start. Needs Flush Decals on. Sprays not proven: none drew on screen, fresh or rewound. Two takes of one moment still differ in smoke, close bullet holes and flicker, however they're made | Steps 10, 11, 14 |
| Q6 | Answered: Option B below, frame rate per clip in one batch, with no reload for the rate alone | | | Step 14 |
| Q7 | Answered: Studio writes turn-offs, decision 11. Hook restore can come later | | | Step 12 |
| Q8 | The smaller calls below | Answered unless marked | | Listed |
| Q9 | Should Review's POV back jumps use the same rewind? | Yes. It fixes the wrong flags and scoreboard you remembered, on both installs | A POV back jump takes about 2-3 s instead of being instant. HLTV already works this way | Step 10 |

| Canvas-check item | Answer | Where |
|---|---|---|
| C2 Classic textures | Label: "Classic walls and sky (guns, models and HUD icons stay HD until the game restarts)". Tested: a same-map reload with HD off returns walls, floors, sky and detail to exact stock; models and sprites stay HD. Restart option: **your call now**, recommended no for the first release, since a whole project with HD off gets the same result | Step 13 |
| C5 Row after settings change | "Settings changed · Capture again", takes left alone | Steps 5, 16 |
| C6 Export to VEGAS | Parked. Not decided | Step 16 |
| C8 Sound level | Add `mirv_movie_sound_volume 0.25` to Clean clips, take check reports clipping | Step 8 |
| C9 Old "Their own spray" | Becomes "No spray", said once in the summary | Step 3 |
| C10 Issue numbers on the canvas | Leave them there, strip them when each page is built | Steps 6, 8, 16-19 |
| `mirv_fx_xtendvis` in the Wallhack add-on | Left out: no visible effect at 5 spots, and `mirv_fx_wh_xtendvis` already defaults to 1 | Step 12 |

## Data model

**Rule:** add to what's saved, never reshape it. Everything below is chosen so a build that doesn't know it either keeps it or loses only something a backup covers.

**In the project file (`project_session.js:162`):**

```jsonc
{
  "version": "0.12.0",     // stays: nothing reads it, older builds expect it
  "schema": 2,             // new: whole number, up one for each new kind of data
  "saveNeeds": 1,          // new: a build below this can only Save a copy. Stays 1
  "settings": {            // new: the project owns all of these. Missing only in a file not migrated yet
    "picture": { "captureMode", "width", "height", "fov", "captureFps", "obsCaptureFps", "agrFps", "finished" },
    "clipLength": { "beforeSeconds", "afterSeconds", "killGaps" },
    "clipSettings": { "preset": "Clean clips", "initCommands": [], "customCommands": [] },
    "clipNames": { "template" },
    "folders": { "<pcId>": { "destinations", "exports" } },  // full paths, this PC only, removed on export
    "install": { "madeWith" },
    "players": { }         // the project's own table
  },
  "profile": "FOTW",       // new: the Project profile it started from, for "(edited)"
  "demos": [ { "streaks": [ /* highlights, plus the fields below */ ] } ],
  "takeIndex": {}, "takeCaptures": {}, "takeRenders": {}, "teams": {}, "review": {}, "mode": "workspace"
}
```

**A clip stays a highlight row.** Being "in the project" is worked out, not stored: Keep, or a status, a take, a note, a clip name or a narrowed range. So 364 Keeps need no migration, and ticks stay `streak.selected`. New fields on a row: `cleared` (Clear only hides), `mainTake` (a take key, N7; default newest good take).

**A variant is another row in `demo.streaks`, not a list inside its clip.** That's the main data decision.

```jsonc
{ /* a copy of the clip's row */
  "uid": "<clip uid>~v1",          // saved, unlike a clip's uid (see below)
  "start_index": 2, "end_index": 2, "status": "Pending", "clipName": "...",
  "variantOf": "<clip uid>",
  "variant": { "name": "X-ray", "changes": [], "addOns": [],
               "commands": [], "captureMode": null, "captureFps": null, "killGaps": null }
}
```

Why a row, checked on dev:
- `streakUid` uses a saved `uid` if the row has one (`take_index.js:23`), so two rows over the same kills stay two identities, and takes, status and renders work as they do today.
- `demos: getDemos()` saves whole row objects, so a build that doesn't know variants keeps them. It shows them as extra rows.
- A list inside the clip would have its takes wiped by an older build: `pruneTakeIndex` drops any uid it can't find on a row (`take_index.js:219`).
- 25 files read `.streaks`. Rows reach capture, render, history and take checks with no change; a nested list would touch all of them.
- The old screens show a variant the day it exists, so there's no half-migrated state.

The cost is that some of those files must skip variants. Step 11 audits all 25. Known ones: `review_queue.js`, `queue_counts.js`, `split_carry.js`, `rescan_carry.js`, EDL scope. Helper: `isVariant(row)` in a new `variant_rows.js`.

**Take records (full settings, N3, N16).** New fields go inside each `takeCaptures` entry, not in a new top-level key: `attachCaptures` and `carryCaptures` keep whole entries, so older builds keep them.

```jsonc
"takeCaptures": { "session_x/dodstudio_chain_01_b0": {
  "session_id": "", "captured_at": 0, "capture_mode": "", "capture_fps": 0, "take_folder": "",
  "settings": { "hash": "", "width": 0, "height": 0, "fov": 0, "install": "", "hook": "",
                "lines": [], "commands": [], "before": 0, "after": 0, "killGaps": "" }
} }
```

The same `settings` goes in `dodstudio_take.json` beside the take, so it travels with the folder. **Don't bump `take_meta::FORMAT`.** Fields are `#[serde(default)]`, and an older build ignores a file with a newer format, which means a frame sequence falls back to Source FPS and renders at the wrong speed (the #14 bug, `take_meta.rs:186`).

**Presets stay command profiles** (`CommandProfile`, #442's lists). Clip settings rows are an editor over `init_commands` lines, so there's no new preset format and settings.json doesn't change shape. Before and After in one row saves as two `CustomCommand`s with a shared `pair` id; an older build sees two commands.

**Project profile:** a saved copy of `settings` plus the project's `teams`, without `install`, kept in settings.json like `render_presets`, and exported as one small JSON file with `folders` removed. `new_project_profile` in settings.json names the one new projects start from.

**Cache versions.** None of this bumps `analysis::cache::SCHEMA_VERSION` or `HIGHLIGHTS_VERSION`: no scan result changes. Variant rows are not scan-filled, so `scanIsCurrent` stays. `settings_migrations` (settings.json) gets one step, for C9.

**In memory:** a batch works from a snapshot. Rows edited while it runs change the next batch (N3).

## Project-file compatibility

Chris moves between builds on one project, and WSOD25 is live with 42 ticked. What each build does on Save:

| New data | Build older than step 1 | Build from step 1 on |
|---|---|---|
| `settings`, `profile` (new top-level keys) | **Dropped.** Only the backup covers this | Kept, not used |
| Variant rows | Kept, shown as plain extra rows. Capturing one there uses plain settings, and it can merge with its clip | Kept, used if new enough |
| Fields inside `takeCaptures` | Kept | Kept |
| New fields in `dodstudio_take.json` | Dropped if that build renders the take (`update_history` rewrites the file). The project keeps its copy | Same, until each build knows them |
| Rescan in that build | Variant rows find no match and are lost; the log says so | Step 11 carries them |
| Move a demo in that build | A variant's saved `uid` keeps the old path, so its take link is lost on save | Step 11 handles it |

**Why step 1 comes first, before any new key exists:** a guard only protects builds that have it. Old builds can't be fixed, so the one cover for them is a backup.

- **Schema check on open.** `project_file.js` reads `schema` (missing = 1) and runs `project_migrations.js`, a list of small tested steps, as `analysis/src/cache/migrations.rs` does.
- **Keep what you don't know.** Unknown top-level keys are held on load and written back on save. That stops N4 for every later change.
- **Backup on the first save that raises `schema`:** `<name>.before-v<schema>-<date>.dodstudio` beside the file, reusing `rescan_recent.js`'s naming, and a toast says where. Never overwritten, never deleted by Studio.
- **Three writers, one rule:** `project_session.js:185`, `batch_catch_up.js:120` (keeps keys already: it edits the parsed file) and `rescan_recent.js`. Its once-per-update run saves many projects at once, each backed up first.
- **Newer file:** the header line says "Made with a newer DoD Studio", and the first save toasts that its new settings are kept but not used here (Q3).
- **One-time summary (N5):** "Updated: 364 clips, 45 takes found, 3 not found. Backup: [path]". Existing takes stay in their folders, with "Captured with: not recorded". `settings` is filled once from today's app settings at this point, with today's folders under this PC's id.

**Tests:**
- Vitest fixture `studio/src/test_fixtures/project_schema1.js`, shaped like WSOD25 with made-up paths: load, migrate, save, compare.
- Round trip through the frozen older save logic, as `take_record_before_1264.js` does, for each row of the table above.
- Unknown keys survive save, `batch_catch_up` and a rescan.
- Local only: `DS_ROUNDTRIP_PROJECT` points a test at a **copy** of the WSOD25 file, in old and new builds. Fails loudly with no file; never in CI.

**Your test before any step past 3:** copy WSOD25's project file, open it in the new build, save, reopen in the old one, and check clip count, takes and statuses.

**Rollback:** every step is additive, so `git revert` is safe from step 1 on: the reverted build keeps what it doesn't understand. Steps 4-14 change nothing for a project that sets nothing, which step 10's golden test proves.

## How the old UI keeps working

- **Steps 1-14 need no flag.** New data does nothing until something sets it, and the old table shows variants as rows.
- **Pages get one switch, `new_ui`** (settings.json, `#[serde(default)]`), shown only in debug and dev-channel builds, with the row `hook_dll_row.js` uses as the pattern. Off by default until step 20. It only picks which pages show; it never changes data.
- **Steps 6 and 8 are built once, in final form,** and open from File in the old layout, so they aren't drawn twice.
- **Old Configuration while a project sets things:** a read-only line, "This project sets: 1440p · 120 fps · Clean clips · Open Project settings". Not #1085's per-field chip, which would be work on a tab that goes away.
- **New pages render from their own modules** (`page_clips.js` and so on), not into `index.html`, which every open PR touches.
- **Rollback of the swap:** turn the default off again. Step 21 waits a full release, so a bad swap never has no way back.

## Steps

Each is one PR into `dev`, shippable alone. For all of them: activity-log lines from the first PR, text in the area's catalog, `main.js` gets one `createX` call, and anything over half a second shows a bar and a step line. Your test times are yours only; add CI at ~6 min a round.

### 1. Project-file safety
- **Files:** new `studio/src/project_file.js`, `project_migrations.js`; `project_session.js` (save and load), `rescan_recent.js` (shares backup naming), `project_log.js`, `strings.js`.
- **Data:** `schema`, `saveNeeds`. No new behaviour on an old file except the first-save backup.
- **Done when:** the Tests list above passes. **Your test:** 10 min, WSOD25 copy old to new to old.

### 2. Nothing pulls the rug during a batch
- **Bug today:** `isCaptureRunning` is only read by `close_guard.js`, and no check stops opening a project mid-batch. `markBlockCaptured` then writes takes into the new project's index and old streak objects, and `pruneTakeIndex` throws them away on save.
- **Fix:** new `batch_guard.js`. Open, New and Clear are greyed with a reason during a batch; Restart for an update waits ("Restart after the batch", N12).
- **Files:** `project_session.js`, `recent_projects.js`, `update_chip.js`, `updater_pane.js`. **Your test:** 5 min.

### 3. The lambda spray fix (#1322)
- **Probe done (2026-11-06):** the lambda reproduces at 30 fps or more on PRE and POST, fresh or after the batch's demo order, which changes nothing (C11). Recipe: `wsod25_grp4_h1_m00cat_dod_railroad2_test_1.dem`, 27:21:36 to 27:31:36, never a 4 fps clip.
- **Key on the message, not the pool:** the decal's pool slot changes by session; only index `0x6a` at `CL_ParseTEnt` is stable. Never call `Draw_DecalTexture` with the message's index: a slot not yet loaded is a fatal error.
- **Aim looks back about 0.3 s:** one of m00cat's lambdas is 25.8 degrees off on arrival and 3.7 degrees off just before, because he flicks away. `impulse 201` is in the demo for only one of his two.
- **Open call:** in `wsod25_grp4_h1_stealth.dem` the same lambda is 93 units away and 6.4 degrees off the recorder, stealthlol, standing beside m00cat, so the rule paints it there; the other 39 demos checked had no false match. Candidate tie-break: skip a recorder with a `TE_PLAYERDECAL` of their own. Check first who that demo's player byte 12 is.
- **Hook:** `goldsrc-hooks/src/spray_hltv.rs`, `spray.rs`. Pick out `TE_WORLDDECAL` index `0x6a` along the POV recorder's aim within 128 units; give it the recorder's table spray. Every other lambda gets the fallback or nothing. A `hw.dll` table needs a `BUILDS` entry for both engines and a `tools/verify_*_offsets.py`.
- **Studio:** `native/src/player_overrides.rs` (`pov_fallback` default, `own` removed), `players_ui.js`, `players.js`, `strings.js`, renamed rows; one `settings_migrations` step (C9).
- **Any time;** independent of the rest. **Blow-up:** +1-2 h if the decal path differs on POST. **Your test:** 15 min on both installs.

### 4. Project settings block and resolver
- **The trap:** the form is the store. `persistAppSettings` reads the DOM (`settings_sync.js`), so a project value put in the form becomes the app's value. About 20 files read `#config-*`.
- **So:** new `project_settings.js` and `project_settings_logic.js` hold `settings`. `resolveSettings(form, project)` is pure and tested. `batchSettingsPayload` (`capture_pane.js:593`) calls it last. Also: `capture_summary.js`, `capture_estimate.js`, `finish_clips.js:283`, `clip_name_ui.js`.
- **Rust:** `launch_config` takes values from saved settings at five call sites (`game_process_manager.rs:142,232,297`, `demo_preview.rs:252`, `review_manager.rs:167`), so Review and Launch Preview would start the game at the app's size and a reuse check would then ask to relaunch. Pass the resolved size in the request; the field exists.
- **Install offer:** "This project was made with POST. Switch?", never silent.
- **Data:** `settings`, per decisions 1, 4 and 6. Folders keep today's same-drive and free-space checks. **Your test:** 10 min with a hand-edited copy; no page yet.

### 5. History with full settings
- **Early on purpose:** only takes captured after this have the record.
- **Files:** `take_record.js` (`CAPTURE_FIELDS`), `take_index.js` (`markBlockCaptured`), `native/src/hlcr/take_meta.rs`, `studio/src-tauri/src/capture_manager.rs:1010` (`record_capture_settings`), `capture_history_logic.js`. `batch_catch_up.js` gets the same record from the manifest (`manifest_file.rs`).
- **Hash:** sorted lines plus the other settings, so N3 can compare. A take with none is "not recorded" and is never flagged.
- **Render history** already exists (`RenderAttempt`); this adds nothing there.
- **Blow-up:** none. **Your test:** 5 min, capture one clip and read History.

### 6. Project settings page
- **Canvas:** Project settings as a page with tabs (decision 2), except Profile (step 9). The Clip settings and Players and sprays tabs arrive at steps 8 and 7. Before the first kill and After the last kill map to Start Lead and Stop Trail; pre-roll and post-roll become automatic only after the roll check is tested (`review.md` section 5, rows 92 and 94).
- **Needs R4, R14, R16, S9, S18 from the fix list.** R5 is replaced by decisions 1 and 5. AGR's own rate shows when Capture with is AGR.
- **Files:** new `project_settings_page.js`; pending edits for all tabs live there until OK or Apply. `app_shortcuts.js` gets one off switch. Opened from File › Project Settings now, from the project switcher at step 19.
- **Your test:** 20 min.

### 7. Players per project
- **Trap:** `players_tab::mirror_table` runs on every settings save (`lib.rs:131`) and at load (`settings_manager.rs:746`), so a settings save would put the app's table back over the project's. It must write the effective table.
- **Also:** mirror on project open and close, and read `use_in_game` from the payload, not saved settings (`capture_manager.rs:1303`). Keeps #686's "Use as" names in the same table.
- **Hook:** none. It rereads the file when its time moves, which is why step 2 comes first.
- **Move (decision 7):** the page becomes a tab of Project settings; it's read-only during a batch from this step.
- **Add by SteamID (D11):** "+ Add player" also takes a SteamID in any of the three forms, and nothing else: no name or alias on a manual add. The row shows "Not in any demo yet", then the name once a demo matches it. `steam_ids.js` only converts from SteamID64 today, so it gains the classic and SteamID3 forms.
- **Files:** `players_tab.rs`, `settings_manager.rs`, `players_ui.js`, `native/src/player_overrides.rs`. **Your test:** 15 min, two projects, one clip each.

### 8. Clip settings rows and presets
- **Files:** new `clip_settings.js`, `clip_settings_logic.js`, `clip_settings_rows.js` (the 38 rows, each with its cvar, load or at once). `command_profiles_ui.js` stays for the old tab.
- **Order, later wins:** game configs, your own commands, Clip settings rows, then (step 12) What's different and add-ons.
- **Picture wins over presets:** Clean clips carries `mirv_fov 90` and `r_decals 256` (`builtin_profiles.rs:30`). Field of view moves to Picture; the resolver drops the preset's line. Changing Clean clips means adding to `CLEAN_CLIPS_SHIPPED`.
- **N9, preset life:** nothing links to a preset (decisions 9 and 10), so Delete just deletes, and an edit reaches only what uses it next. An in-game change on a built-in makes "Clean clips (edited)" with a toast.
- **Tiers:** every line goes through `cfg_scan` as today; scan, don't ban, `ds_` commands. No new internal command, so no new tier.
- **Files also:** `builtin_profiles.rs`, `cfg_warnings.js`, `strings.js`. **Your test:** 20 min, a clip with and without a row.

### 9. Project profiles (#444)
- Canvas row (decision 3): dropdown, Save, Delete, ⋯ Save as new, Import, Export, the window listing sections. "(edited)" as before. Settings · App gets "New projects start from".
- Import runs `cfg_scan` and ends with "2 things in this profile aren't on this PC" (N9), sprays and crosshairs counted too. Export removes `folders`.
- **Files:** new `project_profiles.js`, `settings_manager.rs` (`project_profiles: Vec<serde_json::Value>`). **Your test:** 10 min, export and import.

### 10. Batch planner, seek back and a dry run
- **Why now:** `build_batch_queue` has side effects (deletes cfgs, makes folders), so the tick bar can't ask it for a cost.
- **New:** `native/src/patch/builder/passes.rs`, pure. Groups by demo, player and load settings, then orders each playthrough so a variant follows its original. Only a load-only setting (step 13's list) gets its own load. `group_highlights` (`blocks.rs:31`) gets this third key.
- **Hook (Q5), PRE and POST:** `rewinds()` in `demo_seek.rs` drops its `hltv` condition, the rewind starts at the demo's first recorded frame rather than the clock's start, and `batch.rs`'s `Cursor` allows a backward seek (`Cursor::due`, about line 236). A batch that seeks back must check the hook's build first (as #1146 does for reuse).
- **`Cursor` isn't the whole change:** `batch.rs`'s `parse` sorts each demo's entries by time, which scrambles a second pass, so a backward seek has to end a pass. About 30 lines plus tests, and one existing test goes.
- **Tested (2026-11-06):** a real `ds_capture_batch` captured Original, Variant with wallhack, then Original again, POV and HLTV, PRE and POST. The first-frame rewind took 1.0-2.6 s on 5 POV demos from 5 players, against 10-14 s for a reload. Sprays stay right across a rewind on 2 players' demos of one match, with Flush Decals on; HLTV sprays weren't tested.
- **Line-up:** at shipped timing, takes start up to a frame apart, so an Original and Variant match to the frame only if the batch pins the rate. A flickering light can come out a different brightness after a rewind (armory), as a reload can.
- **Rewind depth:** always from the first frame. Round start saves about 1 s, needs a round table and gives nothing on a one-round demo; 60 s back was wrong in 3 of 4 runs.
- **Flush Decals off:** a rewound take keeps decals a fresh one wouldn't, so a batch that seeks back warns. It doesn't stop.
- **`blocks_merge` never joins a variant** to anything, its original included (`blocks.rs:137`, `mod.rs:291`).
- **`plan_batch`:** a `#[tauri::command(async)]` returning playthroughs, seek-backs, seconds and bytes. `capture_estimate.js` and `estimate_batch_capture_blocks` both read it, so there's one source.
- **Golden test:** today's payload gives the same batch file and helper cfg, byte for byte. This is what keeps WSOD25 capturing through the rework.
- **Your test:** none for a batch, it must look the same. 10 min on Review's back jumps if Q9 is yes, POV on PRE and POST.

### 11. Variant rows
- **Files:** new `variant_rows.js`, `variant_rows_logic.js`; `detail_pane.js` ("↳ name", a Duplicate as variant action), `split_carry.js`, `rescan_carry.js`, `project_demos.js` (`relocateDemo` rewrites saved uids), `queue_counts.js`, `review_queue.js`, `clip_name.js` (`{variant}`, R16). Rust: `SerializedStreak` and `CaptureStreak` get `variant`.
- **Rescan (N11):** carry the clip, then its variants. No match keeps old times and a "Kills changed · Check" chip. A range is clamped with a note.
- **Hook route only.** A variant batch that can't run from the hook stops with the reason rather than patching (`capture_manager.rs:1438,1544`). #434 removes the patch route anyway, so it gets no new feature.
- **Capture:** one kill range, its own take, status and clip name, same settings as the project. **Your test:** 20 min, a clip and a 3-3 variant in one batch, on PRE and POST. Must capture both in one playthrough, two takes, right rows.

### 12. Per-clip commands and add-ons
- **Files:** new `native/src/patch/clip_layers.rs` (what each clip and job runs, one place), `schedule.rs:399` (`push_custom_commands` reads the clip's list too), `first_banned_command_error` (`capture_manager.rs:1218`) scans each variant, `note_if_runs_during_fast_forward`, `roll_floors.js`. New `add_ons.js`.
- **Layers:** a variant's rows and add-ons are the clip's Before and After. A load-only row needs a load of its own (step 13). Before over the lead-in gets the "Runs before the clip starts recording" note (S7).
- **The leak (tested):** all four `mirv_` cvars keep their value across reloads and other maps. So:
  - **Union rule:** each job's init states every key any job in the batch touches, or a partial preset carries into the next job.
  - The batch's `end` lines state them too.
  - A batch stopped between Before and After leaves the value on, so the next batch doesn't reuse that game (`batch_reuse.rs`).
- **`mirv_movie_fps` stays a select, not a typed command:** it must land before the clip's `mirv_recordmovie_start`, so Studio places it (step 14).
- **Wallhack add-on:** `mirv_fx_wh_enable` only. `mirv_fx_xtendvis` showed nothing at 5 spots.
- **Fast-forward gaps on a variant:** `schedule_clips` reads the clip's own value.
- **Watch in game on any row (N10):** a one-clip batch file with the record lines left out. 45 min of this step.
- **Your test:** 20 min, the Wallhack add-on on one variant, a plain clip after it on PRE and POST, with a look at the next clip for a leak.

### 13. A batch stage before the demo loads
- **Why:** init lines run after the map has loaded (`batch.rs:519-529`), so `ds_hd_enabled`, `ds_hd_style` and `ds_map_run_cfgs` reach only the next load. Sprays are a half-case.
- **Hook:** a new batch line `load <command>`, run in `go_to` before `viewdemo` (`batch.rs:418`). No engine offsets, so one table, tested on PRE and POST. No new console name.
- **Studio:** `runtime_batch.rs` writes it, and only when a job needs it. **An older DLL's `parse` stops on a line it doesn't know** ("unknown line"), so writing it only when needed keeps ordinary batches working on a test DLL, and a variant batch must check the hook's build first (as #1146 does for reuse).
- **The next job restates its own values,** and `end` puts the project's back.
- **Per-variant crosshair or spray ("Crosshair for Nifty"):** the hook only reads the table, and `ds_player_spray` is refused. This needs a table per job, a new `ds_capture_players` (refused everywhere, a tier in `cfg_scan.rs` and `command_suggest.js`, plus `Commands.txt`, `console_commands_data.js`, `docs/dodstudio_commands.md`, `NEW_SINCE_555`). Its own small PR, 1-2 h, only if you want those two rows in the first release.
- **What a reload reaches (tested on 2 maps):** HD off reaches walls, floors, sky and detail. `ds_hd_style` reaches walls only; detail, models and sprites keep the old style until a restart. Models and sprites never change without a restart. So the HD style row carries that note, and Classic uses the C2 label.
- **POST trap:** the in-game panel's saved settings put `ds_hd_enabled` and `ds_hd_style` back at start-up, over the launch line. The `load` line must send them before the first `viewdemo`.
- **Your test:** 15 min, a variant on another HD style. Must show on walls, and the next clip must be back.

### 14. Per-clip frame rate and Capture mode in one batch
- **Idea (Q5, Q6):** one batch, one launch, one manifest, with rate and mode on each `CaptureBlock`. No parts.
- **Rate on HLAE (tested, PRE and POST):** 60, then 300, then 60 between clips, set 0.6 s before each start; frame counts match. `builder/mod.rs:130` moves from the job to the block, and the `cfg_scan.rs` hazard text changes.
- **Rate on OBS:** `SetVideoSettings` works between recordings with no restart and is refused during one. Tested 2026-11-06 with Game Capture: 6-21 ms, and the source stays hooked. OBS rescales scene items when the canvas changes, so set bounds after the canvas, and check `provision.rs` keeps that order.
- **Launch:** `-wavonly` when any clip uses OBS. It changes nothing in HLAE's `sound.wav`, so the whole batch uses that one launch. OBS is started and checked at the batch's start (decision 12). OBS answers 207 (not ready) for about 1 s after its socket opens, and Studio has no handling for that code, so the check waits.
- **Mode between clips (tested 2026-11-06):** every pair of Frames, Video, AGR and OBS works in one game with no demo load, PRE and POST. The record aliases are written once per batch (`helper_cfg.rs:190-206`), so they go per clip, and that needs the hook-run batch: the FFmpeg options line has quotes and runs over 64 bytes, so an alias or a demo frame can't carry it.
- **OBS sound offset:** the -270 ms offset is unreliable, with takes at about +10 or +300 ms, sometimes jumping within a take. Mode switching isn't the cause, and the cause isn't known. A decision for Chris; it isn't a blocker for this step.
- **Per clip, not per batch:** `record_capture_settings`, `take_meta`, `short_take`, `finish_clips.js`, `capture_estimate.js` and `block_byte_estimate`.
- **Files also:** `capture_pane.js`, `capture_progress.js`, `render_while_capturing.js`. Variant fields `captureMode`, `captureFps`.
- **Blow-up:** mode switching in one game, and the footer's states. **Your test:** 20 min, a 60 fps project with one 300 fps variant and one AGR variant, on PRE and POST, then one OBS variant.

### 15. Shell and tokens
- `:root` tokens, sidebar, top bar, the `new_ui` switch. New `page_shell.js`. Light and dark can follow (#49, the theme system); the main colour is still open.
- **Canvas fix list R1-R16, S1-S18 and P1-P14 comes first:** the desktop agent applies it, and these steps read the canvas.
- `styles.css` is hand-copied by `web-analyzer/www`. Tokens go in a new `tokens.css`, so the website doesn't drift.

### 16. Clips page with tick bar
- **Tick bar:** ticks are `selected`. Count, disk, time and "Adds 12 seek-backs" come from `plan_batch`, debounced. R10: a variant under a filter shows its clip's name; "Tick all" ticks what's shown, and the bar says "3 ticked, 1 hidden".
- **Row states:** R11, R12 (AGR's Captured is final), N3 "Settings changed · Capture again", N6 "Take not found · Find it…" and "Drive E: not connected", S2 one status set, "Bring back" on a cleared row (N14).
- **Scale (N8):** Sort by, Player filter, draw one page at 364 rows. With no ticks: "Capture all 318 · 1.9 TB", asks first above about 20.
- **Which video (N7, C6):** newest good take is main, "Use this take" in History. Play, Combine and Export use it.
- **Merged takes (N15):** the Delete prompt names the other clip in that take.
- **Files:** new `page_clips.js`, `clips_table.js`, `tick_bar.js`, `tick_bar_logic.js`, `variant_drawer.js`. **Your test:** 30 min at real size.

### 17. Capturing, rendering, finished
- Flat list, each row showing its own rate and mode, reasons on stops, R8 start checks with the config warnings, R9 OBS on close, S14 batch done.
- **Stop (N13):** "Stop after this clip · Stop now" and "Render the 19 captured?". Cut-short takes appear in History with Move to Recycle Bin.
- **N16:** the install in Captured with, a start check for a trimmed copy on the wrong install. **Your test:** 20 min.

### 18. Demos page and Review
- Keep sets Pending (a real behaviour change: today no review answer touches Status). Review uses the project's Clip settings (the proposal). R13: Play order and the long-gap line.
- **N14:** the prompt says what goes. "Their clips leave the project. Takes and videos stay on disk." The Demos chip never removes a clip with variants or notes without asking.
- **Your test:** 20 min, one in-game review on each install.

### 19. Settings · App, Setup, Help, pop-ups, tools
- R5, R6, S17. N17 not-found states. Project switcher. The 20-odd pop-ups, with Visible progress for each S5 item.
- **Auditor pages wait for #874 and #879** (see below). **Your test:** 30 min.

### 20. Turn on for everyone
- Default `new_ui` on. Rewrite the e2e specs that drive old pages: 64 spec files, about 9,900 lines, and at least 14 reference the capture and config ids. `app-boot.spec.js` first. That rewrite is most of this step.
- Docs: `docs/features.md`, `README`, every old tab name (rename rule), issue numbers off visible text (C10).

### 21. Remove the old pages
- One release later. Old tabs, `command_profiles_ui.js`'s old tab, `render_table_height_px` and the like become unused fields.

## Frame rate on a variant

**The ask:** one variant at 300 fps in a 60 or 120 fps project, with no reload for the rate alone. Option B is chosen and tested; the table is what it changes.

**Traced on dev:**

| What | Today | Per-variant rate means |
|---|---|---|
| HLAE rate | `mirv_movie_fps` in every job's init (`builder/mod.rs:130`), batch-wide | Per clip, set before each `mirv_recordmovie_start`. Tested. The hazard at `cfg_scan.rs:406` is Studio's assumption, not an engine limit |
| `host_framerate` | AGR only, per block in the route alias (`helper_cfg.rs:229`). HLAE's movie modes pin the timestep themselves | Nothing for Frames or Video; AGR takes its rate per block |
| OBS | Real time, set in OBS for the session, `fps_max` follows (`mod.rs:157`). 300 at 1440p lost 98.5% of frames | Set between recordings, tested with Display Capture. Hardware limits warn, never grey (decision 12) |
| Take record | `record_capture_settings` stamps `manifest.capture_fps` on every block (`capture_manager.rs:1010`) | Goes per block |
| Render | Reads the take's own file (`take_meta::render_fps`), so the render is right if the record is. `finish_clips.js:350` passes one batch rate | Same |
| Disk and time | `block_byte_estimate`, `capture_estimate.js`: one rate | Sum per clip. About 1 GB a game second at 300 fps |
| Take checks | `short_take(block, capture_fps)`, OBS drop tracker: one rate | Same as the record |
| Demo reloads | One job per demo and player | None for the rate |
| VEGAS | `jobFps` reads each render's own line, one row per clip, PlayRate 1 | Works. A 300 fps event in a 60 fps project needs your ctrl-drag and Frame Blend, as now |
| Combine | Mixed rates can't stream-copy, so it re-encodes at the first clip's rate (`combine.rs:186,263`) | The slow-motion frames are thrown away quietly. Say so in the Combine prompt |

**Dropped: Option A** (own part of the batch, a demo load per demo) **and Option C** (a second project at 300 fps).

**Option B, chosen: rate per clip in one batch.** A select on the variant: Same as project · 60 · 120 · 300 · Custom. One batch and one manifest, with the rate on each `CaptureBlock`. The record, estimates, checks and `finish_clips.js` go per clip, and time and disk add up per clip. AGR's variant uses the same row (R14).


**Needs on the canvas:** the select and its OBS warning on StudioVariant, per-clip rates on StudioCapturing, and R4's side note drops frame rate from the project-wide list.

## Open PRs

| PR | Touches | Collision | Do |
|---|---|---|---|
| #1292 long names and narrow windows | `detail_pane.js`, `styles.css`, `strings.js`, e2e | Step 11 edits the same row template; the e2e spec goes at step 20 | **Merge before step 4.** Old screens live for months, so its fixes earn their keep |
| #1293 draggable panels | `settings_manager.rs` (4 fields), `settings_sync.js`, `layout_splits.js`, `index.html`, `styles.css` | Next to step 4 in `settings_sync.js`, only a spread. Render and Auditor heights become dead fields at step 21; Overviews' stay | **Merge before step 4** |
| #1303 Export Project to VEGAS | `render_pane.js` (+5), `index.html`, `project_log.js`, `vegas_edl_export.js`, new `project_vegas_*`, `strings.js` | Step 1 adds to `project_log.js` (trivial). "Every render of a highlight goes in" will include variants, and C6 narrows that at step 16. Its button moves to the Export menu at step 20 | **Merge first of these three** (oldest first), keep its logic, change scope at step 16. If "all or ticked" (its own question) is still open, C6 answers it |
| #879 Split status table | `split_pane.js`, `split_status.js`, `demo_split_manager.rs`, `strings.js`, `styles.css` | None with the data. Split must carry variants (step 11, `split_carry.js`, which it doesn't touch). Step 19 restyles that page | **Can wait** for the Auditor review. Step 19's Auditor page goes after it |
| #874 Consolidate demos | `main.js`, `main_js_budget.test.js` (526), `project_demos.js`, `ipc_bridge.js`, `pick_path.js`, `index.html`, `strings.js`, Rust | `main.js` budget: each step raises the same line, so a text conflict. `relocateMovedDemos` calls `relocateDemo`, which step 11 changes in place, so it inherits the fix | **Can wait.** Whichever lands second renumbers BUDGET with its reason. Step 19's Auditor page goes after it |

**The rest:** #1306 (game opens in front), #1311 (review form clicks on POST) and #1294 (British head icon) touch none of this. #1317 (reload a light file) and drafts #1047 (menu background), #906 (POV overview) and #882 (restyled HUD) regenerate `console_commands_data.js`, and only step 13's optional command would meet them. Draft #622 (Highlight Finder) needs room in the Demos filter row. Draft #425 (second upscaler) touches `hd_pane.js` only.

**Order:** #1303, #1292, #1293, then step 1. PR count stays under the cap, since steps merge before the next starts.

## Issues to update or file

List only; nothing is filed.

**Update:**
- #1085 (a project's own settings): replace "App default or This project" with the decisions above, "Capture profile" is now Project profile, safety step first, the four traps in steps 4 and 7. Its "no new version needed" holds for reading, not for older builds saving.
- #45 (variants): post the draft comment in `45-tests.md`; add the row model, Q5 and the seek-back results. Priority-low no longer fits.
- #444 (profiles): holds Picture, Clip length, Clip settings, names, HD style, folders, Players and Teams; folders never exported; depends on steps 6, 7 and 8. The Configuration dropdown text is out of date.
- #442 (command profiles, closed): its "a variant row can use a profile" part moved to #45.
- #1322 (lambda spray): the probe first; C9.
- #434 (retire patching): the batch file's `load` line, and that variants are hook-route only.
- #450 (AGR as a second pass): "Keep the original and add an AGR variant" replaces it. Close or reword.
- #600 (fewer settings tabs): replaced by the facelift.
- #686 ("Use as" names): goes with step 7.
- #1264 (project export): variants' scope, C6.
- #108 (render presets): against Finished clips' Custom.

**File (deliberately deferred or not yet tracked):**
- Project can be opened during a capture batch, losing the take links (step 2).
- A POV seek back jumps with no replay, so Review shows wrong flags and scores (step 10, if Q9 is yes).
- POST crashes at start-up, third time (`hw+0x280403` family), and a same-session reload of an HLTV demo crashed or exited POST twice. A rewind avoids that reload.
- Per-variant crosshair and spray need a table per job (step 13, optional).
- Watch in game on a Clips row (N10).
- Takes that moved or sit on an unplugged drive (N6).
- Report gunfire clipping in the take check (N18).
- Combine says when mixed frame rates are re-encoded.
- A variant on a Moment (S10), once Moments exist.

## Risks nobody listed

| Risk | Cover |
|---|---|
| Opening a project during a batch loses that batch's take links today, and with step 7 would swap the Players table mid-capture | Step 2, before step 7 |
| Every settings save rewrites the hook's Players table with the app's | Step 7 |
| The form is the store, so project values could leak into the app's settings | Step 4 never writes them to the form |
| Review, Preview and Launch Game start the game at app settings | Step 4 |
| Bumping `take_meta::FORMAT` makes older builds render at the wrong speed | Never bump for added fields |
| An older build that renders a new take strips its settings from the take file | The project holds a copy |
| A nested variant list loses its takes in an older build's prune | Variants are rows |
| A partial preset leaks into the next job | Union rule, step 12 |
| A stopped batch leaves a wallhack on in a reused game. The tests prove values survive reloads | No reuse after a batch stopped inside a clip |
| A variant's saved `uid` holds the demo path | Step 11 rewrites it on a move |
| Clean clips still holds `mirv_fov`, and the decal flush reads `r_decals` back from init lines (`ring_limit_from_init`) | Step 8 keeps `r_decals`, moves only field of view |
| Hand-copied `styles.css` and `strings.js` on the website drift | `tokens.css`, step 15 |
| The test rewrite is hidden work: 64 e2e spec files drive old ids | Named in step 20 |
| `main.js` is at 519 of 520 lines, and 12 steps add wiring | Raise BUDGET with a reason in each PR. Never squeeze |
| `index.html` and `strings.js` conflict across long-lived branches | Pages in own modules; one step at a time |
| A 300 fps H.264 "Small" render hasn't been tried | Try it in step 14's test |
| The first-frame rewind is proven on one demo, and a real batch hasn't sought back | First checks in step 10, on several players' demos |
| Sprays across a rewind aren't proven: none drew on screen, fresh or rewound | Step 11's test looks at a sprayed wall with Studio's PNG sprays |

## Where every Not-thought-of and For-Chris item went

| Item | Goes to | Item | Goes to |
|---|---|---|---|
| N1 cost of a variant | Steps 10, 16 | N14 removal | Steps 16, 18 |
| N2 mode per batch | Step 14, Q5 | N15 merged takes | Step 16 |
| N3 settings changed | Steps 5, 16; snapshot in the data model | N16 which install | Steps 5, 17 |
| N4 older build | Step 1, Q3 | N17 first-time player | Step 19 |
| N5 old project | Step 1, Q2 | N18 sound | Step 8 (C8), issue for the take check |
| N6 moved take | Step 16 | C1 frame rate | Q6, step 14 |
| N7 which video | Step 16 | C2 Classic textures | Table, step 13 |
| N8 real size | Step 16 | C3, C4 | Q5 |
| N9 preset life | Steps 8, 9 | C5, C6 | Table, steps 5, 16 |
| N10 try a variant | Step 12 | C7 | Q4 |
| N11 rescan | Step 11 | C8, C9 | Table, steps 8, 3 |
| N12 update | Step 2 | C10 | Table |
| N13 stopping | Step 17 | C11 | Step 3 |

The fix list R1-R16, S1-S18, P1-P14 is the desktop agent's, before step 15; the build steps name the ones they depend on.

## Checked, and what wasn't

- **Canvas-check claims used here, read in the code:** R1 (`batch.rs:236`), N2 (`helper_cfg.rs:190`, `launch.rs`), N4 (`take_record.js:5-10`), R12 (`take_meta.rs:85`), N12 (no batch check in the updater).
- **One correction:** N5 says a re-capture overwrites the row's record. It doesn't: a take key is session folder plus block (`paths.rs:261`), and each batch has a new session, so dev already keeps several takes per row, and History lists them.
- **R3:** its replacement text says walls, sky and detail. Only walls were tested; sky and detail weren't checked separately. Use "Walls only", or test them.
- **#45 tests, final:** hook finds the four cvars, values survive reloads and other maps, scheduled lines change the picture and HLAE's recording, on PRE and POST. **Not tested:** a hook restoring a value, a real `ds_capture_batch` run with these lines, HLTV demos, wallhack with players behind walls. Steps 12 and 13 cover the batch run.
- **Map-load list, final:** of 38 rows only `ds_hd_enabled`, `ds_hd_style` and `ds_map_run_cfgs` act at a load, sprays are a half-case, and `ds_map_apply_lighting` is at once.
- **Estimates** use the repo's measured bands (my time only, from `time-estimates.md`). Nothing here was tried in the app or game.
