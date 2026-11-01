# X-ray: players through walls (#1297)

`ds_spec_xray 1` draws every player behind a wall as a glow in his team's
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

TODO(after tests)
