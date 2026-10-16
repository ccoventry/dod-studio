# Demo-derived stats for KTP League — where this stands

**Start here if you're picking this up cold.** This is a separate work stream from the
Capture/Render Studio track — it
lives entirely on `dev`/`main` (commits `a86973a`, `8e6c3a5`, `c3b9c88`, `669d9f9`, all
merged; hashes corrected 2026-08-24, the originally-recorded ones were unreachable),
not on the capture/render feature branches. Tracked by issue #192, which is still open. If you're resuming work on capture/render quick-wins, this doc doesn't affect
you; if you're resuming the stats/league work, start here instead of re-deriving context.

## What this is

A friend runs the server/stats infrastructure for the KTP DoD 1.3 league (ktpleague.gg,
HLStatsX + custom AMXX plugins) and needs to backfill match stats for seasons 1–9, where
demos are the only surviving record. The question: how much of an HLStatsX-style
scoreboard can be reconstructed from `.dem` files alone, using `dod-studio`.

**The deliverable is a private Claude Artifact, not a file in this repo:**
`https://claude.ai/code/artifact/0481832b-dc0a-4726-baf3-e8be34fc55f5`

That artifact ("What the demo knows") is the actual spec — column-by-column coverage,
wire-format reference for every relevant DoD 1.3 user message, nine aggregation rules
with pseudocode, a `dod-studio` readiness assessment, and a HLStatsX join strategy. It is
the thing to read and update, not this file. This file just anchors it in the repo so a
fresh session (or a different AI) knows the artifact exists and what's true about the
codebase as of the last time it was checked against reality.

There's also a stale standalone copy in a local Downloads folder (`ktp-demo-stats-spec.html`)
(944 KB, fonts inlined) — it predates the fixes below and should not be treated as current.

## Headline findings (verified across 624 real demos)

- **9 of 13 HLStatsX scoreboard columns are demo-derivable**: player identity, kills,
  deaths, K/D, teamkills, suicides, objective points, flag-capture credits, flag-capture
  breaks. **4 are not, ever**: assists, damage, headshots, headshot%. GoldSrc/DoD 1.3
  never sends per-hit data to spectators — `DeathMsg` was exactly 3 bytes (killer,
  victim, weapon) in all 211,335 instances across the corpus. No hit group exists.
