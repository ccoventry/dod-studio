# Recovering damaged GoldSrc demos (#15)

How a GoldSrc `.dem` is put together, what has to be true for a spliced or repaired one to play, and the traps hit recovering four damaged DoD 1.3 POV demos. **Studio now does the repair** ("The repair in Studio" below): `analysis::repair`, run by the Demo Auditor's Damaged Demos tab, the Master Queue's and the Demo Analyzer's Repair buttons, and `hl-demo-auditor --repair`. The probes in `analysis/examples/` (PR #227) are the R&D trail and tools for checking a build. Issue #15 tracks what is left.

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
- **Different players' POV recordings of one match share the server clock** (`svc_time` is the server's `sv.time`, so it agrees to 5 decimals with no offset at all; the HLTV recording's `svc_time` is the proxy's own clock, a per-half constant away, see `goldsrc_dod_quirks.md` and #708) but **not their entity index sets** (each leaves out different entities, including the recorder). Matching size or a matching kill feed doesn't make two files the same recording.
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

## The repair in Studio

`analysis::repair::repair_demo` takes a demo's bytes and returns the repaired demo's bytes and a report, or refuses. Pure: no files, no threads, so it builds for the wasm web analyzer too. One file per step:

1. **Bridge** (`repair/bridge.rs`, was `multi_bridge`): walk the frame headers (`repair/frame_walk.rs`), cut at each hole, find where the recording resumes (shape first, then a parse of the run it starts), trim damaged frames off the end of each stretch, and splice. In process: the parser is bounds-checked at its read sites since #225, so a bad candidate is an `Err`, never an abort, and the child process `multi_bridge` used is gone (`m3_h1-1`'s 2,675 candidates took 4.5 min that way; in process the CLI repairs all four Monday demos in 2 min).
2. **Joins** (`repair/joins.rs`, `join_state.rs`, `snapshot.rs`, was `snapshot_inject`): at each `incoming_sequence` jump, a full snapshot of the entities a real packet carried (baseline fields, reset at an inline `SvcSpawnBaseline`), the ramp with its `svc_clientdata`, replayed client data and `CurWeapon`, each ramp frame its own ordinal.
3. **Entity counts** (`repair/entity_count.rs`, `entity_carry.rs`, was `entity_count_fix`).
4. **Checks**: the result is parsed again; it must parse, keep nearly every frame the bridge kept (a stray section end ends an entry early and still parses), have no entity packet the engine would discard (the flush predicate, as `flush_predict`), every count agree with the carried set, and every join a snapshot. A result that fails one is not written, and the report says which.

What the R&D builds did that the product doesn't:

- **No breadcrumbs.** `inject_breadcrumbs` (`echo [BC t]` every 5 s) stays a test tool.
- **No time sort.** `inject_breadcrumbs` also sorted every frame by time, which turned out to be doing a job: the bridge kept damaged frames at the edges of holes, and the sort moved them out of the way. The product drops them instead: a frame within 8 frames of a hole whose time is more than 1 s from the median of its side of the hole (`m1_h2`: a `Sound` frame stamped 0.000 s at hole 1, a client-data frame stamped 2031 s at hole 3; `m3_h1-1`: a client-data frame at 889 s and a network frame stamped 877 s among frames at 893 s, whose `incoming_sequence` is 2^17 too high, which the R&D build read as two more joins). The damage at these edges is bit flips: the network frame's sequence and time are each one bit off, and some resume frames' ordinals have bit 31 set. And the ramp goes in front of the client-data and demo-buffer frames that open the stretch after a hole, not after them, so frame times rise through every join as the sorted builds' did; after them, playback waits at their time for the whole hole and then plays the ramp at once.
- **Hole times in the report** are each side's frames, in recording time (seconds since the recording started; the hook's demo clock matches it on a repaired build within a few seconds).

Equivalence with the R&D builds, frame by frame (frames matched by type, time, ordinal and sequence, so an order change still matches; `analysis/examples/repair_compare.rs`):

