# The spectator camera under `spec_match_pov`: following one player, the in-eye height, and the death cam

Four fixes to where the camera sits while watching a player from an HLTV
demo, all part of the `ds_spec_match_pov` / `spec_lock` / `spec_target`
family in the hook DLL: which player the camera is on (#206, first section),
how high the in-eye camera is when that player goes prone (#329, second) and
as he crouches (#1144, third), and what it shows when he dies (#709,
fourth). The first two were
`goldsrc_spectator_follow.md` and `goldsrc_spectator_eye_height.md`. A fourth,
always on, makes the chase cameras draw the players around them (#918, last
section).

---

## Keeping the camera on one player in an HLTV demo (#206)

Two settings in the hook DLL (`goldsrc-hooks/src/spectator_follow.rs`):

| | |
| --- | --- |
| `ds_spec_lock 1` | the camera stays on the player being watched when he dies, instead of moving to the next player four seconds later. A cvar, off by default. |
| `ds_spec_target <player>` | puts the camera on a player, by number. `ds_hud_deathmsg players` lists the numbers. With no number it says who the camera is on. A command. |

Both act on HLTV demo playback only. A first-person demo's view is the
recording player's own.

### What moves the camera

Not the demo. An HLTV recording carries no camera commands: its `svc_director`
messages are `START`, a 16-second `STATUS` heartbeat and title cards, and the
proxy is not sent `iuser1`/`iuser2`. The player being watched is a plain
global in `client.dll`, `g_iUser2`, and only the client's own code writes it:
the viewer's keys, the spectator panel's dropdown, a reset when a demo loads,
and one routine that runs on its own.

That routine is `CHudDoDCommon::Draw` (`client+0x2c6f0`; it draws nothing).
Each frame it looks up whether the watched player is dead, and four seconds
after he dies it issues the keypress a viewer would:

```text
+0x2c762  mov eax, [g_iUser2]
+0x2c771  mov al, [player * 48 + g_PlayerExtraInfo.dead]
+0x2c777  test al, al
+0x2c779  je   +0x2c7af              ; alive: clear the timer
...                                  ; dead for 4s:
+0x2c7a1  push "+attack;wait;-attack"
+0x2c7a6  call [pfnClientCmd]
```

Seen live, with `ds_debug_log_spectator_target 1`, on
`monday-wsod25_r07_m1_h1_hltv`:

```text
587.778  the watched player (2) dies
591.767  client.dll ClientCmd: "+attack;wait;-attack"
591.773  g_iUser2 = 3
```

### The lock

One byte: the `je` at `+0x2c779` becomes a `jmp`. To this routine alone the
watched player is always alive, so the timer never starts and the keypress is
never issued. Nothing is put right after the fact, so no frame is drawn from
the wrong player. The viewer's own `+attack` and the dropdown still change
player.

With the lock on, the same stretch of the demo has no `ClientCmd` line and
`g_iUser2` stays 2. While he is dead the view is the game's own for a dead
target (near the ground where he fell); it is his view again when he respawns,
6.6 seconds later in that stretch.

### The viewer's buttons during a review

Every spectator button reaches the camera through
`CHudSpectator::HandleButtonsDown(int buttons)` (`client+0x386a0`, thiscall,
`ret 4`), called only from the nine `+` commands' down handlers: `+attack`
(1) and `+attack2` (0x800) step to the next or previous player, `+jump` (2)
cycles the mode, `+duck` (4) opens the spectator menu, `+use` (0x20) toggles
the inset, and the four moves (8, 0x10, 0x200, 0x400) do nothing outside the
map modes DoD doesn't have. The death switch's `+attack` goes through it too.
The console's `spec_mode`, the spectator panel's dropdown and ESC do not.

While Review highlights runs (#826), `spectator_buttons.rs` writes `ret 4`
over its first instruction (`sub esp, 0xc`, three bytes) and puts it back
when the review ends, so a stray click or jump can't move a highlight's
camera. `goldsrc-hooks/tools/verify_spectator_buttons_offsets.py` checks the
function and its nine callers.

### The target

`ds_spec_target <player>` writes `g_iUser2`. The in-eye camera, the
viewmodel and the spectator panel read it every frame, and the view is on the
new player on the next frame:

```text
573.790  ds_spec_target: the camera is on player 9
573.796  g_iUser2 = 9
573.803  anim_fix: now spectating idx 9 (was idx 2)
```

It refuses a number outside 1-32, a number with no player, a first-person
demo, and a demo whose spectator view is not up yet.

`g_iUser2`'s address comes from the routine above (the operand of its first
`mov`), not from a fixed offset. `client.dll` is the same file in the
pre-Anniversary and 25th Anniversary builds, and both were tested.

### Not covered

- Rewinding a demo to before a death, which was reported to move the camera
  as if the player were still dead. Not reproduced here; with the lock on the
  death switch cannot fire, which is the only unprompted writer found.
- The view while the player is dead is the game's own for a dead target
  unless `ds_spec_deathcam` is on, which gives it his recording's death
  cam first (third section).

---

## The in-eye camera and a prone player (#329)

Watching a player in first person from an HLTV demo, the camera stays at
crouch height when he goes prone. His own view drops to the ground.
`ds_spec_match_pov 1` makes the spectated view do the same. The code is
`goldsrc-hooks/src/spectator_eye.rs`.

### Why the game gets it wrong

`V_GetInEyePos` (`client+0x50c90`, the same in both builds) puts the camera at
the spectated player's origin plus a height picked from two replicated fields:

```text
if curstate.solid == SOLID_NOT   roll 80, z -= 8      (dead)
else if curstate.usehull == 1    z += 18              (small hull)
else                             z += 22
```

A prone player is in the small hull, the same one as a crouched player. His
own client knows he is prone because the server sends it `view_ofs` and
`iuser3` in `clientdata`; neither is sent for other players
(`dod/delta.lst`).

### What the player's own view does

Read out of POV demos with
`cargo run --release -p analysis --example crosshair_pov_probe -- <demo> 0`
(fields `view_ofs[2]`, `eye`, `camera_z`, `origin_z`, `usehull`):

| stance | eye above the origin |
| --- | --- |
| standing | 22 |
| crouched | 18 |
| prone | **-6** |

- Going prone: `view_ofs[2]` becomes -6 on the same update that starts the
  body's `get_down` and sets `usehull` to 1. The camera is 28 lower on the
  next frame, then falls another 18 over 0.2s as the origin drops into the
  small hull.
- Getting up: `usehull` returns to 0 on the update that starts `get_up`, and
  the camera is at standing height 0.12s later.

Neither follows the body animation (`get_down` runs 1.3s, `get_up` 2.0s).

### What the fix reads and writes

Prone, for a spectator, is `usehull` 1 together with a sequence only a prone
player has: body `get_down`, `prone_*`, `bipod_*` (a machine gun deployed
prone) or `hs_prone_*` (hand signals), or gait `prone_idle` / `prone_forward`.
`get_up` is left out on purpose: `usehull` is already 0, so the game's own
+22 applies.

The patch is one operand. `fadd dword [18.0]` at `client+0x50d5b` is made to
read a float in the hook DLL instead, which holds 18.0 or -6.0. The code is
written once when the switch turns on and once when it turns off; from frame
to frame only the float changes. The standing and dead branches are untouched.

### Checked

`monday-wsod25_r07_m1_h1_hltv`, in-eye on the player who goes prone behind a
wall at about 9:15, recorded with the switch on and off, on both builds. On:
the camera is at ground level and the wall fills the view. Off: the camera
looks over the wall from crouch height.
`ds_debug_log_weapon_model 1` logs each change of stance it sees.

---

## The in-eye camera through a crouch (#1144)

Watching a player crouch in first person from an HLTV demo, the camera drops
about four times faster than his own view does. `ds_spec_match_pov 1` eases
it at his own view's speed. The code is
`goldsrc-hooks/src/spectator_eye/crouch.rs`.

### The survey

`analysis/examples/crouch_eye_probe.rs` prints each frame's camera and origin
from a POV demo and every player's origin and `usehull` from each snapshot.
Pairs: `wsod25_grp2_h1_*` (5 POVs) and `wsod25_grp4_h1_*` (6 POVs) against
their halves' HLTV demos, 6 players; 12 more `solifotw-*` POVs from 12 other
players for the POV curve alone. On-ground crouches with the key held 0.7 s:

| | camera starts | half-way | 95% |
| --- | --- | --- | --- |
| going down, POV, from the key | +0.03 s | +0.20 s | +0.35 s |
| going down, HLTV in-eye | +0.41 s | +0.49 s | +0.51 s |
| getting up, POV | at once | +0.04 s | +0.11 s |
| getting up, HLTV in-eye | at once | +0.09 s | +0.11 s |

(254 paired crouches and 498 stand-ups; 606 and 915 more POV-only.)

- **POV going down** eases the camera all 22 units (58 to 36 above the feet)
  over 0.4 s while the player is still in the standing hull:
  `22 x smoothstep(t / 0.4)`, the same in all 18 players. The server flips
  `usehull`, and the gait to `dod_crouch_idle`/`dod_crawl`, when the ease
  ends. That flip is the first thing an HLTV demo shows, so the in-eye camera
  cannot start before it.
- **HLTV going down** then drops 4 at once (22 to 18) and 18 more as the
  interpolated origin falls into the small hull, over the interpolation
  time.
- **Getting up** both rise in about 0.11 s.
- **In the air** (57% of the flips in `wsod25_grp2_h1_hltv`: crouch-jumps)
  the origin doesn't move at the flip and both views drop 4 at once.
  On the ground the snapshot origin jumps 18 at the flip, against the motion
  before it, which tells the two apart.

### What the fix does

The camera follows POV's curve on the demo clock, at POV's speed. With the
look-ahead below it starts 0.41 s before the flip arrives and ends with it,
as POV's does; without it (the first second or so of a demo, a file it can't
read) it starts at the flip and ends 0.4 s after POV's (the game's own drop
ends about 0.1 s after). Getting up, it rises at POV's rate from wherever
the ease had got to, so a crouch let go half-way down comes straight back up.

