# The `dodstudio_*` console surface

One place to scan the whole `goldsrc-hooks` cvar/command surface without
grepping `commands.rs`. Closes issue #313.

Every name here shares the `dodstudio_` prefix ([`names.rs`](../goldsrc-hooks/src/names.rs)'s
`console_name!` macro). `dodstudio_debug_status` reports the live value of
everything below in one place (cvars unconditionally; the two fixes with
preconditions only while enabled), and each item's own doc file has the full
engine-level detail this table deliberately doesn't repeat.

Diagnostics (status, logging, the HD miss list) all sit under
`dodstudio_debug_`, so typing that prefix in the console lists every one of
them; everything else is a setting you'd use for a capture. No name is the
start of another, because the console's autocomplete would otherwise swap
the shorter one for the longer when you press space.

**Scope:** the surface actually on `dev` today: sixteen cvars and nine
commands. Entries still in open PRs are not listed -- update this file as
part of merging each one, the same way every one of them already updates
`README.md`'s own control-surface list.

## Cvars

A cvar shows up in the console's own type-ahead with its current value,
answers a bare-name query, and can be set from `+name value` on the launch
line or a line in any `.cfg` the user execs -- unlike a command, none of that
needs remembering a subcommand shape. None of them are `FCVAR_ARCHIVE`: the
game never writes one into the user's `config.cfg`, matching the pipeline's
standing "user `.cfg` files are never written" rule (`CLAUDE.md`).

