# Command Tiers: what Initial and Scheduled Commands may contain

A command a user types into Initial or Scheduled Commands falls into one of five lists in `native/src/patch/cfg_scan.rs`. **That file is the source of truth.** This page explains *why* each list exists; it names example members only to illustrate, and the set has been re-tiered more than once (`mirv_movie_filename` moved in #161; `mirv_movie_separate_hud` disappeared with #214). Read `cfg_scan.rs` before describing the current set.

Enforcement runs twice, independently: in `map_manager::scan_game_configs`'s report, and again in `capture_manager::start_capture_batch_impl`. The Commands type-ahead (`studio/src/command_suggest.js`) mirrors the tiers, so a tier change must be mirrored there too.

## The five lists

- **`BANNED_COMMANDS`** — refused **everywhere**, Initial and Scheduled alike. Two different reasons land a command here:
  - *The pipeline owns it outright* (`mirv_recordmovie_start`/`_stop`, `mirv_movie_ffmpeg`, `host_framerate`). No setting corresponds to any of them, and a user's own value misroutes footage or desyncs playback with no visible failure.
  - *DoD's client quits the game over it* (`r_drawentities`, `cl_lw`). `CHud::Redraw` forces the value back, prints an error and calls `quit` if either is not `1` (binary-level evidence: `docs/goldsrc_client_dll_internals.md` §5).

  Reachability differs between those two, and `FATAL_CVARS` encodes it: `cl_lw` always takes the value it is given, whereas GoldSrc itself clamps `r_drawentities` back to `1.0` while `sv_cheats` is `0`, making a config line setting it inert. Both stay refused as *typed commands* — cheap, and `cfg_scan` cannot see what else a user's configs did — but only `cl_lw` is reported as a fatal config cvar unconditionally.
- **`SCHEDULED_BANNED_COMMANDS`** — fine at demo load, refused **only when scheduled** (`r_decals`, `mirv_fov`, `gl_widescreenfov`, `mirv_movie_filename`, `mirv_agr`). Why `r_decals` must be set exactly once: `docs/goldsrc_dod_quirks.md`.
- **`MID_DEMO_HAZARDS`** — shadowed with a warning, not refused, because each corresponds to a real setting.
- **`NOOP_EVERYWHERE_COMMANDS`** (`exec`, `quit`) — reported as doing nothing: the engine drops them from a demo's message stream.
- **`NOOP_IN_INIT_COMMANDS`** (`mirv_movie_filename`) — reported as doing nothing: the pipeline overwrites it before anything reads it.

## Adding a pipeline-internal command

If Initial or Scheduled Commands could reach a new command the pipeline relies on, decide which list it belongs to before shipping it unprotected, and mirror the choice in `command_suggest.js`.

## User config files

The game's own `.cfg` files are the user's: **detect and warn, never write.** A `config.cfg` ending in `exec movie.cfg` can set `mirv_fov` or `r_decals` behind the pipeline entirely, which is why `cfg_scan` also reads the user's configs. It is read-only by construction; keep it that way.
