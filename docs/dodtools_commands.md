# The `dodtools_*` console surface

One place to scan the whole `goldsrc-hooks` cvar/command surface without
grepping `commands.rs`. Closes issue #313.

Every name here shares the `dodtools_` prefix ([`names.rs`](../goldsrc-hooks/src/names.rs)'s
`console_name!` macro). `dodtools_debug_status` reports the live value of
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
| `dodtools_hltv_gunshots_fix` | `1` if `GOLDSRC_HOOKS_FORCE_WEAPON_VOLUME=1` at launch, else `0` | forces DoD weapon-fire sounds to full volume with no distance attenuation while spectating | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodtools_hltv_show_viewmodel_animations` | `0` (off) unless `GOLDSRC_HOOKS_ANIM_FIX` sets a starting level | `0`=off, `1`=empty hand on throw, `2`=redraw immediately, `3`=never empty, `4`=redraw after a 1s lookahead (the recommended setting) -- corrects MG42/MG34/BAR/Bren viewmodel deploy animations while spectating in-eye | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodtools_hltv_gunshot_attenuation` | `0.8` (`ATTN_NORM`, the game's own default) | how far gunshots carry while the gunshots fix is on, `0.05..0.79` (lower carries further); no effect while the fix is off | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodtools_log_weapon_model` | `0` | logs the third-person weapon model the spectated player holds, each time it changes | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `dodtools_hide_scoreboard` | `0` | stops a POV demo's recorded TAB presses from putting the scoreboard over the shot | [`goldsrc_scoreboard.md`](goldsrc_scoreboard.md) |
| `dodtools_mute_voice_commands` | `0` | silences "fire in the hole!" and the rest, without touching the game's own `.wav` files; subtitles and speaker icons still show | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `dodtools_hide_crosshair` | `0` | hides the crosshair and keeps it hidden, which the stock `crosshair` cvar can't do because `CHud::Redraw` forces the value back every frame | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `dodtools_match_pov_crosshair` | `0` | draws the spectator crosshair from `sprites/customXHair.spr` using the same tile `cl_xhair_style` gives the POV view, instead of DoD's hardcoded 24x24 tile of `crosshairs.spr`. Loses to `dodtools_hide_crosshair`. Doesn't cover `cl_xhair_style 0` -- see issue #308 | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) §6 |
| `dodtools_hide_hand_signals` | `0` | replaces any `hs_*` body sequence (the nod, the point, the wave -- players miming their own voice commands) with that player's last ordinary one, for everyone in view | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) §12 |
| `dodtools_ex_interp_max` | `100` (the engine's own ceiling) | raises the engine's clamp on `ex_interp` above its stock 100 ms ceiling, for smoother entity motion between snapshots; refuses `<=50` or `>1000`. Mechanism live-proven, no specific value settled on yet | [`goldsrc_ex_interp.md`](goldsrc_ex_interp.md) |

## Commands

Always a command rather than a cvar when it has subcommands or a variable
argument count, which a cvar's single value can't hold.

### `dodtools_debug_status`

No arguments. Unconditionally reports every cvar above plus
`dodtools_deathmsg`'s own status; the animation fix and gunshots fix are only
included while enabled, since a flag being on says nothing about whether
their preconditions are currently being met and they'd otherwise be noise
when off. Force-calls the per-frame `poll()` first, so chaining a cvar set
and a status query on one `;`-joined console line reports the post-change
state, not the state from before that line ran.

### `dodtools_deathmsg`

Raises DoD's hard-coded four-line cap on the kill feed, moves it, hides
frags, or injects one by hand. HLAE's own `mirv_deathmsg` supports only
`cstrike` and `tfc`, so none of it works for DoD. See
[`goldsrc_death_notices.md`](goldsrc_death_notices.md).

| subcommand | does |
| --- | --- |
| `dodtools_deathmsg` | status + usage |
| `dodtools_deathmsg max <4..127>` | lines of kill feed shown at once (default 4) |
| `dodtools_deathmsg offset <0..4096>` | y the feed starts at (default 20) |
| `dodtools_deathmsg offset default` | hand y back to the game |
| `dodtools_deathmsg block <id>...` | hide frags involving these players (replaces the set) |
| `dodtools_deathmsg block !<id>...` | hide everything *except* these players |
| `dodtools_deathmsg block clear` | stop hiding anything |
| `dodtools_deathmsg fake <killer> <victim> <weapon>` | inject one by hand; weapon is a name (`d_garand`, `garand`) or `1..43` |

### `dodtools_hide_hudelement`

`dodtools_hide_hudelement <name> <0|1>` with no arguments lists the twelve
elements DoD draws that the stock `cl_hud_*` cvars don't already reach --
chat, the kill feed, the status bar, the objective icons and the rest.
`dodtools_hide_hudelement all 0` puts everything back. See
[`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) §7.

### `dodtools_clear_decals`

No arguments. Empties the engine's 4096-slot decal pool on command,
unlinking each decal from its surface first the way the engine's own remove
functions do. Nothing to do with `r_decals`. Pre-Anniversary `hw.dll` only.
See [`goldsrc_decals.md`](goldsrc_decals.md).

### `dodtools_overviewmap`

`dodtools_overviewmap <full|mini> <x> <y> <w> <h>` places and sizes DoD's
overview map; `default` releases one back to the engine. With no arguments,
lists both maps and what the engine currently has. See `src/overview_map.rs`'s
module doc, including why it closes while a spectated player is scoped into a
sniper (the engine's own FOV gate, not this command).

### `dodtools_msglog`

`dodtools_msglog <name>... | all | clear` dumps chosen DoD user messages and
their payloads to the log file, forwarded to the game untouched. See
`src/msglog.rs`'s module doc.

### `dodtools_objectives`

Places the territory-flag icon row and the objective timer beside it, which
the game draws ~117px lower at 1080p while spectating than in a POV demo. See
[`goldsrc_objective_icons.md`](goldsrc_objective_icons.md).

| subcommand | does |
| --- | --- |
| `dodtools_objectives offset <y>` | y the objective icons are drawn at |
| `dodtools_objectives xoffset <x>` | x the icon row starts at |
| `dodtools_objectives timer <y>` | y the objective timer is drawn at (no x -- the game hardcodes it) |
| `dodtools_objectives <any> default` | hand that one back to the game |

## Open, not yet merged

Tracked here so a scan of this file doesn't miss what's about to land, but
these aren't real until their PR merges -- check the PR before relying on
anything below.

| command | issue | PR |
| --- | --- | --- |
| `dodtools_hide_sprite <model-path>...` | #315 | #318 |

## Keeping this in sync

Manual, like the rest of this crate's docs -- there is no generator from
`commands.rs`. Each PR that adds or changes a `dodtools_*` entry should
update this file the same way it already updates `README.md`'s own
control-surface list; the two lists should never drift apart. Issue #313
left open whether this eventually becomes a generated table or a GitHub wiki
page instead -- not resolved here.
