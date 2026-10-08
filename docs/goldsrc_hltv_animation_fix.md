# The HLTV viewmodel animation fix

> **Status 2026-09-08 — working and live-proven across every weapon class.**
> Lives in `goldsrc-hooks/src/anim_fix/`, on `dev`. Built under
> [#204](https://github.com/ccoventry/dod-studio/issues/204) (closed).
> Defaults **off**. It is one of the things `dodstudio_spec_match_pov` turns
> on (with the lost gunshots and the POV crosshair): `dodstudio_spec_match_pov 1` in
> the console, `+dodstudio_spec_match_pov 1` on the launch line, or from any `.cfg`
> the session execs. It had a cvar of its own,
> `dodstudio_hltv_show_viewmodel_animations`, until 2026-10-01.

Watching a DoD demo in first person, the weapon on screen barely moves. It does
not recoil when the player fires, does not reload when they reload, and does not
change when they switch weapons. This is what that costs to fix, and why the fix
is shaped the way it is.

For how the DLL gets a foothold in `client.dll` at all, see
[`goldsrc_client_dll_internals.md`](goldsrc_client_dll_internals.md). This
document is only about what the fix does once it is running.

---

## 1. Why the animations are missing

Structural, not a bug. GoldSrc's weapon event scripts animate the viewmodel only
for the **local** player — the `EV_IsLocal` check at the top of every
`events/weapons/*.sc`. Everyone else gets the sound and the muzzle flash but no
first-person animation, because under normal play nobody is ever looking down
somebody else's sights.

Spectating in-eye is exactly the case that assumption does not hold for. The
viewmodel on screen belongs to a player who is not the local player, so firing,
reloading and drawing never reach it.

## 2. There is nothing to replay, so it has to be inferred

The obvious fix would be to replay whatever the demo recorded. It recorded
nothing. `analysis/examples/weapon_anim_probe` counts both carriers of a
viewmodel animation in a demo:

| | `Dem_WeaponAnim` frames | `svc_weaponanim` |
| --- | --- | --- |
| POV demo of one half | 3391 | 743 |
| **HLTV demo of the same half** | **0** | **0** |

Zero of either. An HLTV recording carries no viewmodel animation for the players
it is watching, so inference is not a shortcut here — it is the only route. That
measurement is what justifies everything below.

## 3. The signal: the spectated player's own body

DoD's player models name every sequence `<stance>_<weapon>_<action>`.
`models/player/us-inf/us-inf.mdl` has **345** of them:

```
stand_bolt_shoot   crouch_bar_reload   bipod_mg_shoot
prone_webley_reload   sprint_sten_aim   sandbag_30cal_shoot
```

So the spectated player's own body animation states outright what they are
doing — and `curstate.sequence` is **replicated**, which almost nothing else
about another player's weapon is. It survives into an HLTV demo, and it names
the stance and the bipod state as well as the action.

Between shots the server returns the player to `*_aim`, so repeated single shots
show up as real sequence *changes*. That is the whole trigger mechanism: watch
`curstate.sequence` change, read the new label, classify it.

## 4. What runs each frame

`apply()` is installed as a per-frame callback (`engine::set_per_frame_callback`,
driven by the `HUD_Frame` hook) and walks a fixed sequence of preconditions. Each
has a named stage, and the stage is logged **only when it changes** — this runs
60+ times a second, so a line per call would flood the log and slow a capture.

```
disabled → no engfuncs → not spectating → no viewmodel entity
        → no viewmodel model → no spectated player → viewmodel mismatch
        → RUNNING
```

Reaching `RUNNING` means the preconditions held. It does **not** mean anything
was corrected — that is what the animation counter is for (§8).

Once running, on each frame it:

1. reads the spectated player's body sequence label,
2. resolves bipod state (§7) if this weapon has two families,
3. publishes the current spectated index, viewmodel and deploy state for the
   sound-driven fire trigger, which runs on the same thread but from a different
   hook and has none of this context,
4. decides which animation, if any, to force.

## 5. The four animations, and what triggers each

| animation | trigger |
| --- | --- |
| **shoot** | the body sequence changes to a `*_shoot` (or `*_roll`) label — plus a second, sound-driven trigger, §6. Grenades take their own path, §13 |
| **reload** | the body sequence changes to a `*_reload` / `*_zoomload` label |
| **draw** | the viewmodel *settles* on a different weapon, §9 |
| **idle** | the camera switches to a different player, so the new viewmodel does not inherit whatever sequence the last one was left on |

Sequence lookup is exact-match first, substring fallback. That ordering matters:
a bare substring search for `reload` picks `reload_empty` on the models that have
both. Candidate names come from a dump of all 41 `v_*.mdl` sequence lists rather
than from assumption, because DoD is not consistent — a firing animation is
`shoot` on the 98k, `shoot1` on the Garand, `up_shoot` on the BAR, `launch` on
the rocket weapons, `fire` on the mortar, `throw` on grenades and `slash1` on
melee.

## 6. Two firing triggers, and the window between them

The body sequence cannot cover a held trigger. The server sets the body to
`stand_mg_shoot` once and leaves it there for the whole burst, so every round
after the first has no sequence change to key off, while the *sound* fires per
round. So there are two triggers:

- **body sequence** — catches semi-auto fire and the first round of a burst.
- **`EV_PlaySound`** (via `fire_sounds`' hook) — catches every round of automatic
  fire. Its `ent` argument is the shooter: verified against 34 matches across
  five different spectated players, with real varying indices.

A single shot reaches both, milliseconds apart, so `claim_fire` holds a
**0.03s dedup window**. That number is not arbitrary: the MG42's ~1200rpm puts
0.05s between rounds, so a wider window would swallow real automatic fire, which
is the one case the sound trigger exists for.

Measured live on 2026-09-08 (BAR + MP40, 100+ animations), the split behaves
exactly as designed: a burst's first round is claimed by both (sound plays it,
body deduped ~6 ms later) and every round after is sound-only, landing at the
weapon's cyclic rate — MP40 rounds animated 0.097–0.103 s apart against a
measured 98 ms cyclic, BAR at ~0.13 s against 122–132 ms. Totals were 19 sound /
2 body / 4 deduped for automatics, versus 5 / 5 / 3 for a semi-auto session.

## 7. Bipod weapons

MG42, MG34, BAR and Bren keep two parallel sequence families in one model — "up"
(hip-fire) and "down" (deployed), `upidle`/`downidle`, `up_shoot`/`down_shoot`.
Every animation above is looked up within whichever family is current.

State is read from the **body sequence label first** (`bipod_` / `sandbag_`
prefixes), which carries it in every stance. The `p_mg42bu` / `p_mg42bd` model
name is only the fallback, because DoD ships far more `p_` models than weapons —
they encode stance too (`p_mg42pr`, `p_mg42sr`, `p_brenbr`, `p_bren_l`) — and
the deploy marker goes unreadable exactly when a machine gunner is prone. When
neither source reads, the last observed state carries forward; it is cleared on a
player switch, because it says nothing about the next person.

This part matters far less in practice than the plain animations: league configs
generally limit the MGs to zero and deploying the BAR's bipod is rare. It is a
refinement on top, not the point.

## 8. Diagnostics

- `dodstudio_spec_match_pov <0|1>` — a **cvar**, so it also takes
  `+dodstudio_spec_match_pov 1` on the launch line or a line in any `.cfg`,
  and shows its value in the console type-ahead.
- `dodstudio_debug_log_weapon_model <0|1>` — cvar. Logs every held-model change *and*
  every body-sequence change, which is the trail to read a session back from.
- `dodstudio_debug_status` — what each fix is *doing*, not just what it is set to. A
  cvar can answer "what is this set to" on its own; whether the fix's
  preconditions are being met in the current view is a different question, and
  this is where it is answered.
- Log file: `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks.log`, with wall clock **and** a `[demo NNN.NNN]`
  prefix. Read it directly.
- **"animations corrected" counter** — the honest number. A running total is
  printed every 100 animations.

**Before concluding something is not firing, check whether its log budget ran
out.** Two separate sessions were misread as "the fix doesn't work" when the code
was fine and only the *log* had stopped: a single shared 40-line animation cap
was consumed by draws and player switches 44 seconds in, and a single 25-line
fire-sound cap was consumed by "ignored" lines from other players. Caps are now
per category, and the counter is the thing to trust.

## 9. Four things that were got wrong, and are worth not repeating

**Seven weapons name their `v_` and `p_` models differently.** The "is the
viewmodel the weapon this player holds" filter compares stems as substrings, and
these share nothing:

```
v_98k → p_k98            v_scoped98k → p_k98s       v_mp44 → p_stg44
v_greasegun → p_grease   v_m1carbine → p_m1carb
v_panzerschreck → p_pschreck        v_enfield_scoped → p_enfields
```

Every frame was discarded for all seven — 7139 in one session for the STG44
alone — silently disabling draw, reload and the body-sequence fire trigger.
Only the sound trigger survived, which is why it presented as "the STG44 has no
draw animation" rather than a whole weapon being skipped. Fixed via
`VIEWMODEL_ALIASES`; an unmatched pair is now logged **unconditionally**, not
behind the verbose switch, because nobody would think to turn logging on for a
weapon they had no reason to suspect.

**Rapid weapon switching is real input, not engine noise.** The viewmodel pointer
flaps between a player's weapons many times a second, and a 0.4 s settle window
was added to suppress it. That was wrong: the uncapped logs show the spectated
player's **body sequence changing on the same frame** as the held model —
`stand_pistol_aim` ↔ `stand_rifle_aim` tracking `p_colt` ↔ `p_garand`, six times
in 1.5 s — and nothing confined to the viewmodel could move a player's body
animation. A draw per switch, each cut short by the next, is what a POV recording
shows, so that is what to reproduce. `VIEWMODEL_SETTLE_SECONDS` is now **0.05 s**,
only absorbing a change-and-change-back inside a frame or two. At 0.4 s a
kar→pistol→kar flick produced no draw at all.

**`HUD_Frame(double time)` is a frame *duration*, not a clock.** It receives
`host_frametime` (6–11 ms), and reading it as elapsed time cost a full test run:
the fire-dedup window compared 0.006 against 0.006, sat permanently inside its
own 0.03 s window and suppressed every shot. It is also why every `[demo N]` log
prefix read `[demo 0.006]` no matter how far into the demo. Time is now summed
from the deltas.

**The grenade body sequence is the release, not the pin pull.** The steady 0.49 s
between the thrower's body sequence and `weapons/grenthrow.wav` (846 throws
across three HLTV halves, 0.461–0.566 s) looked like a pin pull, and the throw
was deferred by it. Shipped and reverted. The tightness was the tell: the *real*
cook time, from a POV demo's own `pinpull`→`throw` animations, is 0.065–4.852 s
with medians of 0.64 and 1.46 — a held button, widened further by players
"priming" grenades. The 0.49 s is the server's own wait between the release and
the grenade leaving the hand, so the right reading was the third one: `pinpull`
at the body change, as a stand-in for a pull that cannot be seen, and `throw`
half a second later (§13). See `analysis/examples/grenade_timing_probe`.

