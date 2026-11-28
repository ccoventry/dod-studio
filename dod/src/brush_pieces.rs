//! Which of the loaded map's brush models a demo's brush entity should be
//! drawn with, for a demo recorded on another build of its map (#1540).
//!
//! A brush entity (a wall, a door, water, a trigger) reaches the client as an
//! inline model `*N`, by number. HLTV demos carry no map checksum the engine
//! checks, so an HLTV demo recorded on an older build of a map plays on the
//! installed one, and when the map's numbering changed between the builds the
//! demo's render settings land on the wrong pieces: black boxes where
//! triggers are, opaque water, invisible clip walls drawn.
//!
//! The demo says which piece it means. Each brush entity's spawn baseline
//! carries `mins`/`maxs`, relative to the entity, and the engine set them from
//! its own copy of the model's bounds, which it pads by one unit on every side
//! as it loads the map (`Mod_LoadSubmodels`). So a baseline's bounds equal the
//! padded bounds of the piece it was spawned with, to delta-encoding rounding
//! (at most ~0.12 units measured). The loaded map's pieces carry the same
//! padded bounds, so the right piece is the one whose bounds fit.
//!
//! The rules here, in order:
//!
//! - **Its own piece fits:** nothing to do. A demo on the right build is
//!   always this case (185 of 185 on a current-build dod_chemille POV demo,
//!   114 of 114 on dod_railroad).
//! - **Exactly one other piece fits:** draw that one. Bounds alone tie
//!   between pieces of the same shape (569 of the 34,651 brush models in the
//!   251 installed maps), mostly rotating doors, whose bounds are relative to
//!   their `origin` key; the entity's spawn origin tells 382 of those apart,
//!   so it narrows a tie but never rules a fit out (a `func_train` spawns away
//!   from its key).
//! - **Several still fit:** left as the demo says.
//! - **None fits:** hidden, as today's map has nothing there.
//!
//! Nothing acts, the first included, unless the demo shows it is on another
//! build: at least [`MIN_RENUMBERED`] entities fit a piece at another number.
//! Without that, every entity is left as it is, so a correct demo with a brush
//! resized by a server plugin, or one that happens to match another piece, is
//! never touched.

/// How many entities must fit a piece at another number before any entity is
/// drawn with another piece or hidden: one could be a coincidence (a brush a
/// plugin resized to another piece's shape); a renumbered map moves many (58
/// in the old dod_chemille demos).
pub const MIN_RENUMBERED: usize = 2;

/// How far, in map units, a demo's bounds or origin may sit from a piece's
/// and still be it. The largest gap measured on a match is ~0.12 units, over
/// three demos on two maps (old and current dod_chemille, dod_railroad).
pub const TOLERANCE: f32 = 0.5;

/// An axis-aligned box: the padded bounds of a piece, or a demo entity's
/// `mins`/`maxs`, relative to the entity's origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

impl Bounds {
    /// Whether this is all zeros: a baseline that carries no bounds.
    pub fn is_empty(&self) -> bool {
        self.mins.iter().chain(&self.maxs).all(|&v| v == 0.0)
    }

    /// Whether every corner of `self` is within [`TOLERANCE`] of `other`'s.
    pub fn fits(&self, other: &Bounds) -> bool {
        close(&self.mins, &other.mins) && close(&self.maxs, &other.maxs)
    }
}

fn close(a: &[f32; 3], b: &[f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= TOLERANCE)
}

/// One of the loaded map's brush models, `*N` at index `N` of a slice of
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Piece {
    /// The engine's padded bounds of the model.
    pub bounds: Bounds,
    /// The `origin` key of the map entity that uses the model, 0 0 0 when it
    /// has none, or `None` when the map's entities couldn't be read.
    pub origin: Option<[f32; 3]>,
}

/// A brush entity as the demo spawned it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spawned {
    /// The `N` of the `*N` the demo names.
    pub named: usize,
    /// Its baseline's `mins`/`maxs`.
    pub bounds: Bounds,
    /// Its baseline's origin.
    pub origin: [f32; 3],
}

/// How one entity's bounds sit against the map's pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The piece it names fits.
    Named,
    /// It fits exactly one other piece, `*N`.
    Other(usize),
    /// It fits several other pieces, which its origin doesn't tell apart.
    Several,
    /// It fits no piece of the map.
    Nothing,
    /// Its baseline carries no bounds, so nothing can be told.
    NoBounds,
}

