# Test fixtures

## `ci_fixture.dem`

A real Day of Defeat 1.3 POV demo, checked in so CI parses real network
traffic. [`dem-patch/src/tests/demotest.dem`](../dem-patch/src/tests/README.md) is built in code and has no
network messages, so before this file nothing in CI exercised the message
parsers or the analyzer on a real recording.

- **Recorded** 2026-10-07 on the maintainer's test account, from the stock
  (25th Anniversary) client, on a local dedicated server with Sturmbot bots
  (`dod_anzio`, about 5 minutes; KTP's Windows test server running
  Sturmbot 1.9 at `sys_ticrate 100`).
- **What's in it:** 6 players (the recorder plus 5 bots, both teams), 16 kills
  with rifle, carbine, pistol, scoped rifle, knife, spade and grenade, a team
  kill, a 4-kill streak, 15 flag captures (2 by the recorder), a round won by
  Axis, team and all chat, and `kill` suicides. The final scoreboard shows 21
  kills: three bots were playing before recording started.
- **Bots** have no SteamID, so the analyzer keys them by connection
  (`CONNECTION_n`). One quirk worth knowing: Cpl. Preddy was renamed and moved
  to Allies by Sturmbot's team balancing, and 2 of his 3 scoreboard kills
  predate the recording but he isn't flagged as active before it.
- **Size:** 10.9 MB; git stores it compressed (about 2.4 MB).

**Tests that use it:**
- `dem-patch`: a parse and full re-encode round trip.
- `native`: `a_real_demo_survives_the_verbatim_write` (the decal strip's
  verbatim writer), by default.
- `analysis`: known-good analysis values, and the optimised-vs-unoptimised
  comparison.

**Replacing it:** the analysis test pins this recording's numbers, so a new
recording means updating `analysis/src/tests_fixture.rs` from the analyzer's
own output (`cargo run -p native --bin dod-studio-cli -- analyze
test-fixtures/ci_fixture.dem`) after checking the numbers against what was
played.

## `dodstudio_benchmark_anzio.dem`

The demo Studio's OBS capture test plays (#646), bundled with the app under
`obs-test\` (`tauri.conf.json`'s resources; `BUNDLED_DEMO_FILE` and
`BUNDLED_WINDOW` in `studio/src-tauri/src/obs_test_manager.rs`).

- **Recorded** by the repo owner, m00cat ("ChillRx (m00cat)"), as a POV demo
  of a 6v6 KTPS4 vs Thunder match on `dod_anzio` (Axis), with no lost
  packets. The original is `ktps4-thunder-anzio-axis.dem`, 77.8 MB.
- **Cut** to its first 100 s of play (the demo player's bar runs 0:58 to
  2:38; the clock is kept) with
  `cargo run --release -p analysis --example trim_demo -- ktps4-thunder-anzio-axis.dem 100 dodstudio_benchmark_anzio.dem`,
  which gives this file byte for byte (SHA-256 `d57e6667...6dea453`). Only
  the end can be cut: a demo's one full entity snapshot is at its start.
- **Why this one:** about 10 times the grenades and 3 times the kills of
  `ci_fixture.dem` (8-12 grenades and 12-16 deaths a minute, like a real
  match), and 15-21% slower for the game to draw. A bot demo made the test
  promise one or two rates more than a real match holds.
- **The test records** 1:32 to 2:30: 1:32-1:52 has 11 grenades and 6 kills,
  2:10-2:30 has 6 grenades and 6 kills.
- **Size:** 6.5 MB.
- **Still lighter to encode than a detailed map:** on an RTX 3070 Ti at OBS
  Quality High it holds 200 fps where a real `dod_harrington` match fails at
  165. So "Find my highest steady FPS" recommends one ladder step below the
  highest rate that held on this demo (`recommendedRate` in
  `studio/src/obs_capture_test_logic.js`); on a player's own demo it
  recommends the highest steady rate itself.

**Tests that use it:** `desktop-studio`'s `obs_test_manager` tests (the file
is there, and its highlights fill `BUNDLED_WINDOW`), and `hl-demo-auditor`'s
integrity check.

## `players_table.txt`

The Players table file Studio writes for the hook DLL (#428: sprays and
crosshairs), the one format both sides share:
`native::player_overrides::table_text` must write exactly this from its test
table, and `goldsrc-hooks`' `player_table` parser tests read it back.
Change the format in all three places together.

## `dod_anzio_sound_entities.txt`

The sound wiring from `dod_anzio.bsp`'s entity lump, verbatim: its control
points and capture areas, the control point master, and every
`ambient_generic` with a `targetname` (the win music beside the bell, the tree
and the scripted gunshots, which must stay unmuted). Every other entity is left
out: 17 entities, not the map's whole lump.

**Tests that use it:** `dod`'s `map_sounds` (the structural selection picks
anzio's two win tracks and two capture announcements, nothing else) and
`goldsrc-hooks`' `map_sounds` (the hook's runtime classification of the same
text, as NUL-terminated bytes the way the engine keeps it, gives the same
names).
