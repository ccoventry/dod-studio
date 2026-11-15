# Watching another player in a POV demo (`dem_forcehltv 1`, #1452)

`dem_forcehltv 1` before `viewdemo` makes the demo player treat a POV demo as
an HLTV one: the spectator camera, `spec_mode`, in-eye on any player, and the
hook's HLTV features (`ds_spec_target`, `ds_spec_lock`, `ds_spec_match_pov`).
The question here is how good another player's first-person view looks that
way, and what it would take to make it look like his own POV.

The case behind it: Caleb ("[H]ard | Tron.bsod") has no demo of his own of an
MG42 run on `dod_avalanche`, round 1, 4:54:19 to 5:12:17 on the viewdemo bar
(Luger kill at 5:06:05), but his teammate Digital*Jedi recorded a POV demo of
that round (`tr2.dem`, a 2004 server build; it parses since #1459).

## Short answer

- **It works for the stretches where the recorder's client was sent the
  watched player.** Then the in-eye view follows him well: his aim, his
  kills in the kill feed, the enemies, blood and impacts, his gunshots (heard
  and, with `ds_spec_match_pov 1`, the animation fix runs on him).
- **Where the recorder's client was not sent him, there is nothing to show.**
  The camera freezes at his last known place. This is the big gap and no
  hook can fill it: the demo does not hold where he was. For Tron's run,
  Jedi's demo has him in 52% of the entity packets from 4:50 to 5:14; the
  first MG42 kill (4:54:19) is not in the demo at all, and of his 23 kills in
  the half only 6 have him in the demo through the 4 s before the kill.
- **With `ds_spec_match_pov 1` the gun disappears.** In-eye with the switch
  on, no view model in any frame on PRE or POST; with `ds_spec_match_pov 0`
  the MG42 is drawn. That is a hook bug and the first thing to fix (below).
- Everything else is close to an HLTV demo's in-eye view, which the hook
  already brings near POV.

**Recommendation:** worth a small follow-up, not a feature build. Fix the
hidden gun under `ds_spec_match_pov` (a bug), have Studio tell the user which
stretches of a player a POV demo holds (the probe below already computes it),
and leave the rest. Rebuilding a player the recorder never saw is not
possible from a POV demo.

For Caleb's run today: play `tr2.dem` with `dem_forcehltv 1`, in-eye on
Tron.bsod (`spec_mode 4; ds_spec_target 7`) with `ds_spec_match_pov 0` until
the gun bug is fixed, from 5:02:04. 5:02:04 to 5:12:17 is all there (his
kills at 5:02:22, 5:02:53, 5:03:83, the Luger at 5:06:05, 5:11:21, 5:12:17).
4:50 to 5:02 is not in the demo: for that part, Jedi's own POV or a chase
camera from Jedi is all there is.

## What `dem_forcehltv 1` does

From ReHLDS's reconstruction of the demo player (`rehlds/HLTV/Core/src/Server.cpp`,
`HLTV/DemoPlayer/src/DemoPlayer.cpp`, `World.cpp`); the binary behaviour
matches what was seen in the game, the code paths were not re-derived from
`Core.dll`.

- `dem_forcehltv` is a command of `DemoPlayer.dll`, not a cvar. It only
  matters at load: `LoadGame` passes it to `Server::LoadDemo`. Set it before
  `viewdemo`. Studio's Review locks it (`review_lock.rs`).
- The world is marked HLTV and the client gets `svc_hltv HLTV_ACTIVE`, so
  `IsSpectateOnly()` is true and every hook module treats it as an HLTV demo
  (`anim_fix`, `missing_shots`, the spectator modules all ran).
- The local player becomes slot `maxclients - 1`. If a real player had that
  slot, the engine would treat him as the local player; HLAE's own fix
  (`AfxHookGoldSrc/HltvFix.cpp`, debug cvar `__mirv_fixforcehltv`, on by
  default) makes `CL_IsThirdPerson` return 1 inside `CL_EmitEntities` so he is
  still drawn. In `tr2.dem` (maxclients 14) and the two wsod25 halves that
  slot is empty; in the other 6 demos surveyed below (maxclients 13) it is a
  real player.
