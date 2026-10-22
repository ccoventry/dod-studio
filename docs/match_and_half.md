# Match and half from a demo's contents

`analysis::group_matches` puts a folder's demos into matches and halves, and `analysis::half_of` names one demo's half, using only what the demos contain (#685). The code is `analysis/src/match_group.rs`; the R&D it comes from is in the issue (probe: branch `rnd/match-half-685`, `analysis/examples/match_half_probe.rs`).

## API

- `group_matches(&[(id, &Analysis, Option<modified_unix_secs>)]) -> Vec<MatchGroup<id>>`. Every demo lands in one half of one group. A `MatchGroup` has the map, two `MatchTeam`s (tag and SteamIDs; team 0 is the Allied side of the first half listed) and one or two `MatchHalf`s (the demos, the side each team played, and a `HalfCall`).
- `half_of(&Analysis) -> HalfCall` for a file on its own: fewer signals, so more often unknown.
- `HalfCall { half: First | Second | Unknown, reason, detail }`. `reason` names the signal that decided; `detail` lists what every signal saw.
- `find_demo(&groups, &id)` returns the match and half one demo went into.
- `trim_for_grouping(&mut Analysis)` drops what the grouping never reads (health histories, weapon breakdowns, flags), so a folder's analyses can be held for one call.

## Where Studio uses it

- `native::match_groups` numbers the matches (1 is the match whose oldest file is oldest; undated last) and flattens the result per demo.
- **Rename Demos:** `demo_rename_list` groups the listed folder, so `{match}` and `{half}` see every demo in it. `{half}` is `h1`, `h2`, or the `|fallback` (else `unknown`) — never a default first half.
- **Master Demo Queue:** `queue_match_groups` runs after each scan and project load, from the analyzer cache. The answer stays on each demo for the clip name's `{match}` and `{half}` (`queue_match_groups.js`), not in the project file. Numbers stay put as the queue grows. "Group by: Match" (`queue_group_by.js`, #823) shows the queue in match order under one header line per match, and reads any demo the cache lacks, also on later regroups while it stays picked.

## Grouping rules

- **Same half:** same map name, at least 60% of the smaller roster's SteamIDs in both, at least 80% of those on the same side. Then the kill feeds must pair (`is_same_half`, #708): two recordings of one half carry the same kills, HLTV included. When the feeds don't pair or one demo has too few kills, the names must agree instead (75% of shared SteamIDs showing the same name). This covers pieces of one half recorded at different times.
- **Other half:** same map and players, the sides swapped, names agreeing.
- **Two halves per match at most.** A half with several candidate partners takes the one most of its recordings relate to, then the one whose names agree best. The rest stay alone.
- **No SteamIDs** (per-demo `CONNECTION_n` ids): the sides' clan tags (`detect_team_tag`) and shared names stand in for the roster.
- **Never used:** server name, server address, map checksum. HLTV demos report the proxy (`<name>:0`, a loopback address, checksum 0), and one match's halves can be on two servers.

## Half order

The first signal that decides wins:

1. The server's announcement (KTP's `=== 2nd HALF STARTING ===`).
2. End-of-half chat after the match went live, across a half's recordings: "gh", "nh" or "good half" ends a first half, "gg" ends a match. Chat before live doesn't count, since warm-up "gg" is often about the previous map.
3. The server clock, if it kept running over the half: one half's recordings start after the other's end. Only recordings that saw the match go live count.
4. Warm-up length (server clock at match-live): long before a first half (616–1447 s in WSOD25), short before a second (108–241 s). This is a habit, so it decides only when the longer warm-up is at least twice the shorter and 120 s longer.
5. File dates, only from one recorder's two files dated about a half's length apart. Copied files carry copy times seconds apart, which say nothing.

If nothing decides, the answer is `Unknown`, never a default first half. `half_of` on one file uses only 1 and 2.

## Measured

- The probe: 371/371 named demos in the movie install's `dod` folder in the right match, 369/371 in the right half. The two misses look misnamed.
- This module, on the 68 `wsod25_grp*` demos (`wsod25_matches_and_halves_match_the_file_names`, `#[ignore]`d, about 35 s in release): 68/68 right match, 68/68 right half. Chat decided every half. Warm-up length would also have ordered all five matches (619–1447 s against 111–241 s). The server clock and file dates decided none: the server reloaded the map at the half, and the files carry copy times.

## Open risks

- **Rematch with unchanged names:** the same two teams on the same map, where the recordings share no kills and nobody renamed, read as pieces of one half and merge. The test `known_risk_a_rematch_with_unchanged_names_merges` pins this. It didn't happen in the probe's 371 demos, but these are mostly one season. The server clock (pieces of one half follow each other on one map load) and the round clock or score carried over could separate the two.
- **Narrow test set:** almost every match has one clan in it. Side habits don't matter, because no rule uses sides to order halves.
- **Multi-map demos:** the analysis keeps only the first map, so an unsplit demo's half is that map's (#217).
