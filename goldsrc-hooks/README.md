# goldsrc-hooks

Standalone companion DLL for DoD 1.3 GoldSrc capture sessions. Injected into
`hl.exe` alongside (not instead of) HLAE's own hook DLL -- see `src/lib.rs`
for why this doesn't need HLAE's build toolchain or any DoD-specific
reverse-engineering. For every `dodstudio_*` cvar/command in one scannable
table, see [`docs/dodstudio_commands.md`](../docs/dodstudio_commands.md)
instead of the list below.

Two independent fixes, each off by default and toggled by its own env var:

- **Sound fix** (`GOLDSRC_HOOKS_FORCE_WEAPON_VOLUME=1`): forces DoD weapon-fire
  sounds to play at full volume with no distance attenuation while
  spectating, instead of fading out based on camera distance.
- **Animation fix** (`GOLDSRC_HOOKS_ANIM_FIX=1`): corrects MG42/MG34/BAR/Bren
  viewmodel deploy (bipod up/down) animations while spectating in-eye.

Plus thirteen control surfaces, always available and doing nothing until used:

- **Death notices** (`dodstudio_deathmsg`): raises DoD's hard-coded four-line
  cap on the kill feed, moves it down the screen, hides frags involving chosen
  players, or injects one by hand. HLAE's `mirv_deathmsg` supports only
  `cstrike` and `tfc`, so none of it works for DoD -- see
  `docs/goldsrc_death_notices.md`.
- **Message log** (`dodstudio_debug_msglog <name>... | all | clear`): dumps chosen
  DoD user messages and their payloads to the log file, forwarded to the game
  untouched -- a way to see what the client actually receives, in session,
  instead of reconstructing it from a demo parse. See the module doc in
  `src/msglog.rs`.
- **Hide map sprite** (`dodstudio_hide_sprite <model-path>...`): suppresses
  specific map-placed `env_sprite` entities by model path (e.g.
  `sprites/mapsprites/flames.spr`) -- an allow-list, not a blanket toggle,
  since most sprites in that folder are meaningful (smoke, fire, tracers). Not
  every sprite-looking element qualifies: several DoD draws itself as an
  ordinary 2D HUD element (the crosshair, the capture-area icon) rather than a
  world-placed entity, and those never reach this command regardless of
  spelling -- `dodstudio_hide_crosshair`/`dodstudio_hide_hudelement` reach those
  instead. Hooks `HUD_AddEntity`, a `cldll_func_t` slot `engine.rs` didn't
  previously use. See the module doc in `src/hide_sprite.rs`.
- **Scoreboard** (`dodstudio_hide_scoreboard 1`): stops a POV demo's recorded TAB
  presses from putting the scoreboard over the shot. The demo replays
  `+showscores` exactly as the player typed it; this blocks the command rather
  than editing `dod/resource/ui/ScoreBoard.res` -- see
  `docs/goldsrc_scoreboard.md`.
- **Voice commands** (`dodstudio_mute_voice_commands 1`): silences "fire in the
  hole!" and the rest, without overwriting the game's own `player/us*.wav`,
  `player/brit*.wav` and `player/ger*.wav`. Subtitles and speaker icons still
  show -- see `docs/goldsrc_hud_suppression.md`.
- **Crosshair** (`dodstudio_hide_crosshair 1`): hides the crosshair and makes it stay
  hidden, which the stock `crosshair` cvar cannot do -- `CHud::Redraw` forces
  that value back every frame. Same doc.
- **Hand signals** (`dodstudio_hide_hand_signals 1`): stops players miming their
  voice commands -- the nod, the point, the wave. Replaces any `hs_*` body
  sequence with that player's last ordinary one, for everyone in view, not just
  the spectated player. `dodstudio_mute_voice_commands` does not cover this:
  `client.dll` has no `hs_` string at all, because the sequence is replicated
  entity state. See `docs/goldsrc_hltv_animation_fix.md` section 12.
- **Overview map** (`dodstudio_overviewmap <full|mini> <x> <y> <w> <h>`): places
  and sizes DoD's overview map, so the big one can be a corner inset instead of
  something that has to be off. The rects are cached in `gHUD` rather than
  recomputed per frame, so this is four dword writes, re-asserted each frame
  because `VidInit` recomputes them. `dodstudio_overviewmap` with no arguments
  lists both and what the engine currently has; `default` releases them.
  Note the kill feed and objective icons take their y from the full map's
  bounds.
- **Interpolation ceiling** (`dodstudio_ex_interp_max <ms>`): raises the engine's
  clamp on `ex_interp` above its 100 ms ceiling, for smoother entity motion
  between snapshots. `ex_interp` is engine-managed -- a per-frame clamp writes
  the value back through `Cvar_Set` -- so setting the cvar by hand does not
  stay. `cl_updaterate` still sets the floor. See `docs/goldsrc_ex_interp.md`.
- **Clear decals** (`dodstudio_clear_decals`): empties the engine's 4096-slot
  decal pool on command, unlinking each decal from its surface first the way
  the engine's own remove functions do. Nothing to do with `r_decals`, which
  bounds a rotating index and evicts nothing. Pre-Anniversary `hw.dll` only,
  and it says so loudly on any other engine -- see `docs/goldsrc_decals.md`.
