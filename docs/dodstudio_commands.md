# The `dodstudio_*` console surface

One place to scan the whole `goldsrc-hooks` cvar/command surface without
grepping `commands.rs`. Closes issue #313.

Every name here shares the `dodstudio_` prefix ([`names.rs`](../goldsrc-hooks/src/names.rs)'s
`console_name!` macro). `dodstudio_debug_status` reports the live value of
everything below in one place (cvars unconditionally; the two fixes with
preconditions only while enabled), and each item's own doc file has the full
engine-level detail this table deliberately doesn't repeat.

**Scope:** the surface actually on `dev` today. Several more entries are open
PRs, listed separately at the bottom so this stays honest about what's
*shipped* versus what's *proposed* -- update this table as part of merging
each one, the same way every one of them already updates `README.md`'s own
control-surface list.

## Cvars

A cvar shows up in the console's own type-ahead with its current value,
answers a bare-name query, and can be set from `+name value` on the launch
line or a line in any `.cfg` the user execs -- unlike a command, none of that
needs remembering a subcommand shape. None of them are `FCVAR_ARCHIVE`: the
game never writes one into the user's `config.cfg`, matching the pipeline's
standing "user `.cfg` files are never written" rule (`CLAUDE.md`).

| cvar | default | what it does | doc |
| --- | --- | --- | --- |
| `dodstudio_hltv_gunshots_fix` | `1` if `GOLDSRC_HOOKS_FORCE_WEAPON_VOLUME=1` at launch, else `0` | forces DoD weapon-fire sounds to full volume with no distance attenuation while spectating | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodstudio_hltv_show_viewmodel_animations` | `0` (off) unless `GOLDSRC_HOOKS_ANIM_FIX` sets a starting level | `0`=off, `1`=empty hand on throw, `2`=redraw immediately, `3`=never empty, `4`=redraw after a 1s lookahead (the recommended setting) -- corrects MG42/MG34/BAR/Bren viewmodel deploy animations while spectating in-eye | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodstudio_hltv_gunshot_attenuation` | `0.8` (`ATTN_NORM`, the game's own default) | how far gunshots carry while the gunshots fix is on, `0.05..0.79` (lower carries further); no effect while the fix is off | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodstudio_log_weapon_model` | `0` | logs the third-person weapon model the spectated player holds, each time it changes | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodstudio_hide_scoreboard` | `0` | stops a POV demo's recorded TAB presses from putting the scoreboard over the shot | [`goldsrc_scoreboard.md`](goldsrc_scoreboard.md) |
| `dodstudio_mute_voice_commands` | `0` | silences "fire in the hole!" and the rest, without touching the game's own `.wav` files; subtitles and speaker icons still show | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `dodstudio_hide_crosshair` | `0` | hides the crosshair and keeps it hidden, which the stock `crosshair` cvar can't do because `CHud::Redraw` forces the value back every frame | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `dodstudio_match_pov_crosshair` | `0` | draws the spectator crosshair from `sprites/customXHair.spr` using the same tile `cl_xhair_style` gives the POV view, instead of DoD's hardcoded 24x24 tile of `crosshairs.spr`. Loses to `dodstudio_hide_crosshair`. Doesn't cover `cl_xhair_style 0` -- see issue #308 | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) §6 |
| `dodstudio_hide_hand_signals` | `0` | replaces any `hs_*` body sequence (the nod, the point, the wave -- players miming their own voice commands) with that player's last ordinary one, for everyone in view | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) §12 |
| `dodstudio_ex_interp_max` | `100` (the engine's own ceiling) | raises the engine's clamp on `ex_interp` above its stock 100 ms ceiling, for smoother entity motion between snapshots; refuses `<=50` or `>1000`. Mechanism live-proven, no specific value settled on yet | [`goldsrc_ex_interp.md`](goldsrc_ex_interp.md) |
| `dodstudio_hd` | `1` if there's a `dod/dodstudio_hd` folder, else `0`; `GOLDSRC_HOOKS_TEXTURE_HIRES=1`/`0` at launch overrides | HD textures on/off: map textures, model skins, sprites, detail textures and skies from `dodstudio_hd`. A change applies to what loads next -- walls, detail and skies from the next map, models and sprites already loaded after a restart. Turning it on in a session that started off installs the hook then | `goldsrc-hooks/src/texture_hires.rs`, `goldsrc-hooks/tools/hd/README.md` |
| `dodstudio_hd_style` | `ultrasharp` | which `dodstudio_hd/<type>/<style>` folder to use; a name with no folder means originals (plus `overrides`). Same timing as `dodstudio_hd` | same |
| `dodstudio_log_texture_loads` | `0` | logs every HD-eligible texture load: replaced (from which file) or why not | same |

## Commands

Always a command rather than a cvar when it has subcommands or a variable
argument count, which a cvar's single value can't hold.

### `dodstudio_debug_status`

No arguments. Unconditionally reports every cvar above plus
`dodstudio_deathmsg`'s own status; the animation fix and gunshots fix are only
included while enabled, since a flag being on says nothing about whether
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
| `dodstudio_deathmsg offset <0..4096>` | y the feed starts at (default 20) |
| `dodstudio_deathmsg offset default` | hand y back to the game |
| `dodstudio_deathmsg block <id>...` | hide frags involving these players (replaces the set) |
| `dodstudio_deathmsg block !<id>...` | hide everything *except* these players |
| `dodstudio_deathmsg block clear` | stop hiding anything |
| `dodstudio_deathmsg fake <killer> <victim> <weapon>` | inject one by hand; weapon is a name (`d_garand`, `garand`) or `1..43` |

### `dodstudio_hide_hudelement`

`dodstudio_hide_hudelement <name> <0|1>` with no arguments lists the twelve
elements DoD draws that the stock `cl_hud_*` cvars don't already reach --
chat, the kill feed, the status bar, the objective icons and the rest.
`dodstudio_hide_hudelement all 0` puts everything back. See
[`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) §7.

### `dodstudio_clear_decals`

No arguments. Empties the engine's 4096-slot decal pool on command,
unlinking each decal from its surface first the way the engine's own remove
functions do. Nothing to do with `r_decals`. Pre-Anniversary `hw.dll` only.
See [`goldsrc_decals.md`](goldsrc_decals.md).

### `dodstudio_overviewmap`

`dodstudio_overviewmap <full|mini> <x> <y> <w> <h>` places and sizes DoD's
overview map; `default` releases one back to the engine. With no arguments,
lists both maps and what the engine currently has. See `src/overview_map.rs`'s
module doc, including why it closes while a spectated player is scoped into a
sniper (the engine's own FOV gate, not this command).

### `dodstudio_msglog`

`dodstudio_msglog <name>... | all | clear` dumps chosen DoD user messages and
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

### `dodstudio_hd_misses`

Lists every texture that kept its original this session, map by map, grouped
by why: the HD file is for a different version of the texture, there's no HD
file (naming the file that would match), the file couldn't be used, or it
was left alone on purpose (tool textures, blank sprites, per-player skins). A
texture several maps use is listed under each. `dodstudio_hd_misses <map>`
shows one map; `dodstudio_hd_misses clear` forgets the list.

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
