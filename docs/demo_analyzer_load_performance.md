# Demo Analyzer Load Performance — Audit & Implementation Plan

Status: **Tiers 1a/1b/2/3 are done and on `dev`**: the on-disk analyzer cache, real
progress events, the dead-code removal and cache warm-up from the Capture scan,
plus later work: the scan warms the cache (#512), the decoded demo is freed off
the calling thread (#511), and older cache schema versions are swept (#510).
**Tier 4/5 are not started**, tracked as
[GitHub issue #37](https://github.com/ccoventry/dod-studio/issues/37) (open).
Written so a fresh chat can pick Tier 4/5 up without re-deriving anything: read
the measurements and the Tier 4/5 section before touching code.

## Goal

Opening a demo in the Demo Analyzer (`studio/src/analyzer_pane.js` ->
`analyze_demo_full`) takes a few seconds. User wants it faster — ideally
instant, or at least <= 1s.

## Relationship to the "more player stats" work

Tier 4/5 (skip decoding message types the analyzer doesn't read, to save the
last big chunk of parse time) directly targets `SvcClientData` and
`SvcDeltaPacketEntities`, which carry per-tick position, velocity, angles,
health and ammo for every player and are the richest source of new stats. The
stats review that this was waiting on has partly happened: the objective and
flag data (#517, the Flags tab #593) came from other messages, but nothing
reads these two streams yet (`docs/demo_stats_feasibility.md`, #192). So
decide which fields the stats work wants before designing the skip, or risk
building an optimization you partly undo.

## The benchmark

`benchmark/src/main.rs` is a phase-attribution profiler for the real
`analyze_demo_full` path. Use it to reproduce every number below:

```
cargo build --release -p dod-benchmark
./target/release/dod-benchmark.exe ./local/demos          # or any folder of .dem files
```

## Measured baseline (release build, 4 demos, 52-89 MB each)

Taken before #511 and #512, so the drop row now runs off the calling thread and the scan path shares the parse. Re-measure before trusting a number for a new decision.

| Phase | Avg | Share | What it is |
|---|---|---|---|
| read | 15 ms | 1.1% | `fs::read` the whole file into `Vec<u8>` |
| **decode** | **824 ms** | **62.4%** | `Demo::parse_from_bytes(Parse)` — structural walk + full netmessage decode |
| **drop** | **441 ms** | **33.4%** | freeing the decoded frame tree |
| events | 38 ms | 2.9% | the `AnalyzerState` event loop (13 handlers/event) |
| serialize | 2 ms | 0.2% | `serde_json` of the IPC payload |
| **total** | **~1.3 s** | | |

Structural walk alone (`MessageDataParseMode::None`, no netmessage decoding)
is ~180 ms — so **~650 ms of the 824 ms decode is netmessage parsing
specifically**, not directory/frame bookkeeping.

Netmessage stream composition (summed across the 4 demos, 1.8M messages):

- **74.6% of all messages are decoded then discarded** — nothing in
  `analysis/src/lib.rs` reads them.
- Three message types are 68.5% of all volume, **100% discarded**:
  `SvcClientData` (24.0%), `SvcDeltaPacketEntities` (24.0%), `SvcSound` (20.5%).
- The analyzer's full consumed set: EngineMessages `SvcDirector`, `SvcHltv`,
  `SvcServerInfo`, `SvcStuffText`, `SvcTime`, `SvcUpdateUserInfo`; UserMessages
  matching `is_relevant_message()` in `analysis/src/lib.rs` (~line 232):
  `RoundState, ClanTimer, TimeLeft, WaveTime, TeamScore, ScoreShort, ObjScore,
  Frags, PClass, PTeam, ScoreInfo, ScoreInfoLong, SayText, TextMsg, DeathMsg,
  PStatus, Scope, CurWeapon, ReloadDone, ResetHUD, Health`.

Debug vs release: only ~1.3-1.7x, not the 10x+ you'd expect from an
unoptimized build. `dem-patch` is a workspace **dependency**, so
`[profile.dev.package."*"]` (root `Cargo.toml`) already builds it optimized
even under `tauri dev`. Not a real lever — don't spend time here.

## What this rules out

- **The analyzer's own event-loop logic is not the bottleneck** (2.9%). The
  old `parse_with_diagnostics` machinery only ever measured that loop in
  isolation, which is why tuning it seemed to change nothing; it was deleted in
  Tier 2.
- **IPC / frontend render is not the bottleneck.** Payload is ~0.3 MB;
  serialize + Tauri IPC + `JSON.parse` is single-digit ms.
- **File read is not the bottleneck** (~1%). Don't memory-map the file — it
  wouldn't help and adds complexity.

## What actually costs the ~1.3s: decode + drop (95.8% combined)

This is the real finding. `Demo::parse_from_bytes(..., MessageDataParseMode::Parse)`
fully decodes every netmessage into typed structs — including the 74.6% of
messages the analyzer never reads — and then that entire tree gets dropped a
few hundred ms later, which is itself expensive. The drop cost being almost as
large as the decode cost (441ms vs 824ms) points at allocation-heavy
representations rather than the walk itself — most likely
`Delta = HashMap<String, Vec<u8>>` (per-entity-update delta table) in
`dem-patch/src/types.rs`, given every `SvcDeltaPacketEntities` message
(24% of all messages) builds one. **Not independently profiled with a memory
tool this session — the 441ms drop-cost measurement is real, but "which
allocation is responsible" is inference from reading the types, not a
heap profile.** If you want to confirm before doing the bigger delta-rewrite
work (Tier 4), that's the first thing to instrument.

## Verified separately (source-read, not benchmarked)

- **`native/src/patch/scanner.rs::scan_demo_for_highlights`** (called by
  `studio/src-tauri/src/capture_manager.rs::scan_directory_impl`,
  i.e. every Capture Studio folder scan) already calls
  `analysis::Analysis::try_from_bytes(&bytes)` — **the exact same full parse**
  `analyze_demo_full` does — for every demo it scans, then discards
  everything except `(tickrate, streaks, is_pov, local_player_index,
  playback_frames, match_start_tick, frame_times)`. Confirmed by reading the
  function body directly. This is real, reusable work being thrown away — see
  Tier 3.
- **`Analysis` and `FileInfo` already derive `Serialize`/`Deserialize`**
  (`analysis/src/lib.rs` ~line 225, `native/src/lib.rs` ~line 29) — a JSON
  cache needs zero new derive work.
- **Incidental bug, resolved 2026-08-22 by removal:** the old `analyze_demo`
  command fed an inline telemetry panel from top-level keys the serialized
  `Analysis` never had, so it paid the full parse and always rendered nothing.
  The command, its panel and its IPC wrapper are gone; the "View Match
  Telemetry" button jumps to the Demo Analyzer instead.
- Confirmed directly in this session's own build output:
  `[profile.release]` in `studio/src-tauri/Cargo.toml` is silently
  ignored by Cargo ("profiles for the non root package will be ignored,
  specify profiles at the workspace root") — profiles only apply from the
  workspace-root `Cargo.toml`. Zero runtime effect today; harmless but
  misleading config.
- `panic = "abort"` in the workspace root `Cargo.toml`'s `[profile.release]`
  means the `std::panic::catch_unwind` in
  `Analysis::try_from_bytes_with_progress` (`analysis/src/lib.rs` ~line 615)
  cannot actually catch anything in release builds. Correctness/crash-handling
  gap, not a speed issue — flagging so it doesn't get mistaken for "handled."

## What was built (Tiers 1a, 1b, 2, 3: all done)

- **Tier 1a, on-disk analyzer cache.** One JSON file per demo under
  `%APPDATA%\dod-studio\analyzer_cache\v<SCHEMA_VERSION>\<fnv1a of the canonical path>.json`,
  valid while the demo's size and modified time match. The code is
  `analysis/src/cache.rs` (`SCHEMA_VERSION`, currently 4), used by
  `native::run_analyzer_cached`; write errors never fail the analysis. A warm
  open is about 10-15 ms; a cold one still costs the full parse. **Bump
  `SCHEMA_VERSION` whenever what gets computed changes**, or an old entry
  silently deserializes with the new field missing. Older version folders are
  swept in the background (#510).
- **Tier 1b, progress events.** `analyze_demo_full` emits `analyzer_progress`
  (`{processed, total}`), throttled to 33 ms per CLAUDE.md. A cache hit never
  calls the progress callback, so the progress bar appears on cold parses only.
- **Tier 2, dead code.** `parse_with_diagnostics`, `ParseDiagnostics` and
  `check_states_equal` are deleted.
- **Tier 3, cache warm-up from the scan.** `scan_demo_for_highlights_with_analysis`
  returns the `Analysis` its parse already built, and `scan_directory_impl`
  passes it to `native::warm_analyzer_cache`, so a Capture folder scan fills the
  cache for the Analyzer (#512 made the scan read the same cache it writes).
- **Free off the calling thread (#511).** The decoded demo is dropped on a
  background thread, taking the drop row of the baseline off the open path.

### Tier 4/5 — not started (issue #37)

Tracked as [issue #37](https://github.com/ccoventry/dod-studio/issues/37). Kept in full below since it's the technical detail an implementer of that issue will actually need (which message types, which fields, why the sequencing with the stats work matters).

- **Selective netmessage parsing** (skip decoding message bodies for the
  74.6% the analyzer never reads, e.g. don't decode `SvcSound`/`SvcClientData`
  bodies past their length prefix). Real potential — could cut a large chunk
  of the 824ms decode — but genuinely risky: some of those "discarded"
  messages maintain delta/baseline state (`SvcDeltaPacketEntities` especially)
  that other decoders may depend on downstream in the same stream. Silently
  wrong output (bad scoreboard/kill numbers) is a worse outcome than "slow."
- **Rewriting the `Delta` representation in `dem-patch`** (the
  `HashMap<String, Vec<u8>>` per entity update, suspected of driving both the
  824ms decode and the 441ms drop). Bigger, cross-cutting change to a shared
  parsing crate other tools depend on.

Both need a `check_states_equal`-style correctness harness (a real "compare
before/after parsed state across a broad demo corpus" check) rebuilt and run
wide before shipping. That code was deleted in Tier 2; its pattern is still in git history
(commit `fa0d9d4`) if you want a starting point.

**This tier is not just deferred, it is actively in tension with adding more
player stats — read this before starting either.** The two biggest
"discarded" message types are also the richest untapped data source in the
whole demo:

- **`SvcClientData`** (`dem-patch/src/netmsg_doer/client_data.rs`) carries the
  POV player's per-tick `clientdata_t` delta — origin, view angles, velocity,
  health, FOV, punch angle — plus a `weapon_data_t` delta per held weapon
  (ammo/clip state). Verified by reading the decoder directly.
- **`SvcDeltaPacketEntities`** (`dem-patch/src/netmsg_doer/delta_packet_entities.rs`)
  carries an `entity_state_player_t` delta for **every player entity on the
  server** (`entity_index <= aux.max_client`), every network update — i.e.
  full positional/state data for the whole match, not just the recording
  player. Verified by reading the decoder directly.
- **Confirmed via `grep -rn "SvcDeltaPacketEntities\|SvcClientData\|entity_state\|client_data" analysis/src/*.rs`: zero hits.**
  The analyzer has never touched either stream. This is fully greenfield —
  distance traveled, movement heatmaps, average engagement distance,
  time-to-kill, ammo economy, health-over-time, positioning at death, all of
  it lives in these two message types and nothing reads them today.

Both fields decode via server-sent field definitions
(`SvcDeltaDescription` -> `aux.delta_decoders`), so the *set* of fields
available isn't fixed at compile time — it's whatever `clientdata_t` /
`entity_state_player_t` / `weapon_data_t` the mod defines. Worth dumping one
decoded delta's keys before designing anything, to see the real field list
rather than guessing from the GoldSrc SDK headers.

**Sequencing, restated plainly**: do the future-stats review first, then design the selective-parse mode around its answer (keep decoding whatever fields the stats work wants, skip only what's still unwanted regardless — e.g. `SvcSound`, `ClientAreas`, `SvcTempEntity`). Shipping Tier 4 first risks partially reverting it once a wanted stat turns out to need `SvcClientData`/`SvcDeltaPacketEntities`. The part of that review about which fields to keep is still open.

## How to verify any of this yourself

```
cargo build --release -p dod-benchmark
./target/release/dod-benchmark.exe ./local/demos
```

Reproduces the phase table and the consumed/discarded netmessage histogram
above — still accurate for a cold/cache-miss parse, since the benchmark
exercises the parse path directly and doesn't go through the Tier 1a cache.

**The cache:** open a demo in the Demo Analyzer tab, check
`%APPDATA%\dod-studio\analyzer_cache\v4\` (the current `SCHEMA_VERSION`)
gets a `<16-hex-digit>.json` file, then re-open the same demo and confirm it is
near-instant with no progress bar. A Capture folder scan fills it too, before
you ever open the Analyzer tab.
