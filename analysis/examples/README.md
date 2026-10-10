# Demo stats probes

Measurement tools used to establish what match statistics can and cannot be
recovered from a Day of Defeat 1.3 demo file. They exist so the findings below
are reproducible rather than asserted.

These are `examples/`, so they are not part of any shipped binary. `cargo build`
ignores them; `cargo build --examples` and `cargo test` compile them.

## The stats probes

The table covers the probes that measure what statistics a demo can yield.
Every other example is a one-off R&D tool, listed in the next section and
documented in its own `//!` header.

| Probe | Question it answers |
| --- | --- |
| `msg_probe` | What messages are on the wire, and how wide is each one? |
| `scoreboard_probe` | How much of a scoreboard can one demo produce? (exploratory, naive) |
| `batch_probe` | One TSV row of coverage metrics per demo, for corpus-wide sweeps |
| `hltv_probe` | Is this a true HLTV recording or a POV demo carrying director frames? |
| `reconcile_probe` | Do derived kill counts agree with the server's own frag counter? |
| `reconnect_probe` | What does a reconnect do to the server's score counters? |
| `capwindow_probe` | How far is a flag capture from the objective-score credits it earned? |
| `userinfo_times` | When did a player's name first reach the recording, and when did it change? |
| `seek_burst` | How much network data can a few seconds of a demo hold (the stepped seek's budget, #596)? |
| `objective_probe` | What does the analyzer report for captures, cap credits and cap blocks? |
| `weapon_switch_probe` | Where does a player rapidly cycle weapons, on the demo's own clock? |
| `map_text_probe` | Which channel carries a map's on-screen text, and what does it say? |
| `svc_sound_probe` | Which carrier does a given sound arrive on, and does it name an entity? |
| `position_field_probe` | Which movement fields do entity/client deltas carry, and how often? (#448) |
| `kill_position_probe` | Does the entity replay agree with a naive one, and what does it cost? (#448) |

`weapon_switch_probe` exists because `goldsrc-hooks`' log cannot answer "where
in the demo was that?". Its clock counts from when the *client* loaded and
keeps running while playback is paused, so it has no fixed relationship to a
position in the file. This reads `entity_state_t::weaponmodel` — the same
replicated field the animation fix reads — off `frame.time`, which is the
demo's own, and prints bursts of rapid switching. Matching a burst's shape
against the log is how the two clocks get lined up; see the module doc.

Run any of them against a single demo:

    cargo run --release -p analysis --example msg_probe -- path/to/demo.dem

`batch_probe` is designed for sweeps. Drive it one process per demo so a file
that panics the parser cannot take down the run:

    for f in /path/to/demos/*.dem; do
      timeout 120 ./target/release/examples/batch_probe "$f" >> out.tsv
    done

## Other R&D probes

### Salvaging and bridging damaged demos (#15, #41, #58)

| Probe | What it does |
| --- | --- |
| `salvage_probe` | How much of a demo that DoD refuses to play is intact? |
| `salvage_demo` | Recovers the intact prefix of such a demo. |
| `resync_probe` | Is the material after a demo's corruption still intact? |
| `resync_validate` | Finds a resync point that is clean at the message layer. |
| `bridge_search` | Searches for a working bridge across a demo's damage. |
| `bridge_best` | Finds the best bridge and cuts in the right place. |
| `bridge_skip_probe` | Bridges damage by resuming a little after the resync point. |
| `bridge_ceiling` | Why a bridged demo parses far fewer frames than its bytes contain. |
| `multi_bridge` | Bridges every hole in a damaged demo, not just the first. |
| `graft_signon` | Repairs a damaged signon using another demo of the same match. |
| `same_recording_check` | Cross-checks whether two demo files are the same recording. |
| `stitch_probe` | Can two distant slices of a healthy demo be stitched into one file? |
| `reseq_probe` | Renumbers a stitched demo's tail so its delta sequence continues. |
| `trim_demo` | Trims a demo to its first N seconds, still valid. |
| `trim_survey_probe` | Where can a demo be cut without breaking delta continuity? |
| `event_rate` | Are events spread evenly through a recovered demo? |

### Snapshots, deltas and the writer (#224)

| Probe | What it does |
| --- | --- |
| `snapshot_inject` | Rebuilds the client's entity table at a join with a synthetic full snapshot. |
| `snapshot_check` | Does an injected snapshot hold as many entities as the stream expects? |
| `entset_probe` | How many entities does a snapshot describe, and how does the set evolve? |
| `entity0_encoding` | How a real `svc_packetentities` encodes entity 0 (world). |
| `index_encoding_survey` | Every entity-index encoding used across a demo's packets. |
| `flush_predict` | Predicts offline every entity packet the engine will throw away. |
| `delta_rewrite_probe` | Can entity fields be rewritten and survive re-serialisation? |
| `delta_seq_probe` | Do the tail's deltas reference snapshots the client no longer has? |
| `reencode_diff` | Does a full re-encode change content, not just bit choices? |
| `reencode_value_diff` | Do a round-trip's values survive for the untouched tail? |
| `writer_fidelity` | Does dem's writer reproduce GoldSrc's real bytes? |
| `packet_entity_probe` | Peak packet-entity count against the engine limit that crashes playback. |
| `map_entity_probe` | Names the entities an HLTV demo puts on the wire, for trimming a map. |

### HLTV, spectating and weapons

| Probe | What it does |
| --- | --- |
| `director_probe` | What the HLTV director records, and whether target switching is in the stream (#206). |
| `iuser_probe` | Where the spectator target comes from during HLTV playback. |
| `gait_probe` | Which movement states are replicated for other players; can sprint be told apart? |
| `sprint_viewmodel_probe` | What DoD does to the first-person viewmodel while a player sprints. |
| `crosshair_pov_probe` | What the recording player was doing, as far as the crosshair rule cares (#310). |
| `hltv_sound_probe` | Are weapon-fire events present in an HLTV demo's raw stream? |
| `hltv_shot_gap_probe` | Detects fire events a recording dropped, from that recording alone. |
| `hltv_shot_evidence_probe` | Does a recording carry any trace of a shot whose fire event it dropped? |
| `weapon_anim_probe` | What the engine does to the first-person viewmodel, read out of a demo. |
| `weapon_id_probe` | Tallies the weapon-ID byte of every `DeathMsg` across a folder of demos. |
| `statusicon_probe` | Every `StatusIcon` message: icon name, enable/disable, colour. |
| `statusvalue_probe` | Every `StatusValue` message, with the `viewdemo` window's timer. |
| `grenade_family_probe` | Which of a grenade viewmodel's two sequence families a throw uses. |
| `grenade_pinpull_tell_probe` | Does a demo carry any signal when a player pulls a pin? |
| `grenade_pov_timeline_probe` | What a player's own recording shows around a grenade throw. |
| `grenade_prime_probe` | Can a recording tell a primed grenade from a plain throw? |
| `grenade_timing_probe` | When a thrower's body animation changes, relative to the throw. |

### Crash hunting and live-test aids

| Probe | What it does |
| --- | --- |
| `deathmsg_diag` | Raw `DeathMsg` messages in a time window, typed decode and bytes. |
| `find_chat_spot` | Locates a chat line's exact frame, time and sequence. |
| `find_crash_spot` | Dumps every kill/score event with frame index and time. |
| `injection_context` | Message-type sequence around a demo's injected join frame(s). |
| `inject_breadcrumbs` | Injects periodic `echo` commands so a tester can read a running timestamp. |
| `verify_curweapon` | Verifies the join's injected `CurWeapon` message landed, and what it names. |
| `tempentity_scan` | Every `SvcTempEntity` in a time window, by variant. |
| `patch_resource_url` | Replaces a demo's recorded `SvcResourceLocation` (download URL). |

### Grouping demos

| Probe | What it does |
| --- | --- |
| `match_half_probe` | Groups a folder's demos by match and half from their contents, scored against the file names (#685). |

## What they established

Measured across 624 demos — a mixed POV library plus 126 LAN HLTV recordings.

- **No headshots, ever.** `DeathMsg` was exactly 3 bytes in all 211,335
  instances. There is no hit group in the message, so headshot and
  headshot-rate columns cannot come from a demo.
- **No damage, and no assists.** Every one of the 126 true HLTV demos carried
  exactly one `Health` message — the proxy's own slot. POV demos average 168,
  but only for the recording player. Assists need damage attribution, so they
  fall with it.
- **Flag-capture credit needs two messages.** `CapMsg` named exactly one capper
  in all 26,476 captures, never two. Co-cappers appear as `ObjScore` increments
  in the *same frame*, 99.5% of the time, so no tolerance window is needed.
  About 20% of captures had at least one co-capper.
- **The analyzer's cap credits match the raw measurement.** `objective_probe`
  runs the real `Analysis` (#192), so its figures start at the match going
  live. Over the 52 HLTV demos in the local library: 2,103 captures, 22.3%
  multi-capper, 1.255 credits per capture, against `capwindow_probe`'s
  whole-file 2,140 captures, 22.1% and 1.252 on the same files. About 30% of
  timed capture attempts are cancelled (cap blocks).
- **Kill counts reconcile, once resets are handled.** Comparing a reset-aware
  derived count against the server's own frag counter over 6,567 player rows:
  75.3% agree exactly, 89.8% within one kill. Counting naively, without
  handling the match-start scoreboard wipe, drops that to 15%.
- **Reconnects restart the server's counters.** Reading the last value seen
  undercounts by 0.64% overall but loses up to 94 kills in a single demo;
  reading the highest value overcounts by 8.5%, because the peak predates the
  match-start wipe. Derived counts keyed on SteamID are immune to both.
- **`SvcDirector` is not an HLTV marker.** It appears in ordinary POV demos
  whenever an HLTV caster is spectating, and demo patchers inject it. `SvcHltv`
  is the reliable signal.
- **About 1% of demos will not parse.** Plan for a per-file failure path.
- **Map text is `HudText`, and only `HudText`.** Across all 36 demos in the
  local library, `svc_temp_entity`/`TE_TEXTMESSAGE` and `svc_centerprint` were
  carrying **nothing at all** — the two carriers #287 nominated first. Every
  map-authored line on screen arrived as the `HudText` user message: the
  round-result text from a `dod_score_ent`'s `message` keyvalue
  (`MAP_ALLIED_VICTORY2`, or a literal `"Allies take control over the
  village!"` on maps that skip the token), and the spawn-exit warning from an
  `env_message` (`MAP_SPAWN_WARNING`, four times in one anzio half). DoD's own
  clan-match prompts (`#Clan_allies_ready`) share the channel, so suppressing
  the channel wholesale is not the same thing as suppressing the map.
- **A map's own sounds never reach `EV_PlaySound`.** The two the mute requests
  ask about arrive on *different* engine messages, and neither is the event
  hook: a flag capture plays the `dod_control_point`'s `point_*_capsound`
  keyvalue as `svc_sound`, carrying that control point's entity index, while
  round-win music is an `ambient_generic` the map triggers, which arrives as
  `svc_spawnstaticsound` — the same message the map's placed ambience is
  registered with at signon, told apart only by *when* it appears. Measured
  across the demo library: win music lands in mid-demo frames and is absent
  from the halves that end without one, while placed ambience always sits in
  the first ~50 frames.