| demo | product vs. R&D build | difference |
| --- | --- | --- |
| `m1_h1` | `recovered_m1_h1_v14` | none but the 253 breadcrumbs (528,843 frames each; snapshots 48 and 49 entities) |
| `m1_h2` | `recovered_m1_h2_v15` | the breadcrumbs, and the damaged `Sound` frame at hole 1 (705,715 frames against 705,716) |
| `m3_h1-1` | `recovered_m3_h1-1_v14` | hole 3 is one join, not two: the bit-flipped frame and its neighbour are dropped, so one ramp (3,000 frames) and one snapshot (37 entities) instead of two of each (531,212 frames against 534,214) |
| `m3_h2` | `recovered_m3_h2_v11` | v11 predates the real-entity snapshot and the count fix (3,372 count disagreements, snapshot 25 entities against 13); against today's R&D pipeline run without breadcrumbs, no difference |

Every product build: 0 discarded entity packets, 0 count disagreements. Live (2026-11-10, `playdemo` and `ffwd` to the end, the capture-wsod25o hook DLL): all four play to `Host_EndGame: End of demo.` on PRE and on POST, with no `Fatal Error`, `svc_bad`, `without model`, `aaatrigger` or `couldn't uncompress`; `m3_h2` included, whose September build `_v11` crashed PRE 33 s after its hole (`hw.dll+0x128d2`) and stalled POST: it had 3,372 counts below the carried set. On the four damaged originals the engine itself reads past some rubble the parser cuts: they end at about 369 s (`m1_h1`), 992 s (`m1_h2`), 397 s (`m3_h1-1`, `svc_bad`) and 917 s (`m3_h2`, `Demo message > MAX_POSSIBLE_MSG`), on both engine builds, later than the first hole in each.

**A damaged demo that recorded more than one map is repaired first, then split.** The split reads with the strict parser, which stops at the damage; it now says the demo is damaged and points at the repair. Studio's Repair & Split does both in one job, and the original's "already split" check counts the repair's pieces (`<name>_repaired_split<N>_<map>.dem`).

## The R&D pipeline (the probes the repair came from)

1. **`multi_bridge`**: walk the file, cut at each damaged region, find the next honest resume point, repeat. Demos usually have several holes; assuming one hides every resume point before the last break.
2. **`snapshot_inject`**: at every join, inject a full entity snapshot built by replaying the prefix, **seeded from `SvcSpawnBaseline`** (below) but listing only entities a real packet carried (see "Still open"), then a ramp of synthetic no-op frames up to the tail.
3. **`flush_predict`**: verify no entity packet will be discarded. Sequence-level only (see Traps).

`multi_bridge`, `snapshot_inject` and `entity_count_fix` were removed when they became `analysis::repair`; `flush_predict`, `snapshot_check` and `inject_breadcrumbs` stay.

Supporting probes: `salvage_probe`, `salvage_demo`, `bridge_ceiling` (swallowed-frame and backwards-time checks), `snapshot_check` (declared vs. replayed entity counts), `event_rate` (events per minute, to spot silent loss), `graft_signon`, `inject_breadcrumbs`, `patch_resource_url`, `find_crash_spot`, `deathmsg_diag`. Superseded and not to be revived: re-numbering the tail's `delta_sequence` (it stops `svc_bad` only by making the engine discard every entity packet after the join), `bridge_search`/`bridge_best`/`reseq_probe`.

Results: all four damaged demos recovered and parsing (`m3_h2` 520,165 frames, 1 hole; `m1_h1` 525,086, 2 holes, 96.7% of the original bytes; `m1_h2` 701,898, 3 holes, 97.7%; `m3_h1-1` 524,407, 3 holes). Live on PRE, `m3_h2`, `m1_h1` and `m3_h1-1` play to their ends; `m1_h2` too (`_v15`, see "Still open").

## Checking a copy for lost frames (#41)

A copy that silently lost frames (a bad thumb-drive transfer) still parses, because frames are length-prefixed with no checksum, and then crashes the engine. `hl-demo-auditor --integrity <demo-or-folder>...` catches it: it reports every packet-entity or client-data delta whose from-frame is not in the file, so the engine would decode it against stale ring state (`analysis::integrity::check_delta_continuity`; `native/examples/demo_integrity.rs` lists each one and can drop frames in memory to simulate a loss). It finds nothing on healthy demos and fires on a single dropped network frame; a demo that does not parse at all is reported as such. The recovered `_v10` builds above are flagged too, by client-data deltas only: the ramp's cloned `svc_clientdata` keeps one from-frame for its whole length. `m3_h2` played anyway, so a client-data-only hit on a salvaged build means stale client data, not necessarily a crash.

## A copy with no camera data (#920)

