# HLTV demos lose most of their gunshots, and how they are put back

An HLTV demo is quieter than the match was. Whole bursts play with no sound,
no muzzle flash and no bullet impact, while the player's model stands there
firing. `dodstudio_spec_match_pov 1` puts those rounds back (it also turns on the
viewmodel animations and the POV crosshair; until 2026-10-01 this part had a
cvar of its own, `dodstudio_hltv_play_missing_gunshots`).

Code: `goldsrc-hooks/src/missing_shots.rs`. Probe:
`analysis/examples/hltv_shot_evidence_probe.rs`.

---

## 1. What is missing

A shot reaches a client as a **fire event** (`events/weapons/<gun>.sc`). The
client's handler for that script (`EV_FireMP40`, `EV_FireGarand`, …) makes
everything a spectator perceives of the shot: the sound, the muzzle flash, the
tracer and the bullet's impact. No event, none of it.

Measured on one match half (`monday-wsod25_r07_m1_h1_hltv`, 12 players, about
28 minutes):

| | count |
| --- | --- |
| bullet-weapon fire events in the demo | 1180 |
| bullet-weapon rounds fired with **no** event | 1812 |
| share of rounds with no event | **61%** |

The check that those 1812 are real rounds and not an artefact of the method is
the kill feed. A kill by gunfire is a shot that certainly happened:

| the killer's shot, in the 0.25 s before the kill | kills |
| --- | --- |
| fire event present | 147 |
| **no fire event** | **177** |
| neither signal (grenades, blades, and anything missed) | 39 |

More kills have no gunshot than have one.

`hltv_shot_gap_probe` put the loss at 17%. It was measuring something
narrower: gaps *inside* a burst that still had events either side. Most of the
loss is whole bursts, which leave no gap to find.

How much is lost depends on the recording. Across every HLTV half on hand:

| recordings | halves | events | rounds with none | lost |
| --- | --- | --- | --- | --- |
| `wsod25_*` | 28 | 30,568 | 47,185 | 61% (59–63% per half) |
| `ktps9qf-*` | 4 | 8,477 | 1,541 | 15% (14–16%) |
| a `forcehltv` conversion | 1 | 1,536 | 495 | 24% |

Why a proxy's recording loses them was not established. The loss is not
random per round (whole engagements go missing while others are complete),
and it is steady within one event's recordings and very different between
two, which fits how each server or proxy was set up deciding it, but that is
an inference.

## 2. What still says the round was fired

The shooter's own body. `CBasePlayer::SetAnimation(PLAYER_ATTACK1)` runs on
every shot and restarts the attack animation, and `sequence` and `frame` are
in `entity_state_player_t`, replicated in nearly every player update (504,340
of 518,353 carried `frame`). HLTV updates arrive about every 33 ms.

So a round shows up in a player's state one of two ways:

- `sequence` changes to a `*_shoot` sequence (the first round, or a round fired
  from a new stance);
- `frame` steps **backwards** inside a `*_shoot` sequence. These sequences do
  not loop — a finished one sits on 255 — so the only thing that takes `frame`
  back is the server starting it again.

An MP40 burst, one player, `frame` at each update:

```
1286.752  sequence=52 (stand_mp40_shoot)  frame 11    <- fire event
1286.786                                   frame 34
1286.818                                   frame 58
1286.850                                   frame  3    <- fire event
1286.882                                   frame 33
1286.915                                   frame 57
1286.949                                   frame  3    <- fire event
   …
1287.244                                   frame 73
1287.276                                   frame 21    <- no event
1287.340                                   frame 73
1287.370                                   frame 18    <- no event
```

The sawtooth runs at the weapon's cyclic rate whether the event survived or
not.

How good the signal is, on the same half:

- **Every fire event has a restart beside it**: 1180 of 1180, once an event is
  attributed to its shooter the way the client does it (`packet_index` counts
  into the frame's packet-entity list in entity order; no HLTV event carried
  its own `entindex`).
- **Both kinds of restart lose their event equally often**: 39.2% of `frame`
  step-backs kept theirs (745 of 1900), 39.2% of sequence changes (423 of
  1080). If one kind were something other than a round, its share would
  differ.
- Bayonet stabs and rifle-butt swings have their own sequences
  (`*_bolt_stab`, `*_rifle_swing`), so a `*_shoot` restart on a rifle is a
  bullet.

## 3. What the hook does

Each frame, for each of the 32 player slots, it compares `curstate.sequence`
and `curstate.frame` with the frame before. A restart in a bullet weapon's
`*_shoot` sequence is a round. Then:

1. **Did its own event come?** `fire_sounds`' `EV_PlaySound` hook reports every
   real `_shoot` sample with the entity that fired it. The restart and its
   event ride in the same demo message, so the sound lands within one rendered
   frame of the restart, either side. A restart waits one frame; a real shot
   in that window cancels the stand-in.
2. **Which weapon?** The third-person model the player holds
   (`curstate.weaponmodel`): `p_k98` → `kar`, `p_stg44` → `mp44`, `p_barbu` →
   `bar`, and so on for 22 bullet weapons (`WEAPONS` in `missing_shots.rs`).
