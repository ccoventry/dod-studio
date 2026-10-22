# Decal Flush: BSP-Derived Surface Coordinates

> **Status 2026-08-27 — stages 1, 2, 3, 4 and 6 built and measured; 3 also verified in game.
> Stage 5 declined, deliberately.**
> The R&D direction for the decal flush ([#60](https://github.com/ccoventry/dod-studio/issues/60)).
>
> - **Stage 1 built** (`a4bed14`, `1c86571`): `native/src/patch/bsp.rs` reads BSP v30 geometry,
>   leaves, the node tree and visibility.
> - **Stage 2 passed** (`46957a2`): 98.93% of **107,584 coordinates the engine provably accepted**,
>   across 83 demos and 18 maps, land on a world face within 1 unit. The node tree validates the
>   same way — a point nudged 4 units along a face normal lands in open space 100% of the time and
>   in solid 99.5% the other way. Re-run with `native/src/bin/validate_bsp`.
> - **Stage 4 built** (`a9c9eab`, `da99dd7`) and it is the one that mattered. Across the whole
>   85-demo sample at `mirv_fov 105`: **85/85 demos reach a full 68-position sweep, with zero
>   in-clip frames showing a flush position, every one decided from geometry.** Before it, three
>   of those demos were getting a single position — a sweep that turns 4 of 256 ring slots.
> - **Stage 6 built** (`35fdee9`), measured, and off by default: a sweep at MAX_RENDER_DECALS stops
>   pinning `r_decals` at all. At ring 4096 all 85 demos still reach a full sweep with zero
>   on-camera frames and 1 short burst in 668. The default stays 256 — see that section for why.
> - **Stage 3 built** (2026-08-27): `Bsp::face_candidates` samples a grid across the world's own
>   faces. Reached only when the demo's decals and the coordinate store together cannot fill a
>   sweep, so a mature map pays nothing for it. `DOD_FLUSH_MAP_GEOMETRY_ONLY` forces it for an
>   in-game look. See "What the map is worth on its own" below.
> - **Stage 5 declined (2026-08-27).** Stage 3 feeds `resolve_flush_positions` directly and persists
>   nothing. Writing map-derived coordinates to disk was considered and dropped: the scan is ~15 ms
>   per map and the coordinates are a pure function of the map file, so a store would save about six
>   seconds across a 442-demo library in exchange for a second file format, a version field and a
>   staleness rule. The separation it was meant to protect already holds — only
>   `survey.world_harvested` is ever written to `decal_atlas`, so computed coordinates cannot reach
>   the store that `validate_bsp` tests against. The expensive half (testing candidates against every
>   in-clip camera) depends on the demo's camera path and cannot be cached across demos regardless.
>   Revisit only if a map appears whose scan is not cheap.
>
> Read `docs/goldsrc_dod_quirks.md` and issue #60 first — the engine facts the flush rests on
> are recorded there and must not be re-derived.

---

## Current state, and why this is the next step

The flush clears accumulated wall decals between capture clips by pinning `r_decals` to a small
ring and injecting one full revolution of synthetic decals into the gap ahead of each clip. It
needs one distinct position per few ring slots — **68 positions for a 256-slot ring** — and every
injected decal must land on a real surface, because a coordinate that misses creates no decal,
allocates no pool slot, and silently costs the sweep a slot.

Finding those 68 positions has been the whole difficulty. Four sources exist today, in priority
order (`native/src/patch/decal_strip.rs`, `resolve_flush_positions`):

1. **Tiled planes** — a 16-unit grid laid across planes fitted to real decal positions.
2. **Harvested decals** — positions the demo proves the engine accepted.
3. **Map coordinate store** (`native/src/patch/decal_atlas.rs`) — those positions pooled per map
   build and unioned across every demo processed.
4. **Player floor path** — floor points under wherever the player stood.

An 85-demo survey across 18 maps (sampled from the user's Steam library, both `allies`/`axis` and
`h1`/`h2` sides) put the current state at **67/85 demos reaching a full 68-position sweep**.

### The failure that remains, and its shape

The demos that fall short fail the same way: thousands of candidates are generated and **none**
survive the line-of-sight test. Harrington's worst demo laid 32,388 tiles and kept zero.

That is not bad luck. Every source above is drawn from player behaviour — harvested decals are
places somebody *shot*, tiled planes are fitted to those decals, the store is the union of those
across demos, and floor points are where somebody *walked*. "Places people shoot" correlates
strongly with "places people look", which is exactly what the cone test rejects.

**We are mining candidates from the same distribution we then have to filter against.** Widening
that funnel is why the coordinate store helped where different demos fought over different ground
(`element_left_soli_h1` went 23 → 68 positions) and why it cannot help a demo whose camera
covered everything its own gunfire proved.

Breaking the correlation needs a source of surface coordinates that owes nothing to what anyone
did in the match. That is the map itself.

---

## The proposal

Parse the map's `.bsp` and derive candidate coordinates directly from its geometry. The map
contains every wall, floor and ceiling — including the quiet back rooms nobody entered, which is
precisely the inventory a flush wants and the one thing gunfire-derived sources can never supply.

Maps are already reachable: `PatcherConfig.game_path` points at `hl.exe`, so the map is
`<game_path>/../dod/maps/<map_name>.bsp`, and `map_name` comes free in the demo header
(`decal_atlas::MapKey::from_header`, byte 16 of the 544-byte header).

### What gets read

GoldSrc BSP version 30. Header is `i32 version` followed by 15 lump entries of
`{i32 offset, i32 length}` — 124 bytes. Six lumps carry what we need, plus two for filtering:

| Lump | Index | Struct | Size |
|---|---|---|---|
| `PLANES` | 1 | `float normal[3]; float dist; i32 type` | 20 |
| `TEXTURES` | 2 | miptex directory — names, for filtering | var |
| `VERTICES` | 3 | `float point[3]` | 12 |
| `TEXINFO` | 6 | `float vecs[2][4]; i32 miptex; i32 flags` | 40 |
| `FACES` | 7 | `u16 planenum; i16 side; i32 firstedge; i16 numedges; i16 texinfo; u8 styles[4]; i32 lightofs` | 20 |
| `EDGES` | 12 | `u16 v[2]` | 4 |
| `SURFEDGES` | 13 | `i32` (negative means the edge is traversed backwards) | 4 |
| `MODELS` | 14 | `float mins[3], maxs[3], origin[3]; i32 headnode[4]; i32 visleafs; i32 firstface; i32 numfaces` | 64 |

A face's polygon is recovered by walking `firstedge .. firstedge + numedges` through `SURFEDGES`
into `EDGES` into `VERTICES`. Sample points inside that polygon, inset from its edges, and each
one lies exactly on the face's plane.

**These offsets are stated from the documented format, not from having parsed a file here.** They
are not taken on trust — stage 2 below is designed to catch any of them being wrong.

### What this gives beyond "more coordinates"

Three heuristics currently standing in for information the BSP simply has:

- **Real surface normals.** `tile_positions` infers planes by clustering coordinates along X, Y
  and Z, so it only ever finds axis-aligned surfaces. Angled geometry is invisible to it — the
  anzio sand ramps noted on #60 as contributing nothing are a direct consequence. Face normals
  make sloped surfaces first-class.
- **Real polygon extents.** `TILE_REACH` (64 units of dilation from a real decal) and
  `TILE_MAX_EXTENT` (200 units from the patch centroid) are both guesses standing in for "where
  does this surface end". A face knows. This also removes the cause of the one regression the
  coordinate store introduced (`dyelife_m2_harr_h2`, 64 → 62 positions): store points bridged two
  previously-separate patches in `connected_patches`, which moved the centroid and with it where
  the extent cap centred the grid.
- **An exact brush-entity filter.** Model 0 is the world; faces belonging to any other model
  belong to a brush entity — a door, a lift, a train. `decal_atlas` currently approximates this by
  checking the decal message's entity index is 0 (`decal_strip::is_world_decal`). Model 0 is the
  precise version, and it matters more here: a coordinate on a door is only true while the door is
  where it was.

Face area is also available, which makes "prefer a large wall in a quiet corner" expressible
directly rather than inferred from decal density.

### Filtering

Not every face can hold a decal. At minimum, skip:

- **Sky** — `TEX_SPECIAL` (flag `1`) in texinfo, and texture names beginning `sky`.
- **Water and other liquids** — names beginning `!`, `*` or `water`.
- **Trigger and clip brushes** — `aaatrigger`, `clip`, `nodraw`-equivalents.
- **Faces below a minimum area**, which cannot hold a 4-unit-radius decal clear of its edges.
- **Anything not in model 0.**

---

## What the map is worth on its own (stage 3, built 2026-08-27)

`Bsp::face_candidates` lays a 32-unit grid across every world face, keeps samples 8 units clear of
the polygon's edges, and lifts each one 2 units off along the face's outward normal — open space,
inside the 4-unit projection reach, so `decal_draw_point` lands it back on the face it came from.
Faces are taken largest first, capped at 48 samples each so one hangar floor cannot spend the
budget, and stopped at 12,000 overall.

**A sample that lands inside solid is dropped at the source.** Between 0.5% and 21% of raw samples
do, measured across five DoD maps: faces sealed against another brush, faces pointing into the void
outside the playable hull, thin geometry where two units crosses into the next brush. Every one of
those is the projection bug waiting to happen — a decal aimed inside a wall is drawn on whichever
face the engine's walk reaches, which can be the one in shot — so the sampler never offers them.
The pipeline would reject them anyway; doing it here makes the supply figure mean what it says.

Measured with `native/src/bin/probe_map_candidates` across **all 219 installed maps**:

| | |
|---|---|
| maps parsed | 219 / 219 |
| candidates per map | min **6,561**, mean 11,835, cap 12,000 |
| maps reaching the 1,028 a 4096 ring needs | **219 / 219** |
| distinct leaves the candidates occupy | mean 323 |
| projection hit rate (strided sample) | **100%** on every map |
| scan cost | ~15 ms per map |

The smallest supplier is `dod_tensions` at 6,561 candidates from 1,484 world faces — still six times
what a maximum ring asks for. Scan cost turned out to be a non-issue; what costs time is the camera
test afterwards, which is why the source is reached lazily rather than sampled on every demo.

This is an **upper bound**: it is what the map holds before any camera has looked at it. What
survives the in-clip camera test is the number that decides a sweep, and it needs a demo to
measure.

### What it is worth in a capture

A/B over a 64-demo sample spread across the library, at **ring 4096 with no coordinate store** —
which is the new-map case, and the only configuration where the proven sources are stretched:

| | before | after |
|---|---|---|
| demos reaching a full 1,028-position sweep | 57 / 60 | **60 / 60** |
| smallest sweep any demo got | **293** | **1,028** |
| demos with a position on camera | 0 | **0** |
| demos where the map source fired | — | 16 / 60 |
| camera-safe map candidates when it did | — | mean 7,210, min **4,832** |

The three demos that fell short before are the shape of the problem the source was built for.
`monday-wsod25_r07_m3_h1.dem` laid **55** tiles and kept 20 of them, and the map carried it from 293
positions to a full 1,028. `probe_fine_in.dem` kept **zero** of its 76 tiles — every one in shot —
and the map supplied 8,895 camera-safe candidates on its own.

Note what the "map fired" row does *not* say. It fired on 16 demos but only 3 of those were short
without it; on the other 13 the proven sources were merely a few positions from the target and the
map closed a gap that the floor-path source would otherwise have closed with spots under the
player's own feet. That is a quality improvement rather than a count one, and the count cannot show
it.

At ring 256 the change is invisible on this library: **60/60 demos reach a full 68-position sweep
before and after**, so the map source is never reached. That is the intended shape — headroom, not
a dependency — and it is also why `DOD_FLUSH_MAP_GEOMETRY_ONLY` exists.

### The map source standing entirely on its own

Forced with `FLUSH_MAP_ONLY` (the offline twin of the capture gate), every source drawn from the
match refused — no harvested decals, no tiled planes, no store, no floor path, and no spawn-floor
last resort. Same 64-demo sample, ring 256:

| | |
|---|---|
| demos reaching a full 68-position sweep | **60 / 60** |
| smallest sweep | 68 — every demo full |
| demos with a position on camera | **0** |
| demos whose source was anything but the map | **0** |
| camera-safe map candidates | mean 8,428, min **4,832** |

Every demo in the sample can be swept from map geometry alone, from an empty store, with nothing on
camera. That is the structural half of the claim; the half it cannot make is what the decals look
like on screen, which is what the in-game gate run is for — see the masked-texture caveat above for
the specific artefact to watch.

### Verified in game, 2026-08-27

Run under `DOD_FLUSH_MAP_GEOMETRY_ONLY` on `wsod25-po_r2_qf-m00cat_clinic_m2_lenn_h2.dem`, 15
highlights, ring 256 — the same demo and configuration as the two runs already watched, so the only
variable is where the coordinates came from.

| | |
|---|---|
| positions | **68 of 68**, every one from the map's own faces |
| candidates sampled / camera-safe | 12,000 / 5,365 |
| decals injected | 4,080 across 15 bursts |
| takes | 15/15 on disk, 15 renderable |
| playback | 429.8s |
| decal grids seen on camera | **none** |
| `SZ_GetSpace` | 8 — identical to both prior runs |
| `svc_bad`, engine errors | 0 |

**No clearance notice fired.** The two earlier runs both logged positions closer to the camera than
the 900-unit preference (267 and 203 units); this one had none — map-derived spots sit further from
the lens than harvested ones do, which is what you would expect from a source that is not drawn from
where people fought. That is the first evidence the map source is not merely *adequate* but
*better-placed* than the ones it backs up.

The masked-texture artefact did not appear. That is one map and one camera path, so it remains a
live caveat rather than a closed one.

## The second prize: real occlusion

Everything above treats the BSP as a supply of coordinates. It is also the only thing that can
answer the question the flush actually cares about — **is this spot hidden from the camera?** —
and that is arguably worth more.

The current test (`decal_strip::resolve_flush_positions`, `hidden`) rejects a candidate that falls
inside a 40-degree cone of any sampled camera within 1800 units, **with no occlusion test at all**.
As recorded on #60: a wall two rooms away passes the cone test happily. It errs in both
directions, but the expensive one is over-rejection — on the starving demos, most of those 32,000
discarded tiles are behind walls, and they are exactly the spots wanted.

Two mechanisms, in cost order:

> **What actually happened (2026-08-26):** the trace was kept and the PVS shortcut was NOT. They
> disagreed — at ring 4096, 200 in-clip frames on one demo had a clear line of sight to positions
> PVS had called invisible. One of the two is wrong and nothing short of the game can say which, so
> PVS no longer grants safety; it is measured alongside as `pvs_agrees_hidden` for the in-game check
> to settle. Dropping it cost nothing measurable, because the frame cone already rejects most
> candidates before a trace is reached. The design below is preserved as written.

- **PVS as a hard guarantee.** `LUMP_VISIBILITY` is a run-length-encoded potentially-visible set
  per leaf. If a candidate's leaf is absent from a camera leaf's PVS, nothing in that leaf is
  visible from anywhere in the camera's leaf — the engine cannot render it. Take the union of PVS
  across the leaves the cameras actually occupy (they cluster hard; expect a few hundred distinct
  leaves at most) and any candidate outside that union is provably hidden for one leaf lookup.
- **A hull trace for the rest.** For candidates inside the union, trace eye → point through the
  BSP the way `SV_RecursiveHullCheck` does, on hull 0. Blocked means hidden. O(tree depth) per
  trace rather than O(faces), which is what makes it affordable at all.

Ordering matters: the naive form is every camera sample times every candidate, on the order of
10^9 operations. PVS first eliminates most candidates for free, camera positions dedupe heavily,
and traces early-out on the first camera that can see the point.

Needs four more lumps: `NODES` (5) for the point-to-leaf descent, `LEAVES` (10), `VISIBILITY` (4),
and `CLIPNODES` (9) for the trace.

**Caveats, all of which matter:**

- **Masked textures.** Grates, fences and `{`-prefixed textures are solid in the BSP and
  see-through on screen. A trace would call a spot hidden that a viewer can see straight through.
  They must be treated as non-blocking.

  **Stage 3 raises the exposure here and does not change the mechanism.** The hazard is the
  *occluder*, not the candidate's own face, so it cannot be fixed by filtering which faces get
  sampled. What changes is the odds: every earlier source draws from ground people fought over,
  where a spot behind a fence is unlikely to be the furthest-from-camera candidate available, while
  the map source deliberately reaches into places nobody went — including the far side of fences.
  Nothing has been seen in game (the 2026-08-27 run at ring 4096 went through the same trace and
  came back clean), and the clearance ranking prefers distant spots, which helps. But this is the
  specific artefact to watch for when the map source is first watched under
  `DOD_FLUSH_MAP_GEOMETRY_ONLY`: a grid of decals visible *through* something.
- **Brush entities.** Tracing the world model alone ignores doors and lifts, so a spot hidden
  behind a closed door reads as visible. That fails in the safe direction — a usable spot is
  rejected rather than an exposed one accepted.
- **This corrects the cone test, it does not retire it.** The rule becomes *reject if inside the
  cone AND actually unoccluded*, which is both more permissive and more accurate than today.

It also gives the first real verification of the flush's central safety claim. Today the strongest
statement available is "outside the camera's cone". With this the report could say "N of 68
positions were provably occluded from every camera in every clip" — computable without loading the
game.

## The assumption that needs proving in game

Everything here rests on one question: **does the engine create a decal on a surface the player is
neither near nor looking at?**

From the Xash3D FWGS reimplementation (clean-room, *not* Valve source — same caveat that applies
to every engine claim on #60), `R_DecalShoot` recurses `R_DecalNode` from the root of the **world
model's** BSP tree. That is a geometric walk rather than a rendering one, which suggests it does
not consult the PVS. It is also why decals on brush entities take a different path, corroborating
the model-0 rule above.

Supporting evidence already exists but does not settle it: the validated sweep (in game,
2026-08-24) places decals 900+ units away, routinely elsewhere in the map, and it worked. But
"elsewhere in the map" is not "a sealed room across the level", and PVS regions in GoldSrc are
generous enough that the existing spots may well have been inside one.

**This is exactly the class of assumption that reports green structurally and fails in game.** It
cannot be settled from the bytes.

---

## The structural test that *does* work here

This feature almost never admits a meaningful test without loading the game. This time it does.

We already hold thousands of coordinates per demo that the engine provably accepted — every
harvested decal is one. **If the parser is correct, nearly all of them should land within a couple
of units of some world face.** Running that check across the 85-demo sample validates the lump
offsets, the surfedge winding, the vertex indexing and the coordinate space in a single pass. A
high hit rate confirms the parser end to end; a low one localises the bug before anyone starts the
engine.

It also sidesteps a question that would otherwise need answering: whether the demo header's
`map_checksum` can be recomputed from a local `.bsp` to confirm the file matches the demo's map
build. (The engine's checksum algorithm would need verifying first — it is not simply a CRC32 of
the file.) The cross-check answers "is this the right map" empirically instead, which is both
cheaper and stronger.

---

## How it integrates

**It does not replace the coordinate store — it becomes its best contributor.** Parse the map
once, generate coordinates, write them into the same per-map store under the same
`(map name, checksum)` key.

Nothing on the consuming side changes. Tiling, camera filtering and clearance ranking all read the
store exactly as they do now, and `FlushSource::MapAtlas` already exists as a label.

This also strengthens the distribution idea recorded on #60: BSP-derived stores are *complete* and
identical for everyone on a given map build, which makes them genuinely worth shipping with the
app and refreshing through the updater — as opposed to accumulating locally over time.
`decal_atlas::load_all` already accepts read-only seed directories alongside the writable store
for exactly this, and the file carries a `format` version field.

---

## What an unlimited coordinate supply makes possible: stop pinning the cvar

Once positions are no longer scarce, the sweep size stops being a budget decision, and that
reopens something the whole design currently works around.

`r_decals` is clamped to `MAX_RENDER_DECALS` (4096), so **a sweep of 4096 turns a full revolution
whatever the cvar is set to** — any smaller ring simply gets swept several times over. That would
let the pipeline stop pinning the cvar entirely, which is worth more than the convenience:

- **It removes a silent failure mode.** The design currently depends on the precondition recorded
  on #60 — "the demo pins the cvar itself, so `r_decals` must not be set anywhere else". Anything
  that sets it after our init command (an autoexec, a config, a stray console command) leaves the
  sweep under-clearing with nothing to report. Sweeping the maximum makes the value irrelevant.
- **It stops the ring evicting decals during the clip.** A small ring cuts both ways: at 256, a
  busy 20-second clip with grenades and sustained fire can reach the limit, and past it the ring
  starts eating its own — bullet holes vanishing on screen mid-action. Pinning small was only ever
  a cost optimisation, and it has this cost.

**What it needs, in order of how binding it is:**

1. **~1028 distinct positions** (4096 + 16 margin, at `DECALS_PER_POSITION` = 4). BSP-derived
   coordinates supply this easily; some demos already reach 11,000-35,000 camera-safe tiles.
2. **~1028 carrier frames per burst** — the real constraint, and the one still unmeasured.
   Injection is capped at 4 per frame into packets under 1024 bytes, so a full sweep needs ~10
   seconds of demo time at 100fps ahead of each clip. Gaps are usually minutes, but back-to-back
   and chained clips will report `bursts_short` far more often than the current 272-injection
   burst does.

   Position supply is no longer the question: measured across the 85-demo sample after stage 4,
   **every demo has at least 3,916 camera-safe candidates** (mean ~14,000), against the 1,028 a
   4096 sweep needs. Stage 6 is therefore reachable without stage 3.
3. **An in-game unknown**: 4 decals per frame across ~1028 consecutive frames is roughly 15x
   today's burst, arriving during fast-forward. Whether the engine ingests that without hitching
   is not answerable from the bytes.

### Built, measured, and off by default (2026-08-26, `35fdee9`)

No adaptive machinery was needed in the end. `decal_ring_limit` was already configurable, so the
whole change is that a sweep **at** `MAX_RENDER_DECALS` stops pinning the cvar — below the ceiling
it pins exactly as before. Set `decal_ring_limit = 4096` and `r_decals` is left alone entirely.

Measured across the 85-demo sample at FOV 105, with occlusion:

| | ring 256 | ring 4096 |
|---|---|---|
| demos reaching a full sweep | 85 / 85 | **85 / 85** |
| positions per sweep | 68 | 1,028 |
| demos with a position on camera | 0 | **0** |
| bursts short of a full sweep | 0 of 668 | **1 of 668** |
| smallest camera-safe pool | — | 3,852 (needs 1,028) |

The carrier-frame limit that looked like the binding constraint bites exactly once in 668 bursts.

**The default stays 256.** Not because 4096 measured worse — it did not — but because the one thing
measurement cannot speak to is how the engine ingests 16x the injected messages during
fast-forward, and the in-game validation on 2026-08-24 was done at a small ring. The first capture
watched in game should be the configuration closest to the one already known to work. After that,
4096 is a one-value change and the pin disappears on its own.

Unchanged either way: `FDECAL_PERMANENT` decals are skipped by `R_DecalAlloc`, so no sweep of any
size clears those.

## Staging

1. **BSP lump parser.** Pure, read-only, no I/O beyond reading the file. Planes, vertices, edges,
   surfedges, faces, texinfo, textures, models.
2. **Cross-validate against harvested coordinates** across the 85-demo sample. Parser correctness,
   no game required. **Stop and look at the result here** — if the hit rate is poor, everything
   after this is built on sand.
3. **Face → candidate coordinates.** Sample inside the polygon, inset from the edges, apply the
   texture and model filters, prefer larger faces.
4. **Occlusion.** `NODES`, `LEAVES`, `VISIBILITY`, `CLIPNODES`; PVS union first, hull trace for the
   remainder. Replaces the cone-only test with "in cone AND unoccluded", and lets the report state
   how many chosen positions are provably hidden from every camera.
5. **Feed the coordinate store**, keyed identically to the demo-harvested coordinates.
6. **Adaptive sweep size.** With positions no longer scarce, sweep 4096 where the supply and the
   gap allow and stop pinning `r_decals` at all; otherwise pin the largest ring the positions can
   turn. Needs the pin moved out of `builder.rs` into the pre-pass.
7. **In-game check** — which is outstanding for the flush as a whole regardless.

Steps 3 and 4 are independent: occlusion improves every existing source immediately, without any
BSP-derived coordinates existing, so it can land first if stage 2 says the parser is sound.

---

## Open questions

- **PVS dependence** (above) — the one that decides whether this works at all.
- ~~**Does a decal on a face the player has never rendered still allocate a ring slot?**~~
  **Answered yes, in game, 2026-08-27.** Run behind `DOD_FLUSH_PVS_ONLY`, which restricts placement
  to positions whose drawn surface falls outside the union of the capture's camera PVS — faces the
  engine provably never renders. 68/68 positions, 4080 decals, ring 256, 15 clips, and no old
  decals survived into any of them; qconsole clean, 15/15 takes renderable. `R_DecalShoot` claims
  its ring slot at shoot time and rendering is a separate later pass. This is the assumption the
  whole BSP direction rested on, and it is the one that made stage 3 worth building: a dump site
  does not have to be a surface anyone could ever see, only a *surface*.
- **Displacement of the fitted plane.** Harvested-decal planes sit ~0.5 units proud of the true BSP
  plane (measured, #60). BSP faces give the true plane, so the existing "place on the fitted plane,
  never offset more than ~3 units" rule may need revisiting for this source — probably in our
  favour, but it should be re-measured rather than assumed.
- **Map availability.** A map that is not installed means no `.bsp`, which falls back to today's
  behaviour — correct, but the log should say so. Measured 2026-08-26: of the 17 maps referenced by
  a 442-demo library, 16 were present and one (`dod_railyard_s9a`) was not, so this is a real but
  narrow gap.

  The KTP league mirrors the whole `/dod` folder at `https://fastdl.ktpdod.com/dod/...`, path for
  path, so a missing map is `.../dod/maps/<name>.bsp`. Whether the pipeline should *fetch* one
  itself is a separate decision — it would put network access in the capture path, which wants to
  be opt-in and off by default — but the option exists, and it also makes a **pre-built,
  distributable coordinate store** practical: BSP-derived coordinates are identical for everyone on
  a given map build, so they could be generated once for every map on the mirror and shipped
  through the updater rather than accumulated locally. `decal_atlas::load_all` already takes
  read-only seed directories for exactly that.

  **Verifying a downloaded map is the right build** is already solved by stage 2: run
  `validate_bsp` against a demo that uses it. The correct build scores ~97-99% within 1 unit; a
  wrong one collapses. That avoids having to reimplement the engine's `map_checksum` algorithm.
  `dod_railyard_s9a` scored 99.4% and 97.5% on its two demos.
- **Cost.** Map parsing is once per map per session and cacheable through the store, so it should
  not touch the per-demo pre-pass budget (~5s per demo today).

---

## Related

- `native/src/patch/decal_strip.rs` — the flush, tiling, and position selection.
- `native/src/patch/decal_atlas.rs` — the per-map coordinate store this would feed.
- `native/src/patch/decal_probe.rs` — the measurement rig, and where the plane-fitting helpers
  came from before they moved into `decal_strip`.
- `docs/goldsrc_dod_quirks.md` — engine constraints.
- Issue #60 — the full flush history, including why the `r_decals` cvar approach cannot work.

---

## Map identity: is this the map the demo was recorded on?

Added 2026-08-26. Not part of the seven-stage plan above — it fell out of the BSP work, and it
answers a question the rest of the pipeline had been assuming away.

A demo header carries the map's name **and its checksum**. That checksum turns out to be
computable from the map file:

    CRC-32 (reflected IEEE, init 0xFFFFFFFF) over lumps 1..14 in header order,
    LUMP_ENTITIES (0) excluded, and left UNFINALISED — no closing XOR.

The entities lump is left out on purpose: entities are what a server operator edits, so excluding
them lets a tweaked server run without every client reporting a mismatch. It also makes the
checksum answer exactly the question worth asking here — is this the same *geometry*.

The missing final XOR is the part that is easy to get wrong. `CRC32_Final` is not called on this
value before it is written, so a textbook CRC-32 misses by exactly `0xFFFFFFFF`.

Verified against the whole library: **397 of 397 first-person demos match their own map file.**
The 45 HLTV demos zero the field — nothing in an HLTV demo records a map build — so they are
reported as *unverifiable*, never as wrong.

### Why it matters here

Three states, not two:

| state | consequence |
| --- | --- |
| missing | the demo cannot be played at all |
| **wrong build** | it plays, and every coordinate taken from the map refers to a different world |
| matching | fine |

The middle one is the reason this exists. `_b2` against `_b3e`, or a recompile that kept its name,
gives a map that parses perfectly and describes somewhere else. Occlusion would answer
confidently and wrongly, which is precisely the failure mode this feature cannot show in its
own output. `load_map` now refuses a mismatched map and falls back to the cone.

It also fixed a real defect in the atlas: `MapKey::from_header` filed every HLTV demo under
`<map>_00000000`, separate from the same map's first-person bucket. Those now key off the local
map's checksum.

### Fetching

`patch::map_fetch` downloads from the KTP mirror, which lays every file out at its path under
`dod/` — so the URL is a pure function of the map name and there is no index to keep in step.

The order is the whole point: download to a scratch file, read the checksum out of **what actually
arrived**, and only then move it into the library. A mirror can serve an error page with a 200,
and a wrong build installed automatically is exactly what the checksum work exists to prevent.
An existing file is moved aside as `<name>.<crc>.bsp.bak`, never overwritten.

Still open: the `.res` file lists a map's models, sounds and sprites, which would make this a full
dependency fetch rather than a map fetch. WADs are not reliably in `.res`, but the TEXTURES lump
already names them, so they can come from the map itself.

### Tools

- `native/src/bin/check_maps <demo-or-folder> [maps-dir] [--fetch]` — reports, or fetches.
- The Master Queue banner (`desktop-studio/src/map_warnings.js`), grouped by map rather than by
  demo: twenty demos short of one map is one download.