A different bad copy keeps every frame and every delta and still plays as a frozen camera: each network frame's own header (`refparams`: view origin, angles, view entity) is zero, as in a file rebuilt from server traffic. The kill feed and scoreboard play; the camera sits at the map origin with no gun. `wsod25_ply2_m3_h2_m00cat.dem` is one (all 118,291 playback frames zero, and every frame stamped 0.001 s); its `-1` copy is the real recording. `analysis::camera_data` flags a POV demo none of whose playback frames carries a view, and `--integrity` reports it. An HLTV proxy's own recording has every view zeroed too and plays fine (the spectator camera follows the entities), so HLTV demos are never flagged. Studio marks a flagged demo in the Master Queue ("no camera"), asks before capturing or previewing it, and says so in the Demo Analyzer.

## Analysing a damaged demo without repairing it (#811)

The analyzer reads a damaged demo up to its first frame that does not parse and keeps everything before it (`dem::demo_parser::parse_demo_tolerant`; the patcher and writer keep to the strict parse). The result says where it stopped (`DemoInfo::damage`: the viewdemo clock there, the frame, the byte offset and why), and the Demo Analyzer and the in-game Highlights tab show it. A directory lost with the end of the file is rebuilt by walking the frames, as for an unfinalised demo. On the four originals above it stops at the first hole: `m1_h1` at 8:27, `m1_h2` 12:09, `m3_h1-1` 18:23, `m3_h2` 10:59.

## What each live crash taught

Every one of these passed every offline check that existed at the time. Each was found by playing the build in the engine.