## 10. Where the boundaries are

Three things are *not* missing from the fix, and are settled rather than open:

- **The moment of the grenade pin pull is unreachable.** `p_grenade`, `p_stick`
  and `p_mills` carry one `idle` sequence each, and `weapons/grenpinpull.wav`
  appears in no demo's `svc_sound`, POV included — it is played client-side for
  the local player only, exactly like the animation it accompanies. The
  animation plays at the release instead, about 0.1 s late (§13).
- **Sprint does nothing to the viewmodel.** DoD 1.3 does not lower or hide the
  first-person weapon while sprinting; what changes is the *body*, which the
  engine already animates. See `goldsrc_client_dll_internals.md` §8.
- **The `exploding_` grenade family is a primed grenade**: one rolled out and
  caught again with USE. The held model and body token are the plain
  grenade's, but the catch is visible in the world, and the family is played
  from that (§13).

**A POV demo gets none of it (#613).** It is the recording the fix copies, so
`anim_fix::active()` stands every match-POV part down while one plays: a demo
playing back while `IsSpectateOnly()` is false. That includes the recorder's
dead stretches spent watching someone in-eye, which is what they saw live. The
bug that made this explicit: `on_weapon_fired` kept the entity and viewmodel an
HLTV demo earlier in the session had published, so in the POV demo every round
entity 13 fired played the old model's "shoot" index on the STG44 in view, which
is a reload. A frame with no spectated view now forgets them.

One genuine `TODO` remains, marked in the source: on a bipod deploy state change
the viewmodel snaps to the new family's idle rather than playing the model's own
`uptodown` / `downtoup` transition. Unverified live.

## 11. How to check it is working

1. Build for `i686-pc-windows-msvc` and inject into the **PRE-Anniversary for
   Movies** install (never the stock Half-Life one — see
   `docs/goldsrc_dod_quirks.md` and the two-installs rule).
2. `dodstudio_spec_match_pov 1`, `dodstudio_debug_log_weapon_model 1`.
3. Play an HLTV demo in-eye and let the director move between players.
4. Read `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks.log`. The lines that matter, in order of value:
   - `now spectating … holding … viewmodel "…"` on every camera switch,
   - `body sequence -> stand_bar_reload (index N)` as the player acts,
   - one line per animation forced, with the sequence label it chose,
   - the running total every 100 animations.
5. If a weapon looks dead, check for an unmatched-pair line before anything else
   — that failure is silent and total.

---

## 12. The other direction: `dodstudio_hide_hand_signals`

Everything above puts an animation *back*. This one takes one away, and it
belongs here because it works on the same field, from the same per-frame hook.

Using a voice command in DoD also plays a gesture on the player -- a nod for
"Yes Sir!", a point for "Enemy Ahead". `dodstudio_mute_voice_commands` silences
the sound and leaves the mime, because the two are unrelated mechanisms:
`client.dll` contains **no `hs_` string at all**. The client never picks these
by name. The server picks a sequence index and it arrives as replicated
`curstate.sequence` -- the same field §3 reads to infer firing, and the reason
it survives into an HLTV demo.

### Detection is by label, not by index

#283 measured the indices -- 54 `hs_*` sequences per player model, in two
contiguous runs at 212-238 and 287-313, identical across all five stock models
-- and flagged the risk in trusting them, since a custom player model could
reorder its sequence list.

So the implementation does not use them. It reads the model's own labels,
through the same cached `mstudioseqdesc_t` walk this fix already does
(`model_sequence_info`), and asks whether the label starts with `hs_`. Exact
for any model, and a reordered one is handled rather than mis-suppressed.

### What replaces it

A sequence index has to be *something*. Each player's last non-`hs_` sequence
is remembered and put back for the signal's duration -- their stance or aim in
every case that matters. A player first seen mid-signal has nothing to put
back, so they are left alone and counted rather than given a guess.

`gaitsequence` is untouched. It drives the legs independently, and the
standing/prone split in the `hs_` names says the signal is upper-body.

### The ordering question, answered from the call order

#283's remaining unknown was whether a write lands before the renderer reads
it. It runs from `commands::poll`, which this crate drives from the `HUD_Frame`
trampoline -- and `HUD_Frame` is called once per frame *before* the engine
renders the view, unlike `HUD_Redraw`, which paints the HUD after it. So the
write is in place for the same frame's `StudioDrawPlayer`.

That is an argument, not a measurement. If a live test shows the gesture
surviving, the fallback is the one #283 names: the studio renderer's
`StudioDrawPlayer`, reachable through the interface already captured in slot
39.

### It applies to every player in view

Not only the spectated one. That is what clean footage wants, but it is a
behavioural choice rather than an obvious default, so `dodstudio_status` says so.

---

## 13. Grenades: copied from what a POV demo shows

Code: `goldsrc-hooks/src/anim_fix/grenade.rs`. Probes:
`analysis/examples/grenade_pov_timeline_probe` (what the thrower's own
recording plays) and `grenade_prime_probe` (what any recording shows of it).

The fix used to offer four ways to treat the hand after a throw, values 1 to 4
of the cvar, because nothing said which was right. A POV demo does, so there
is now one behaviour and the cvar is on/off.

### What the thrower's own view plays

From 145 POV match demos (4730 throws) and one recorded for the purpose with
every kind of throw in it. Times are from the `throw` animation, which lands
with `weapons/grenthrow.wav`, 0.50 s after the body enters its grenade attack.
Before it there is `pinpull`, a median 0.6 s ahead.

| what the player did | hand grenade | stick grenade |
| --- | --- | --- |
| plain throw, a grenade left | `draw` at once | `draw` at +0.5 s |
| plain throw of the last one | next weapon at once | next weapon at +0.5 s |
| primed it | `draw` at once, then as the stick | `exploding_idle` at the catch, `exploding_pinpull` when fire is pressed again, the next weapon when the live grenade is thrown |

**Priming** is rolling the grenade out in front (right click) and catching it
again with USE, which starts its fuse; the player then holds it as long as they
dare and throws it. It is what most throws in a match are: 300 of 322 in one
HLTV half. The `exploding_` sequences on the grenade viewmodels are the primed
grenade's.

### What a spectator can see of it

| moment | what an HLTV demo carries | what plays |
| --- | --- | --- |
| the release | the body enters `*_gren_shoot`, `*_stick_roll`, … | `pinpull`, and `throw` is booked 0.5 s on |
| the throw | `grenthrow.wav`, and a world grenade (`w_stick`, `w_grenade`, `w_mills`) beside the thrower in the same update | `throw`; on a hand grenade `draw` right behind it |
| the catch | that world grenade is gone again, a median 0.13 s later, while the thrower still holds the grenade model — or, when a last hand grenade had already given way to the rifle, the grenade model comes *back* within 1.5 s | `exploding_idle` |
| the wind-up of the primed grenade | nothing | `exploding_pinpull` at the POV median: 0.7 s after the catch on the stick, 1.1 s on the hand grenade |
| the primed throw | a new world grenade beside the thrower, no sound, and the held model changing in the same update (299 of 300) | the next weapon's `draw`, from the ordinary weapon-change path |
| no catch, grenade still held at +0.5 s (stick) | the world grenade is still out | `draw` |

A grenade that was not caught lives 1.5 s or more, to its fuse; the slowest
catch took 1.5 s, the shortest uncaught one 1.53 s.

The world grenade is found by walking the entity list for a grenade model
within 96 units of the thrower in the current update, only while a throw is
being followed. One that was already a world grenade on the previous frame is
not taken for the primed throw, so a grenade thrown *at* the player is not
mistaken for theirs.

### What it cannot do

- **Time the pin pull.** It plays at the release, a median 0.1 s later than
  the thrower saw it, and for a cooked grenade much later than that.
- **Time the primed wind-up.** The press is not networked. A player who winds
  up sooner or later than the median is shown at the median.
- **Speak for the Mills bomb.** It is the hand grenade's class and is treated
  as one; no British recording was to hand.

### Checked in the game

2026-10-01, pre-Anniversary build, `monday-wsod25_r07_m1_h1_hltv`, in-eye on a
rifleman priming two hand grenades. First: thrown, world grenade found the
same frame, gone 0.067 s later, `exploding_idle`, wind-up 1.13 s after the
catch, rifle up 2.23 s after the catch (the file says the primed throw was
2.236 s after it). Second, his last: rifle up at the throw, the grenade back in
hand 0.70 s later (the file: caught after 0.656 s), `exploding_idle`, wind-up,
rifle up 3.17 s after the catch (the file: 3.170 s). Frame by frame it reads
as the POV recordings do: pin pull, a grenade back in the hand, the arm drawn
back out of view, the rifle.
