# docs/

Reference for people and coding agents working on DoD Studio. Work in progress lives in GitHub issues and PRs, not here. Where a doc and the code disagree, the code wins; fix the doc.

## Start here
- [features.md](features.md) — what DoD Studio does today, page by page.
- [app_architecture.md](app_architecture.md) — how the Tauri app, `native` and the parsers fit together, and the rules they follow.
- [goldsrc_dod_quirks.md](goldsrc_dod_quirks.md) — engine and DoD behaviour the pipeline depends on. Read before touching capture or patching.
- [vac_safety.md](vac_safety.md) — what keeps the hook DLL away from VAC-secured servers.

## Capture and render
- [hlae_protocols.md](hlae_protocols.md) — launching HLAE, how a batch ends, checking a take.
- [command_tiers.md](command_tiers.md) — which commands Initial and Scheduled Commands refuse, warn about or ignore, and why.
- [direct_to_video_capture.md](direct_to_video_capture.md) — capturing straight to video with `mirv_movie_ffmpeg`.
- [obs_alternate_capture.md](obs_alternate_capture.md) — OBS as a capture method: how it is driven, measurements, failure modes, open questions.

## Demo analysis
- [demo_analyzer_load_performance.md](demo_analyzer_load_performance.md) — making the Demo Analyzer open demos fast.
- [demo_stats_feasibility.md](demo_stats_feasibility.md) — which league stats a demo can and cannot give (#192).

## The hook DLL (`goldsrc-hooks`)
- [dodstudio_commands.md](dodstudio_commands.md) — every `dodstudio_*` cvar and command. Also the source of the wiki's Commands page.
- [goldsrc_client_dll_internals.md](goldsrc_client_dll_internals.md) — how DoD's `client.dll` is wired and how the hook reaches it.
- [goldsrc_client_dll_survey.md](goldsrc_client_dll_survey.md) — what in `client.dll` can be controlled.
- [goldsrc_hw_dll_survey.md](goldsrc_hw_dll_survey.md) — what in `hw.dll` HLAE already owns, and what is left.
- [goldsrc_viewdemo.md](goldsrc_viewdemo.md) — how `viewdemo` plays and seeks a demo.

### Per feature
- [goldsrc_death_notices.md](goldsrc_death_notices.md) — the kill feed (`dodstudio_deathmsg`).
- [goldsrc_decals.md](goldsrc_decals.md) — clearing decals at runtime.
- [goldsrc_ex_interp.md](goldsrc_ex_interp.md) — raising the interpolation ceiling.
- [goldsrc_hltv_animation_fix.md](goldsrc_hltv_animation_fix.md) — gun animations in HLTV demos.
- [goldsrc_hltv_missing_gunshots.md](goldsrc_hltv_missing_gunshots.md) — putting back the gunshots HLTV demos lose.
- [goldsrc_hud_suppression.md](goldsrc_hud_suppression.md) — the shared pattern for hiding or silencing what DoD draws, plus voice-command muting.
- [goldsrc_crosshair.md](goldsrc_crosshair.md) — hiding the crosshair, and the spectator crosshair under `spec_match_pov`.
- [goldsrc_hud_elements.md](goldsrc_hud_elements.md) — hiding any HUD element by name (`dodstudio_hide_hudelement`), and why some are not offered.
- [goldsrc_objective_icons.md](goldsrc_objective_icons.md) — placing the objective icons.
- [goldsrc_scoreboard.md](goldsrc_scoreboard.md) — hiding the scoreboard.
- [goldsrc_spectator_bars.md](goldsrc_spectator_bars.md) — the spectator top and bottom bars.
- [goldsrc_spectator_camera.md](goldsrc_spectator_camera.md) — keeping the camera on one player, and the in-eye camera on a prone player.

## Project
- [versioning_and_releases.md](versioning_and_releases.md) — version numbers, channels and how a release is cut.

## Folders
- [wiki/](wiki/) — source for the GitHub wiki, published when `main` changes.
- [archive/](archive/) — finished design records that code still cites, and superseded research (spectator bars R&D, the OBS design, the 2026-08 capture/render UX audit, the HLCR parity notes). History, not current state.