`V_GetInEyePos` adds 18 or 22 to the interpolated origin. Both `fadd`
operands (`+0x50d5b`, `+0x50d65`) read floats in the hook, set each frame
just before `V_CalcRefdef` to `22 + shown - depth`, where `depth` is the
eased drop (0 to 22) and `shown` is how much of the 18-unit origin step the
interpolated origin already shows, read off the origin between the last
standing snapshot and the first crouched one. So the camera moves only as
`depth` does, at any `ex_interp`, update rate or capture frame rate.

It settles at the game's heights, with no ease, on a seek, a switch of player,
a teleport, an in-air flip, while prone (#329's height wins), and when the
switch is off or a POV demo plays.

Not covered: a duck tapped for under 0.4 s never flips `usehull`, so an HLTV
demo shows nothing of it; and POV's eye sometimes sinks from 18 to 0 for
about 0.2 s on a duck-jump landing, which HLTV has no trace of.

### Look-ahead: starting with POV (#1154)

Nothing the game holds says a crouch is coming before its flip: the watched
player's newest snapshot lands within 0.02 s of the game's clock, and the
flip comes when POV's ease is already over. But a ground flip only ever comes
0.40 s after the key (p10 0.394 s, p90 0.406 s over 267 crouches of 5
players), so the flip dates the key, and the demo file has every flip ahead
of playback.

So when an HLTV demo plays, `goldsrc-hooks/src/lookahead.rs` reads the `.dem`
once on a thread of its own at below-normal priority, a frame at a time
through `dem-patch`'s parser with about 1 MB of the file held at once, and
keeps each player's ground flips (and body sequence changes, for the gun's
sprint let-go, #704) by `svc_time`: about 1 s and 155 KB for a 40 MB,
33-minute half. The crouch ease asks it each frame for the watched player's
next flip and starts 0.41 s before it is due. The game's clock (and every
`msg_time`) is not `svc_time` under `viewdemo`, and a seek doesn't move it,
so the lookup goes through the demo player's world clock minus the game's,
read every 0.1 s; a looked-ahead flip arrived a mean 0.001 s after it was due
(sd 0.010 s) on PRE and POST. About 1 µs a frame on the engine thread.

