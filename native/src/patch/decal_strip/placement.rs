// patch/decal_strip/placement.rs
// Where the flush burst goes: the map it is judged against, what a camera can
// see, and the ranked walk through every source of candidate positions.

use super::geometry::{DECAL_PROJECTION_REACH, DECAL_SURFACE_LIFT, distance, tile_positions};
use super::survey::Survey;
use super::{DecalCleanOptions, DecalCleanStats, MIN_POSITION_SPACING};
use crate::patch::{bsp, decal_atlas};

/// How the flush coordinate was chosen, for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushSource {
    /// Caller supplied the coordinate outright.
    Override,
    /// A decal position lifted from the demo — the engine already accepted it,
    /// so the surface is proven — picked as the one nearest the spawn.
    HarvestedNearSpawn,
    /// Computed floor point beneath the settled spawn position. Geometrically
    /// derived rather than proven, so only used when nothing was harvested.
    ComputedSpawnFloor,
    /// Floor points under the player's own walked path. Proven surfaces (they
    /// stood on them) and naturally spread apart, which is what a sweep needs.
    PlayerFloorPath,
    /// A coordinate from the map's accumulated store — proven by some earlier
    /// demo on this exact map build, and too isolated to have formed a tileable
    /// patch. See `decal_atlas`.
    MapAtlas,
    /// A grid tiled across a plane fitted to the demo's own decals. The surface
    /// is proven by those decals; the individual tiles are inference from them,
    /// which the engine permits — it constrains distance from a surface, not
    /// movement along one.
    TiledPlane,
    /// Sampled straight off the map's own world faces. The only source that
    /// owes nothing to what anyone did in the match, and therefore the only one
    /// that can supply a map the coordinate store has never seen. See
    /// `bsp::Bsp::face_candidates`.
    MapGeometry,
}

/// How the flush decided what a camera can see, for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisibilityBasis {
    /// Map geometry: leaf visibility plus a line-of-sight trace. The only
    /// basis that can tell a wall from a sightline.
    Geometry,
    /// No map available, so the frame cone alone. Correct about what is in
    /// front of a camera, silent about what stands in the way, and therefore
    /// over-rejects.
    #[default]
    ConeOnly,
}

/// Loads the map a demo was recorded on, when the caller supplied a maps
/// directory.
///
/// A missing or unreadable map is not an error: the flush worked without one
/// before this existed and still does, just with the cone alone. It is logged
/// rather than swallowed, because "this capture was planned without knowing
/// where the walls are" is worth seeing.
/// The key this demo's harvest is filed under.
///
/// HLTV demos leave the header's checksum field zeroed, so every one of them
/// would otherwise share a single `_00000000` bucket per map name — separate
/// from the bucket the same map's first-person demos fill, and unable to tell
/// two builds apart. When the map is here, its own checksum stands in, which
/// puts both kinds of demo in the same store.
///
/// That trusts the local library to be the build the HLTV demo was recorded on.
/// Nothing in an HLTV demo can confirm it, and the coordinates themselves are
/// proven either way — the cost of being wrong is a mis-filed harvest, not a
/// bad coordinate.
pub(super) fn atlas_key(
    header: &dem::types::Header,
    opts: &DecalCleanOptions,
) -> Option<decal_atlas::MapKey> {
    let mut key = decal_atlas::MapKey::from_header(header)?;
    if key.checksum == 0
        && let Some(dir) = &opts.maps_dir
        && let Ok(found) = bsp::map_checksum_of_file(&dir.join(format!("{}.bsp", key.name)))
    {
        key.checksum = found;
    }
    Some(key)
}

