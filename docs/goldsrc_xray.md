# X-ray: players through walls (#1297)

`ds_spec_xray_show 1` draws every player behind a wall as a glow in his team's
colour while a demo plays, like the X-ray of CS:GO's and CS2's GOTV.

## What CS:GO and CS2 do

- **The switch.** `spec_show_xray` (0/1), a client setting for spectators,
  GOTV and demos: "If set to 1, you can see player outlines and name IDs
  through walls - who you can see depends on your team and mode" (the cvar's
  own help text). A live spectator toggles it with a key (X by default).
  CS2 adds `spec_xray_dropped_defusekits` and `spec_xray_dropped_unoccluded`
  for the bomb and defuse kits.
- **What is drawn.** A glow in the player's team colour around his whole
  body: the Source glow, a soft, blurred outline with a faint fill. CT blue,
  T yellow-orange. The player's name shows through the wall too.
- **Only where hidden.** The glow is drawn where the player is occluded,
  not where he is in plain view: a player in the open looks normal, one
  behind a wall glows through it, one half behind a box glows only where the
  box hides him.
- **Who.** Every living player, both teams, for a spectator or a demo.
  The player the camera looks out of (first person) is not drawn at all, so
  has no glow; in the free and fixed cameras the selected player's glow
  turns white.
- **Brightness follows noise.** A quiet player glows dimmer than one who
  just made a sound: `spec_glow_full_time` (1 s at full brightness after a
  noise), `spec_glow_decay_time` (then 2 s down to) `spec_glow_silent_factor`
  (0.4 in CS2's list, the "lurking player" level), `spec_glow_spike_factor` /
  `spec_glow_spike_time` (a brief pop on the noise). No health tint.

Sources: the cvar list with help texts in
[OhLasFar/CS2-ALL-Console-Commands-Repository](https://github.com/OhLasFar/CS2-ALL-Console-Commands-Repository)
(`spec_show_xray`, `spec_glow_*`, `spec_xray_*`);
[totalcsgo.com: spec_show_xray](https://totalcsgo.com/commands/specshowxray)
("showing the outlines of players through walls", demos and Overwatch);
[csdb.gg: spec_show_xray](https://csdb.gg/command/spec-show-xray);
the Counter-Strike wiki's
[Spectator](https://counterstrike.fandom.com/wiki/Spectator) page (the X key,
"the team-colored silhouette of all players is shown through the walls").

## What DoD Studio does

`ds_spec_xray_show` (off by default; the Settings tab of the DoD Studio window
has it under Spectating):

- `1`: every living player on a team glows in his team's colour wherever
  something hides him: Allies green, Axis red (the kill feed's hues; set
  your own with `ds_spec_xray_allies_color` / `ds_spec_xray_axis_color`,
  "R G B" 0-255 like the game's `con_color`, console only). A
  player in plain view looks as he always does, one half behind a crate
  glows only where the crate hides him. As in CS:GO.
- `2`: the same, and the players in view get an outline in their team's
  colour too.
- The player the in-eye camera looks out of has no glow (he isn't drawn).
  In the chase cameras the watched player glows like anyone else when a
  wall hides him.
- Never in a live game, where it would be a wallhack: demo playback only.

What isn't CS:GO's: no names through walls (`ds_spec_show_player_info` draws
those in HLTV demos, through walls too), no dimming of quiet players, no white selection
highlight, nothing for dropped items.

### How it is drawn

The game's own glow shell (`kRenderFxGlowShell`, renderfx 19): the engine
pushes the model out along its normals by `renderamt` x 0.05 units (the
constant at PRE `hw+0x8228c`) and draws it in `rendercolor`, added to the
picture. Before the renderer draws each player (`StudioDrawPlayer`, wrapped
in `strafe_twitch.rs`), `spec_xray.rs` draws him once as a shell with:

- the depth test turned round (`GL_GREATER`), so the shell only lands where
  something already drawn is in front of it; drawn before the player
  himself, so his own body never hides his other limbs. With `2` the depth
  test passes everywhere and the player then draws over the shell's middle,
  which leaves a ring: the outline;
- no depth writes, so nothing drawn later sees the shell;
- only the shell: the client draws a glow-shell model twice (the model, then
  the shell), and the engine's `StudioDrawPoints`, wrapped in the
  `engine_studio_api_t` before the client copies it, skips the first;
- `r_glowshellfreq` at 0 for the pass: the stock shell is lit from a
  direction that turns with the clock (PRE `hw+0x8230c`), which shimmers.
  Held at 0 it stays the same frame to frame (#1297: movies at 300 fps);
- no texture: a flat colour, lit by the engine's model light;
- a thickness that grows with distance (`0.004` units per unit away, 1.5 to
  12.75 units), so the glow's edge stays about the same on screen.

**Every player on the map, in an HLTV demo.** The engine drops each entity
outside the PVS of one point before the renderer sees it (`ClientDLL_AddEntity`,
PRE `hw+0xbae0`, POST `hw+0x1969b0`; see `goldsrc_spectator_camera.md`). That
check is skipped while `dev_overview` is set (PRE `hw+0x20920`, POST
`+0x1ad020`: `dev_overview` and, in an HLTV demo, no `sv_cheats` needed), and
`HUD_AddEntity` runs inside the same function, just before it. So for each
player who glows, `dev_overview`'s value is 1 for that one check and 0 again
from the next entity on, and before `V_CalcRefdef` at the latest, so
nothing is drawn with it set. Only the value is written, never the string,
and only when it is 0. No `hw.dll` code or address is touched: the same on
both builds. A POV demo carries only the players the recorder's server sent
him (his own PVS), so those are all it can show.

### What a clean CS:GO outline would take

CS:GO draws its glow as a screen-space effect: each player into a stencil
mask, the mask blurred, the blur kept only outside the body. In GoldSrc the
cheap route to an outline is the stencil buffer (mark the body, draw the
fatter shell only outside it), but HLAE's GL context on PRE has no stencil
bits (`GL_STENCIL_BITS` 0, measured), so that route is closed there.

Tried and dropped:

- a texture bound for the shell that is dark in the middle and bright at
  the rim (the chrome mapping the engine gives the shell would have made it
  a rim light): the engine's shell ignores it, the result was identical to
  the flat shell;
- the stock shell (the game's chrome sprite, shimmering): sparse dots, and
  it moves every frame;
- stacking three shells of growing thickness for a soft edge: brighter
  where they overlap, no softer.

So `1` is a filled glow, faceted where the model's faces are (the shell is
the model pushed out, lit per face), brighter where a limb crosses the body.
A soft outer edge would need our own blur pass over a mask, which is a
render-to-texture step GoldSrc's fixed-function GL can do but this crate
doesn't have yet.

### Cost

One extra draw of each player who glows (two `StudioDrawPlayer` calls a
frame instead of one). Measured on `wsod25_grp1_h1_hltv` (`dod_armory_b6`,
12 players), in-eye, `fps_max 1000`, 6 s samples alternating off and on
(`ds_debug_record_frame_times`): PRE off 232.2 / 238.9 / 238.4, on 239.5 /
237.4 / 238.7; POST off 238.9 / 239.5 / 239.8, on 238.6 / 238.2 / 238.6. Both
sit on the same ~239 fps ceiling, so no cost shows below it; above it was not
measured.

### In a recording

The glow is drawn into the frame like any model, so HLAE records it: the
probe's `shot`/`clip` steps are HLAE recordings (`mirv_streams`), and show
it on both builds.
