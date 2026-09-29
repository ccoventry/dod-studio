// patch/decal_strip/mod.rs
// Decal hygiene for capture demos: keeps walls clean at the start of every
// recorded clip without reloading the demo.
//
// ── Why r_decals alone cannot do this ────────────────────────────────────────
// GoldSrc stores decals in a fixed pool with a single rotating index:
//
//     limit = min(r_decals, MAX_RENDER_DECALS)
//     if (gDecalCount >= limit) gDecalCount = 0;
//     pdecal = &gDecalPool[gDecalCount++];
//     R_DecalUnlink(pdecal);          // the ONLY path that clears an old decal
//
// A decal is removed *only* when the ring index lands on its slot. The cvar
// never sweeps anything — it just bounds how far the index may travel before
// wrapping. Lowering r_decals mid-demo therefore strands every decal sitting in
// a slot >= the new limit: the index can no longer reach them, so they stay on
// the wall permanently while new decals churn through the small surviving
// window. (That asymmetry is exactly what you see in game when dropping
// r_decals from 5555 to 1.) The clean walls at demo load come from
// R_ClearDecals() on level load, not from the cvar.
//
// ── What this module does instead ────────────────────────────────────────────
// Two complementary passes over the demo's own byte stream:
//
//  1. STRIP — replace decal-creating messages outside the capture windows with
//     SvcNop, so the fast-forwarded stretches between clips contribute no
//     buildup at all. SvcNop is one byte on the wire and demo_writer recomputes
//     payload lengths, so no manual offset math is involved.
//
//  2. FLUSH BURST — pin r_decals to a modest ring size, then inject exactly
//     that many synthetic decals into the gap before each clip. That walks the
//     ring index a full revolution, unlinking every real decal still on a wall.
//     They are spread across many positions rather than stacked at one (see
//     `resolve_flush_positions` and MAX_OVERLAP_DECALS below — a stack stops
//     advancing the ring after the sixth), each ranked by how far it stays from
//     every in-clip camera, so they land where the capture never looks.
//
// Pinning the ring small is what makes the burst cheap: a sweep costs
// `ring_limit` injections regardless of how many decals are actually out there,
// so 256 keeps a full flush at ~2KB of injected messages instead of the ~36KB a
// 4096-slot ring would need.
//
// ── Layout ───────────────────────────────────────────────────────────────────
// This file is the clean itself: its options, its stats and the pass that
// strips and injects. The rest is split by what it answers:
//
//  - `geometry`  — decal wire layouts, plane fitting and tiling.
//  - `survey`    — the read-only walk that harvests decals and cameras.
//  - `placement` — where the burst goes, and what a camera can see.
//  - `flush_job` — the batch-pipeline pre-pass: capture config in, scratch
//                  demo out.

mod flush_job;
mod geometry;
mod placement;
mod survey;

pub use flush_job::{
    CleanedSource, capture_fov, capture_fov_from_init, capture_fov_resolved, on_screen_half_angle,
    prepare_flushed_source, ring_limit, ring_limit_from_game_config, ring_limit_from_init,
};
pub use placement::{FlushSource, VisibilityBasis};
pub use survey::DEFAULT_LEAD_SECONDS;

// Shared with `decal_probe`, which measures on the same surfaces this places on.
pub(super) use geometry::{
    build_world_decal, cluster, connected_patches, decal_texture_index, distance, extent,
    tangent_axes,
};
pub(super) use survey::{frame_ordinals, survey};

use placement::{Visibility, atlas_key, load_map, resolve_flush_positions};
use survey::{FALLBACK_LEAD_FRAMES, deadline_before, in_window};

use super::decal_atlas;
use dem::open_demo_from_bytes;
use dem::types::{
    ByteString, ConsoleCommand, EngineMessage, Frame, FrameData, MessageData, NetMessage,
};

/// Temp-entity `entity_type` values that place a persistent decal onto a wall
/// or world surface: bullet holes, grenade scorch marks, generic BSP/world
/// decals, and player spray logos.
///
/// Note TE_PLAYERDECAL (112) belongs here: it is the spray-paint logo message
/// (`impulse 201`), writing a player index plus a position and decal index to
/// stamp that player's logo onto a surface. It is wall clutter in exactly the
/// sense this pass exists to remove.
const WALL_DECAL_ENTITY_TYPES: &[u8] = &[13, 104, 109, 112, 116, 117, 118];