pub(super) fn load_map(
    demo: &dem::types::Demo,
    opts: &DecalCleanOptions,
    stats: &mut DecalCleanStats,
) -> Option<bsp::Bsp> {
    let dir = opts.maps_dir.as_ref()?;
    let key = decal_atlas::MapKey::from_header(&demo.header)?;
    let path = dir.join(format!("{}.bsp", key.name));

    // A map of the right name but the wrong build is worse than no map. Every
    // face, leaf and plane would be read successfully and refer to a different
    // world, so occlusion would answer confidently and wrongly — and a position
    // called hidden on geometry that is not the geometry being rendered is
    // exactly the failure this feature cannot show in its output.
    if key.checksum != 0 {
        match bsp::map_checksum_of_file(&path) {
            Ok(found) if found != key.checksum => {
                crate::log_markdown(&format!(
                    "⚠️ **Decal flush is ignoring `{}`** — the map here is a different build than \
                     the demo was recorded on (demo wants `{:08x}`, this is `{:08x}`). Its \
                     geometry describes a different world, so it cannot be trusted to say what a \
                     camera can see. Falling back to the frame cone alone.",
                    key.name, key.checksum, found
                ));
                return None;
            }
            Ok(_) => {}
            // Unreadable is left to `Bsp::from_file` below, which reports it.
            Err(_) => {}
        }
    }

    match bsp::Bsp::from_file(&path) {
        Ok(map) => {
            stats.visibility_basis = VisibilityBasis::Geometry;
            stats.map_faces = map.world_faces().len();
            stats.map_has_vis = map.has_vis();
            Some(map)
        }
        Err(e) => {
            crate::log_markdown(&format!(
                "ℹ️ **Decal flush has no map geometry** for `{}`: {}. Falling back to the frame \
                 cone alone, which cannot tell a wall from a sightline and so rejects spots that \
                 are actually hidden.",
                key.name, e
            ));
            None
        }
    }
}
/// Decides whether a candidate position is ever on screen during a clip.
///
/// The cone alone answers "is it in front of a camera", which is not the
/// question — a wall two rooms away is in front of the camera and invisible.
/// Given map geometry this asks the real one, cheapest test first:
///
///   1. PVS. If the candidate's leaf is absent from the union of every camera
///      leaf's potentially-visible set, the engine cannot render it from
///      anywhere those cameras stood. One lookup, and it settles most
///      candidates outright.
///   2. The cone, per camera. Cheap, and skips the trace for anything behind
///      the viewer.
///   3. A segment trace, only for candidates a camera is actually pointing at.
///
/// Without a map it degrades to the cone alone, which is what shipped before.
pub(super) struct Visibility<'a> {
    bsp: Option<&'a bsp::Bsp>,
    /// Union of the PVS of every leaf a camera stood in. `None` when the map
    /// has no vis data or a camera leaf had no row, in which case nothing can
    /// be ruled out this way.
    camera_pvs: Option<Vec<u8>>,
    cos_cone: f32,
    max_distance: f32,
    /// Experiment gate — see `DecalCleanOptions::require_pvs_hidden`.
    require_pvs_hidden: bool,
}

impl<'a> Visibility<'a> {
    pub(super) fn new(
        bsp: Option<&'a bsp::Bsp>,
        cameras: &[([f32; 3], [f32; 3])],
        opts: &DecalCleanOptions,
    ) -> Self {
        // Cameras cluster hard — thousands of samples collapse to a handful of
        // rooms — so the union is computed over distinct leaves, not samples.
        let camera_pvs = bsp.and_then(|b| {
            if !b.has_vis() {
                return None;
            }
            let mut leaves: Vec<usize> = cameras.iter().map(|(eye, _)| b.leaf_at(eye)).collect();
            leaves.sort_unstable();
            leaves.dedup();
            b.pvs_union(&leaves)
        });

        Self {
            bsp,
            camera_pvs,
            cos_cone: opts.visibility_cone_degrees.to_radians().cos(),
            max_distance: opts.visibility_max_distance,
            require_pvs_hidden: opts.require_pvs_hidden,
        }
    }

