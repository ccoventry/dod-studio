# goldsrc-hooks

Standalone companion DLL for DoD 1.3 GoldSrc capture sessions. Injected into
`hl.exe` alongside (not instead of) HLAE's own hook DLL -- see `src/lib.rs`
for why this doesn't need HLAE's build toolchain or any DoD-specific
reverse-engineering. For every `dodstudio_*` cvar/command in one scannable
table, see [`docs/dodstudio_commands.md`](../docs/dodstudio_commands.md)
instead of the list below.

**Match POV** (`dodstudio_spec_match_pov 1`, off by default;
`GOLDSRC_HOOKS_SPEC_MATCH_POV=1` starts a session with it on): one switch for
making a spectated first-person view look and sound like the player's own
recording. Five things come on together, and anything else of that kind
joins this switch rather than adding a command:

- **Viewmodel animations**: the first-person weapon fires, reloads and draws,
  the MG42/MG34/BAR/Bren use the right bipod family, and grenades play the pin
  pull, the throw, and the catch and wind-up of a primed one. See
  `docs/goldsrc_hltv_animation_fix.md`.
- **Missing gunshots**: an HLTV demo has no fire event for about 60% of the
  rounds fired in some recordings, so they play with no sound, flash or
  impact. Each one still restarts the shooter's body animation; this finds
  them there and calls the weapon's own event handler. See
  `docs/goldsrc_hltv_missing_gunshots.md`.
- **Spectator crosshair**: drawn from `sprites/customXHair.spr`, using the same
  tile `cl_xhair_style` gives the POV view, instead of DoD's hardcoded 24x24
  tile of `crosshairs.spr`, and hidden when the player's own would be while
  playing: sprinting, in the air after a jump, going prone or getting up,
  crawling, on a ladder, reloading, just after a weapon switch, cycling a bolt
  rifle, holding a knife, spade or sniper rifle, or a machine gun that is not
  deployed. Loses to `dodstudio_hide_crosshair`, which stubs the whole
  function. See `docs/goldsrc_hud_suppression.md` section 6.
- **Prone eye height**: the camera drops to the ground when the spectated
  player goes prone. The game's in-eye camera has no prone case and leaves it
  at crouch height. See `docs/goldsrc_spectator_eye_height.md`.
- **Gun lowering**: the gun drops off the bottom of the screen while the
  spectated player sprints, jumps, goes prone or gets up, crawls or climbs a
  ladder, as his own does (DoD's `DoDGunGoOnOffScreen`, which skips itself
  while spectating). Same speed as the game: 55 frames down, 19 back. See
  `src/spectator_gun.rs`.

Plus twenty control surfaces, always available and doing nothing until used:

- **Death notices** (`dodstudio_deathmsg`): raises DoD's hard-coded four-line
  cap on the kill feed, moves it down the screen, hides frags involving chosen
  players (by slot, SteamID or `self`), or injects one by hand. HLAE's `mirv_deathmsg` supports only
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
- **Spectator lock** (`dodstudio_spec_lock 1`): in an HLTV demo the camera
  stays on the player being watched when he dies; the game otherwise moves to
  the next player four seconds later. One byte in `client.dll`'s own death
  switch. See `docs/goldsrc_spectator_follow.md`.
- **Spectator target** (`dodstudio_spec_target <player>`): puts the camera on
  a player by number, the one `dodstudio_deathmsg players` lists. Same doc.
- **Spectator bars** (`dodstudio_hide_spectator_bars 1`): hides the two dark
  bands across the top and bottom of the screen while spectating, and the
  text and menu row on them, on screen and with no capture running, which
  HLAE's `mirv_movie_hidepanels` cannot do. A filter on vgui2's
  `IPanel::PaintTraverse`; see
  `src/spectator_bars.rs` and `docs/goldsrc_spectator_bars.md`.
- **Hand signals** (`dodstudio_hide_hand_signals 1`): stops players miming their
  voice commands -- the nod, the point, the wave. Replaces any `hs_*` body
  sequence with that player's last ordinary one, for everyone in view, not just
  the spectated player. `dodstudio_mute_voice_commands` does not cover this:
  `client.dll` has no `hs_` string at all, because the sequence is replicated
  entity state. See `docs/goldsrc_hltv_animation_fix.md` section 12.
- **HLTV text** (`dodstudio_hide_hltv_messages 1`): drops the text an HLTV
  proxy puts on screen during playback -- "You're watching HLTV. Visit
  www.valvesoftware.com", about once a minute, and a proxy operator's own
  `msg` lines -- so an HLTV demo needs no patched copy. Hooks
  `HUD_DirectorMessage` (`cldll_func_t` slot 38) and drops only
  `DRC_CMD_MESSAGE` (6); the pipeline's highlight labels (`DRC_CMD_STUFFTEXT`)
  and the camera commands pass through. Both builds. See `src/hltv_messages.rs`.
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
- **Seek** (`dodstudio_seek_to <seconds>`, `dodstudio_seek_by <seconds>`):
  jumps `viewdemo` playback to a time, the way the demo editor's Goto does,
  through `DemoPlayer.dll`'s own `IDemoPlayer`. A forward jump normally runs
  every director event and console command it skips, all at once;
  `dodstudio_seek_skip_between 1` lands clean instead. Refuses while the demo
  is still loading. Both builds; see `docs/goldsrc_viewdemo.md`.
