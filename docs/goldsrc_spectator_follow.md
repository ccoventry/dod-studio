# Keeping the camera on one player in an HLTV demo (#206)

Two settings in the hook DLL (`goldsrc-hooks/src/spectator_follow.rs`):

| | |
| --- | --- |
| `dodstudio_spec_lock 1` | the camera stays on the player being watched when he dies, instead of moving to the next player four seconds later. A cvar, off by default. |
| `dodstudio_spec_target <player>` | puts the camera on a player, by number. `dodstudio_deathmsg players` lists the numbers. With no number it says who the camera is on. A command. |

Both act on HLTV demo playback only. A first-person demo's view is the
recording player's own.

## What moves the camera

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

## The lock

One byte: the `je` at `+0x2c779` becomes a `jmp`. To this routine alone the
watched player is always alive, so the timer never starts and the keypress is
never issued. Nothing is put right after the fact, so no frame is drawn from
the wrong player. The viewer's own `+attack` and the dropdown still change
player.

With the lock on, the same stretch of the demo has no `ClientCmd` line and
`g_iUser2` stays 2. While he is dead the view is the game's own for a dead
target (near the ground where he fell); it is his view again when he respawns,
6.6 seconds later in that stretch.

## The target

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

## Not covered

- Rewinding a demo to before a death, which was reported to move the camera
  as if the player were still dead. Not reproduced here; with the lock on the
  death switch cannot fire, which is the only unprompted writer found.
- The view while the player is dead. It is whatever the game shows for a dead
  target, which is not what his own recording would show.