    /// Whether one camera can see a position: inside the frame, and with
    /// nothing solid in the way.
    ///
    /// This is also what the on-camera statistic counts, so the number the log
    /// reports and the rule the selection applied cannot disagree. They did
    /// briefly, and the stat screamed about hundreds of frames while every
    /// position it was complaining about was behind a wall.
    fn on_screen_from(&self, pos: &[f32; 3], eye: &[f32; 3], fwd: &[f32; 3]) -> bool {
        let v = [pos[0] - eye[0], pos[1] - eye[1], pos[2] - eye[2]];
        let dist = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if dist < 1.0 || dist > self.max_distance {
            return false;
        }
        let fl = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2]).sqrt();
        if fl < 0.5 {
            return false;
        }
        if (v[0] * fwd[0] + v[1] * fwd[1] + v[2] * fwd[2]) / (dist * fl) < self.cos_cone {
            return false; // outside the frame
        }
        match self.bsp {
            Some(b) => !b.line_blocked(eye, pos),
            None => true,
        }
    }

    /// Where a decal aimed at this candidate is actually drawn, or `None` when
    /// no surface is close enough for one to land on — which disqualifies the
    /// candidate rather than making it safe. Without a map there is nothing to
    /// project onto and the coordinate stands in for itself, as it always did.
    fn draw_point(&self, pos: &[f32; 3]) -> Option<[f32; 3]> {
        let Some(b) = self.bsp else {
            return Some(*pos);
        };
        // A coordinate inside solid is not a surface spot at all. The engine
        // still draws something for it — on whichever face its walk reaches,
        // which can be the one facing the camera — so these are dropped rather
        // than reasoned about. They are also exactly what the old point test
        // scored as safest, being occluded from everywhere by construction.
        let contents = b.leaf_contents(b.leaf_at(pos));
        if contents == bsp::CONTENTS_SOLID || contents == bsp::CONTENTS_SKY {
            return None;
        }
        b.decal_draw_point(pos, DECAL_PROJECTION_REACH, DECAL_SURFACE_LIFT)
    }

    fn hidden(&self, pos: &[f32; 3], cameras: &[([f32; 3], [f32; 3])]) -> bool {
        // Deliberately the trace alone, not PVS.
        //
        // PVS was a fast accept here: a candidate whose leaf is absent from
        // every camera leaf's PVS cannot be rendered, so it could be called
        // hidden without tracing. Sound in theory; the two disagreed in
        // practice. At a 4096-slot ring — where selection reaches far enough
        // down the ranked list to need marginal candidates — 200 in-clip
        // frames had a clear line of sight to positions PVS had called
        // invisible. One of the two is wrong, and nothing available here can
        // say which without loading the game.
        //
        // So the cheaper test does not get to grant safety. Losing it costs
        // time, and buying that back by trusting a check that has already been
        // caught disagreeing with the geometry is not a trade worth making for
        // the one defect this pass must never introduce.
        //
        // `camera_pvs` is kept because it is the thing to re-examine when the
        // in-game check happens: if the traces prove right, this is where the
        // speed goes back.
        // The candidate is a coordinate; the engine draws somewhere else. Judge
        // the place it draws, or the test certifies a wall in plain view as
        // hidden because the coordinate behind it is.
        let Some(draw) = self.draw_point(pos) else {
            return false;
        };
        // Experiment gate: demand that leaf visibility also say the engine never
        // renders the drawn surface. Asked of `draw`, not `pos`, for the same
        // reason the camera test is — a coordinate inside solid sits in a leaf
        // no PVS row contains and would pass this trivially.
        if self.require_pvs_hidden && !self.pvs_says_hidden(&draw).unwrap_or(false) {
            return false;
        }
        !cameras
            .iter()
            .any(|(eye, fwd)| self.on_screen_from(&draw, eye, fwd))
    }

    /// Whether leaf visibility also considers a position unrenderable, or
    /// `None` when the map carries no vis data.
    ///
    /// Not used to decide anything — see `hidden` for why. It is measured so
    /// the in-game check has a number to settle: if PVS agrees with the traces
    /// on every chosen position, the fast accept can come back.
    fn pvs_says_hidden(&self, pos: &[f32; 3]) -> Option<bool> {
        let b = self.bsp?;
        let pvs = self.camera_pvs.as_ref()?;
        Some(!bsp::Bsp::pvs_contains(pvs, b.leaf_at(pos)))
    }

    /// How many of these positions leaf visibility also calls hidden.
    pub(super) fn pvs_agreement(&self, positions: &[[f32; 3]]) -> Option<usize> {
        self.camera_pvs.as_ref()?;
        Some(
            positions
                .iter()
                .filter(|p| self.pvs_says_hidden(p).unwrap_or(false))
                .count(),
        )
    }

    /// In-clip camera samples from which any of these positions is on screen.
    /// Must be zero.
    pub(super) fn on_camera_frames(
        &self,
        positions: &[[f32; 3]],
        cameras: &[([f32; 3], [f32; 3])],
    ) -> usize {
        // Measured on the drawn surface, the same rule the selection applied.
        // Counting the coordinates instead reports zero for a sweep that covers
        // a wall the camera is looking straight at.
        let drawn: Vec<[f32; 3]> = positions
            .iter()
            .filter_map(|p| self.draw_point(p))
            .collect();
        cameras
            .iter()
            .filter(|(eye, fwd)| drawn.iter().any(|p| self.on_screen_from(p, eye, fwd)))
            .count()
    }
}
/// Where the burst will go, plus enough of how that was decided to explain a
/// shortfall from the log alone.
#[derive(Default)]
pub(super) struct Placement {
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) source: Option<FlushSource>,
    /// Tiles laid across the fitted planes, before any camera filtering.
    pub(super) tiled: usize,
    /// How many of those survived the clearance and line-of-sight tests. The
    /// gap between these two separates "this demo has no surface to work with"
    /// from "everything it has is in shot", which want opposite fixes.
    pub(super) tiled_safe: usize,
    /// The same pair for the map-geometry source, when it was reached.
    pub(super) map_sampled: usize,
    pub(super) map_safe: usize,
}