- HLTV demos are strictly better than POV for scoreboard purposes — everything broadcasts
  to all clients either way, but HLTV has cleaner rosters and reconciles better (76.9% vs
  74.6% exact agreement with the server's own frag counter).
- Two real bugs were found and fixed while validating this (see below): a live
  localization bug affecting 1,190 tokens, and a demo-type misclassification
  (`SvcDirector` appears in POV demos too, whenever an HLTV caster spectates — already
  hit on the capture/render side too, root cause is the same message).
- `CapMsg` only ever names one flag-capper; ~20% of captures are multi-capper and the rest
  are recovered from same-frame `ObjScore` increments. This is scoped to the 126-demo LAN
  HLTV subset specifically, not the full corpus — flagged as a correction after an earlier
  draft mismeasured it with too wide a correlation window (27.4% → correct 19.8%).

## What dod-studio produces now (2026-09-29, #192)

The five objective messages are admitted (#103), and `analysis/src/objective.rs`
consumes them. Per player, alongside the untouched server-counter `stats`:
`obj_points` (the sum of `ObjScore` increments), `cap_credits` (the `CapMsg` capper plus
every same-frame `ObjScore` riser, minus anyone known to be on the other team),
`teamkills` and `suicides`. Per demo, `state.objectives`: the flag layout and owners,
every capture with its cappers and the flag's owner just before it (`taken_from_enemy()`
is the artifact's "enemy-owned flag" rule, which it called a break; it was renamed once
breaks got the league's meaning, below), and every timed capture attempt with its outcome
(cancelled = a cap block for the defenders). All of it clears when the match goes live.
`dod-studio-cli stats <demos>` prints it as JSON.

Measured by `analysis/examples/objective_probe` over the local library (484 of 488
demos parsed; 52 classed HLTV by `SvcHltv`): HLTV 2,103 captures, 22.3% multi-capper,
1.255 credits per capture; POV 16,786 captures, 24.7% multi-capper, 1.278. The raw
whole-file `capwindow_probe` on the same 52 HLTV demos gives 22.1% and 1.252, and puts the
named capper's own increment in the capture frame 99.3% of the time. About 30% of timed
attempts are cancelled (28.6% HLTV, 31.5% POV). The artifact's 19.8% / 1.22 are from a
different corpus (the 126 LAN HLTV demos).

## Cap breaks (#192)

**Definition** (decided 2026-10-12): a break is stopping an enemy capture in progress.
The enemy starts a timed capture (`StartProg`) and it is cancelled (`CancelProg`) because
a defender killed the capper or stepped onto the point; the break is credited to that
defender. This matches the league scoreboard the stats copy, which credits one player
with 2 breaks and 0 captures.

**What the demo can show.** Neither `StartProg` nor `CancelProg` names a player, so an
attempt's cappers are never on the wire, and the capture areas are brush triggers the
server never sends. `InitObj`'s flag origin is no help either: the attackers whose deaths
cancelled a capture died a median 7,148 units from it, so it is not a world position. So
the analyzer attributes by timing, in `objective::cancel_cause`, for every cancelled
attempt:

1. An attacking-team player who died after the attempt started and at most
   `BREAK_WINDOW` (**1.1 s**) before the cancel is taken to be a capper. If a defender
   killed one, the last such defender gets the break (`Player::cap_breaks`).
2. If attackers died in the window but none by a defender (a teamkill, the world, a
   suicide), the block is counted in `objectives.blocks_unattributed` and credited to
   nobody.
3. If no attacker died in the window, the cappers walked off alive:
   `objectives.attempts_abandoned`, not a break. A defender stepping onto the point lands
   here too: with no area bounds, the demo cannot show who stood in one, so that branch
   of the definition is not built.

Each attempt keeps its `cancel_cause`. `dod-studio-cli stats` prints `cap_breaks` per
player and `blocks_unattributed` and `attempts_abandoned` per demo; each capture's old
`break` field is now `taken_from_enemy`. The analyzer's Flags tab has a Breaks column.

**The window, measured** by `analysis/examples/cap_break_probe` over the PRE `dod\`
library (458 demos, 2,558 cancelled attempts after the match went live). The cancel does
not come with the death: the gap from the last
attacker death to the cancel piles up between 0.5 and 1.1 s (1,441 of the 1,744 cancels
with an attacker death in the 2 s before; 1.0–1.1 s alone holds 121, 1.1–1.2 s 12). The
same lookup 5 s before each cancel, where nothing is being cancelled, finds an attacker
death within 1.1 s 9.5% of the time, and past 1.1 s the real gaps are no more common
than that chance rate: everything a death causes lands within 1.1 s. The cost is
coincidence: an attacker dying elsewhere within the window gets counted, roughly 5% of
the breaks credited.

**Totals over the library** with the 1.1 s window: of the 2,558 cancels, 1,673 breaks
(65.4%), 21 unattributed (0.8%: 23 of the deaths were teamkills, 5 the world or the
victim), 864 abandoned (33.8%). Demos of the same half from different recorders can
differ by a break (`wsod25_ply2_m1_h1`: 6 on the HLTV demo, 7 on m00cat's POV); not
looked into yet.

Still open from the punch list: half modelling (the CLI only reads `_h1`/`_h2` from the
file name; a content-based library is on #685).

## Repo changes already made in service of this (on dev/main)

- **Fixed a real localization bug** (`analysis/src/localization.rs`): `translate_key`
  prepended a `#` sigil on every lookup but never stripped one on insert, so any key
  stored bare (which is all 1,190 of them — `dod_english.txt`, `valve_english.txt`,
  `gameui_english.txt`, and now `dod_studio_english.txt` after this fix) silently failed
  to resolve. Fixed by normalizing (`trim_start_matches('#').to_lowercase()`) on both
  insert and lookup. `localizations/dod_studio_english.txt` had its 327 keys stripped of
  their `#` prefix to match the convention every other file already used. See
  `normalize_key` in `analysis/src/localization.rs`.
- **Brought `main` current** (2026-08-22) — it was ~300 commits behind `dev` and still
  advertised a removed `egui` GUI. Fast-forwarded then; `main` now only takes `dev` releases.
- **Test suite was green** at that point: 21 passed, 0 failed (was 4 failing before the localization fix
  and one stale fixture-dependent test — `test_inspect_lenn_demo` — was changed to skip
  rather than panic when its uncommitted fixture demo is absent).
- **Seven measurement probes committed** under `analysis/examples/` (with a README) —
  `msg_probe`, `scoreboard_probe`, `batch_probe`, `hltv_probe`, `reconcile_probe`,
  `reconnect_probe`, `capwindow_probe`. These produced every corpus-wide figure in the
  artifact; re-run them against your own demo folder to reproduce or extend the findings.
  They are on `dev`/`main`; a branch cut before 2026-08-22 won't have them (`git show
  dev:analysis/examples/<file>`).
- **README rewritten** (2026-08-22) with a component-maturity table (stable: `dod/`, `analysis/`,
  `dem-patch/`, `hl-demo-auditor/`; active development: `native/`, `studio/`), the
  `dem`-fork rationale, and the localization key convention.

## One thing worth knowing if you're evaluating the `dem` crate independently

This project vendors a patched fork (`dem-patch/`) of the public `dem` crate
(crates.io, v0.2.3, github.com/khanghugo/dem) because the published crate `.unwrap()`s
delta-decoder table lookups at 29 call sites across 7 files — a malformed or unexpected
demo panics the whole process instead of returning a parse error. Confirmed still present
in v0.3.0. Full detail and the fix rationale is in the artifact's prior-art section, not
duplicated here.

## Next step, if resumed

See "What dod-studio produces now" above for what #192 built and what is still open.
The artifact was last updated 2026-08-22 and predates that work.
