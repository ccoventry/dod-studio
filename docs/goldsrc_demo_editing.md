# Editing what happens in a demo (#1469)

How far a recorded GoldSrc demo can be rewritten so the game plays back
something that never happened: a player who walks another way, or a player
who is there for seconds the demo never had him. The R&D tool is
`analysis/examples/entity_edit_probe.rs` (its header lists the modes).

Every number below was measured on the demos named; what was seen in the
game is marked as such, and what is inferred says so.

## Short answer

- **Moving a player works**, as a file edit through `dem-patch`'s writer,
  and plays on PRE and POST with no error: he is drawn on the new path and joins
  the real data with no snap; his shots would follow him (events take his
  drawn position; inferred, he does not fire in the test). **What
  breaks:** nothing collides, so a bend into a wall draws him inside it (all
  black: no light inside solid), and his footsteps and other sounds stay
  where he really was (`svc_sound` carries its own position).
- **Filling a gap works** under `dem_forcehltv 1`: 4 s of made-up data for
  Tron.bsod in `tr2.dem` before he first appears, walked along a real path
  borrowed from another player's track in the same demo, gives an in-eye and
  chase camera that move through those seconds and join his real data at
  5:02:07 with no visible snap, on PRE and POST. The data is invented: no
  shots, no sounds, not where he really was. The 4:54:19 kill is in the demo
  only as the kill feed and the victim; nothing of Tron's.
- **The recorder's own view (step 3) can be moved too**: four plain fields
  per frame. The gun must move with it (the recorded view-model origin), and
  the camera passes through walls.
- **Edit the file, not the hook**, for all three (table at the end).
- **Recommendation:** a "fill the gap" tool for clips is not worth building
  as a Studio feature. It can only start a clip a few seconds earlier with
  movement that never happened and no action in it, and the moments users
  want (Tron's 4:54:19 kill) need the shooter, which no POV demo of someone
  else holds. Keep `entity_edit_probe` as the R&D tool; if a creative "edit
  a scene" feature is ever wanted, the pieces below (bend with a BSP hull
  check, borrowed paths, the event and count bookkeeping) are what it needs.

## How a player is stored, and what an edit has to keep true

A player another client sees is an entity in the `svc_deltapacketentities`
stream (`docs/goldsrc_demo_salvage.md` has the structure). The facts an edit
rests on:

- **`origin` and `angles` are absolute.** A packet lists only the fields that
  changed since the snapshot its `delta_sequence` names; a field it leaves
  out carries over from that snapshot. So an edit that changes his origin
  must list it in every packet that holds him, or packets that do not list it
  carry a stale (edited or unedited) value forward. `bend` lists all three
  origin fields in every packet of the window that holds him, inserting an
  entry for him where the packet had none.
- **A player new to a snapshot is decoded against his spawn baseline**, not
  against what the client last had. So the first real packet after a gap
  lists every field that differs from the baseline, and leaves out the ones
  that equal it. If made-up frames before it set a field the real packet
  leaves out, that made-up value survives into the real data. `fill` makes
  every made-up packet and the first real one list his whole state.
- **Indices are re-encoded per packet.** An inserted entry changes the
  6-bit index difference of the entry after it; `set_entity_fields`
  re-encodes the packet's differences from 0, absolute only for a gap over
  63, as the engine writes them.
- **`entity_count` is the resulting snapshot's size** and a count below the
  carried set is a crash (`goldsrc_demo_salvage.md`, "Still open"). Adding a
  player to 117 packets raised 117 counts; `restate_counts` rewrites them
  from the carried set (the repair's `entity_count` step). A bend never
  changes a snapshot's membership, so it changed none.
- **Events find their player by place in the snapshot, not by number.** An
  `svc_event` names its entity with `packet_index`, an index into the
  frame's snapshot, and the client takes the event's origin and angles from
  that entity when the event carries none (Tron's 28 `mg42.sc` events in
  302.0-304.0 s all do it that way: packet index 4, no origin of their own).
  So:
  - a bent player's shots fire from where he is drawn (inferred from the
    encoding, not watched: the bent player in step 1 does not shoot);
  - adding a player to a snapshot moves every higher-numbered entity one
    place up, and an event pointing at one of them would fire from the
    wrong entity. `shift_event_packet_indices` moves those events up one in
    every frame he was added to.
