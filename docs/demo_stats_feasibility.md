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

There's also a stale standalone copy at `C:\Users\chris\Downloads\ktp-demo-stats-spec.html`
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
every capture with its cappers and the flag's owner just before it (`is_break()` is the
artifact's "enemy-owned flag" rule; `owners_before` supports narrower ones), and every
timed capture attempt with its outcome (cancelled = a cap block for the defenders). All of
it clears when the match goes live. `dod-studio-cli stats <demos>` prints it as JSON.

Measured by `analysis/examples/objective_probe` over the local library (484 of 488
demos parsed; 52 classed HLTV by `SvcHltv`): HLTV 2,103 captures, 22.3% multi-capper,
1.255 credits per capture; POV 16,786 captures, 24.7% multi-capper, 1.278. The raw
whole-file `capwindow_probe` on the same 52 HLTV demos gives 22.1% and 1.252, and puts the
named capper's own increment in the capture frame 99.3% of the time. About 30% of timed
attempts are cancelled (28.6% HLTV, 31.5% POV). The artifact's 19.8% / 1.22 are from a
different corpus (the 126 LAN HLTV demos).

Still open from the punch list: half modelling (the CLI only reads `_h1`/`_h2` from the
file name), the demo-type check (PR #395, still open), and a per-player cap-break column, which needs
the league's definition first: the scoreboard it copies credits one player with 2 breaks
and 0 captures, so its "break" is not a capture at all.

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