- **Any HUD element** (`dodstudio_hide_hudelement <name> 1`): hides one of the
  twelve elements DoD draws that the stock `cl_hud_*` cvars don't already
  reach -- chat, the kill feed, the status bar, the MG-deploy and capture-area
  icons, the objective icons and the rest. (The ammo counter/weapon-select
  menu is left out on purpose: it's already fully gated behind `cl_hud_ammo`,
  no hook needed. The overview map, the mortar HUD, the spectator overlay, and
  the sniper scope overlay are also left out -- their `Draw` functions turned
  out not to draw anything at all in this build, hook or no hook; the scope's
  real effect lives in `Think`, gated on the local player's own weapon, so it
  never fires while spectating either way.) Run it with no
  arguments to list them. `dodstudio_hide_hudelement all 0` puts everything
  back. Writes `CHudBase::Draw` into the element's vftable slot 3 -- one
  dword, no code patch -- see `docs/goldsrc_hud_suppression.md` section 7.
- **Spectator crosshair** (`dodstudio_match_pov_crosshair 1`): draws the
  spectator crosshair from `sprites/customXHair.spr`, using the same tile
  `cl_xhair_style` gives the POV view, instead of DoD's hardcoded 24x24 tile of
  `crosshairs.spr`. Loses to `dodstudio_hide_crosshair`, which stubs the whole
  function. Same doc, section 6.
- **Objective icons** (`dodstudio_objectives`): places the territory-flag icon
  row in the top-left corner, and the objective timer beside it. The game draws
  both ~117 pixels lower at 1080p while spectating than it does in a POV demo,
  which is why the same map captured both ways does not line up -- see
  `docs/goldsrc_objective_icons.md`.

All of them live in one DLL since they share the same engine-interface
bootstrap; set only the env var for whichever fix you want active.

## Building

DoD 1.3 (and the whole GoldSrc engine) is **32-bit**, so this must be built
for the `i686-pc-windows-msvc` target, not the default 64-bit one:

```
rustup target add i686-pc-windows-msvc   # one-time
# --lib matters: the injector is a standalone binary that does not depend on
# the cdylib, so --bins alone silently leaves a stale DLL in place.
cargo build -p goldsrc-hooks --release --target i686-pc-windows-msvc --lib --bins
```

Produces `target/i686-pc-windows-msvc/release/dodstudio_goldsrc_hooks.dll` and
`inject.exe`.

## Testing manually

> [!WARNING]
> **Only ever inject into a separate movie copy of Half-Life, never the one
> you play online with, and never join a server afterwards.** This DLL patches
> the game in memory, which is what VAC detects. Injecting by hand skips the
> connect warning HLAE shows in every DoD Studio launch, so nothing will stop
> you. See [`docs/vac_safety.md`](../docs/vac_safety.md).

1. Launch DoD 1.3 (with or without HLAE) from your movie copy and load an HLTV/POV demo.
2. Find `hl.exe`'s PID (Task Manager, or `Get-Process hl | Select Id`).
3. Set whichever env var(s) you want *before* launching `hl.exe` --
   `inject.exe` only delivers the DLL, it doesn't set environment variables
   for a process that's already running.
4. `inject.exe <pid> path\to\dodstudio_goldsrc_hooks.dll`
5. Check `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks.log` for its own diagnostics (never pops a
   dialog -- this is meant to run inside an unattended capture pipeline).

## Status

The animation fix and all four `dodstudio_deathmsg` subcommands are live-proven
against a running game. `dodstudio_ex_interp_max`'s mechanism is live-proven
too -- the clamp visibly takes effect -- but no specific value is confirmed
good yet; see `docs/goldsrc_ex_interp.md` §7. `dodstudio_objectives` is
live-proven too: `offset`/`xoffset` reposition the icon row correctly, and
`timer` was confirmed on `dod_charlie`, the one DoD 1.3 map with a
reinforcement timer -- see `docs/goldsrc_objective_icons.md`. `dodstudio_hide_sprite`
is live-proven as well: `sprites/mapsprites/flames.spr` on `dod_railroad2_s9a`
(found by scanning the map's own BSP entity lump for `env_sprite` classnames,
since the command's target has to be a real map-placed entity, not a 2D HUD
element like the crosshair or the capture-area icon -- see the module doc's
"Why `dodstudio_hide_hudelement` can't reach this") visibly disappeared and
came back across a `clear`/re-set cycle, which confirms `HUD_AddEntity`'s
return-value contract (0 = suppress) actually holds in this build and not
only in Xash3D's open-source equivalent. The
sound fix, `dodstudio_hide_scoreboard`, `dodstudio_mute_voice_commands`,
`dodstudio_hide_crosshair`, `dodstudio_match_pov_crosshair` and
`dodstudio_debug_msglog` are confirmed by static analysis only -- see the module
docs in `src/engine.rs`, `src/sound_fix.rs`, `src/scoreboard.rs`,
`src/voice.rs`, `src/crosshair.rs`, `src/spectator_crosshair.rs` and
`src/msglog.rs` for what is established from the DoD 1.3 game files vs. what
still needs a live check. `dodstudio_debug_msglog` reuses `dodstudio_deathmsg`'s
already-proven prepend/forward mechanism unchanged, so the open question is
only its own 71-entry name/thunk table, not the hook itself. `tools/` holds
a verifier per patched site, which checks the Rust constants against a real
`client.dll`.

`src/texture_hires.rs` swaps in upscaled map textures, model skins, sprites,
detail textures and skies as the game loads them: on when there's a
`dod/dodstudio_hd` folder, `dodstudio_hd_enabled 0/1` in game, and
`GOLDSRC_HOOKS_TEXTURE_HIRES=0/1` to force it at startup. `tools/hd/` holds
the scripts that build
those files; see its README.

A crash inside the game leaves no dump, WER record or event-log entry, because
GoldSrc installs its own unhandled-exception filter. `src/crash.rs` logs the
faulting address as `module+RVA` so a crash is diagnosable from the log alone. `tools/crash_report.py` summarises every crash on record: grouped by where it happened, with what led up to it, which map was loaded, the engine's own fatal errors from `qconsole.log`, and which crashes are already known.