/// Picks the set of positions the flush burst is spread across.
///
/// `wanted` is how many distinct spots are needed to place the whole burst at
/// `DECALS_PER_POSITION` each. Returning fewer means the sweep will fall short,
/// which the caller reports rather than hiding.
pub(super) fn resolve_flush_positions(
    survey: &Survey,
    atlas: &[[f32; 3]],
    visibility: &Visibility,
    opts: &DecalCleanOptions,
    wanted: usize,
) -> Placement {
    if let Some(coord) = opts.flush_coord {
        return Placement {
            positions: vec![coord],
            source: Some(FlushSource::Override),
            ..Placement::default()
        };
    }

    // Never in shot. This is the guarantee: a position inside the camera's cone
    // at any sampled in-clip frame is rejected outright, however far away it is.
    //
    // Clearance deliberately is NOT part of this test. It was, as a hard floor,
    // and it quietly wrecked the pass on a third of a 28-demo survey: on
    // harrington, all 8165 tiles sat within 900 units of some camera at some
    // point in eight clips, so every one was thrown away and the flush fell
    // back to a single position — a sweep that turns 4 of 256 ring slots.
    // Dropping the floor to 250 there kept `flush_on_camera_frames` at 0 while
    // restoring a full sweep, which is the measurement that settles it: the
    // cone test is what keeps decals off screen, and distance is a tiebreak.
    // With map geometry this also asks whether anything stands in the way,
    // which the cone alone cannot. That matters most where the cone is
    // strictest: on a busy map most of what it discards is behind a wall.
    let hidden = |pos: &[f32; 3]| -> bool { visibility.hidden(pos, &survey.window_cameras) };

    // Furthest approach any in-window camera makes to a position. Ranking by
    // this — rather than by nearness to spawn — matches what these spots are
    // actually for. Spawn proximity was a holdover from "hide it near spawn";
    // a flush spot has no reason to be anywhere in particular except far from
    // the lens.
    let clearance = |pos: &[f32; 3]| -> f32 {
        survey
            .window_cameras
            .iter()
            .map(|(eye, _)| distance(pos, eye))
            .fold(f32::INFINITY, f32::min)
    };

    // Tiles across the fitted planes first: same proven surfaces the harvested
    // decals sit on, but a whole grid per patch instead of one point per decal,
    // which is what lets a sweep reach a full ring revolution. Raw harvested
    // positions follow, covering decals too isolated to form a patch. Floor
    // points under the player's own path come last — plentiful and proven, but
    // by construction where the player walks, which is the worst place to hide
    // something. Used only to make up a shortfall in count.
    // Tiled across everything proven to be surface on this map, not just what
    // this demo proved. The atlas is what makes a quiet wall tileable when the
    // POV player never shot it.
    let mut proven: Vec<[f32; 3]> = survey.harvested.clone();
    proven.extend_from_slice(atlas);
    let tiled = tile_positions(&proven);
    let mut placement = Placement {
        tiled: tiled.len(),
        ..Placement::default()
    };

    let mut pool: Vec<[f32; 3]> = Vec::new();
    let mut source = None;

    // Returns how many of the candidates cleared every camera, having taken
    // what it could from them.
    let absorb = |candidates: &[[f32; 3]],
                  src: FlushSource,
                  pool: &mut Vec<[f32; 3]>,
                  source: &mut Option<FlushSource>|
     -> usize {
        // Clearance is measured against every in-window camera sample, so it is
        // computed once per candidate rather than inside the comparator — tiling
        // multiplies the candidate count by an order of magnitude and a sort
        // that recomputed it would dominate the whole pass.
        let mut ok: Vec<(f32, [f32; 3])> = candidates
            .iter()
            .copied()
            .filter(|p| hidden(p))
            .map(|p| (clearance(&p), p))
            .collect();
        let safe = ok.len();
        // Furthest from the camera first.
        ok.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        for (_, p) in ok {
            if pool.len() >= wanted {
                break;
            }
            // Enforce spacing across the whole pool, not just within a source,
            // so two sources cannot contribute overlapping spots.
            if pool.iter().all(|q| distance(&p, q) >= MIN_POSITION_SPACING) {
                pool.push(p);
                source.get_or_insert(src);
            }
        }
        safe
    };

    // Everything drawn from the match, unless the experiment gate is holding
    // them back so the map source can be judged on its own.
    if !opts.map_geometry_only {
        placement.tiled_safe = absorb(&tiled, FlushSource::TiledPlane, &mut pool, &mut source);
        if pool.len() < wanted {
            absorb(
                &survey.harvested,
                FlushSource::HarvestedNearSpawn,
                &mut pool,
                &mut source,
            );
        }
        // Atlas coordinates too isolated to have formed a tileable patch still
        // stand on their own as proven surface.
        if pool.len() < wanted {
            absorb(atlas, FlushSource::MapAtlas, &mut pool, &mut source);
        }
    }
    // The map's own faces, and only now. Sampling them is not free — every
    // candidate is projected onto a face and traced against every camera — and
    // on a map whose store is populated the sources above have already filled
    // the pool, so paying for it there would be pure cost for no position. The
    // case it exists for is the opposite one: a map nothing has been harvested
    // from yet, where everything above comes back nearly empty.
    if pool.len() < wanted
        && let Some(map) = visibility.bsp
    {
        let sampled = map.face_candidates(&bsp::FaceSampling::default());
        placement.map_sampled = sampled.len();
        placement.map_safe = absorb(&sampled, FlushSource::MapGeometry, &mut pool, &mut source);
    }
    if pool.len() < wanted && !opts.map_geometry_only {
        absorb(
            &survey.floor_candidates,
            FlushSource::PlayerFloorPath,
            &mut pool,
            &mut source,
        );
    }

    if !pool.is_empty() {
        placement.positions = pool;
        placement.source = source;
        return placement;
    }

    // Only the last-resort single position needs a spawn reference, to compute
    // a floor point beneath it. Gating the whole function on one cost two
    // demos in the survey their entire flush: both were scrim recordings whose
    // refparams never yielded a settled on-ground origin, yet both carried
    // ~1500-2200 real decals that would have served as positions perfectly
    // well. No reference now means no fallback, not no flush.
    let Some(reference) = survey.grounded_origin.or(survey.spawn_eye) else {
        return placement;
    };
    // Under the experiment gate a spawn-floor guess would quietly put the
    // sweep back on ground the player walked, which is the one thing the gate
    // exists to exclude. Better to report no positions at all.
    if opts.map_geometry_only {
        return placement;
    }

    let (positions, source) = legacy_single_position(survey, opts, reference);
    Placement {
        positions,
        source,
        ..placement
    }
}