- **Sounds carry their own origin.** The other player's footsteps,
  landings and similar sounds arrive as `svc_sound` with an explicit
  position (`heard`: entity 4 in step 1's window has 91
  `player/jumplanding.wav` and 11 `pl_step*`/`pl_wood*`, every one with x, y
  and z). Bent, they still play where the demo had him.
- **The writer round-trips.** `solifotw-pb2.dem` writes back byte for byte
  (96,936,350 bytes). `tr2.dem` (the 2004 build) writes back the same length
  with 6 bytes different, all in the directory at the end of the file
  (offset 20,216,515 on): a directory field the writer recomputes. The
  frames are unchanged.

## Step 1: move a player in a POV demo

`solifotw-pb2.dem` (a smooth POV in both installs), entity 4, a player who
walks a straight 907 units south ahead of and beside the recorder, 150-1500
units from his camera within 30 degrees of where he looks, at 13:57:80 to
14:01:90 (svc 837.8-841.9; `find` lists such stretches). `bend ... 4 837.8
841.9 160` swerves him up to 160 units to his left (east), `160 * sin^2(pi
u)`, out and back, so the real packets after the window join with no snap.
375 packets edited, no count changed (he was in every snapshot already), the
file grows by 563 bytes and re-parses (`pb2_bend_ent4.dem`).

**In the game** (viewdemo by full path, `ds_demo_seek_to 831`, HLAE frames at
30 fps of demo time, side by side with the original, PRE and POST):

- Plays on both builds: no `svc_bad`, no `Fatal Error`, no crash.
- **He goes where the file says.** In the original he walks through the arch
  ahead of the camera; bent, he leaves that line and is drawn off to the side.
- **Nothing stops him at a wall.** At the swerve's widest he is inside the
  arch's wall, and the model is drawn there, all black: a model is lit from the
  world under its origin, and an origin inside solid gets no light (PRE,
  2.16 s on `step1_bend_ent4_PRE.mp4`). The footprint check (no bent point more
  than 64 units across from a place some player stood) passed for all 375
  points: a place someone stood 60 units away can still be on the other side
  of a wall. A bend needs a real collision test against the BSP (a hull trace),
  or a path borrowed from real movement as step 2 does.
- **No snap where the edit starts or ends**: the sin^2 ramp starts and ends at
  zero offset, and the frames at the window's edges match the original
  (inferred from the data and seen on the sampled frames; the clips were not
  watched frame by frame for interpolation jitter).
- **What stays at the old place** (from the file; not listened to in the
  game): his footsteps and landings, 102
  `svc_sound`s in the window, each with its own x, y and z. His shots would not
  (events take his drawn position), but he does not fire in this window.
- His legs and upper body play the server's sequences for walking straight;
  a sideways swerve with unchanged `angles` is a strafe. Whether DoD's client
  turns the legs (GoldSrc estimates gait from the movement between frames)
  could not be judged from the clips at this distance.

## Step 2: fill a gap before a player first appears

`tr2.dem` (2004 build, recorded by Digital*Jedi, `docs/goldsrc_forcehltv_pov.md`):
Tron.bsod is entity 7. The demo has him at 4:55:10-4:55:31 (one blip) and
from 5:02:07 (svc 302.075) on; nothing between.

**What was built** (`fill ... 7 302.0 4 borrow`, `tr2_fill_ent7.dem`): 4 s of
him before 5:02:07, 117 packets from 4:58:07. The path is a real one: the
probe looks through every other player's track in the demo, the recorder's
own included (his frame headers' `simorg`), for a 4 s stretch at walking pace
that ends within 128 units of where Tron arrives, heading within 60 degrees
of his way. It found Jedi's own run 842.39-846.36 s (later in the half),
ending 4 units from Tron's arrival point, and shifts it by that last 4 units,
growing from nothing at its start. Angles are Jedi's (view pitch turned into
model pitch: a third, the other way up), turned into Tron's own over the last
0.5 s. Every other field is Tron's arrival state; his upper-body `frame`
runs on at the rate his next real packet shows (400/s, wrapping at 256).
Straight lines walked back from the arrival were tried first and dropped:
30-90 of 117 points had no footprint of any player within 64 units, which on
`dod_avalanche` means a wall or a drop.

Offline: 117 counts restated, 4 events whose `packet_index` pointed past him
moved up one, the result re-parses with every count equal to the carried set,
and the analyzer reads it (13 players).