/// Which piece `entity` fits among `pieces` (`*N` at index `N`; index 0, the
/// world, is never a candidate).
pub fn fit(entity: &Spawned, pieces: &[Piece]) -> Fit {
    if entity.bounds.is_empty() {
        return Fit::NoBounds;
    }
    let fits = |n: usize| pieces.get(n).is_some_and(|p| entity.bounds.fits(&p.bounds));
    if entity.named != 0 && fits(entity.named) {
        return Fit::Named;
    }
    let candidates: Vec<usize> = (1..pieces.len())
        .filter(|&n| n != entity.named && fits(n))
        .collect();
    match candidates.as_slice() {
        [] => Fit::Nothing,
        [only] => Fit::Other(*only),
        several => {
            let at_origin: Vec<usize> = several
                .iter()
                .copied()
                .filter(|&n| {
                    pieces[n]
                        .origin
                        .is_some_and(|origin| close(&origin, &entity.origin))
                })
                .collect();
            match at_origin.as_slice() {
                [only] => Fit::Other(*only),
                _ => Fit::Several,
            }
        }
    }
}

/// What to do with one entity when it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Draw it as the demo says.
    Keep,
    /// Draw it with piece `*N` instead.
    Draw(usize),
    /// Don't draw it.
    Hide,
}

/// How many entities fell in each [`Fit`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub named: usize,
    pub other: usize,
    pub several: usize,
    pub nothing: usize,
    pub no_bounds: usize,
}

impl Counts {
    /// Whether the demo shows it was recorded on another build of the map:
    /// at least [`MIN_RENUMBERED`] entities fit a piece at another number.
    pub fn is_other_build(&self) -> bool {
        self.other >= MIN_RENUMBERED
    }
}

/// The BSP models lump's index, and one `dmodel_t`'s size on disk.
const LUMP_MODELS: usize = 14;
const DMODEL_SIZE: usize = 64;

/// A map file's pieces as the engine loads them: the models lump's bounds
/// padded by one unit (`Mod_LoadSubmodels`), each with its entity's origin.
/// For the offline tools and tests; the hook reads the engine's own copy.
pub fn pieces_from_bsp(bsp: &[u8]) -> Result<Vec<Piece>, String> {
    let (offset, length) = crate::map_entities::lump_span(bsp, LUMP_MODELS)?;
    let lump = offset
        .checked_add(length)
        .and_then(|end| bsp.get(offset..end))
        .ok_or_else(|| "models lump runs past the end of the file".to_string())?;
    let entities = crate::map_entities::parse_entities(bsp)?;
    let (models, _) = lump.as_chunks::<DMODEL_SIZE>();
    let origins = piece_origins(&entities, models.len());
    Ok(models
        .iter()
        .zip(origins)
        .map(|(model, origin)| {
            let f = |i: usize| {
                f32::from_le_bytes([
                    model[i * 4],
                    model[i * 4 + 1],
                    model[i * 4 + 2],
                    model[i * 4 + 3],
                ])
            };
            Piece {
                bounds: Bounds {
                    mins: [f(0) - 1.0, f(1) - 1.0, f(2) - 1.0],
                    maxs: [f(3) + 1.0, f(4) + 1.0, f(5) + 1.0],
                },
                origin: Some(origin),
            }
        })
        .collect())
}

/// Each piece's origin, `*N` at index `N` of `count`: the `origin` key of the
/// map entity whose `model` is `*N`, or 0 0 0 when it has none (as the
/// engine spawns it) or no entity uses the piece.
pub fn piece_origins(entities: &[crate::map_entities::MapEntity], count: usize) -> Vec<[f32; 3]> {
    let mut origins = vec![[0.0; 3]; count];
    for entity in entities {
        let Some(n) = entity.brush_submodel() else {
            continue;
        };
        let Some(slot) = origins.get_mut(n as usize) else {
            continue;
        };
        if let Some(origin) = entity.get("origin").and_then(parse_origin) {
            *slot = origin;
        }
    }
    origins
}

/// `"x y z"` as three floats, or `None` when it isn't three numbers.
fn parse_origin(text: &str) -> Option<[f32; 3]> {
    let mut parts = text.split_whitespace().map(|p| p.parse::<f32>().ok());
    let origin = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(origin)
}