/// TE_PLAYERDECAL — tracked separately in the stats so sprays are visible as
/// their own number rather than lost among bullet holes.
const TE_PLAYERDECAL: u8 = 112;

/// Distance from a standing player's origin down to the floor: the origin sits
/// at the centre of a 72-unit hull, so the feet are 36 below it.
const ORIGIN_TO_FLOOR: f32 = 36.0;

/// The engine's own `MAX_OVERLAP_DECALS`. `R_DecalCreate` counts how many
/// existing decals a new one would overlap, and once that reaches this many it
/// recycles one of them instead of allocating:
///
/// ```c
/// pold = R_DecalIntersect( decalinfo, surf, &count );
/// if( count < MAX_OVERLAP_DECALS ) pold = NULL;
/// ```
///
/// `R_DecalAlloc` only walks the ring when handed NULL, so a recycled decal
/// does NOT advance `gDecalCount`. This is why a flush burst must be spread
/// across distinct positions: piling every decal on one spot stops advancing
/// the ring after the sixth, and the sweep silently accomplishes nothing.
pub const MAX_OVERLAP_DECALS: usize = 6;

/// Flush decals to place at each distinct position. Kept below
/// `MAX_OVERLAP_DECALS` so every one of them allocates a fresh ring slot.
pub const DECALS_PER_POSITION: usize = MAX_OVERLAP_DECALS - 2;

/// Minimum spacing between two flush positions, so the engine cannot see them
/// as overlapping.
///
/// `m_Size` for the small bullet hole was later measured at ~4 units, and that
/// is the decal's own radius — two overlap only within ~8 units of each other.
/// This was 28 when the footprint was a guess; it now sits at 1.5x the measured
/// overlap distance, which is what lets a tiled grid at `TILE_PITCH` survive the
/// spacing filter instead of being decimated by it.
const MIN_POSITION_SPACING: f32 = 12.0;