- Every demo-only frame is dropped (`Server::ProcessMessage` keeps demo data
  only when not forcing HLTV), and so is the per-frame `demo_info_t` (the
  recorded view and view model). The client gets an empty `svc_clientdata`.

What that drops, counted by `analysis/examples/forcehltv_pov_probe.rs` over 9
POV demos (`solifotw-pb2`, `solifotw-warwithin`, four KTP season 8 halves by
gorilla, element, db and stealth, two wsod25 halves by warchyld and dyelife,
and `tr2.dem` by Digital*Jedi):

| demo-only frame | per half | what it carried | lost under forcehltv |
| --- | --- | --- | --- |
| type 3 console commands | 1,900-9,100 | the recorder's own commands (`+map`, `+attack`, binds) | yes: nothing the recorder typed runs |
| type 4 client data | every frame | the recorder's origin, angles, weapon bits, FOV | yes (only the recorder's view needs it) |
| type 6 events | 100-390, all the recorder's own | his gunshots: sound, muzzle flash, tracer, shell, impact | yes, and the server never sends them (0 `svc_event`s name the recorder) |
| type 7 weapon anims | 270-76,000 | the recorder's view-model animations | yes |
| type 8 sounds | 1,400-2,700 | the recorder's footsteps and landings (`pl_step*`, `jumplanding`) | yes |
| type 9 client.dll buffer | every frame | DoD's own per-frame demo data | yes |

All of it is the recorder's own, so it matters only when watching the
recorder himself. Watching someone else, nothing of his is in those frames:
his shots come from `svc_event` and his state from the entity packets, as in
an HLTV demo. The recorder's lost gunshots come back through
`missing_shots` (`ds_sound_restore_gunshots`): it logged Jedi's MP44 rounds
"with no fire event" all through the forcehltv sessions.

## What a POV demo holds about another player

The server sends a client only the entities in its PVS, plus whatever the mod
adds. The probe's `FH_TRACK=<entity>:<from>:<to>` mode counts the entity
packets that hold one player. Over a whole half (lower bounds: time dead or
not yet joined counts as missing):

| demo | recorder | teammates | enemies |
| --- | --- | --- | --- |
| `ktps8w10qf-element_soul_m1_saints_h2` | element | 81-86% | 22-31% |
| `ktps8w5-db_thunder_thunder2_h2` | db | 81-85% | 30-37% |
| `tr2.dem` (2004 build) | Digital*Jedi | 23-55% | 17-39% |

On the KTP servers teammates are sent most of the time (the overview map
needs them); enemies about a quarter to a third of the time. The 2004 server
sent only what was in the PVS, teammates included. Tron.bsod (teammate,
entity 7) through his run:

| viewdemo bar | in Jedi's demo |
| --- | --- |
| 4:50:02-4:55:10 | no |
| 4:55:10-4:55:31 | yes (one blip) |
| 4:55:31-5:02:04 | no |
| 5:02:04-5:14:00 | yes |

His kills in the half with the 4 s before each in the demo: 6 of 23 fully,
6 partly, 11 not at all.

## What the in-eye view looks like (Tron.bsod, PRE and POST)

Measured with `game_probe.py` (Brain account, game kept behind), test DLL
from `origin/dev` 416148b3, `tr2.dem` copied into each install's `dod_addon`
as `ds1452_tr2`, `spec_autodirector_internal 0; spec_mode 4; ds_spec_target 7`,
stepped seek forward (`ds_demo_seek_to`), HLAE frames at 30 fps of demo time.
The same on PRE and POST in every row.

Ranked by how much each hurts a clip:

| # | gap | why | fixable? |
| --- | --- | --- | --- |
| 1 | camera frozen at his last spot whenever the recorder's client did not have him (4:50-5:02 of the run) | not in the demo | no. Studio can say which stretches exist (the probe's track mode) |
| 2 | no gun with `ds_spec_match_pov 1`; drawn with `0` (POST, two moments, 30 frames each) | the hook: something under the switch hides or moves the view model off screen for this view. Suspect: `spectator_gun`'s lowering stuck on (a lowering signal reading differently on a POV demo's player entity); not confirmed | yes, small: find which signal holds the gun down (`ds_debug_status` prints the lowering), fix the rule |
| 3 | odd shapes at the edges of the view at times (a roof, a camo-textured wedge) | the in-eye camera sits at origin + fixed eye height (DoD's `V_GetInEyePos`, 22 or 18 units); on a roof edge or prone it ends up in or against geometry, and possibly inside his own model. Same as HLTV in-eye, not forcehltv-specific | partly: `ds_spec_match_pov`'s prone/crouch heights apply; the rest is the HLTV in-eye camera |
| 4 | his own footsteps and landing sounds missing | never sent: footsteps are played by each client for itself; a POV demo holds only the recorder's | no (an HLTV demo lacks them too) |
| 5 | movement a little less smooth than a POV | entity updates at the server's rate, interpolated, vs the recorder's per-frame view | no, same as HLTV |
| 6 | HUD: no health, ammo or stamina of his | a spectator HUD; the HUD messages in the demo are the recorder's (`Health`, `CurWeapon` go to Jedi only) | no, not his. Same as HLTV |
| 7 | spectator UI: bars, "Press DUCK for Spectator Menu", camera mode name | spectator view | already hidden by `ds_spec_hide_bars`, `ds_spec_hide_text` (on in the user's settings) |

What works as in an HLTV demo: kill feed (his kills, from the demo's
`DeathMsg`s), crosshair (spectator crosshair, hidden for an undeployed MG42 as
in his own POV under `ds_spec_match_pov 1`), other players with their
weapons, blood, wall impacts, explosions' scorch marks, the gun sounds of
every shooter, `ds_spec_lock`/`ds_spec_target` keeping the camera on him,
`anim_fix` (bipod up/down, fire, reload, Luger draw: all logged on Tron).

Watching the recorder himself (Jedi, in-eye) gives the HLTV-style view of
him, not his POV: it is worse than just playing the demo normally, which
draws the recorded view.

## Screenshots and clips

Kept out of the repo. In the main checkout's `local\review-images\1452\caleb\`:

- `pre_pov_vs_tron_eye.png`: Jedi's own POV at 5:02:30 next to the forcehltv
  in-eye view of Tron at the same moment, then Tron at the Luger kill
  (5:06:05) and an MG42 kill (5:11:21). PRE.
- `pre_tron_eye_run.png`, `post_tron_eye_run.png`: frames through the run,
  frozen until 5:02, then following him.
- `post_knobs_matchpov_gun.png`: `ds_spec_match_pov 1` (no gun) next to `0`
  (MG42 drawn), `__mirv_fixforcehltv 0`, and a chase camera.

Clips (with a stopwatch) in `D:\dodstudio-tests\caleb-avalanche\preview\`:
`tron_forcehltv_ineye_4m52-5m18_PRE.mp4`, `tron_forcehltv_ineye_5m01-5m15_POST.mp4`.
Both were recorded with `ds_spec_match_pov 1`, so without the gun.

## Pitfalls found on the way

- **`dem_jump <t>`** (the demo player's own jump-and-pause) backwards in a
  POV demo landed at the demo's start, not at `t` (PRE). Seek forward with
  `ds_demo_seek_to`.
- A `viewdemo` of a demo in `dod_addon` leaves `lookahead` looking for it in
  `dod\` ("can't open ...\dod\ds1452_tr2.dem"), so the look-ahead part of the
  animation fix is off for such demos.
- After a backward jump `spectator_input_gate` logs "the demo clock went
  back" on every frame (217 lines in one session): its log line needs a
  budget.
- PR #1458 drops `+map`/`+mapzoom` "while a POV demo plays", judged by
  `IsSpectateOnly()`. Under forcehltv that is true, so the hook counts it as
  HLTV and lets `+map` through; it does not matter, because forcehltv drops
  the type-3 frames that carry the recorder's presses.

## Follow-ups

1. The gun under `ds_spec_match_pov` in a forcehltv view (gap 2).
2. `lookahead` should open the demo the engine actually opened (`dod_addon`,
   subfolders, `../` paths).
3. `spectator_input_gate`'s per-frame log line.
4. If Studio offers "watch another player" for POV demos: show the stretches
   where the demo holds that player (`forcehltv_pov_probe` track mode), and
   start forcehltv playback from there.