| cvar | default | what it does | doc |
| --- | --- | --- | --- |
| `dodstudio_spec_match_pov` | `0` (off) unless `GOLDSRC_HOOKS_SPEC_MATCH_POV=1` at launch | one switch for making a spectated first-person view look and sound like the player's own recording. It turns on five things together: the first-person weapon's animations (firing, reloading, drawing, the bipod families, and grenades including primed ones); the gunshots an HLTV demo has no fire event for (about 60% of rounds in some recordings), restored with their sound, muzzle flash, tracer and impact; and the spectator crosshair drawn from `sprites/customXHair.spr` with the tile `cl_xhair_style` gives the POV view (needs `cl_xhair_style` 1 or higher, see issue #308; loses to `dodstudio_hide_crosshair`), and hidden when the player's own would be while playing: sprinting, in the air after a jump, going prone or getting up, crawling, on a ladder, reloading, just after a weapon switch, cycling a bolt rifle, holding a knife, spade or sniper rifle, or a machine gun that is not deployed; the camera dropping to the ground when the player goes prone, where the game leaves it at crouch height; and the gun lowering off screen while he sprints, jumps, goes prone, crawls or climbs a ladder, as his own does (`goldsrc-hooks/src/spectator_gun.rs`). `2` also logs every restored gunshot. It replaces `dodstudio_hltv_show_viewmodel_animations`, `dodstudio_hltv_play_missing_gunshots` and `dodstudio_match_pov_crosshair`; `dodstudio_hltv_gunshots_fix` and `dodstudio_hltv_gunshot_attenuation` are gone, since making gunfire carry further than POV hears it is not matching POV | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md), [`goldsrc_hltv_missing_gunshots.md`](goldsrc_hltv_missing_gunshots.md), [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) §6, [`goldsrc_spectator_eye_height.md`](goldsrc_spectator_eye_height.md) |
| `dodstudio_debug_log_weapon_model` | `0` | logs the third-person weapon model the spectated player holds, each time it changes | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodstudio_debug_log_spectator_target` | `0` | logs who the spectator HUD thinks is being followed next to the entity the engine renders a viewmodel for, whenever either changes (issue #206) | `goldsrc-hooks/src/spectator_target.rs` |
| `dodstudio_hide_scoreboard` | `0` | stops a POV demo's recorded TAB presses from putting the scoreboard over the shot | [`goldsrc_scoreboard.md`](goldsrc_scoreboard.md) |
| `dodstudio_mute_voice_commands` | `0` | silences "fire in the hole!" and the rest, without touching the game's own `.wav` files; subtitles and speaker icons still show | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `dodstudio_hide_crosshair` | `0` | hides the crosshair and keeps it hidden, which the stock `crosshair` cvar can't do because `CHud::Redraw` forces the value back every frame | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `dodstudio_hide_spectator_bars` | `0` | hides the spectator panel while spectating, in a demo or live: the two dark bands across the top and bottom of the screen, the score, timer and player name on them, and the menu row DUCK brings up. On screen, with no capture running | [`goldsrc_spectator_bars.md`](goldsrc_spectator_bars.md) |
| `dodstudio_spec_lock` | `0` | HLTV demos: the camera stays on the player being watched when he dies. Without it the game moves to the next player four seconds later. The viewer's own keys still change player | [`goldsrc_spectator_follow.md`](goldsrc_spectator_follow.md) |
| `dodstudio_hide_hand_signals` | `0` | replaces any `hs_*` body sequence (the nod, the point, the wave -- players miming their own voice commands) with that player's last ordinary one, for everyone in view | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) §12 |
| `dodstudio_hide_map_text` | `0` | hides the text a map puts on screen itself -- the `dod_anzio` mortar warning, the round result -- by matching each `HudText` message against the `message` strings the loaded map's own entities declare. DoD's own prompts on the same channel (`#Clan_allies_ready` and friends) still show. Reads the map's BSP once per level | `goldsrc-hooks/src/map_text.rs` |
| `dodstudio_ex_interp_max` | `100` (the engine's own ceiling) | raises the engine's clamp on `ex_interp` above its stock 100 ms ceiling, for smoother entity motion between snapshots; refuses `<=50` or `>1000`. Mechanism live-proven, no specific value settled on yet | [`goldsrc_ex_interp.md`](goldsrc_ex_interp.md) |
| `dodstudio_hd_enabled` | `1` if there's a `dod/dodstudio_hd` folder, else `0`; `GOLDSRC_HOOKS_TEXTURE_HIRES=1`/`0` at launch overrides | HD textures on/off: map textures, model skins, sprites, detail textures and skies from `dodstudio_hd`. A change applies to what loads next -- walls, detail and skies from the next map, models and sprites already loaded after a restart. Turning it on in a session that started off installs the hook then | `goldsrc-hooks/src/texture_hires.rs`, `goldsrc-hooks/tools/hd/README.md` |
| `dodstudio_hd_style` | `ultrasharp` | which `dodstudio_hd/<type>/<style>` folder to use; a name with no folder means originals (plus `overrides`). Same timing as `dodstudio_hd_enabled` | same |
| `dodstudio_allow_shaders` | `0` | 25th Anniversary only: lets the engine draw map surfaces through its own GLSL shaders (`platform/gl_shaders/fs_world.frag`) during demo playback. The engine gates them on `sv_allow_shaders`, which a demo can never turn on: the console refuses it in multiplayer and every demo load resets it to 0. This writes 1 into it while a demo plays. Needs `gl_use_shaders 1` too. `gl_reloadshaders` recompiles the files live. Does nothing on the pre-Anniversary engine | `goldsrc-hooks/src/world_shaders.rs` |
| `dodstudio_hide_hltv_messages` | `0` | drops the text an HLTV proxy puts on screen during playback ("You're watching HLTV. Visit www.valvesoftware.com", and a proxy operator's own `msg` lines) as it arrives, so an HLTV demo needs no patched copy. Only director text messages: the pipeline's highlight labels and everything that drives the spectator camera still go through. A message already on screen when it's turned on fades out on its own | `goldsrc-hooks/src/hltv_messages.rs` |
| `dodstudio_debug_log_texture_loads` | `0` | logs every HD-eligible texture load: replaced (from which file) or why not | same |
| `dodstudio_seek_skip_between` | `0` | `1` makes `dodstudio_seek_to`/`_by` land without running the director events and console commands they jump over; `0` runs them, as the editor's Goto does | [`goldsrc_viewdemo.md`](goldsrc_viewdemo.md) |
| `dodstudio_resizable_windows` | `0` | every GameUI window (VCR bar, events list, Load Demo, Options...) can be resized by its edges, like the console; its controls stretch as far as their `.res` `autoResize`/`pinCorner` allow. `0` puts back the ones it changed | `goldsrc-hooks/src/window_layout.rs`, #408 |
| `dodstudio_remember_window_layout` | `0` | each GameUI window comes back where it was left, and at its size when resizable, after the game restarts; kept in `%APPDATA%\dod-studio\goldsrc_hooks_windows.txt` | same |

## Commands

Always a command rather than a cvar when it has subcommands or a variable
argument count, which a cvar's single value can't hold.

### `dodstudio_debug_status`

No arguments. Always reports the four suppression cvars, the two
`debug_log_*` cvars and `dodstudio_deathmsg`'s own status; the rest
(including the animation fix and gunshots fix) are only included while
enabled, since a flag being on says nothing about whether
their preconditions are currently being met and they'd otherwise be noise
when off. Force-calls the per-frame `poll()` first, so chaining a cvar set
and a status query on one `;`-joined console line reports the post-change
state, not the state from before that line ran.

### `dodstudio_deathmsg`

Raises DoD's hard-coded four-line cap on the kill feed, moves it, hides
frags, or injects one by hand. HLAE's own `mirv_deathmsg` supports only
`cstrike` and `tfc`, so none of it works for DoD. See
[`goldsrc_death_notices.md`](goldsrc_death_notices.md).

| subcommand | does |
| --- | --- |
| `dodstudio_deathmsg` | status + usage |
| `dodstudio_deathmsg max <4..127>` | lines of kill feed shown at once (default 4) |
| `dodstudio_deathmsg offset <-4096..4096>` | y the feed starts at (default 20); negative pulls it above the top of the screen |
| `dodstudio_deathmsg offset default` | hand y back to the game |
| `dodstudio_deathmsg block <id>...` | hide frags involving these players (replaces the set) |
| `dodstudio_deathmsg block !<id>...` | hide everything *except* these players |
| `dodstudio_deathmsg block clear` | stop hiding anything |
| `dodstudio_deathmsg players` | list each player's slot, name and SteamID as `block` sees them (0 = the engine gave none) |
| `dodstudio_deathmsg fake <killer> <victim> <weapon>` | inject one by hand; weapon is a name (`d_garand`, `garand`) or `1..43` |

A `block` id is any of:

- **A slot number**, as before. It only holds for one demo, because the same
  player gets a different slot in every demo.
- **A SteamID**: the 17-digit SteamID64 (`76561197977930126`),
  `STEAM_0:0:8832199`, or SteamID3 `[U:1:17664398]`. Paste it as is: the
  console splits it at each `:`, and the hook joins it back. It is matched
  against each player's userinfo `*sid` at every death notice, so one command
  works across a whole batch of demos and survives reconnects.
  `dodstudio_deathmsg players` lists every player's slot and SteamID.
- **`self`**: the recording player in a POV demo. An HLTV demo has no
  recording player, so there `self` matches nobody, and the console says so
  once.

Mix them freely. `dodstudio_deathmsg block !self !76561197977930126` shows
only your own frags in both kinds of demo: in a POV demo both entries point
at you, and in an HLTV demo `self` drops out and the SteamID finds you.

### `dodstudio_hide_hudelement`

`dodstudio_hide_hudelement <name> <0|1>` with no arguments lists the ten
elements DoD draws that the stock `cl_hud_*` cvars don't already reach --
chat, the kill feed, the status bar, the objective icons and the rest.
`dodstudio_hide_hudelement all 0` puts everything back. See
[`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) §7.

### `dodstudio_clear_decals`

No arguments. Empties the engine's 4096-slot decal pool on command,
unlinking each decal from its surface first the way the engine's own remove
functions do. Nothing to do with `r_decals`. Pre-Anniversary `hw.dll` only.
See [`goldsrc_decals.md`](goldsrc_decals.md).

### `dodstudio_seek_to` / `dodstudio_seek_by`

`dodstudio_seek_to <seconds>` jumps `viewdemo` playback to a world time (the
clock the editor's events list shows); `dodstudio_seek_by <seconds>` jumps
from where playback is, back when negative. Neither pauses, and both refuse
while the demo is still loading or under `playdemo`. Pre-Anniversary and 25th
Anniversary `DemoPlayer.dll`. Nothing in the pipeline calls them yet. See
[`goldsrc_viewdemo.md`](goldsrc_viewdemo.md).

### `dodstudio_reload_demo`

No arguments. Plays the last demo started with `playdemo` or `viewdemo`
again, from the start, by running the same command with the same name. Says
so when no demo has been played this session. See `src/demo_reload.rs`.

### `dodstudio_overviewmap`

`dodstudio_overviewmap <full|mini> <x> <y> <w> <h>` places and sizes DoD's
overview map; `default` releases one back to the engine. With no arguments,
lists both maps and what the engine currently has. See `src/overview_map.rs`'s
module doc, including why it closes while a spectated player is scoped into a
sniper (the engine's own FOV gate, not this command).

### `dodstudio_spec_target`

`dodstudio_spec_target <player>` puts the camera on a player while watching an
HLTV demo. The number is the one `dodstudio_deathmsg players` lists (1-32).
With no number it says who the camera is on. It refuses a number with no
player, and a first-person demo. See
[`goldsrc_spectator_follow.md`](goldsrc_spectator_follow.md).

### `dodstudio_debug_msglog`

`dodstudio_debug_msglog <name>... | all | clear` dumps chosen DoD user messages and
their payloads to the log file, forwarded to the game untouched. See
`src/msglog.rs`'s module doc.

### `dodstudio_objectives`

Places the territory-flag icon row and the objective timer beside it, which
the game draws ~117px lower at 1080p while spectating than in a POV demo. See
[`goldsrc_objective_icons.md`](goldsrc_objective_icons.md).

| subcommand | does |
| --- | --- |
| `dodstudio_objectives offset <y>` | y the objective icons are drawn at |
| `dodstudio_objectives xoffset <x>` | x the icon row starts at |
| `dodstudio_objectives timer <y>` | y the objective timer is drawn at (no x -- the game hardcodes it) |
| `dodstudio_objectives <any> default` | hand that one back to the game |

### `dodstudio_debug_hd_misses`

Lists every texture that kept its original this session, map by map, grouped
by why: the HD file is for a different version of the texture, there's no HD
file (naming the file that would match), the file couldn't be used, or it
was left alone on purpose (tool textures, blank sprites, per-player skins). A
texture several maps use is listed under each. `dodstudio_debug_hd_misses <map>`
shows one map; `dodstudio_debug_hd_misses clear` forgets the list.

### `dodstudio_hide_sprite`

`dodstudio_hide_sprite <model-path>...` suppresses specific map-placed
`env_sprite` entities by exact model path -- an allow-list, not a blanket
toggle, replacing the whole set on each call (not additive). `clear` stops
hiding anything. Only reaches genuine `env_sprite` entities rendered through
the engine's normal entity list (`HUD_AddEntity`); DoD draws some
sprite-looking things -- the crosshair, the capture-area icon -- as ordinary
2D HUD elements instead, which this command can never reach regardless of
path spelling (`dodstudio_hide_crosshair`/`dodstudio_hide_hudelement` reach
those). No enumeration of valid paths either: an unmatched entry (wrong path,
wrong extension, or a 2D-drawn element like the above) fails silently, with
no error -- see issue #333. See `src/hide_sprite.rs`'s module doc.

## Keeping this in sync

Manual, like the rest of this crate's docs -- there is no generator from
`commands.rs`. Each PR that adds or changes a `dodstudio_*` entry should
update this file the same way it already updates `README.md`'s own
control-surface list; the two lists should never drift apart. Issue #313
left open whether this eventually becomes a generated table or a GitHub wiki
page instead -- not resolved here.