#[derive(Debug, Clone)]
pub struct DecalCleanOptions {
    /// Blank decal messages outside the capture windows.
    pub strip_outside_windows: bool,
    /// Inject ring-sweeping decal bursts ahead of each capture window.
    pub flush_burst: bool,
    /// Value r_decals is pinned to. The burst size follows from this: a full
    /// ring revolution is what guarantees every occupied slot gets unlinked.
    /// Must NOT be changed anywhere else in the demo — lowering it later
    /// strands decals in the slots above the new limit.
    pub ring_limit: u32,
    /// Extra injections beyond `ring_limit`. A decal spanning several surfaces
    /// consumes one pool slot per surface, so the sweep is deliberately
    /// over-provisioned rather than counted 1:1 against messages.
    pub burst_margin: usize,
    /// Cap on synthetic decals added to any single network packet, so injection
    /// never meaningfully grows a frame the engine already sized.
    pub max_per_frame: usize,
    /// Finish the burst this long, in **seconds of demo time**, before the
    /// capture window opens.
    ///
    /// Seconds, not frames, for two reasons. A frame count is not a duration —
    /// frame records are not evenly spaced, and there are ~4.4 of them per
    /// rendered frame, so the flat `300` this used to be was worth a median of
    /// 0.6s across a real library rather than the ~3s it read as. And the
    /// margin has a job that is measured in time: the engine has to actually
    /// ingest the burst — 272 decals at a 256 ring, 4,112 at the maximum — and
    /// finish turning the ring before the first recorded frame.
    ///
    /// Resolved by walking the frames' own timestamps, per the "Pure Float
    /// Timestamps" rule in `docs/app_architecture.md`. Never `seconds * fps`.
    pub lead_seconds: f32,
    /// Emit `r_decals <ring_limit>` as a console-command frame at playback
    /// start, making a patched demo self-contained for testing.
    pub inject_r_decals_command: bool,
    /// Hand-picked flush coordinate, overriding spawn detection.
    pub flush_coord: Option<[f32; 3]>,
    /// Hand-picked decal texture index for the flush burst, overriding the
    /// harvested one. Useful when a demo contains no bullet-hole decal to
    /// borrow a small texture from and would otherwise fall back to a large
    /// grenade scorch.
    pub flush_texture_index: Option<u8>,
    /// Vertical drop applied to the settled spawn origin to reach the floor.
    /// Only used when the demo yielded no real decal to anchor to.
    pub floor_drop: f32,
    /// Consecutive on-ground frames with a stable Z required before a spawn
    /// position is trusted, so a player still falling from an elevated spawn
    /// point is never sampled mid-air.
    pub grounded_settle_frames: usize,
    /// Clearance from every in-window camera position that flush positions are
    /// *preferred* to have. No longer a hard filter: positions are ranked by
    /// clearance and the best are taken, so a demo whose every surface passes
    /// closer than this still gets a full sweep rather than nothing. Falling
    /// short of it is reported.
    ///
    /// Keeping decals off screen is the cone test's job (`visibility_cone_
    /// degrees`); clearance is the margin against that test's own blind spot,
    /// which is that cameras are sampled every fourth frame and a fast turn
    /// between two samples is not seen. It still gates the last-resort
    /// single-position fallback, where there is no spread to rank.
    pub min_camera_clearance: f32,
    /// Half-angle of the cone treated as "on screen" for the line-of-sight
    /// test.
    ///
    /// The pipeline derives this from the capture FOV and frame shape — see
    /// `on_screen_half_angle`. The default here is deliberately wide rather
    /// than accurate, because a caller that does not know its own FOV is
    /// better served by rejecting too many spots than by putting one in shot.
    pub visibility_cone_degrees: f32,
    /// Directory holding the game's `.bsp` files. With one the flush can tell
    /// a wall from a sightline; without it the frame cone is all there is.
    pub maps_dir: Option<std::path::PathBuf>,
    /// Where this run's proven world coordinates are pooled per map, and read
    /// back from. `None` keeps the pass self-contained — it uses only what this
    /// demo proves, which is what the CLI and the probe rig want.
    ///
    /// Writing happens here and nowhere else.
    pub atlas_dir: Option<std::path::PathBuf>,
    /// Additional read-only coordinate stores, unioned in at load and never
    /// written to. Intended for a store shipped with the app and refreshed by
    /// the updater, kept separate so an update can replace it wholesale without
    /// touching what the user's own captures have harvested.
    pub atlas_seed_dirs: Vec<std::path::PathBuf>,
    /// Range past which the on-screen test stops caring. `INFINITY` by default,
    /// i.e. it always cares.
    ///
    /// This was 1800 units, on the reasoning that a single decal that far away
    /// is not readable on screen. The flush does not place single decals: it
    /// places a grid of them, four to a position, and a grid reads at distances
    /// one dot does not. Anything past the cutoff was not merely untested — it
    /// was scored SAFE, because the test returned "not on screen" for it.
    /// Removing the escape cost no candidates on the demo that exposed the
    /// projection bug (a full 1028-position sweep either way) and about six
    /// seconds of tracing, which is not a trade worth making for the one defect
    /// this pass must never introduce.
    pub visibility_max_distance: f32,
    /// Keep every camera sample rather than the usual stride, and hand back
    /// the chosen positions with them. Diagnostics only — off in the pipeline.
    pub collect_diagnostics: bool,
    /// Place the sweep ONLY where leaf visibility says the engine never renders
    /// it — the drawn surface sitting outside the PVS union of every camera
    /// leaf, not merely occluded from them.
    ///
    /// An experiment, not a feature. It answers the one question BSP-derived
    /// coordinates depend on: does a decal on a face the player never renders
    /// still allocate a ring slot? Allocation is what turns the ring; drawing
    /// is irrelevant to the sweep. If it does, a map can be seeded from its
    /// own geometry and the coordinate store stops being a cold-start
    /// dependency. If it does not, a sweep placed in unvisited geometry turns
    /// fewer slots than it reports — silently, which is this feature's
    /// signature failure.
    ///
    /// Tests the DRAWN surface, not the coordinate. A coordinate inside solid
    /// lands in a leaf no PVS row contains, so asking about the coordinate
    /// would call every buried candidate "never rendered" for the same reason
    /// the old camera test called them hidden.
    pub require_pvs_hidden: bool,
    /// Experiment gate: place the sweep from the map's own faces ALONE, with
    /// every source drawn from the match skipped.
    ///
    /// The map source is normally a fallback, reached only when a demo's own
    /// decals and the coordinate store between them cannot fill a sweep — so on
    /// a mature library it never runs, and the one thing that can judge where it
    /// puts decals is the game. This forces it, which is how a source meant for
    /// maps nobody has captured yet gets tested on a map somebody can watch.
    pub map_geometry_only: bool,
}