- **Window layout** (`dodstudio_resizable_windows 1`,
  `dodstudio_remember_window_layout 1`): every GameUI window can be resized
  like the console, and each comes back where it was left after a restart
  (the console loads no `.res`, so build mode can't save its place). Walks
  the engine surface's popups through vgui2's own interfaces; only
  `Frame::SetSizeable`/`IsSizeable` are per-build addresses. See
  `src/window_layout.rs`.
- **Commands from Studio** (on by default): the game serves a local named
  pipe, `\\.\pipe\dodstudio-hl-<pid>`, and runs each line Studio writes
  to it as a console command on the next frame. Launch Preview uses it when
  the game is already open, sending `viewdemo <demo>_preview` instead of
  asking to relaunch. Remote clients are refused and only the same Windows
  user can write to it; `GOLDSRC_HOOKS_REMOTE=0` turns it off. See
  `src/remote.rs` and `native/src/sys/game_remote.rs`.
- **Events to Studio** (on by default): a second pipe,
  `\\.\pipe\dodstudio-hl-<pid>-events`, carries the capture pipeline's
  `[dod-studio]` markers from the game to Studio as the engine runs each
  `echo`, so a batch no longer depends on `qconsole.log` (issue #434, step 1).
  The DLL wraps the engine's `echo` command through its command list
  (`src/cmd_list.rs`, no per-build address). Markers queued before Studio
  connects are sent when it does. `GOLDSRC_HOOKS_EVENTS=0` turns it off, and
  Studio then reads the log as before. See `src/events.rs` and
  `native/src/obs/pipe_tail.rs`.
- **Reload the demo** (`dodstudio_reload_demo`): plays the last `playdemo` or
  `viewdemo` again from the start, with the same name. The engine keeps no
  copy of the name, so the DLL wraps both engine commands to note it; the
  wrap goes through the SDK's command-list functions, with no per-build
  address. See `src/demo_reload.rs`.
- **Refuses to join a server** (on by default): `connect`, `retry`,
  `reconnect` and `listen` are refused while the DLL is loaded, with a console
  message and a log line, because joining a VAC-secured server with it loaded
  is a ban risk. `connect local` (what `map` runs) still works.
  `GOLDSRC_HOOKS_ALLOW_CONNECT=1` turns it off, for testing on your own
  server. Wraps the engine commands the same way as the demo reload. See
  `src/connect_guard.rs` and `docs/vac_safety.md`.
- **Any HUD element** (`dodstudio_hide_hudelement <name> 1`): hides one of the
  ten elements DoD draws that the stock `cl_hud_*` cvars don't already
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
> connect warning HLAE shows in every DoD Studio launch. The DLL refuses
> `connect` once it has hooked the engine, but don't rely on that alone. See
> [`docs/vac_safety.md`](../docs/vac_safety.md).

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
against a running game, except `block` by SteamID or `self` (#468), which is
not yet. `dodstudio_ex_interp_max`'s mechanism is live-proven
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
`dodstudio_hide_scoreboard`, `dodstudio_mute_voice_commands`,
`dodstudio_hide_crosshair`, the spectator crosshair and
`dodstudio_debug_msglog` are confirmed by static analysis only -- see the module
docs in `src/engine.rs`, `src/scoreboard.rs`,
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

`src/tempent_fix.rs` stops a years-old DoD crash (`client.dll+0x225cc`, issue
#374): six places in DoD's client write into a temporary effect entity without
checking the engine gave them one. It is on by default, because it only acts
where the game would otherwise crash; `GOLDSRC_HOOKS_TEMPENT_FIX=0` turns it
off. `dodstudio_debug_status` shows how many effects it has skipped.

`src/hull_trace_guard.rs` stops an engine crash (`hw.dll+0x6c839`, issue #384):
`playdemo` of an HLTV demo on some maps right after another map can hand the
player-movement trace a previous map's collision data, and it recurses until
the stack runs out. The guard refuses any clip node the hull can't have, and
stops a trace that is about to run out of stack. On by default for the same
reason; `GOLDSRC_HOOKS_HULL_TRACE_GUARD=0` turns it off. It knows both the
pre-Anniversary and the 25th Anniversary `hw.dll`
(`tools/verify_hull_trace_offsets.py [--anniversary]` checks either).

`src/pmove_guard.rs` stops a crash (`hw.dll+0x3a77c`, issue #546) when the
session's first demo sends DoD's `InitHUD` in its very first packets: the
client then drops the map's static models to the ground through
`EV_SetTraceHull`, which writes through the engine's `pmove` pointer, and
nothing has set that pointer yet. At start-up the guard points it at the
engine's own `g_clmove`, the value the engine stores there itself a moment
later; in that one case the models keep their authored height instead of the
game closing. On by default; `GOLDSRC_HOOKS_PMOVE_GUARD=0` turns it off.
`tools/verify_pmove_guard.py` re-derives both builds.

`src/sprite_blend.rs` stops `gl_spriteblend 0` at the session's first sprite
load from leaving the crosshair and other sprites dark and dotted until the
game restarts (issue #467). The engine only fills in the colour around a
sprite's edges at upload when the cvar is non-zero, and it never uploads the
same sprite twice. Two bytes in `GL_Upload32` make it always do so, on both
builds, as if the value were its default of 1. Draw-time handling of the cvar
is untouched. On by default; `GOLDSRC_HOOKS_SPRITEBLEND_FIX=0` turns it off.
`tools/verify_spriteblend_offsets.py` re-derives both sites.

A crash inside the game leaves no dump, WER record or event-log entry, because
GoldSrc installs its own unhandled-exception filter. `src/crash.rs` logs the
faulting address as `module+RVA` so a crash is diagnosable from the log alone. `tools/crash_report.py` summarises every crash on record: grouped by where it happened, with what led up to it, which map was loaded, the engine's own fatal errors from `qconsole.log`, and which crashes are already known.
