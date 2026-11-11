# Position data in demos (#448, step 1)

What a Day of Defeat 1.3 demo carries about where players are, measured so the
position stats in #448 (heatmaps, engagement distance, death positions,
distance travelled) are designed on facts. Probe:
`analysis/examples/position_data_survey.rs` (this survey),
`position_field_probe.rs` (one demo's fields and snapshot staleness at each
death) and `kill_position_probe.rs` (replay check and per-weapon distances).

**Coverage.** 26 demos from the PRE install's `dod\` folder and the CI fixture:
22 POV and 4 HLTV, about 20 different recorders (the `solifotw-*` Frags of the
Week set, KTP and WSOD25 recordings, `ci_fixture.dem`) 11-13 players in each demo, 44,000 s of demo time in total, 21 M player
entity deltas, 3.2 M `svc_clientdata`. Build: release.

## What is already built

A first slice exists: `analysis::entity_replay` replays the entity snapshots the
way the engine does (a ring of 64 snapshots keyed by sequence, deltas applied
against the snapshot they name, spawn baselines for re-entering entities), and
`analysis::position` uses it to fill `kill_positions` (killer and victim origin,
straight-line distance) on every analysis, with the recording player's own
origin taken from `svc_clientdata`. This survey answers the open part of step 1:
what the data allows beyond kills.

## Field tables

The server sends its field definitions in `svc_deltadescription`; these are
printed from the demos themselves (the probe keeps the last one seen). `bits`/`divisor` give the precision of a value
(`value = integer / divisor`); flag `0x80000004` is a signed float.

### `entity_state_player_t` (entities 1..=32)

How often a field is in a player's delta (all 21 M deltas, POV and HLTV):

| Field | wire | in % of deltas | Note |
|---|---|---|---|
| `animtime`, `frame` | 8 / 10 bits | 97 / 93 | animation clock; every update |
| `origin[0]`, `origin[1]` | 18 bits / 32 | 63 | 1/32 unit steps; present when the player moved in that axis |
| `origin[2]` | 18 bits / 32 | 15 | jumps, stairs, ladders, falling |
| `angles[0]`, `angles[1]` | 16 bits (flag 0x10: angle) | 60 / 64 | pitch and yaw |
| `angles[2]` | same | 6 | roll (lean/prone) |
| `blending[0]`, `blending[1]` | 8 bits | 53 / 22 | aim and move blend bytes |
| `gaitsequence`, `sequence` | 9 bits | 9 / 5.5 | animation state (running, crouch, prone) |
| `weaponmodel` | 10 bits | 2.4 | model index of the held weapon |
| `mins[2]`, `maxs[2]`, `usehull` | 16 bits / 8, 1 bit | 2 | stance change (stand, crouch, prone) |
| `health` | 8 bits | 0.5 | present on a change only |
| `team`, `playerclass` | 4 bits | 0.3 | |
| `effects`, `body`, `movetype`, `framerate`, `solid` | | below 1 | |

The table defines more (`basevelocity`, `friction`, `gravity`, `scale`,
`rendercolor.*`, `controller[*]`, `spectator`, `fuser1`, `aiment`, `colormap`,
`skin`...), none of which DoD changed in these demos beyond a handful of
deltas.

No velocity field exists on a player entity (`basevelocity` is only for
conveyors and pushes). Speed has to come from consecutive origins.

### `clientdata_t` (the recording player only)

| Field | wire | in % of deltas | Note |
|---|---|---|---|
| `origin[0]`, `origin[1]` | 21 bits / 128 | 65 | 1/128 unit |
| `origin[2]` | 21 bits / 128 | 17 | |
| `velocity[0]`, `velocity[1]` | 16 bits / 8 | 60 | exact speed, no differencing needed |
| `velocity[2]` | 16 bits / 8 | 9 | |
| `flTimeStepSound`, `fuser4` | | 66 / 20 | |
| `m_flNextAttack` | 22 bits / 1000 | 19 | seconds; next time a shot is allowed |
| `flDuckTime`, `bInDuck`, `view_ofs[2]` | | 12 / 1 / 6 | stance and eye height |
| `flags` | 32 bits | 5 | `FL_*`: on ground, ducking |
| `weaponanim`, `viewmodel`, `m_iId`, `weapons` | | 4 / 3 / 3 / 1 | current weapon, weapon bit set |
| `ammo_shells`, `ammo_rockets` | | 3 / below 1 | |
| `punchangle[*]` | 16 bits / 8 | 2 | recoil |
| `health`, `deadflag` | 10 / 3 bits | 0.5 / 0.4 | recording player's health, in the same delta as their position |
| `fov` | 8 bits | 0.2 | zoom (scoped) |
| `iuser1`..`iuser3`, `maxspeed`, `waterlevel` | | below 1 | observer mode and target, speed cap |

### `weapon_data_t` (inside `svc_clientdata`, one per weapon)

`m_flTimeWeaponIdle` (99.8% of weapon deltas: it changes every update), `m_flNextPrimaryAttack`
(13%), `m_flNextSecondaryAttack` (9.5%), `m_iClip` (1.8%), `m_iId`, `m_fInReload`
(0.1%). Clip and reload state are present, so "ammo use" and "reload time" are
possible for the recorder; `m_fInZoom` exists in the table but never changed in
these demos.

## Answers to the step 1 questions