/// One demo's brush entities against the loaded map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// One per entity, in the order given.
    pub actions: Vec<Action>,
    pub counts: Counts,
}

/// What to do with each of `entities` on a map whose pieces are `pieces`,
/// by the rules in the module doc: nothing at all unless the demo is on
/// another build.
pub fn plan(entities: &[Spawned], pieces: &[Piece]) -> Plan {
    let fits: Vec<Fit> = entities.iter().map(|e| fit(e, pieces)).collect();
    let mut counts = Counts::default();
    for f in &fits {
        match f {
            Fit::Named => counts.named += 1,
            Fit::Other(_) => counts.other += 1,
            Fit::Several => counts.several += 1,
            Fit::Nothing => counts.nothing += 1,
            Fit::NoBounds => counts.no_bounds += 1,
        }
    }
    let other_build = counts.is_other_build();
    let actions = fits
        .iter()
        .map(|f| match (f, other_build) {
            (Fit::Other(n), true) => Action::Draw(*n),
            (Fit::Nothing, true) => Action::Hide,
            _ => Action::Keep,
        })
        .collect();
    Plan { actions, counts }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A piece whose unpadded box runs from `mins` to `maxs`, padded by one
    /// unit as the engine loads it, with no origin key.
    fn piece(mins: [f32; 3], maxs: [f32; 3]) -> Piece {
        Piece {
            bounds: padded(mins, maxs),
            origin: Some([0.0; 3]),
        }
    }

    fn padded(mins: [f32; 3], maxs: [f32; 3]) -> Bounds {
        Bounds {
            mins: mins.map(|v| v - 1.0),
            maxs: maxs.map(|v| v + 1.0),
        }
    }

    /// The world at `*0`, then three walls a few hundred units apart.
    fn map() -> Vec<Piece> {
        vec![
            piece([-4096.0; 3], [4096.0; 3]),
            piece([0.0, 0.0, 0.0], [64.0, 8.0, 128.0]),
            piece([200.0, 0.0, 0.0], [264.0, 8.0, 96.0]),
            piece([400.0, 50.0, -16.0], [416.0, 90.0, 0.0]),
        ]
    }

    /// A demo entity spawned with `pieces[n]`, as the demo's delta encoding
    /// rounds it.
    fn spawned_with(named: usize, pieces: &[Piece], n: usize) -> Spawned {
        let b = pieces[n].bounds;
        Spawned {
            named,
            bounds: Bounds {
                mins: b.mins.map(|v| v + 0.12),
                maxs: b.maxs.map(|v| v - 0.08),
            },
            origin: pieces[n].origin.unwrap_or_default(),
        }
    }

    #[test]
    fn a_demo_on_the_maps_own_build_changes_nothing() {
        let pieces = map();
        let entities: Vec<Spawned> = (1..pieces.len())
            .map(|n| spawned_with(n, &pieces, n))
            .collect();
        let plan = plan(&entities, &pieces);
        assert!(plan.actions.iter().all(|a| *a == Action::Keep));
        assert_eq!(plan.counts.named, 3);
        assert!(!plan.counts.is_other_build());
    }

    #[test]
    fn a_renumbered_piece_is_drawn_with_the_piece_it_fits() {
        let pieces = map();
        // The old build had a piece before these, so the demo names each one
        // number higher than today's map.
        let entities = [
            spawned_with(2, &pieces, 1),
            spawned_with(3, &pieces, 2),
            spawned_with(4, &pieces, 3),
        ];
        let plan = plan(&entities, &pieces);
        assert_eq!(
            plan.actions,
            vec![Action::Draw(1), Action::Draw(2), Action::Draw(3)]
        );
        assert_eq!(plan.counts.other, 3);
    }

    #[test]
    fn a_piece_that_fits_nothing_is_hidden_on_another_build() {
        let pieces = map();
        let gone = Spawned {
            named: 1,
            bounds: padded([900.0, 900.0, 0.0], [920.0, 950.0, 40.0]),
            origin: [0.0; 3],
        };
        let entities = [
            gone,
            spawned_with(3, &pieces, 2),
            spawned_with(4, &pieces, 3),
        ];
        let plan = plan(&entities, &pieces);
        assert_eq!(
            plan.actions,
            vec![Action::Hide, Action::Draw(2), Action::Draw(3)]
        );
        assert_eq!(plan.counts.nothing, 1);
    }

    #[test]
    fn one_entity_at_another_number_is_a_coincidence_and_changes_nothing() {
        // A brush resized to exactly another piece's shape, beside one that
        // fits nothing: one match elsewhere doesn't say the map changed.
        let pieces = map();
        let gone = Spawned {
            named: 1,
            bounds: padded([900.0, 900.0, 0.0], [920.0, 950.0, 40.0]),
            origin: [0.0; 3],
        };
        let entities = [
            gone,
            spawned_with(3, &pieces, 2),
            spawned_with(1, &pieces, 1),
        ];
        let plan = plan(&entities, &pieces);
        assert_eq!(plan.actions, vec![Action::Keep; 3]);
        assert_eq!(plan.counts.other, 1);
        assert!(!plan.counts.is_other_build());
    }

    #[test]
    fn a_piece_that_fits_nothing_is_left_alone_when_nothing_was_renumbered() {
        // A brush a server resized: no other entity says the map changed, so
        // the demo's own piece is drawn as it says.
        let pieces = map();
        let resized = Spawned {
            named: 1,
            bounds: padded([0.0, 0.0, 0.0], [64.0, 8.0, 200.0]),
            origin: [0.0; 3],
        };
        let entities = [resized, spawned_with(2, &pieces, 2)];
        let plan = plan(&entities, &pieces);
        assert_eq!(plan.actions, vec![Action::Keep, Action::Keep]);
        assert_eq!(plan.counts.nothing, 1);
        assert!(!plan.counts.is_other_build());
    }

    #[test]
    fn a_number_past_the_maps_last_piece_still_finds_its_piece() {
        let pieces = map();
        let entities = [spawned_with(40, &pieces, 3), spawned_with(41, &pieces, 2)];
        assert_eq!(
            plan(&entities, &pieces).actions,
            vec![Action::Draw(3), Action::Draw(2)]
        );
    }

    /// Two rotating doors of one shape: their bounds are relative to their
    /// `origin` keys, so only the spawn origin tells them apart.
    fn doors() -> Vec<Piece> {
        let mut pieces = map();
        for origin in [[100.0, 300.0, 40.0], [-500.0, 300.0, 40.0]] {
            pieces.push(Piece {
                bounds: padded([-2.0, -32.0, -48.0], [2.0, 32.0, 48.0]),
                origin: Some(origin),
            });
        }
        pieces
    }

    #[test]
    fn a_rotating_door_that_fits_its_own_piece_is_kept() {
        let pieces = doors();
        let entities = [spawned_with(4, &pieces, 4), spawned_with(5, &pieces, 5)];
        let plan = plan(&entities, &pieces);
        assert_eq!(plan.actions, vec![Action::Keep, Action::Keep]);
    }

    #[test]
    fn a_renumbered_rotating_door_is_told_apart_by_its_origin() {
        let pieces = doors();
        let entities = [spawned_with(6, &pieces, 5), spawned_with(1, &pieces, 4)];
        let plan = plan(&entities, &pieces);
        assert_eq!(plan.actions, vec![Action::Draw(5), Action::Draw(4)]);
    }

    #[test]
    fn a_door_at_neither_origin_fits_several_and_is_left_alone() {
        let pieces = doors();
        let mut lost = spawned_with(6, &pieces, 5);
        lost.origin = [0.0, 0.0, 0.0];
        // Two other renumbered pieces say the demo is on another build.
        let entities = [
            lost,
            spawned_with(4, &pieces, 3),
            spawned_with(5, &pieces, 2),
        ];
        let plan = plan(&entities, &pieces);
        assert_eq!(
            plan.actions,
            vec![Action::Keep, Action::Draw(3), Action::Draw(2)]
        );
        assert_eq!(plan.counts.several, 1);
    }

    #[test]
    fn a_tie_is_left_alone_when_the_maps_origins_are_unknown() {
        let mut pieces = doors();
        for p in &mut pieces {
            p.origin = None;
        }
        let entities = [spawned_with(6, &pieces, 5)];
        assert_eq!(fit(&entities[0], &pieces), Fit::Several);
    }

    #[test]
    fn a_moved_train_still_fits_its_own_piece() {
        // A train spawns at its first stop, away from its origin key: bounds
        // are relative, so it still fits.
        let pieces = doors();
        let mut train = spawned_with(4, &pieces, 4);
        train.origin = [1500.0, -800.0, 64.0];
        assert_eq!(fit(&train, &pieces), Fit::Named);
    }

    #[test]
    fn a_baseline_without_bounds_is_never_changed() {
        let pieces = map();
        let blank = Spawned {
            named: 2,
            ..Spawned::default()
        };
        let entities = [blank, spawned_with(3, &pieces, 1)];
        let plan = plan(&entities, &pieces);
        assert_eq!(plan.actions[0], Action::Keep);
        assert_eq!(plan.counts.no_bounds, 1);
    }

    #[test]
    fn the_world_is_never_a_candidate() {
        let pieces = map();
        let like_the_world = Spawned {
            named: 9,
            bounds: pieces[0].bounds,
            origin: [0.0; 3],
        };
        assert_eq!(fit(&like_the_world, &pieces), Fit::Nothing);
    }

    #[test]
    fn origins_come_from_the_entity_using_each_piece() {
        let text = r#"
            { "classname" "worldspawn" }
            { "classname" "func_door_rotating" "model" "*2" "origin" "100 300 40" }
            { "classname" "func_wall" "model" "*1" }
            { "classname" "func_door_rotating" "model" "*3" "origin" "not a vector" }
            { "classname" "func_wall" "model" "*99" "origin" "1 2 3" }
        "#;
        let entities = crate::map_entities::parse_entity_text(text).unwrap();
        assert_eq!(
            piece_origins(&entities, 4),
            vec![[0.0; 3], [0.0; 3], [100.0, 300.0, 40.0], [0.0; 3]]
        );
    }

    /// A version 30 BSP with only an entity lump and a models lump.
    fn bsp(entities: &str, models: &[([f32; 3], [f32; 3])]) -> Vec<u8> {
        let header = 4 + 15 * 8;
        let text = format!("{entities}\0").into_bytes();
        let mut lump = Vec::new();
        for (mins, maxs) in models {
            for v in mins.iter().chain(maxs) {
                lump.extend_from_slice(&v.to_le_bytes());
            }
            lump.extend_from_slice(&[0u8; DMODEL_SIZE - 24]);
        }
        let mut out = vec![0u8; header];
        out[..4].copy_from_slice(&30i32.to_le_bytes());
        let mut put = |index: usize, offset: usize, length: usize| {
            out[4 + index * 8..8 + index * 8].copy_from_slice(&(offset as i32).to_le_bytes());
            out[8 + index * 8..12 + index * 8].copy_from_slice(&(length as i32).to_le_bytes());
        };
        put(0, header, text.len());
        put(LUMP_MODELS, header + text.len(), lump.len());
        out.extend_from_slice(&text);
        out.extend_from_slice(&lump);
        out
    }

    #[test]
    fn a_map_files_pieces_are_padded_as_the_engine_loads_them() {
        let file = bsp(
            r#"{ "classname" "worldspawn" } { "classname" "func_door_rotating" "model" "*1" "origin" "8 16 24" }"#,
            &[
                ([-512.0; 3], [512.0; 3]),
                ([-2.0, -32.0, -48.0], [2.0, 32.0, 48.0]),
            ],
        );
        let pieces = pieces_from_bsp(&file).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(
            pieces[1].bounds,
            padded([-2.0, -32.0, -48.0], [2.0, 32.0, 48.0])
        );
        assert_eq!(pieces[1].origin, Some([8.0, 16.0, 24.0]));
        assert_eq!(pieces[0].origin, Some([0.0; 3]));
    }

    #[test]
    fn a_truncated_map_file_is_an_error() {
        let file = bsp("{ }", &[([0.0; 3], [1.0; 3])]);
        assert!(pieces_from_bsp(&file[..file.len() - 10]).is_err());
        assert!(pieces_from_bsp(&[0u8; 8]).is_err());
    }

    #[test]
    fn bounds_further_off_than_the_tolerance_do_not_fit() {
        let a = padded([0.0; 3], [10.0; 3]);
        let mut b = a;
        b.maxs[2] += TOLERANCE + 0.01;
        assert!(!a.fits(&b));
        b.maxs[2] = a.maxs[2] + TOLERANCE;
        assert!(a.fits(&b));
    }
}
