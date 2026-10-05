# The in-eye camera and a prone player (#329)

Watching a player in first person from an HLTV demo, the camera stays at
crouch height when he goes prone. His own view drops to the ground.
`dodstudio_spec_match_pov 1` makes the spectated view do the same. The code is
`goldsrc-hooks/src/spectator_eye.rs`.

## Why the game gets it wrong

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

## What the player's own view does

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

## What the fix reads and writes

Prone, for a spectator, is `usehull` 1 together with a sequence only a prone
player has: body `get_down`, `prone_*`, `bipod_*` (a machine gun deployed
prone) or `hs_prone_*` (hand signals), or gait `prone_idle` / `prone_forward`.
`get_up` is left out on purpose: `usehull` is already 0, so the game's own
+22 applies.

The patch is one operand. `fadd dword [18.0]` at `client+0x50d5b` is made to
read a float in the hook DLL instead, which holds 18.0 or -6.0. The code is
written once when the switch turns on and once when it turns off; from frame
to frame only the float changes. The standing and dead branches are untouched.

## Checked

`monday-wsod25_r07_m1_h1_hltv`, in-eye on the player who goes prone behind a
wall at about 9:15, recorded with the switch on and off, on both builds. On:
the camera is at ground level and the wall fills the view. Off: the camera
looks over the wall from crouch height.
`dodstudio_debug_log_weapon_model 1` logs each change of stance it sees.