| `wsod25_grp2_h1_hltv` vs its 5 players' POVs | camera within 2 units of POV |
| --- | --- |
| the game | 51.7% |
| ease from the flip | 42.5% |
| ease from the look-ahead | 82.3% (81.2% with the live timing error added) |

Guards: no ease starts that this view didn't see start on time (after a seek,
a switch of player, or a timeline that lands mid-crouch, the flip is waited
for), a looked-ahead flip that hasn't come 0.15 s after it was due lets the
camera back up, and so does one the file stops promising while it is still
0.1 s off (a seek moved the world clock). `ds_debug_status` says which demo
the look-ahead has read, or why it has none.

---

## The death cam when the watched player dies (#709)

Watching a player in first person from an HLTV demo, the camera stays in his
eyes when he dies: the game rolls it 80° and drops it 8 units, and the clip
shows the ground and his dropped weapon. His own recording shows something
else. `ds_spec_deathcam 1` gives the spectated view the same, for 2.4 s.
A switch of its own rather than part of `ds_spec_match_pov`: a movie
wants it, a shoutcast following the fight does not. The code
is `goldsrc-hooks/src/spectator_deathcam.rs`.

### What the player's own view does

Measured from the recorded `ref_params` of two POV demos (`wsod25_grp2_h1_m00cat`,
19 deaths; `wsod25_grp2_h1_element`, 28), by a script kept outside the repo:

