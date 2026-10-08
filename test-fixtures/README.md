# Test fixtures

## `ci_fixture.dem`

A real Day of Defeat 1.3 POV demo, checked in so CI parses real network
traffic. `dem-patch/src/tests/demotest.dem` is built in code and has no
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