**Are player origins exact every update, or interpolated?** Exact: each is the
server's own position, quantised to 1/32 unit, and the demo records every
update the recording client received. Nothing needs interpolating to answer
"where was X at the update". A given axis is omitted when it did not change
(`origin[2]` in 85% of updates), so the replay keeps the last value per axis.
The updates are not a regular tick, though: a refresh gap per player was p50
11 ms, p90 15 ms, p99 34 ms over all demos (24 of 26 demos: p50 8-14 ms,
about 70-120 Hz; the CI fixture 29 ms and one HLTV demo 33 ms: about 30 Hz). One origin step
is p50 2.0 units, p90 4.1, p99 11.4: smooth movement, not hops. The max step is
5,684 units, a teleport (respawn), so a distance-travelled sum must drop steps
over a threshold or split at deaths (full sprint is about 3 units per 11 ms).

**Update rate per player, POV vs HLTV.** Same shape: the rate comes from the
server's update rate, not the demo type. Median refresh gap per demo: POV 8-14
ms except the fixture (29 ms); HLTV 11 ms in 3 demos and 33 ms in one (a
30 Hz proxy). Coverage is the difference: an HLTV demo has all players in every
snapshot (kills with both origins known: **1,250 of 1,253**, 99.8%); a POV demo
has the recorder's team and whoever the recorder could see (a player outside
the recorder's view is removed from the snapshot, 151,000 removals over the
set): **2,546 of 6,260** kills by a player (41%) have both origins. Per POV
demo it ranged 31-54% (75% on the 6-player fixture). A heatmap of "everyone" is therefore HLTV-only; a POV
heatmap is "the recorder's team plus what the recorder saw", biased toward
where the recorder looked.

**Is the recording player's origin in `svc_clientdata`?** Yes, and only there:
the recorder's own entity carries no origin (the client predicts it). 2.1 M of
the 2.7 M POV `svc_clientdata` messages carry an origin field (the rest changed
other things only). Rate: p50 11 ms between messages. It has more precision
(1/128 unit) and exact velocity, health, stance and weapon state. An HLTV
demo's 439,000 `svc_clientdata` messages have **no** origin (0): they describe
the proxy's own view, not a player.

**Can engagement distance be computed at the kill frame?** Yes, already
done: `kill_positions` reads both origins at the `DeathMsg`. A packet carries
its reliable messages before its entity update, so the position is the previous
snapshot's, about 10 ms old (`position_field_probe`; the figure in `position.rs`). At running speed that is under
3 units, which does not matter next to typical engagement distances of 500-1100
units (`kill_position_probe`). Coverage is as above: all kills on HLTV, about
41% of player kills on POV (the other kills keep what is known, `None` for the
rest). The recorder as killer or victim is always known.

**Cost to parse.** Nothing new to pay for what exists: the analyzer already
decodes these messages (the full analysis took 620 ms on average over the 26
demos on a warm cache; the raw parse alone 796 ms, which decodes the
sounds that the analysis skips and ran first on a cold file cache). On `solifotw-dbo2.dem`, the replay keeping three
floats per player costs 67 ms against 580 ms for the whole analysis (11%), and
walking the player deltas without replaying costs 18 ms. Skipping the messages
altogether would save 30-53% of end-to-end parse time (#37's Tier 4
measurement, `rnd/tier4-measure-37`) but loses the kill positions and headshot
flags, which is why the Analyzer keeps them. A per-update movement track
would be at most about 90 points per second per player: for a 25-minute demo
12 players x 1,500 s x 90 = 1.6 M points, 19 MB as three `f32`. Store it
down-sampled (for example one point per 100 ms: 180,000 points, 2 MB) rather
than raw.

## What this makes possible

| Stat from #448 | POV | HLTV | Source and note |
|---|---|---|---|
| Engagement distance per kill, per weapon | recorder's kills and deaths always, others 41% | all | done (`kill_positions`) |
| Death positions on the map | recorder and visible players | all | done, victim origin in `kill_positions` |
| Heatmap per team | recorder's team plus seen enemies | everyone | sample origins at a fixed interval (100 ms) |
| Distance travelled | teammates and recorder yes, enemies only while seen | all | sum origin steps; ignore steps above ~100 units (respawn) |
| Time alive, time to kill | from `health`/`deadflag` for the recorder; others from the existing kill/spawn events | same | `health` is in 0.5% of player deltas: sparse, but exact |
| Speed, time crouched or prone, time scoped | recorder only (`velocity`, `view_ofs`, `fov`) | players: stance from `usehull`/`mins`/`maxs`, `gaitsequence`; no scope | |
| Ammo use, reloads | recorder only (`m_iClip`, `m_fInReload`, `ammo_*`) | no | |

## Conclusions

- Positions are exact and frequent (about 90 Hz, 1/32 unit); no interpolation
  is needed for kill distances or for sampled tracks.
- HLTV demos are the complete source (all players, every update); POV demos
  hold the recorder always and others only in view, so any whole-match stat
  must say it is HLTV-only or "as seen by the recorder".
- The recorder's origin and velocity live only in `svc_clientdata`; an HLTV
  demo has none.
- The data is already decoded and the replay is built; the cost is storing a
  track. Down-sample it (100 ms) and keep it outside the cached `Analysis`
  until a stat needs it (changing the cached format means a
  `SCHEMA_VERSION` bump).

Not measured: a demo whose recorder changes mid-file, other mods' servers
(these are all DoD 1.3 servers: KTP and community), and the Anniversary client
(the PRE install's demos only; the delta table comes from the demo, not the
client).