- A third-person camera, from the first dead frame: 64 units behind a point
  17 above the body's origin, along its own view direction, pulling back to at
  most 168 (shorter where a wall is close).
- No roll, no gun (`viewmodel` 0 on every dead frame).
- The angles turn smoothly from where he was looking toward the killer.
- It lasts until the server respawns him or gives him an observer view of a
  teammate: 2.43 s on every death in one demo, 1.81 s in the other.

### Why the spectated view differs

That camera is DoD's own `V_GetDeathCam` (`client+0x503d0`, the same in both
builds):

```text
v_lastDistance += v_frametime * 96;  if (v_resetCamera) v_lastDistance = 64;
v_lastDistance = min(v_lastDistance, 168);
head = ent1->origin + (0, 0, 17)
target angles = toward ent2, or straight down when ent2 is NULL
V_SmoothInterpolateAngles(v_lastAngles, target, angles, 120);
V_GetChaseOrigin(angles, head, v_lastDistance, origin);
v_lastAngles = angles
```

`V_CalcRefdef` reaches it from `V_GetChasePos` only when the chase target is
the local player, with `ent2` the entity `iuser3` names (the killer). A
spectator's target is never the local player, and HLTV sends no `iuser1..3`
for players, so the in-eye camera takes its own dead branch instead
(`V_GetInEyePos`: `curstate.solid == 0`, roll 80, z -= 8).

### What the fix does

After the game's `V_CalcRefdef` each frame, while the camera is in a dead
player's eyes (`solid` 0, or his body playing a `die_*`/`dead_*` sequence,
since an HLTV demo can be late with `solid`):

- On the first dead frame, `v_resetCamera` is set so the pull-back starts at
  64, and `v_lastAngles` is put back to his last living view angles (the
  frame's own `V_CalcNormalRefdef` has just stored the rolled dead-eye angles
  there).
- `V_GetDeathCam(body, killer, angles, origin)` is called once per frame time
  (the picture-in-picture inset calls `V_CalcRefdef` twice a frame and reuses
  the result), the view is overwritten with its answer, and `v_resetCamera`
  cleared as `V_GetChasePos` does. The killer is the last `DeathMsg` naming
  him as the victim; the world or himself means NULL, the game's look-down
  case. `deathmsg`'s handler stays installed while the switch is on for that.
