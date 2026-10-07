# The spectator camera under `spec_match_pov`: following one player, and the in-eye height

Two fixes to where the camera sits while watching a player from an HLTV demo,
both part of the `dodstudio_spec_match_pov` / `spec_lock` / `spec_target`
family in the hook DLL: which player the camera is on (#206, first section) and
how high the in-eye camera is when that player goes prone (#329, second). They
were `goldsrc_spectator_follow.md` and `goldsrc_spectator_eye_height.md`.

---

## Keeping the camera on one player in an HLTV demo (#206)

Two settings in the hook DLL (`goldsrc-hooks/src/spectator_follow.rs`):

| | |
| --- | --- |
| `dodstudio_spec_lock 1` | the camera stays on the player being watched when he dies, instead of moving to the next player four seconds later. A cvar, off by default. |
| `dodstudio_spec_target <player>` | puts the camera on a player, by number. `dodstudio_deathmsg players` lists the numbers. With no number it says who the camera is on. A command. |

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

Seen live, with `dodstudio_debug_log_spectator_target 1`, on
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

### The target

`dodstudio_spec_target <player>` writes `g_iUser2`. The in-eye camera, the
viewmodel and the spectator panel read it every frame, and the view is on the
new player on the next frame:

```text
573.790  dodstudio_spec_target: the camera is on player 9
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
- The view while the player is dead. It is whatever the game shows for a dead
  target, which is not what his own recording would show.

---

## The in-eye camera and a prone player (#329)

Watching a player in first person from an HLTV demo, the camera stays at
crouch height when he goes prone. His own view drops to the ground.
`dodstudio_spec_match_pov 1` makes the spectated view do the same. The code is
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
`dodstudio_debug_log_weapon_model 1` logs each change of stance it sees.