**In the game** (`dem_forcehltv 1`, `spec_autodirector_internal 0; spec_mode
4; ds_spec_target 7`, `ds_spec_match_pov 0`, viewdemo by full path with
`ds_demo_other_drives 1`, PRE and POST):

- Loads and plays on both builds: no `svc_bad`, no `Fatal Error`, no
  `couldn't uncompress`, no crash (PRE loaded it in 19.7 s, POST in 7.0 s).
- **The in-eye camera moves through the made-up seconds** instead of sitting
  frozen at his 4:55:31 spot: through a house (the borrowed run goes through
  a building and out of a window) and onto the street where the real data
  starts.
- **The join at 5:02:07 does not show**: the frame before and after it are
  the same view, one step apart. The tilted view right after the join is in
  the original too (#1452's clip at 5:02:1x).
- PRE and POST draw the same frames (mean difference 5/255 between the two
  clips once lined up).
- **His body in a chase camera** (`spec_mode 1`) walks the made-up stretch
  with his own model, lit normally, legs in a running stride and the MG42
  carried (his sequence 159, gait 12 from his arrival state; the client
  animates the legs from the movement), through the house and out of the
  window, then carries on into his real data (firing from the hip at 5:02:2x,
  real). Only the borrowed player's path and angles are used, never his
  sequences: his class and weapon differ from Tron's. PRE and POST draw the
  same frames here too (mean difference 1.8/255).

**What a fill cannot give:**

- His HUD, his health, his ammo: never his in this demo (`goldsrc_forcehltv_pov.md`).
- Anything he did: no shots (no events), no sounds of his. A made-up stretch
  is a walk, and any shot in it would be invented too.
- The 4:54:19 kill. The demo has the `DeathMsg` (`[7, 11, 17]`: Tron killed
  entity 11 with the MG42) and the victim (entity 11 is in the recorder's
  snapshot then, so his death plays), but not Tron. Filling 4:54-5:02 would
  mean 8 s of invented movement plus an invented firing position and burst
  that has to line up with entity 11's death; the kill feed would be real and
  everything about the shooter made up.
- Where he really was. The borrowed path is a place someone walked in that
  half, not where Tron went.

## Step 3: change the recorder's own view

The issue asks only for a plan here; it turned out small, so it was built
and tried (`bend-recorder ... 837.8 841.9 120`, `pb2_bend_recorder2.dem`). In a POV demo
the recorder's position is not in the entity packets. What has to move
together, all plain fields:

- each network frame header's `refparams.vieworg` and `simorg` (the drawn
  view: POV playback copies the recorded view over the live one);
- the header's `view` (offset 420 of `demo_info_t`), the **view model's
  origin**: it equals `vieworg` (-251.77, 1963.70, 49.74 against -251.46,
  1963.39, 50.76 on the first edited frame). The first build left it alone,
  and in the game (PRE and POST) the camera moved while the gun stayed on the
  old path: no gun, or a gun floating off to the side. Moved too, the gun
  stays in the hand;
- the type-4 client-data frames' `origin` (982 in the window);
- `svc_clientdata`'s `origin[0]`/`[1]` where a frame lists them (374), for
  prediction;
- his own shots' origins (type-6 events), none in this window.

In the game the swerved camera plays on both builds with no error, the gun
in the hand, and the camera **passes through walls** (inside a stone wall at
2.50 s on `step3_recorder_PRE.mp4`): nothing in playback collides. What it
cannot change: anything the recorder's client baked into the view (his
screen shake, his aim), what the other players did about where he really
was (they shoot at the old spot), and his footsteps (type-8 sounds play at
the listener, so they follow, which is right).

## Step 4: bigger edits (plan)

Each is the same toolkit (`set_entity_fields`, `restate_counts`,
`shift_event_packet_indices`) plus the parts named here. None was built.

- **Remove a player.** Write a `remove_entity` entry for him in the first
  packet of the stretch and drop his entries from every packet after it
  until he should come back (then re-add him as new: full state against the
  baseline, as `fill` does). Counts drop by one (`restate_counts`). Every
  event pointing at a place past his moves *down* one; his own events go.
  His `svc_sound`s go (they name him). What stays and gives him away: the
  kill feed (`DeathMsg`), the scoreboard's user messages, his name in `SayText`, and the
  blood and impacts other players' shots leave where he stood (temp entities and decals carry their
  own positions). The recorder's own view of him, if the recorder aimed at
  him, does not change: the recorded camera is baked in.