- The viewmodel is moved 4096 units below the camera for the frame.
- After 2.4 s (what the measured recordings held it for, see below; the
  pull-back itself takes 1.1 s) nothing is touched until he respawns: the view is the
  game's own for the dead player, from his eyes, as without the switch. With
  `ds_spec_lock 1` a long reinforcement wait is spent there, not
  circling the body. A seek back across the death ends it too.
- Once he is solid again nothing is touched: the game's in-eye view is his.

No code is written. `V_GetDeathCam` is found by its first 30 bytes, and the
two globals come from its own `mov eax, [v_resetCamera]` and the `push
v_lastAngles` before its `V_SmoothInterpolateAngles` call, checked against the
`mov [v_lastAngles], edx` that stores the result.
`goldsrc-hooks/tools/verify_deathcam_offsets.py` checks all of it against a
`client.dll`.

### Known differences

- The death cam lasts a fixed 2.4 s. In the recordings it is constant per
  recording but not per server: 2.43 s on every death in
  `wsod25_grp2_h1_m00cat` (16 of 19; the other three were instant respawns),
  1.81 s in `wsod25_grp2_h1_element` (25 of 28), same half, same server. A
  server rule then, not the client's 4 s spectator switch (which is
  `CHudDoDCommon::Draw`'s, first section); why the two players differ is not
  explained. After it the recording shows a respawn or an observer view of a
  teammate; this one shows his dead eyes.
- Switching the camera to a player who is already dead starts a death cam
  from 64 behind his body.
- POV-only effects are not reproduced: the red screen of a head hit (#606)
  and the "killed by" centre text have no HLTV messages.

---

## Chase cameras draw the players around them (#918)

In locked or free chase the watched player, everyone else and most models
went missing while the world still drew, and came back in first person.
Stock behaviour: HLAE alone, with no hook DLL, loses them too (PRE,
`dod_clinic`, chase after free roaming).

### Why they went missing

In an HLTV demo both engine builds drop every entity outside the PVS of
`cl.simorg`, not of the camera:

- `CL_EmitEntities` marks the leaves visible from `cl.simorg` before it links
  the frame's entities, and re-marks the camera's leaves after (POST
  `hw+0x19d764` and `hw+0x19d82f`).
- `CL_AddVisibleEntity` (PRE `hw+0xbae0`, POST `hw+0x1969b0`) asks
  `HUD_AddEntity` first, then drops an entity whose box is in no marked leaf.
  Measured on POST: the watched player reaches `HUD_AddEntity` every frame and
  `StudioDrawPlayer` never.

The engine copies `ref_params_s::simorg` back into `cl.simorg` after
`V_CalcRefdef` (POST `hw+0x231df2`). DoD's `V_CalcRefdef` writes it in first
person (`V_GetInEyePos`) and free roaming flies the camera from it; the two
chase modes leave it where it was, which is the map's origin after first
person (`0 0 0` on every chase frame) or the roaming camera's last spot. How
much is lost depends on that point: on `dod_harrington` nearly every player,
whatever the route into chase; on `dod_clinic` nobody after first person
(probably because its origin is inside a wall, and the engine sees everything
from a solid leaf), but players after free roaming.

### What the fix does

`goldsrc-hooks/src/spectator_chase_pvs.rs` runs after the client's own
`V_CalcRefdef`. In a chase mode of an HLTV demo, on the main view (not a
picture-in-picture inset), it writes the camera's position into `simorg`, so
entities are culled from the camera, as the world is. No code is patched and
`hw.dll` is not touched, so there is no per-build table. On by default;
`GOLDSRC_HOOKS_CHASE_PVS=0` turns it off.

Checked on PRE and POST (2026-10-22): `wsod25-po_r4_up_ko_f-soul_under_harr_h2`
(`dod_harrington`) from first person into locked chase, two target switches
and free chase, and `ktps9qf-dice_clinic_m1_armory_h1_hltv` (PRE) and
`hltv-2508021304-dod_solitude_b2` (POST) through free roaming into both chase
modes: the watched player and the players near him draw in every chase shot
but one, a target switch on `dod_solitude` after which the spectator bar named
no player either.