impl Default for DecalCleanOptions {
    fn default() -> Self {
        Self {
            strip_outside_windows: true,
            flush_burst: true,
            ring_limit: 256,
            burst_margin: 16,
            max_per_frame: 4,
            lead_seconds: DEFAULT_LEAD_SECONDS,
            inject_r_decals_command: true,
            flush_coord: None,
            flush_texture_index: None,
            floor_drop: ORIGIN_TO_FLOOR,
            grounded_settle_frames: 10,
            maps_dir: None,
            atlas_dir: None,
            atlas_seed_dirs: Vec::new(),
            min_camera_clearance: 900.0,
            visibility_cone_degrees: 40.0,
            visibility_max_distance: f32::INFINITY,
            collect_diagnostics: false,
            require_pvs_hidden: false,
            map_geometry_only: false,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DecalCleanStats {
    pub temp_entity_stripped: usize,
    pub player_spray_stripped: usize,
    pub flush_decals_injected: usize,
    pub bursts_placed: usize,
    /// Windows whose gap had too little room to fit a full sweep. These clips
    /// are NOT guaranteed clean, so they are reported rather than silently
    /// under-flushed.
    pub bursts_short: Vec<(i32, usize, usize)>,
    /// Burst carrier frames that fall *inside* a clip being recorded.
    ///
    /// The sweep is supposed to turn the decal ring in the gap before a clip.
    /// A carrier landing inside an earlier clip's own record window turns the
    /// ring while that clip is being filmed, which can evict the bullet holes
    /// the firefight is putting up — decals vanishing mid-shot, the exact
    /// artefact this feature exists to remove.
    pub burst_frames_inside_clip: usize,
    pub flush_coord: Option<[f32; 3]>,
    pub flush_source: Option<FlushSource>,
    pub flush_texture_index: Option<u8>,
    /// Settled on-ground spawn origin, when one was found.
    pub spawn_reference: Option<[f32; 3]>,
    /// How far the flush coordinate ended up from that spawn reference.
    pub spawn_to_flush_distance: Option<f32>,
    pub harvested_decals: usize,
    /// Closest approach between the flush coordinate and any camera position
    /// inside a capture window.
    pub min_camera_distance: Option<f32>,
    /// Sampled in-window camera frames where the flush point falls inside the
    /// camera's cone. Must be 0 — anything else means the flush stack is on
    /// screen during a recorded clip.
    pub flush_on_camera_frames: usize,
    /// Total in-window camera samples the two figures above were measured over.
    pub camera_samples: usize,
    /// Distinct positions the burst was spread across.
    pub flush_positions: usize,
    /// Positions needed to place the whole burst without any spot exceeding
    /// the engine's overlap limit. If `flush_positions` is below this, some
    /// injected decals get recycled instead of turning the ring.
    pub flush_positions_wanted: usize,
    /// Tiles laid across the fitted planes, before camera filtering.
    pub tiled_candidates: usize,
    /// What the map's coordinate store held, gained and offers after this demo.
    pub atlas: crate::patch::decal_atlas::AtlasStats,
    /// Which map build the store was keyed on, when one was resolved.
    pub atlas_map: Option<String>,
    /// What the on-screen test was actually able to consult.
    pub visibility_basis: VisibilityBasis,
    /// World faces in the map, when one was loaded.
    pub map_faces: usize,
    /// Whether that map carries visibility data.
    pub map_has_vis: bool,
    /// Of the chosen positions, how many leaf visibility ALSO calls hidden.
    /// `None` when the map has no vis data.
    ///
    /// Placement is decided by line-of-sight traces alone. This is the second
    /// opinion, recorded rather than acted on: the two were caught disagreeing,
    /// and which is right is one of the things the in-game check settles.
    pub pvs_agrees_hidden: Option<usize>,
    /// How many of those were clear of every in-clip camera. A shortfall with
    /// these two close together means the demo offers little surface; a
    /// shortfall with a wide gap means the surface it has is all in shot.
    pub tiled_camera_safe: usize,
    /// Candidates sampled off the map's own world faces, and how many of those
    /// cleared every in-clip camera. Both stay 0 unless the proven sources fell
    /// short and the map source was actually reached — it is not sampled
    /// speculatively, because on a map with a populated store it is pure cost.
    pub map_candidates: usize,
    pub map_camera_safe: usize,
    /// The chosen positions and the in-clip camera samples they were judged
    /// against, populated only when `collect_diagnostics` is set.
    pub diagnostic_positions: Vec<[f32; 3]>,
    pub diagnostic_cameras: Vec<([f32; 3], [f32; 3])>,
}

/// Blanks every decal-placing message outside `keep_windows`, reporting
/// `(wall decals, player sprays)` removed.
///
/// An empty `keep_windows` strips the entire demo, which is what the offset
/// probe wants: a blank canvas so the only decals on a wall are the ones it
/// injected.
///
/// Only messages that PLACE a decal are stripped. SvcDecalName (36)
/// deliberately is not: it registers a decal name against an index (how a
/// custom player spray is announced) and places nothing. Blanking it does not
/// remove a decal — it destroys the texture lookup that decals referencing
/// that index still need, including any index this pass harvests for its own
/// flush burst.
pub(super) fn strip_decal_messages(
    demo: &mut dem::types::Demo,
    keep_windows: &[(i32, i32)],
) -> (usize, usize) {
    let (mut wall, mut spray) = (0usize, 0usize);
    let mut ordinal = 0i32;
    for entry in &mut demo.directory.entries {
        for frame in &mut entry.frames {
            ordinal += 1;
            if in_window(ordinal, keep_windows) {
                continue;
            }
            let FrameData::NetworkMessage(net_msg_box) = &mut frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &mut net_msg_box.1.messages else {
                continue;
            };
            for msg in messages.iter_mut() {
                let NetMessage::EngineMessage(eng) = msg else {
                    continue;
                };
                let strip_type = match eng.as_ref() {
                    EngineMessage::SvcTempEntity(te)
                        if WALL_DECAL_ENTITY_TYPES.contains(&te.entity_type) =>
                    {
                        te.entity_type
                    }
                    _ => continue,
                };
                if strip_type == TE_PLAYERDECAL {
                    spray += 1;
                } else {
                    wall += 1;
                }
                **eng = EngineMessage::SvcNop;
            }
        }
    }
    (wall, spray)
}

/// Why `clean_demo_decals` did not produce a cleaned demo.
///
/// The two arms are handled completely differently upstream, which is the whole
/// reason this is not a `String`. A `Failed` is reported to the user and the
/// capture carries on with the unflushed demo — a dirty wall is not worth
/// losing a batch over. A `Cancelled` is neither: the user asked for the batch
/// to stop, so nothing is reported and nothing carries on.
#[derive(Debug)]
pub enum DecalCleanError {
    Failed(String),
    Cancelled,
}

impl std::fmt::Display for DecalCleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecalCleanError::Failed(why) => f.write_str(why),
            DecalCleanError::Cancelled => f.write_str("cancelled by user"),
        }
    }
}