- **Seed the injected snapshot from `SvcSpawnBaseline`.** A field set once at spawn and never resent is invisible to a replay of `SvcPacketEntities`/`SvcDeltaPacketEntities`. Worldspawn's `modelindex` is exactly that. The client does not re-seed from the baseline itself; it builds each entity from what the packet lists. Leaving it out gives `SV_LinkEdict`'s `Tried to link edict 0 without model` once per tick from the join (1,658 times live), then the process exits with no dialog.
- **Encode entity indices as the engine does.** In real full snapshots `is_absolute_entity_index` is `true` for 0 of 2,337 entities (about 4% of delta entities use it), and entity 0 appears in **zero** of ~1.65M real entity observations. Write increment/difference indices from 0 and drop entity 0; absolute only for a gap over 63. Otherwise: `CL_ParseServerMessage: Illegible server message - svc_bad`.
- **Never jump at a join.** An instantaneous jump of thousands in `incoming_sequence` and tens of seconds in frame time is nothing a real recording produces: ~30 s stall under `playdemo`, `svc_bad` under `viewdemo`, whatever the entity content. Ramp synthetic no-op frames one step at a time instead, `delta_sequence` in lockstep with `incoming_sequence` (capped at 3,000 frames per join). Interpolate the outer `Frame.time`: `info.timestamp` is 0.0 on every real frame (109,624 checked).
- **A ramp frame's `svc_time` is the server clock, not the frame time.** The server clock runs ahead of the frame time by however long the server had been up when the recording started (10.58 s in `m1_h2`, the same on both sides of each hole). Ramps stamped with frame times took the clock back that far at every hole and forward again at the join. `playdemo` plays the file and never showed it, so #1375's play-to-the-end test passed; `viewdemo` plays the world by this clock and reads the whole demo into it in about 10 s, so from about 16 s into the repaired `m1_h2` (and its Repair & Split piece) the view left the map, tilted, and went black, long before the hole itself. The ramp's clock now rises from the last `svc_time` before the hole to the first after it.
- **A ramp frame's view comes from a clean frame.** Each ramp frame copies the view (`demo_info`: view origin, angles, health) of the first network frame after its hole, and the damage can reach that frame's body with its header intact: in `m3_h2` it holds a view origin y of 6.3e10, a client yaw of 1.1e12 and a health of -2147483548 (the top byte of each bit-flipped), and a client-data frame next to it a yaw of 4.9e21; `m1_h2`'s second hole has a simulated origin x of 2.4e9. Every ramp frame carried it (2,001 and 582 frames), and the screen stayed black for the whole 22 s cut on PRE and POST. Found by an in-game check of the repaired files, not by playing to the end. Any playback frame whose origin is past ±16,384 units, an angle past ±3,600 degrees or health past ±1,000 now takes the view of the next clean frame of its kind, else the one before (`repair/view_range.rs`); a network frame keeps its messages.
- **Every ramp frame needs `svc_clientdata` and its own frame ordinal.** HLTV's `Core.dll` "World" class keeps a separate per-player clientdata history ring keyed by frame number; omitting clientdata starves it and a shared ordinal collides in it: `World::ParseClientData: couldn't uncompress delta frame %i`, a spinning, wall-clipping crash. A zeroed `svc_clientdata` with the structural fields cloned is enough.
- **Replay weapon state across the hole.** No-op clientdata satisfies the ring but never tells the client the player's real weapon, which keeps predicting the old one (wrong gunfire sounds for a minute or two). Replay the last real `SvcClientData` (own fields plus per-weapon ammo and clip) once on the ramp's first frame, and the last active `CurWeapon` user message (`[is_active, weapon_byte, clip_ammo]`; its id is per file, from `SvcNewUserMsg`). The audio half of this was never confirmed by listening; an unproven theory is that `SvcEvent` has its own history ring the ramp does not feed.
- **Breadcrumbs pin live timing.** An `echo [BC <t>]` console command every 5 demo-seconds (the patcher's `ConsoleCommand` injection) lands in `qconsole.log` with `-condebug`, so a tester reports the last one seen instead of guessing from the kill feed or wall-clock.

## A crash family that is not ours

`Sprite: no such frame N` (N climbing over the whole session, about 32 log lines per step) escalating unpredictably into `Bad model on beam ( not sprite )` or `Decals must hit mod_brush!`. It appears at t=5 s, before any join exists, on every build tested, and only shows with `developer 1/2`, which is why it looked new. Ruled out: HLAE (reproduces under plain `hl.exe -game dod`), the hook DLL (not loaded that session), and the demo's dead fastdl URL (patching `SvcResourceLocation`, id 56, changed nothing: the map was cached). Best guess, unconfirmed: demo playback replays the resource-verification handshake from recorded data differently from a live connection. Treat it as a known risk of that demo/map/build, not a regression.

## Splicing two parts of one demo (#224)

#224 asked for a watchable highlight-reel `.dem`: the highlights of a demo back to back, each cut joined to the next. It was closed unbuilt. Capture no longer needs it, since seeking (#407) skips the stretches between clips. Its findings are kept here because a deliberate splice is the same join a repair makes, with one difference: both sides are chosen, so the gap can be zero and nothing moves while the stream is away. A repair bridging a hole has leftovers a deliberate splice wouldn't.

- **Entity fields can be written, not just read.** `analysis/examples/delta_rewrite_probe.rs` moves every player entity 128 units along x: 2,598 `origin[0]` fields rewritten, the same file size, a clean re-parse, the shift exact, and the analyzer decodes the whole delta chain afterwards. So `dem`'s encoder round-trips authored entity state; what to write at a join is the hard part.
- **What a clean join needs, easiest first:**
  1. The `delta_sequence` renumbering and a full entity snapshot at the join (`snapshot_inject`, above). Without the renumbering the engine rejects the stream within a few frames.
  2. Every player field that differs across the cut, not only `origin`: angles, `sequence`/`gaitsequence`/`frame`/`animtime`, `health`, `weaponmodel`, `playerclass`, `team`, `body`, `colormap`, the stance hull (`mins`/`maxs`/`usehull`), `effects`, `movetype`, `velocity`. A delta may carry any subset of fields, so the join can carry all of them.
  3. Non-player entities: dropped weapons, grenades in flight, doors, capture entities, gibs, smoke.
  4. Time rebasing across the join, keeping the frame-ordinal rules in `CLAUDE.md` (every injected frame shifts every later ordinal).
  5. **Unsolved: state that isn't entity data.** Team scores, the scoreboard, flag and capture status, the round timer and other HUD state arrive as user messages sent once, when they change. A cut drops them, so the HUD and objectives are wrong however exact the entities are. They would have to be replayed or rebuilt at each join.
- **DemoPlayer's `dem_save` can't write such a demo** (#1134, tested on PRE only).
- **A stream can jump back in time and still play.** A KTP pause makes its HLTV proxy replay the last 60 s of the match (#1365). In the demo, `svc_time` keeps counting up, no full `SvcPacketEntities` resync is sent, and the world goes back to where it was 60 s earlier, positions bit-exact. Such demos play through the replay, as viewers of KTP matches know. The proxy encodes every delta against what the client holds, so it's a working example of a splice whose deltas stay consistent.

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

The injected snapshot is the wrong size in two ways (`snapshot_check`): the replay keeps slightly more entities than its own segment declares (13 vs 10), and the tail expects a count the prefix never had (inherent to the hole). Over-supplying leaves ghost entities that are never removed; under-supplying heals when the entity next updates.

**A count below the carried set is a crash, not just ghosts** (measured live on PRE, #15). `CL_ParsePacketEntities` allocates the new frame from the packet's declared `entity_count` and copies every carried entity into it without a bounds check (`rep movsd` of 0x154 bytes at `hw.dll+0x13453`). All three unplayed `_v10` builds die a few seconds after their first join: `m1_h1` with an access violation right there (its tail's first delta names the prefix's 48-entity snapshot and declares 10), `m1_h2` and `m3_h1-1` with heap corruption a few seconds later. `analysis/examples/entity_count_fix.rs` restates every packet's count from the carried-forward set (`examples/shared/entity_carry.rs`, the engine's 64-slot ring by `incoming_sequence & 63`, with the flush rule: keyed by the whole low byte it disagreed with the engine after a sequence jump). With it, `m1_h1` and `m3_h1-1` play to their ends (`recovered_*_v13.dem` in the PRE install's `dod` folder).

**The injected snapshot must list only entities a real packet carried** (fixed, #15). Seeded from `SvcSpawnBaseline` alone, it listed every edict the map spawned, and the server never sends some of them. At `m1_h2`'s first join, 45 of its 113 entities were in no packet of the prefix: 28 trigger brushes (`dod_trigger_sandbag` ×15, `trigger_multiple` ×6, `trigger_hurt` ×5, `dod_capture_area` ×2), 15 other brush entities the recorder never had in view (`func_wall` ×10, `func_breakable` ×2, `func_illusionary`, `func_ladder`, `func_water`) and two player slots. The count-fixed build died 7 s after that join with `Fatal Error: lightmap for texture aaatrigger too large`, the client drawing a trigger brush. `snapshot_inject` now keeps the baseline's fields but lists only entities some real packet before the join listed, and prints what it left out (a `*N` model is the BSP entity whose `"model"` key is `*N`). `recovered_{m1_h1,m1_h2,m3_h1-1}_v14.dem` (PRE `dod` folder) are built from the originals through `multi_bridge`, `snapshot_inject`, `inject_breadcrumbs` and `entity_count_fix`. Measured live on PRE (`playdemo`, `host_framerate 0.1`, 2026-11-10):

| build | joins | result |
| --- | --- | --- |
| `m1_h1_v14` | 2 (first snapshot 245 → 48 entities) | plays to its end (last breadcrumb 1265 s) |
| `m3_h1-1_v14` | 3 | plays to its end (last breadcrumb 1290 s) |
| `m1_h2_v14` | 3 | past joins 1 (740 s) and 2 (1116 s) with the world drawn and no error, through the rest of the `dod_anzio` match, then stopped by the hook DLL's level-change crash (below) |
| `m1_h2_v15` | 3 | **plays to its end** (last breadcrumb 1830 s) with the level-change fix: through both level changes (`dod_pandemic_aim` at 1479 s, `dod_thunder2_b5a` at 1527 s) and join 3 (1519 s), no error |

No `Sprite: no such frame` lines in any of these runs. All four damaged demos now play to their ends on PRE.

**A level change inside the prefix resets the snapshot's state.** It re-sends the signon inline, and the new map's edicts replace the old map's. `m1_h2_v14`'s third join, on `dod_pandemic_aim`, listed `dod_anzio` entities and spammed `Tried to link edict 93 without model` (also 267 and 331) from the join on, and the game died 9 s later. `snapshot_inject` now starts again from an inline `SvcSpawnBaseline`: that snapshot dropped from 42 to 6 entities (`_v15`). `m1_h1` and `m3_h1-1` have no level change, and their `_v15` builds are byte-identical to `_v14`.

**The level-change crash was the hook DLL**, not the salvage: under `playdemo`, the connect guard and the review lock wrapped `reconnect` into a loop, a stack overflow 1.3 s into any level change (a healthy `bewton-playoffs-round1-armory-allied.dem` crashed the same way). Fixed by #1370.

The R&D builds' time sort, once listed here as open, is gone from the product: see "The repair in Studio".