3. **Play it.** `client.dll`'s own handler for that weapon's script is called
   with the argument block the engine would have built from the player's
   state: `entindex`, `origin`, `angles` (pitch turned back from the negated
   third a player's state carries, as `CL_ParseEvent` does), `ducking` from
   `usehull`, a small random spread, and the integer and bool arguments an
   ordinary recorded round of that weapon carries (below).

### What an ordinary round carries

All-zero arguments are not an ordinary shot for every weapon, and the handlers
act on the difference. The first version passed zeros throughout, and every
restored Garand round played the clip ping. From every fire event in the 33
halves above (`hltv_shot_evidence_probe` prints the table per demo):

| weapon | `iparam1` | `bparam1` | `bparam2` |
| --- | --- | --- | --- |
| Garand, K43, M1 carbine | 0 (1 = butt or bayonet) | **1** on every round but the clip's last | 0 |
| scoped K98 | 0 | 0 | 1 on three shots in four |
| BAR | rounds left in the magazine, 19 down to 0 | 0 | 0 |
| Colt, Luger, K98, Springfield, MP40, MP44, Thompson | 0 (1 = bayonet on the K98) | 0; 1 on the last round | 0 |

The Garand's handler plays `weapons/garand_reload_clipding.wav` when `bparam1`
is **0** (`client.dll+0x7c2e`: `test ebp, ebp; jne` past the sample). The
reconstructed source has this the other way round.

No recorded event was available for the MG42, MG34, .30 cal, Bren, FG42, Sten,
grease gun, Enfield or Webley. The MG42 and .30 cal read `iparam1` as belt
rounds left, so theirs follows the BAR's; the rest get zeros.

### The Garand's ping

A restored Garand round pings only when it is the clip's last, and the event
that said so is the thing that was lost. So the hook counts: every Garand
round a player fires, recorded or restored, since the body last started a
`*_garand_reload` sequence or the player died (the respawn brings a full
clip). The eighth pings. Against the recorded events of all 33 halves, the
eighth round since a reload or death was the pinging one 377 times of 409
(92%), and an earlier round pinged 14 times of 5736. Until a player's first
reload or death is seen, none of their restored rounds ping.

Because it is the game's own handler, a restored round gets the same sound,
flash, tracer and impact as a recorded one, is heard as far as one is, and
drives the in-eye viewmodel's firing animation through the same sound trigger
(`goldsrc_hltv_animation_fix.md` §6).

### Getting the handlers without an address

`client.dll` registers its handlers through `gEngfuncs.pfnHookEvent` (slot 69)
from inside `Initialize` (`push handler; push "events/weapons/colt.sc"; call
[gEngfuncs+0x114]` at `client.dll+0x106e5`; 52 slots after `pfnAddCommand`,
the same bytes in both builds). The hook already intercepts `Initialize`. It
swaps the engine table's `pfnHookEvent` for a wrapper just before the real
`Initialize` runs and puts it back just after; the wrapper notes the handler
for each weapon script and forwards the call. The log says how many it got:
`missing_shots: noted 22 of 22 weapon fire handlers`.

## 4. What it cannot do

- **Know the spread.** The lost event carried it. A restored round's impact is
  near where the real one landed, not on it.
- **Be sure of a Garand's last round.** The count above is right about nine
  times in ten; the misses are clips whose start was not seen (a Garand picked
  up off the ground, a class change). A pistol's last round is not attempted
  (its slide-lock is first-person only).
- **Restore melee, grenades or rockets.** Their events carry arguments the
  body does not show (hit or miss, which swing). A grenade's sounds are
  `svc_sound`, not events, and were never part of this loss.
- **See two rounds from one player inside one rendered frame.** At 60 fps a
  frame is 17 ms and the fastest weapon fires every 50 ms. Under about 20 fps
  an MG42 burst could lose a round.
- **Do anything for a POV demo.** A player's own recording only ever held what
  that player could hear, and that is what it should play. The hook acts only
  while `IsSpectateOnly()` is true.

## 5. Checking it

`dodstudio_spec_match_pov 2` logs every round it plays (1 logs the
first twenty, then every 200th):

```
missing_shots: player 8 fired a round with no fire event -- playing mp40 (37 so far)
```

`hltv_shot_evidence_probe <demo> --list` prints every round with no event from
the file itself, as demo time and player. The two lists should agree, apart
from a constant between the game's clock and the file's that differs per
session. `goldsrc-hooks/tools/compare_missing_shots.py <list> [hook log]`
finds that constant and pairs the two lists off.

Checked that way on 2026-09-30, pre-Anniversary build, the first two and a
half minutes of the demo above at normal speed: the file has 64 rounds with no
event in that stretch, the hook played 64, and they pair off one-to-one (same
player, within 0.08 s, clock offset 2.403 s) with none left over on either
side. Another 38 rounds came with their own event and were left alone. The
25th Anniversary build, same demo and stretch: 70 of 70, 40 left alone.

`dodstudio_debug_status` adds a line once the switch has been on:

```
missing gunshots: 412 played, 280 rounds had their own event, 0 skipped (weapon not known)
```