/// Strips decal messages outside `keep_windows` and injects ring-sweeping decal
/// bursts ahead of each window.
///
/// `keep_windows` are inclusive `[start, stop]` pairs in **frame-ordinal space**
/// — the same tick space `PatchJob::scheduled_commands` uses and `engine.rs`
/// compares against `frame_counter`, i.e. a 1-based count of every frame record
/// in file order. They should be the real record-start/record-stop ticks used to
/// schedule `mirv_recordmovie_start`/`stop`, not the wider highlight bounds.
///
/// These are NOT `Frame::frame` values. That field holds the engine tick, which
/// several frame records share, so the two spaces differ by roughly 2.6x on a
/// real demo — passing one where the other is expected silently targets the
/// wrong part of the demo.
pub fn clean_demo_decals(
    demo_bytes: &[u8],
    keep_windows: &[(i32, i32)],
    opts: &DecalCleanOptions,
    cancel: crate::patch::Cancel<'_>,
) -> Result<(Vec<u8>, DecalCleanStats), DecalCleanError> {
    // Checked at every stage boundary below. The stages are not equal: measured
    // on a 110MB, 730k-frame demo (`native/examples/flush_stage_timing.rs`),
    // the parse is ~1.1s and the whole clean ~4.8s, of which `write_to_bytes`
    // is ~3.0s, while survey, strip and
    // burst planning are 16-40ms each and resolve_flush_positions is ~330ms at
    // a 4096 ring. So the two that matter are the parse -- which this crate
    // cannot interrupt, only decline to start -- and the write, which is
    // interrupted from the inside by `write_to_bytes_cancellable`. The rest are
    // boundary checks because they are already short enough not to be felt.
    if cancel.requested() {
        return Err(DecalCleanError::Cancelled);
    }

    let mut demo = open_demo_from_bytes(demo_bytes)
        .map_err(|e| DecalCleanError::Failed(format!("Could not parse demo file: {}", e)))?;

    if cancel.requested() {
        return Err(DecalCleanError::Cancelled);
    }

    let mut stats = DecalCleanStats::default();

    let survey = survey(&demo, keep_windows, opts);
    stats.harvested_decals = survey.harvested.len();
    stats.spawn_reference = survey.grounded_origin;
    let texture_index = opts.flush_texture_index.or(survey.texture_index);
    stats.flush_texture_index = texture_index;

    // Distinct spots needed so every injected decal allocates a fresh ring slot
    // instead of being recycled as an overlap of one already placed.
    let burst_count = opts.ring_limit as usize + opts.burst_margin;
    let positions_wanted = burst_count.div_ceil(DECALS_PER_POSITION);

    // The map's own coordinate store. This demo's proven world coordinates go
    // in, the union of every demo ever processed for this exact map build comes
    // back out. A demo whose player never shot the quiet side of the map can
    // still flush there, because some earlier demo proved that surface exists.
    let mut atlas: Vec<[f32; 3]> = Vec::new();
    if let Some(dir) = &opts.atlas_dir {
        match atlas_key(&demo.header, opts) {
            Some(key) => {
                let (merged, astats) = decal_atlas::merge_and_save(
                    dir,
                    &opts.atlas_seed_dirs,
                    &key,
                    &survey.world_harvested,
                );
                stats.atlas = astats;
                stats.atlas_map = Some(format!("{} ({:08x})", key.name, key.checksum));
                atlas = merged;
            }
            None => crate::log_markdown(
                "⚠️ **Decal atlas skipped** — the demo header carries no usable map name, so \
                 there is nothing to key a coordinate store on.",
            ),
        }
    }

    // The map itself, when the caller told us where to find one. Used both to
    // decide what is genuinely hidden and, in time, to supply coordinates that
    // owe nothing to where anyone happened to shoot.
    if cancel.requested() {
        return Err(DecalCleanError::Cancelled);
    }

    let map = load_map(&demo, opts, &mut stats);
    let visibility = Visibility::new(map.as_ref(), &survey.window_cameras, opts);

    let placement = resolve_flush_positions(&survey, &atlas, &visibility, opts, positions_wanted);
    if cancel.requested() {
        return Err(DecalCleanError::Cancelled);
    }
    let flush_positions = placement.positions;
    stats.flush_coord = flush_positions.first().copied();
    stats.flush_source = placement.source;
    stats.flush_positions = flush_positions.len();
    stats.flush_positions_wanted = positions_wanted;
    stats.tiled_candidates = placement.tiled;
    stats.tiled_camera_safe = placement.tiled_safe;
    stats.map_candidates = placement.map_sampled;
    stats.map_camera_safe = placement.map_safe;

    if let (Some(pos), Some(reference)) = (flush_positions.first(), survey.grounded_origin) {
        stats.spawn_to_flush_distance = Some(distance(pos, &reference));
    }

    if !flush_positions.is_empty() {
        stats.min_camera_distance = survey
            .window_cameras
            .iter()
            .flat_map(|(eye, _)| flush_positions.iter().map(move |p| distance(p, eye)))
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Sampled in-window frames from which ANY flush position is on screen.
        // Non-zero means part of the spread is visible during a recorded clip —
        // the failure this whole selection exists to avoid.
        //
        // Measured with the same rule the selection used, geometry included.
        // Counting cone hits here while selecting on occlusion would report
        // hundreds of frames for positions that are all behind walls.
        stats.camera_samples = survey.window_cameras.len();
        stats.flush_on_camera_frames =
            visibility.on_camera_frames(&flush_positions, &survey.window_cameras);
        stats.pvs_agrees_hidden = visibility.pvs_agreement(&flush_positions);

        if opts.collect_diagnostics {
            stats.diagnostic_positions = flush_positions.clone();
            stats.diagnostic_cameras = survey.window_cameras.clone();
        }
    }

    // ── Pass 1: strip decal messages outside the capture windows ─────────────
    if opts.strip_outside_windows {
        let (wall, spray) = strip_decal_messages(&mut demo, keep_windows);
        stats.temp_entity_stripped += wall;
        stats.player_spray_stripped += spray;
    }

    if cancel.requested() {
        return Err(DecalCleanError::Cancelled);
    }

    // ── Pass 2: flush bursts ahead of each capture window ────────────────────
    if opts.flush_burst
        && let (false, Some(texture_index)) = (flush_positions.is_empty(), texture_index)
    {
        // Eligible carriers, in global frame order: parsed network frames
        // small enough that a handful of extra 9-byte messages cannot push
        // the packet near the engine's buffer ceiling. Built across every
        // entry at once so a window is never confined to one entry's frames.
        // Every frame with its ordinal and its own timestamp, so the burst
        // deadline can be a duration walked through real times rather than
        // a frame count standing in for one.
        let all_frames = frame_ordinals(&demo);
        let all_times: Vec<f32> = all_frames
            .iter()
            .map(|&(entry_idx, frame_idx, _)| {
                demo.directory.entries[entry_idx]
                    .frames
                    .get(frame_idx)
                    .map(|f| f.time)
                    .unwrap_or(f32::NAN)
            })
            .collect();

        let eligible: Vec<(usize, usize, i32)> = all_frames
            .iter()
            .copied()
            // Never inside a clip being recorded. The sweep's whole job is
            // to turn the decal ring in the gap BEFORE a clip; a carrier
            // landing inside one turns the ring while that clip is being
            // filmed, which can unlink the bullet holes the firefight is
            // putting up — decals disappearing mid-shot, the exact artefact
            // this feature removes. At a 256 ring the burst is 68 frames
            // and never reached that far; at the 4,096 maximum it is 1,028
            // and reached into an earlier clip on 10 of 85 demos.
            .filter(|&(_, _, ordinal)| !in_window(ordinal, keep_windows))
            .filter(|&(entry_idx, frame_idx, _)| {
                crate::patch::is_injectable_frame(&demo, entry_idx, frame_idx)
            })
            .collect();

        let mut used: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
        let mut plan: Vec<(usize, usize, usize)> = Vec::new();

        for &(window_start, _) in keep_windows {
            let deadline =
                deadline_before(window_start, opts.lead_seconds, &all_frames, &all_times)
                    .unwrap_or(window_start - FALLBACK_LEAD_FRAMES);
            let mut remaining = burst_count;

            // Walk backwards from the deadline so the sweep finishes as
            // late as possible — nothing after it can re-dirty a wall.
            let start_at = match eligible.iter().rposition(|&(_, _, ord)| ord <= deadline) {
                Some(p) => p,
                None => {
                    stats.bursts_short.push((window_start, 0, burst_count));
                    continue;
                }
            };

            for slot in (0..=start_at).rev() {
                if remaining == 0 {
                    break;
                }
                let (entry_idx, frame_idx, ordinal) = eligible[slot];
                if !used.insert((entry_idx, frame_idx)) {
                    continue;
                }
                if in_window(ordinal, keep_windows) {
                    stats.burst_frames_inside_clip += 1;
                }
                let take = remaining.min(opts.max_per_frame);
                plan.push((entry_idx, frame_idx, take));
                remaining -= take;
            }

            if remaining > 0 {
                stats
                    .bursts_short
                    .push((window_start, burst_count - remaining, burst_count));
            }
            if remaining < burst_count {
                stats.bursts_placed += 1;
            }
        }

        // Walk the position list so no spot receives more than
        // DECALS_PER_POSITION consecutive decals. Exceeding
        // MAX_OVERLAP_DECALS at one spot makes the engine recycle instead
        // of allocate, which stops the ring advancing and voids the sweep.
        let mut placed_here = 0usize;
        let mut pos_idx = 0usize;

        for (entry_idx, frame_idx, count) in plan {
            let Some(frame) = demo
                .directory
                .entries
                .get_mut(entry_idx)
                .and_then(|e| e.frames.get_mut(frame_idx))
            else {
                continue;
            };
            let FrameData::NetworkMessage(net_msg_box) = &mut frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &mut net_msg_box.1.messages else {
                continue;
            };
            for _ in 0..count {
                let pos = flush_positions[pos_idx % flush_positions.len()];
                messages.push(build_world_decal(&pos, texture_index));
                stats.flush_decals_injected += 1;
                placed_here += 1;
                if placed_here >= DECALS_PER_POSITION {
                    placed_here = 0;
                    pos_idx += 1;
                }
            }
        }
    }

    // ── Pin r_decals so the ring stays small and never strands a slot ────────
    if opts.inject_r_decals_command {
        let playback_idx = demo
            .directory
            .entries
            .iter()
            .position(|e| e.type_ == 1)
            .or_else(|| demo.directory.entries.len().checked_sub(1));

        if let Some(entry) = playback_idx.and_then(|i| demo.directory.entries.get_mut(i)) {
            // DemoStart (type 2) must be processed before any ConsoleCommand
            // (type 3), or the engine reads uninitialised memory.
            let insert_at = entry
                .frames
                .iter()
                .rposition(|f| matches!(f.frame_data, FrameData::DemoStart))
                .map(|p| p + 1)
                .unwrap_or(0);
            let anchor = entry
                .frames
                .get(insert_at)
                .or_else(|| entry.frames.first())
                .map(|f| (f.time, f.frame))
                .unwrap_or((0.0, 0));
            let cmd = format!("r_decals {}", opts.ring_limit);
            entry.frames.insert(
                insert_at,
                Frame {
                    time: anchor.0,
                    frame: anchor.1,
                    frame_data: FrameData::ConsoleCommand(ConsoleCommand {
                        command: ByteString::from(cmd.as_str()),
                    }),
                },
            );
            entry.frame_count = entry.frames.len() as i32;
        }
    }

    match demo.write_to_bytes_cancellable(&|| cancel.requested()) {
        Some(bytes) => Ok((bytes, stats)),
        None => Err(DecalCleanError::Cancelled),
    }
}

/// Every world-surface coordinate this demo proves exists.
///
/// No capture windows are applied: this is the demo's whole contribution, not
/// the subset outside a clip. Exposed because these are coordinates the engine
/// provably accepted, which makes them the one honest way to check parsed map
/// geometry without loading the game. See `docs/archive/decal_flush_bsp_surfaces.md`.
pub fn proven_world_coordinates(demo: &dem::types::Demo) -> Vec<[f32; 3]> {
    survey(demo, &[], &DecalCleanOptions::default()).world_harvested
}

/// Strip-only entry point, kept for callers that just want the decal messages
/// outside `keep_windows` blanked with no burst injection or cvar pinning.
pub fn strip_decals_outside_windows(
    demo_bytes: &[u8],
    keep_windows: &[(i32, i32)],
) -> Result<(Vec<u8>, DecalCleanStats), DecalCleanError> {
    let opts = DecalCleanOptions {
        flush_burst: false,
        inject_r_decals_command: false,
        ..Default::default()
    };
    clean_demo_decals(
        demo_bytes,
        keep_windows,
        &opts,
        crate::patch::Cancel::never(),
    )
}