/// Original single-position selection, retained as the last resort for demos
/// that yield no safe spread at all.
fn legacy_single_position(
    survey: &Survey,
    opts: &DecalCleanOptions,
    reference: [f32; 3],
) -> (Vec<[f32; 3]>, Option<FlushSource>) {
    // Two independent disqualifiers, because neither alone is sufficient:
    //
    //  - Distance: the camera physically walking over the spot. Spawn is also
    //    the corridor players leave through, so the decal nearest spawn is a
    //    prime offender.
    //  - Line of sight: a spot 1200 units away, dead centre of frame down a
    //    long sightline, is plainly visible despite comfortable "clearance".
    //    Distance-only selection picks these, so the forward vector recorded
    //    with each in-window camera sample is used for a real frustum test.
    let cos_cone = opts.visibility_cone_degrees.to_radians().cos();

    let on_camera_frames = |pos: &[f32; 3]| -> usize {
        survey
            .window_cameras
            .iter()
            .filter(|(eye, fwd)| {
                let v = [pos[0] - eye[0], pos[1] - eye[1], pos[2] - eye[2]];
                let dist = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if dist < 1.0 || dist > opts.visibility_max_distance {
                    return false;
                }
                let fl = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2]).sqrt();
                if fl < 0.5 {
                    return false;
                }
                let dot = (v[0] * fwd[0] + v[1] * fwd[1] + v[2] * fwd[2]) / (dist * fl);
                dot >= cos_cone
            })
            .count()
    };

    let clearance = |pos: &[f32; 3]| -> f32 {
        survey
            .window_cameras
            .iter()
            .map(|(eye, _)| distance(pos, eye))
            .fold(f32::INFINITY, f32::min)
    };

    if !survey.harvested.is_empty() {
        let mut scored: Vec<([f32; 3], f32, usize)> = survey
            .harvested
            .iter()
            .map(|pos| (*pos, clearance(pos), on_camera_frames(pos)))
            .collect();

        // Never on screen during a recorded clip AND never walked over.
        let mut clear: Vec<&([f32; 3], f32, usize)> = scored
            .iter()
            .filter(|(_, c, seen)| *seen == 0 && *c >= opts.min_camera_clearance)
            .collect();

        if !clear.is_empty() {
            clear.sort_by(|a, b| {
                distance(&a.0, &reference)
                    .partial_cmp(&distance(&b.0, &reference))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            return (vec![clear[0].0], Some(FlushSource::HarvestedNearSpawn));
        }

        // Nothing was fully clear. Prefer the least-seen candidate, breaking
        // ties on distance; the caller reports both so a marginal pick is
        // visible rather than silent.
        scored.sort_by(|a, b| {
            a.2.cmp(&b.2)
                .then(b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        });
        return (vec![scored[0].0], Some(FlushSource::HarvestedNearSpawn));
    }

    (
        vec![[reference[0], reference[1], reference[2] - opts.floor_drop]],
        Some(FlushSource::ComputedSpawnFloor),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A demo header naming a map, with the checksum an HLTV recording would
    /// leave zeroed.
    fn header_for(map: &str, checksum: u32) -> dem::types::Header {
        let mut name = vec![0u8; 260];
        name[..map.len()].copy_from_slice(map.as_bytes());
        dem::types::Header {
            magic: b"HLDEMO\0\0".to_vec(),
            demo_protocol: 5,
            network_protocol: 48,
            map_name: name.into(),
            game_directory: vec![0u8; 260].into(),
            map_checksum: checksum,
            directory_offset: 0,
        }
    }

    /// The smallest thing `map_checksum` will read: a v30 header with fifteen
    /// empty lumps. No geometry needed — only the checksum is being asked for.
    fn empty_map(dir: &std::path::Path, name: &str) -> u32 {
        let mut bytes = vec![0u8; 4 + 15 * 8];
        bytes[0] = 30;
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(format!("{}.bsp", name)), &bytes).unwrap();
        crate::patch::bsp::map_checksum(&bytes).unwrap()
    }

    #[test]
    fn an_hltv_demo_keys_its_harvest_off_the_local_map_not_a_zero() {
        // HLTV demos zero the checksum field, so every one of them would
        // otherwise share a single `<map>_00000000` bucket — separate from the
        // bucket the same map's first-person demos fill, and unable to tell two
        // builds apart.
        let dir = Scratch::new("atlas_key");
        let expected = empty_map(&dir, "dod_anzio");

        let opts = DecalCleanOptions {
            maps_dir: Some(dir.to_path_buf()),
            ..Default::default()
        };

        let hltv = atlas_key(&header_for("dod_anzio", 0), &opts).unwrap();
        assert_eq!(hltv.checksum, expected, "resolved from the map on disk");

        let pov = atlas_key(&header_for("dod_anzio", 0xdead_beef), &opts).unwrap();
        assert_eq!(
            pov.checksum, 0xdead_beef,
            "a demo that states its build is never second-guessed"
        );
    }

    #[test]
    fn an_hltv_demo_with_no_map_available_keeps_its_zero() {
        // Nothing to resolve from, and inventing a checksum would be worse than
        // an honest shared bucket.
        let absent = Scratch::absent("atlas_key_absent");
        let opts = DecalCleanOptions {
            maps_dir: Some(absent.to_path_buf()),
            ..Default::default()
        };

        assert_eq!(
            atlas_key(&header_for("dod_anzio", 0), &opts)
                .unwrap()
                .checksum,
            0
        );
    }

    /// A survey with one camera in the room, facing away from the only wall in
    /// it — so nothing on that wall is ever on screen and the camera test is
    /// not what any of these assertions are about.
    fn survey_facing_away(harvested: Vec<[f32; 3]>) -> Survey {
        Survey {
            harvested,
            world_harvested: Vec::new(),
            texture_index: Some(0),
            spawn_eye: None,
            grounded_origin: None,
            floor_candidates: Vec::new(),
            window_cameras: vec![([50.0, 32.0, 32.0], [-1.0, 0.0, 0.0])],
        }
    }

    #[test]
    fn the_map_is_not_scanned_when_the_demo_can_fill_the_sweep_itself() {
        // The laziness is the design, not an optimisation detail. Sampling a
        // map's faces and tracing each one against every camera costs real time
        // on every demo, and on a map whose coordinate store is populated it
        // buys nothing — the proven sources have already filled the pool. A
        // regression here is invisible in the output and shows up only as the
        // pre-pass getting slower.
        let map = bsp::one_wall_room();
        let opts = DecalCleanOptions::default();
        let survey = survey_facing_away(vec![[98.0, 20.0, 20.0], [98.0, 40.0, 40.0]]);
        let visibility = Visibility::new(Some(&map), &survey.window_cameras, &opts);

        let placement = resolve_flush_positions(&survey, &[], &visibility, &opts, 2);

        assert_eq!(placement.positions.len(), 2);
        assert_eq!(
            placement.map_sampled, 0,
            "the map was scanned even though the demo's own decals sufficed"
        );
    }

    #[test]
    fn a_demo_that_proves_nothing_is_carried_by_the_map_itself() {
        // The case the source exists for: a map nothing has been harvested
        // from, where every source drawn from the match comes back empty. The
        // map still has walls.
        let map = bsp::one_wall_room();
        let opts = DecalCleanOptions::default();
        let survey = survey_facing_away(Vec::new());
        let visibility = Visibility::new(Some(&map), &survey.window_cameras, &opts);

        let placement = resolve_flush_positions(&survey, &[], &visibility, &opts, 2);

        assert!(placement.map_sampled > 0, "the map was never consulted");
        assert_eq!(placement.positions.len(), 2);
        assert_eq!(placement.source, Some(FlushSource::MapGeometry));
        for p in &placement.positions {
            assert!(
                (p[0] - 98.0).abs() < 1e-3,
                "off the wall and into the room: {:?}",
                p
            );
        }
    }

    #[test]
    fn the_gate_with_no_map_places_nothing_rather_than_guessing() {
        // Running the gate on a machine where the map is not installed leaves
        // it with no source at all. The honest outcome is an empty placement,
        // which the caller reports as a partial sweep — not a quiet fallback to
        // the spawn floor, which would put the decals on ground the player
        // walks and look exactly like the map source failing in game.
        let opts = DecalCleanOptions {
            map_geometry_only: true,
            ..Default::default()
        };
        let mut survey = survey_facing_away(vec![[98.0, 20.0, 20.0]]);
        survey.grounded_origin = Some([50.0, 32.0, 32.0]);
        let visibility = Visibility::new(None, &survey.window_cameras, &opts);

        let placement = resolve_flush_positions(&survey, &[], &visibility, &opts, 2);

        assert!(placement.positions.is_empty(), "{:?}", placement.positions);
        assert_eq!(placement.source, None);
    }

    #[test]
    fn the_experiment_gate_refuses_everything_the_match_proved() {
        // `DOD_FLUSH_MAP_GEOMETRY_ONLY` exists to put the map source in front
        // of someone watching the game. It is worthless if a harvested decal
        // can still slip into the pool and be the thing they end up looking at.
        let map = bsp::one_wall_room();
        let opts = DecalCleanOptions {
            map_geometry_only: true,
            ..Default::default()
        };
        let survey = survey_facing_away(vec![[98.0, 20.0, 20.0], [98.0, 40.0, 40.0]]);
        let visibility = Visibility::new(Some(&map), &survey.window_cameras, &opts);

        let placement = resolve_flush_positions(&survey, &[], &visibility, &opts, 2);

        assert_eq!(placement.source, Some(FlushSource::MapGeometry));
        assert_eq!(
            placement.tiled_safe, 0,
            "the proven sources were still consulted under the gate"
        );
    }
}
