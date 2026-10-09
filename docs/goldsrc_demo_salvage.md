# Recovering damaged GoldSrc demos (#15)

How a GoldSrc `.dem` is put together, what has to be true for a spliced or repaired one to play, and the traps hit recovering four damaged DoD 1.3 POV demos. The tools are probes in `analysis/examples/` (PR #227); no library or pipeline code depends on them. Issue #15 tracks what is left.

Every fact below was measured on real demos or confirmed live in the engine, not assumed.

## Demo structure

- **Init state lives in directory entry 0**: `SvcServerInfo`, 7 × `SvcDeltaDescription`, `SvcSpawnBaseline`, the resource list and ~65 `SvcNewUserMsg`. Keep entry 0 whole and decoders and baselines survive any cut.
- **Full `SvcPacketEntities` snapshots only appear at the very start**: one in an HLTV demo, five within the first 0.1 s in a POV demo, then 111k–126k *deltas*. Entity state has **no mid-demo restart point**. Cutting the end is safe (deltas only refer backwards); cutting the front is not.
- **A map change re-sends the whole signon inline** (new ServerInfo, decoders, baseline and fresh full snapshots), so a demo can be split cleanly at a level change. Splitting multi-map demos (#624) and the analyzer's Split now (#217) rely on this.
- **`origin` fields are absolute world coordinates**, not movement offsets (shifted +128, it round-trips). Position desync after a splice heals itself as entities move.
- **Entity fields can be authored and round-trip exactly** through `dem-patch`'s writer (2,598 origins rewritten, identical file size).
- **`entity_count` is not the number of listed entities.** A packet declares the size of the *resulting* snapshot (median 20) while listing only the changed entities (median 6). Anything reconstructing entity state must model the carry-forward or it undercounts badly.
- **`CL_FlushEntityPacket`** (string at `hw.dll` `0x1e38be8`) is the engine discarding a whole entity packet it cannot resolve: the cause of missing crates and doors after a bad join. It prints to the screen only, never to `qconsole.log`, but the condition is exact and computable from the file alone (`flush_predict`): see "The flush predicate" below.
- **Healthy demo shape**: 160–165 bytes per frame, no single frame of 4 KB or more, and type 4 and type 9 frame counts pair exactly (197642/197642, 191043/191043, 103747/103747).
- **Different players' POV recordings of one match share the server clock** (`svc_time` agrees to 5 decimals once offset) but **not their entity index sets** (each leaves out different entities, including the recorder). Matching size or a matching kill feed doesn't make two files the same recording.
- **The HLTV recording of the same match is ground truth** for checking a recovered POV demo: kills and deaths per player matched exactly in all four recoveries. Compare event *rates*, not totals: a POV demo that runs 22 minutes against HLTV's 29 has fewer events for that reason alone.

## The flush predicate

Established by static analysis of the pre-Anniversary `hw.dll` (2026-09-09).

- **Why it never reaches `qconsole.log`**: the function at `0x1d12240` prints through `Con_NXPrintf` with an `{index, ttl, color}` block, which draws straight to the screen and never goes through `Con_Printf`. It is recognisable by its `memset(&from, 0, 0x154)` (`sizeof entity_state_t` = 340), its zeroing of `cl.validsequence` (`0x2d8ae00`) and its marking of the frame invalid.
- **The exact condition**, from `CL_ParsePacketEntities` at `0x1d12e30`:

      flush iff ((incoming_sequence - delta_sequence) & 0xFF) >= 63

  where 63 is the dword at `0x1e3afcc` (`CL_UPDATE_BACKUP - 1`) and `incoming_sequence` is `[0x2d59b20]`. A full (non-delta) packet skips the check and is always accepted.
- **Both terms are in the demo file**: `incoming_sequence` in each network-message frame's sequence header (`SequenceInfo` in `dem-patch`), `delta_sequence` in the message. So `analysis/examples/flush_predict.rs` finds every discarded packet offline.
- Two further flush paths exist at `0x1d13081` / `0x1d130c4` ("oldcopy invalid" / "remove invalid on non-delta compressed update"). They fire only when a *non-delta* packet decodes copy/remove opcodes, which `dem`'s `EntityState` cannot encode, so an injected full snapshot cannot trip them.
- **Why re-numbering the tail's `delta_sequence` was wrong**: it leaves `incoming_sequence` on the tail's own numbering, so the gap becomes 221 and *every* entity packet after the join is discarded. That is why the renumbered demo stopped crashing (nothing bad is parsed) and also why the world was missing around the join. Measured: a healthy demo's worst gap is 13; the stitched `m3_h2` before renumbering, 4 with no flushes; after renumbering, 62,374 packets discarded. Inject a full snapshot at the join instead.

## The recovery pipeline

1. **`multi_bridge`**: walk the file, cut at each damaged region, find the next honest resume point, repeat. Demos usually have several holes; assuming one hides every resume point before the last break.
2. **`snapshot_inject`**: at every join, inject a full entity snapshot built by replaying the prefix, **seeded from `SvcSpawnBaseline`** (below), then a ramp of synthetic no-op frames up to the tail.
3. **`flush_predict`**: verify no entity packet will be discarded. Sequence-level only (see Traps).

Supporting probes: `salvage_probe`, `salvage_demo`, `bridge_ceiling` (swallowed-frame and backwards-time checks), `snapshot_check` (declared vs. replayed entity counts), `event_rate` (events per minute, to spot silent loss), `graft_signon`, `inject_breadcrumbs`, `patch_resource_url`, `find_crash_spot`, `deathmsg_diag`. Superseded and not to be revived: re-numbering the tail's `delta_sequence` (it stops `svc_bad` only by making the engine discard every entity packet after the join), `bridge_search`/`bridge_best`/`reseq_probe`.

Results: all four damaged demos recovered and parsing (`m3_h2` 520,165 frames, 1 hole; `m1_h1` 525,086, 2 holes, 96.7% of the original bytes; `m1_h2` 701,898, 3 holes, 97.7%; `m3_h1-1` 524,407, 3 holes).

## What each live crash taught

Every one of these passed every offline check that existed at the time. Each was found by playing the build in the engine.

- **Seed the injected snapshot from `SvcSpawnBaseline`.** A field set once at spawn and never resent is invisible to a replay of `SvcPacketEntities`/`SvcDeltaPacketEntities`. Worldspawn's `modelindex` is exactly that. The client does not re-seed from the baseline itself; it builds each entity from what the packet lists. Leaving it out gives `SV_LinkEdict`'s `Tried to link edict 0 without model` once per tick from the join (1,658 times live), then the process exits with no dialog.
- **Encode entity indices as the engine does.** In real full snapshots `is_absolute_entity_index` is `true` for 0 of 2,337 entities (about 4% of delta entities use it), and entity 0 appears in **zero** of ~1.65M real entity observations. Write increment/difference indices from 0 and drop entity 0; absolute only for a gap over 63. Otherwise: `CL_ParseServerMessage: Illegible server message - svc_bad`.
- **Never jump at a join.** An instantaneous jump of thousands in `incoming_sequence` and tens of seconds in frame time is nothing a real recording produces: ~30 s stall under `playdemo`, `svc_bad` under `viewdemo`, whatever the entity content. Ramp synthetic no-op frames one step at a time instead, `delta_sequence` in lockstep with `incoming_sequence` (capped at 3,000 frames per join). Interpolate the outer `Frame.time`: `info.timestamp` is 0.0 on every real frame (109,624 checked).
- **Every ramp frame needs `svc_clientdata` and its own frame ordinal.** HLTV's `Core.dll` "World" class keeps a separate per-player clientdata history ring keyed by frame number; omitting clientdata starves it and a shared ordinal collides in it: `World::ParseClientData: couldn't uncompress delta frame %i`, a spinning, wall-clipping crash. A zeroed `svc_clientdata` with the structural fields cloned is enough.
- **Replay weapon state across the hole.** No-op clientdata satisfies the ring but never tells the client the player's real weapon, which keeps predicting the old one (wrong gunfire sounds for a minute or two). Replay the last real `SvcClientData` (own fields plus per-weapon ammo and clip) once on the ramp's first frame, and the last active `CurWeapon` user message (`[is_active, weapon_byte, clip_ammo]`; its id is per file, from `SvcNewUserMsg`). The audio half of this was never confirmed by listening; an unproven theory is that `SvcEvent` has its own history ring the ramp does not feed.
- **Breadcrumbs pin live timing.** An `echo [BC <t>]` console command every 5 demo-seconds (the patcher's `ConsoleCommand` injection) lands in `qconsole.log` with `-condebug`, so a tester reports the last one seen instead of guessing from the kill feed or wall-clock.

## A crash family that is not ours

`Sprite: no such frame N` (N climbing over the whole session, about 32 log lines per step) escalating unpredictably into `Bad model on beam ( not sprite )` or `Decals must hit mod_brush!`. It appears at t=5 s, before any join exists, on every build tested, and only shows with `developer 1/2`, which is why it looked new. Ruled out: HLAE (reproduces under plain `hl.exe -game dod`), the hook DLL (not loaded that session), and the demo's dead fastdl URL (patching `SvcResourceLocation`, id 56, changed nothing: the map was cached). Best guess, unconfirmed: demo playback replays the resource-verification handshake from recorded data differently from a live connection. Treat it as a known risk of that demo/map/build, not a regression.

## Traps

- **"No packets discarded" is not "the packets are correct."** `flush_predict` and `snapshot_check` check sequence numbers and declared sizes, never which fields a snapshot carries. A snapshot missing the world's `modelindex` passes them all and still kills the engine.
- **Find a join by the `incoming_sequence` discontinuity, never by frame time.** One demo's clock resets to 0.00 inside its intact prefix; "largest forward time jump" pointed at the start of playback and an empty snapshot was written over a healthy packet.
- **A walk reaching the end of the file proves nothing.** A garbage length field can skip megabytes and land on a frame boundary by luck. Judge shape (see "Healthy demo shape").
- **Cut at the damaged frame's start, not where the walk broke.** The break is inside the frame, past its 9-byte header; cutting there leaves an orphan header whose length field is read from the tail. One such frame declared 69,206,029 bytes and swallowed 84% of the file.
- **Entry 0 can end on two consecutive section-end (type 5) frames.** Stop after the first and the second ends entry 1 immediately: the demo parses and holds 32 frames out of 524,408. Consume the whole run, and compare the parsed frame count with what the walk found.
- **The frames just before a hole are damaged too**: headers survive, payloads don't. Trim back frame by frame until the segment parses.
- **A plausible resume point must be confirmed by parsing.** On `m3_h2` only the third shape-plausible offset was real. Parse a bounded probe demo (entry 0 plus the run it starts) in a child process.
- **Parsing is not success.** One accepted offset parsed fine but ended at the first section boundary in the tail, recovering nothing. Check the result carries the tail, and count events per minute (`event_rate`): a hole is a dip, silent loss is a long stretch near zero.
- **`DeathMsg` client indices are 1-based against `SvcUpdateUserInfo.index`.** Subtract 1 before looking up a name, as `analysis/src/lib.rs` does. Without it every kill is attributed to the player one slot away.
- **The live kill feed can fabricate, not just lag**: a weapon label that exists nowhere in the file, cross-checked against HLTV and another POV. Once something is wrong, go to the file.
- **Comparing `Debug` output across two parses**: `HashMap` order is random per process and `BitVec`'s `Debug` embeds a heap pointer. Sort keys and strip `addr: 0x…` first.

## Still open

The injected snapshot is the wrong size in two ways (`snapshot_check`): the replay keeps slightly more entities than its own segment declares (13 vs 10), and the tail expects a count the prefix never had (inherent to the hole). Over-supplying leaves ghost entities that are never removed; under-supplying heals when the entity next updates. Fixing it properly needs a real `CL_ParsePacketEntities` carry-forward simulator.
