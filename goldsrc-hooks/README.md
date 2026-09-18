# goldsrc-hooks

Standalone companion DLL for DoD 1.3 GoldSrc capture sessions. Injected into
`hl.exe` alongside (not instead of) HLAE's own hook DLL -- see `src/lib.rs`
for why this doesn't need HLAE's build toolchain or any DoD-specific
reverse-engineering. For every `dodtools_*` cvar/command in one scannable
table, see [`docs/dodtools_commands.md`](../docs/dodtools_commands.md)
instead of the list below.

Two independent fixes, each off by default and toggled by its own env var:

- **Sound fix** (`GOLDSRC_HOOKS_FORCE_WEAPON_VOLUME=1`): forces DoD weapon-fire
  sounds to play at full volume with no distance attenuation while
  spectating, instead of fading out based on camera distance.
- **Animation fix** (`GOLDSRC_HOOKS_ANIM_FIX=1`): corrects MG42/MG34/BAR/Bren
  viewmodel deploy (bipod up/down) animations while spectating in-eye.

Plus five control surfaces, always available and doing nothing until used:

- **Death notices** (`dodtools_deathmsg`): raises DoD's hard-coded four-line
  cap on the kill feed, moves it down the screen, hides frags involving chosen
  players, or injects one by hand. HLAE's `mirv_deathmsg` supports only
  `cstrike` and `tfc`, so none of it works for DoD -- see
  `docs/goldsrc_death_notices.md`.
- **Message log** (`dodtools_msglog <name>... | all | clear`): dumps chosen
  DoD user messages and their payloads to the log file, forwarded to the game
  untouched -- a way to see what the client actually receives, in session,
  instead of reconstructing it from a demo parse. See the module doc in
  `src/msglog.rs`.
- **Scoreboard** (`dodtools_hide_scoreboard 1`): stops a POV demo's recorded TAB
  presses from putting the scoreboard over the shot. The demo replays
  `+showscores` exactly as the player typed it; this blocks the command rather
  than editing `dod/resource/ui/ScoreBoard.res` -- see
  `docs/goldsrc_scoreboard.md`.
- **Voice commands** (`dodtools_mute_voice_commands 1`): silences "fire in the
  hole!" and the rest, without overwriting the game's own `player/us*.wav`,
  `player/brit*.wav` and `player/ger*.wav`. Subtitles and speaker icons still
  show -- see `docs/goldsrc_hud_suppression.md`.
- **Crosshair** (`dodtools_hide_crosshair 1`): hides the crosshair and makes it stay
  hidden, which the stock `crosshair` cvar cannot do -- `CHud::Redraw` forces
  that value back every frame. Same doc.
- **Hand signals** (`dodtools_hide_hand_signals 1`): stops players miming their
  voice commands -- the nod, the point, the wave. Replaces any `hs_*` body
  sequence with that player's last ordinary one, for everyone in view, not just
  the spectated player. `dodtools_mute_voice_commands` does not cover this:
  `client.dll` has no `hs_` string at all, because the sequence is replicated
  entity state. See `docs/goldsrc_hltv_animation_fix.md` section 12.
- **Clear decals** (`dodtools_clear_decals`): empties the engine's 4096-slot
  decal pool on command, unlinking each decal from its surface first the way
  the engine's own remove functions do. Nothing to do with `r_decals`, which
  bounds a rotating index and evicts nothing. Pre-Anniversary `hw.dll` only,
  and it says so loudly on any other engine -- see `docs/goldsrc_decals.md`.
- **Any HUD element** (`dodtools_hide_hudelement <name> 1`): hides one of the
  twelve elements DoD draws that the stock `cl_hud_*` cvars don't already
  reach -- chat, the kill feed, the status bar, the MG-deploy and capture-area
  icons, the objective icons and the rest. (The ammo counter/weapon-select
  menu is left out on purpose: it's already fully gated behind `cl_hud_ammo`,
  no hook needed. The overview map, the mortar HUD, the spectator overlay, and
  the sniper scope overlay are also left out -- their `Draw` functions turned
  out not to draw anything at all in this build, hook or no hook; the scope's
  real effect lives in `Think`, gated on the local player's own weapon, so it
  never fires while spectating either way.) Run it with no
  arguments to list them. `dodtools_hide_hudelement all 0` puts everything
  back. Writes `CHudBase::Draw` into the element's vftable slot 3 -- one
  dword, no code patch -- see `docs/goldsrc_hud_suppression.md` section 7.
- **Spectator crosshair** (`dodtools_match_pov_crosshair 1`): draws the
  spectator crosshair from `sprites/customXHair.spr`, using the same tile
  `cl_xhair_style` gives the POV view, instead of DoD's hardcoded 24x24 tile of
  `crosshairs.spr`. Loses to `dodtools_hide_crosshair`, which stubs the whole
  function. Same doc, section 6.

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

1. Launch DoD 1.3 (with or without HLAE) and load an HLTV/POV demo.
2. Find `hl.exe`'s PID (Task Manager, or `Get-Process hl | Select Id`).
3. Set whichever env var(s) you want *before* launching `hl.exe` --
   `inject.exe` only delivers the DLL, it doesn't set environment variables
   for a process that's already running.
4. `inject.exe <pid> path\to\dodstudio_goldsrc_hooks.dll`
5. Check `%APPDATA%\dod-tools\logs\dodstudio_goldsrc_hooks.log` for its own diagnostics (never pops a
   dialog -- this is meant to run inside an unattended capture pipeline).

## Status

The animation fix and all four `dodtools_deathmsg` subcommands are live-proven
against a running game. The sound fix, `dodtools_hide_scoreboard`,
`dodtools_mute_voice_commands`, `dodtools_hide_crosshair`,
`dodtools_match_pov_crosshair` and `dodtools_msglog` are confirmed by static
analysis only -- see the module docs in `src/engine.rs`, `src/sound_fix.rs`,
`src/scoreboard.rs`, `src/voice.rs`, `src/crosshair.rs`,
`src/spectator_crosshair.rs` and `src/msglog.rs` for what is
established from the DoD 1.3 game files vs. what still needs a live check.
`dodtools_msglog` reuses `dodtools_deathmsg`'s already-proven prepend/forward
mechanism unchanged, so the open question is only its own 71-entry name/thunk
table, not the hook itself. `tools/` holds a verifier per
patched site, which checks the Rust constants against a real `client.dll`.

A crash inside the game leaves no dump, WER record or event-log entry, because
GoldSrc installs its own unhandled-exception filter. `src/crash.rs` logs the
faulting address as `module+RVA` so a crash is diagnosable from the log alone.
