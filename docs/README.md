# docs/

Reference for people and coding agents working on DoD Studio. Work in progress lives in GitHub issues and PRs, not here. Where a doc and the code disagree, the code wins; fix the doc.

## Start here
- [features.md](features.md) — what DoD Studio does today, page by page.
- [app_architecture.md](app_architecture.md) — how the Tauri app, `native` and the parsers fit together, and the rules they follow.
- [coding_standards.md](coding_standards.md) — how code here is written: one area per file, one task per function, names, types, errors, duplication, tests.
- [goldsrc_dod_quirks.md](goldsrc_dod_quirks.md) — engine and DoD behaviour the pipeline depends on. Read before touching capture or patching.
- [vac_safety.md](vac_safety.md) — what keeps the hook DLL away from VAC-secured servers.

## Capture and render
- [hlae_protocols.md](hlae_protocols.md) — launching HLAE, how a batch ends, checking a take.
- [command_tiers.md](command_tiers.md) — which commands Initial and Scheduled Commands refuse, warn about or ignore, and why.
- [direct_to_video_capture.md](direct_to_video_capture.md) — capturing straight to video with `mirv_movie_ffmpeg`.
- [obs_alternate_capture.md](obs_alternate_capture.md) — OBS as a capture method: the settings Studio gives OBS, OBS measured against HLAE, how it is driven, failure modes, open questions.
- [vegas_nameplates.md](vegas_nameplates.md) — each player's nameplate picture on its own track above their clips in VEGAS Pro: setting it on the Players tab, the clip list the EDL export saves, and the VEGAS script (#429).

## Demo analysis
- [demo_analyzer_load_performance.md](demo_analyzer_load_performance.md) — making the Demo Analyzer open demos fast.
- [position_data_in_demos.md](position_data_in_demos.md) — what player position data demos carry (fields, rates, POV vs HLTV coverage) and what stats it allows (#448).
- [demo_stats_feasibility.md](demo_stats_feasibility.md) — which league stats a demo can and cannot give (#192).
- [match_and_half.md](match_and_half.md) — telling a demo's match and half from its contents, and the rules behind it (#685).
- [goldsrc_demo_salvage.md](goldsrc_demo_salvage.md) — how a demo is put together, how Studio repairs damaged ones (`analysis::repair`) and the R&D behind it, and the engine's entity-packet flush rule (#15).

## The hook DLL (`goldsrc-hooks`)
- [goldsrc-hooks/README.md](../goldsrc-hooks/README.md) — what each module does, how to build the DLL and test it in the game.
- [dodstudio_commands.md](dodstudio_commands.md) — every `ds_*` cvar and command. Also the source of the wiki's Commands page.
- [goldsrc_client_dll_internals.md](goldsrc_client_dll_internals.md) — how DoD's `client.dll` is wired and how the hook reaches it.
- [goldsrc_client_dll_survey.md](goldsrc_client_dll_survey.md) — what in `client.dll` can be controlled.
- [goldsrc_hw_dll_survey.md](goldsrc_hw_dll_survey.md) — what in `hw.dll` HLAE already owns, how the engine reads a `playdemo` demo, and what is left.
- [goldsrc_viewdemo.md](goldsrc_viewdemo.md) — how `viewdemo` plays and seeks a demo.
- [goldsrc_linux_debug_symbols.md](goldsrc_linux_debug_symbols.md) — Valve's own names from DoD's unstripped Linux builds: the `client.dll` name map, user message layouts, and how far to trust them (#1127).
- [goldsrc_linux_symbols.md](goldsrc_linux_symbols.md) — Valve's unstripped Linux engine (`sw.so`, `engine_i486.so`, `demoplayer.so`): how to fetch them anonymously and name `hw.dll`'s functions from them (#1133).

### Per feature
- [goldsrc_death_notices.md](goldsrc_death_notices.md) — the kill feed (`ds_hud_deathmsg`).
- [goldsrc_decals.md](goldsrc_decals.md) — clearing decals at runtime.
- [goldsrc_ex_interp.md](goldsrc_ex_interp.md) — raising the interpolation ceiling.
- [goldsrc_hltv_animation_fix.md](goldsrc_hltv_animation_fix.md) — gun animations in HLTV demos.
- [goldsrc_hltv_missing_gunshots.md](goldsrc_hltv_missing_gunshots.md) — putting back the gunshots HLTV and POV demos lose (`ds_sound_restore_gunshots`).
- [goldsrc_hud_suppression.md](goldsrc_hud_suppression.md) — the shared pattern for hiding or silencing what DoD draws, plus voice-command muting.
- [goldsrc_crosshair.md](goldsrc_crosshair.md) — hiding the crosshair, and the spectator crosshair under `spec_match_pov`.
- [goldsrc_player_info.md](goldsrc_player_info.md) — `client.dll`'s per-player record `g_PlayerExtraInfo` (layout, who writes `health`), what POV and HLTV demos fill in it, and the name plates drawn from it (#1130).
- [goldsrc_hud_elements.md](goldsrc_hud_elements.md) — the HUD elements' vftable stub (`ds_hud_hide_teammate_info`), what each element draws, and where `ds_hud_hide_element`'s names went when #342 retired it.
- [goldsrc_objective_icons.md](goldsrc_objective_icons.md) — placing the objective icons.
- [goldsrc_scoreboard.md](goldsrc_scoreboard.md) — hiding the scoreboard.
- [goldsrc_spectator_bars.md](goldsrc_spectator_bars.md) — the spectator top and bottom bars.
- [goldsrc_spectator_camera.md](goldsrc_spectator_camera.md) — keeping the camera on one player, the in-eye camera on a prone or crouching player (#329, #1144), the death cam when he dies, and the chase cameras drawing the players around them (#918).
- [goldsrc_xray.md](goldsrc_xray.md) — X-ray (`ds_spec_xray_show`): what CS:GO's GOTV X-ray does, and how DoD Studio draws players through walls with the game's glow shell (#1297).

## Project
- [versioning_and_releases.md](versioning_and_releases.md) — version numbers, channels and how a release is cut.
- [third_party_notices.md](third_party_notices.md) — the libraries whose licences ask for more than a licence file, or aren't open source (UnRAR, for reading .rar files).
- [../tools/agent-kit/](../tools/agent-kit/README.md) — dev-only helpers for testing the app: the shared game lock, playing demos to the end on PRE and POST, a throwaway Studio driven over CDP, frame checks and the comparison-video stopwatch, the activity log, the open-PR board, issue audits, open PRs by file, demos and kill moments from the analyzer cache, the hook log by game run, and HLAE's command names.

## Folders
- [wiki/](wiki/) — source for the GitHub wiki, published when `main` changes.
- [archive/](archive/) — finished design records that code still cites, and superseded research (spectator bars R&D, the OBS design, the 2026-08 capture/render UX audit, the HLCR parity notes), the #1107 codebase audit findings (`archive/codebase_audit_1107.md`). History, not current state.