- **Swap who wins a fight.** The kill is a `DeathMsg` user message
  `[killer, victim, weapon]` (step 2's tr2 has `[7, 11, 17]` at 4:54:19):
  rewriting the bytes swaps the kill feed. The bodies are entity fields:
  the loser's `health` going to 0, his death `sequence`, `movetype`, and the
  winner staying alive means his `health`/`sequence` continue from his last
  living state, and the old loser's data after the death (a corpse, then a
  respawn) has to be replaced with plausible living data, which is step 2's
  problem again. The scoreboard and team scores would need their messages
  rewritten too. The shots (events) fire from wherever each player is
  drawn, so they follow without changes; the blood (temp entities) is at
  the old victim's spot.
- **Move a whole scene.** Shift every entity and every positioned message
  (events with their own origin, `svc_sound` origins, temp entities,
  `svc_spawnstaticsound`) by the same offset; the world does not move, so it
  only works where the target place has the same geometry, which no DoD
  map offers. Not worth it.
- **What can never be edited this way:** anything baked into the recorded
  view in a POV demo (the camera, the recorder's screen shake, his view
  model), and anything the client builds from the world (decals already
  placed, which persist; lighting). And no edit adds what the server never
  sent: a fill is always made up, never recovered.

## Edit the file or fake it in the hook

| step | file edit | hook at playback | pick |
| --- | --- | --- | --- |
| 1, move a player | origin fields in the packets, a few hundred per swerve; the result plays in every pipeline (viewdemo, playdemo, forcehltv, HLAE capture, Studio's seek) and on both builds with no code in the game | override his origin every frame in `HUD_AddEntity`/`CL_EmitEntities`; needs his interpolation history kept in step, per-frame work in the hot path, a table per engine build, and still leaves the server's sounds where they were | file |
| 2, fill a gap | the only way under `dem_forcehltv 1`: `Core.dll`'s HLTV `World` rebuilds snapshots from the file, and the spectator camera and `ds_spec_target` follow what that world holds | the hook would have to make an entity exist that no snapshot has (model, state, the HLTV world's player list) before the camera can target it | file |
| 3, move the recorder | plain struct edits of the frame headers and client-data frames, no bit-packing | the hook already rewrites the drawn view (the bob, #710), so an offset added after the demo's view override is small; but every consumer (HLAE campaths, captures) would need the hook, and his own shots' origins would not move | file for a fixed edit, hook for an interactive one |

The file edit is simpler in each case because the demo is the one input
every pipeline shares; a hook edit is per-session code in the game's hot
path. The hook wins only for something chosen live (a "nudge the camera
while reviewing" control), not for a stored edit.

## Files

Kept out of the repo, in `D:\dodstudio-tests\unattended-2026-11-16\demoedit\`:

- `pb2_bend_ent4.dem`, `pb2_bend_recorder2.dem` (`pb2_bend_recorder.dem`: the
  first build, without the view-model origin), `tr2_fill_ent7.dem`, with the
  probe's logs.
- `step1_bend_ent4_PRE.mp4` / `_POST.mp4`: original beside bent, stopwatch;
  the swerve (13:57:80-14:01:90) is about 0.7-4.8 s on the clock.
- `step3_recorder_PRE.mp4` / `_POST.mp4`: original beside the swerved
  recorder (with the gun fix).
- `step2_fill_tron_ineye_PRE_POST.mp4`: in-eye on Tron, PRE beside POST,
  clock 0 at about 4:58:80; the made-up data ends at 3.28 s.
- `step2_fill_tron_chase_PRE_POST.mp4`: the same in a chase camera, clock 0
  at about 4:58:10; the made-up data ends at about 3.97 s.
- `step1*_pre|post`, `step2*_pre|post`: the game_probe reports and frames.

Stills in the main checkout's `local\review-images\1469\`:
`step1_bend_ent4_PRE_detail.png` (the black model in the wall at 2.16 s),
`step1_bend_ent4_*_sheet.png`, `step3_recorder_*_sheet.png`,
`step2_fill_POST_frames.png` (in-eye across the join),
`step2_fill_PRE_chase_frames.png` (his body in the made-up stretch),
`ref_1452_orig_around_302.png` (#1452's original clip around the join).
