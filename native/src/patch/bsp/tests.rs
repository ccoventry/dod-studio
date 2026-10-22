use super::*;

/// A single square face at x = 100, spanning y and z from 0 to 64, built by
/// hand so the parser's own arithmetic is exercised without a map file.
pub(super) fn synthetic_bsp() -> Bsp {
    let mut bsp = Bsp {
        planes: vec![Plane {
            normal: [1.0, 0.0, 0.0],
            dist: 100.0,
        }],
        vertices: vec![
            [100.0, 0.0, 0.0],
            [100.0, 64.0, 0.0],
            [100.0, 64.0, 64.0],
            [100.0, 0.0, 64.0],
        ],
        edges: vec![[0, 1], [1, 2], [2, 3], [3, 0]],
        surfedges: vec![0, 1, 2, 3],
        faces: vec![Face {
            plane: 0,
            side: 0,
            first_edge: 0,
            num_edges: 4,
            texinfo: 0,
        }],
        texinfo: vec![TexInfo {
            miptex: 0,
            flags: 0,
        }],
        texture_names: vec!["brick_wall".to_string()],
        models: vec![Model {
            first_face: 0,
            num_faces: 1,
            head_node: 0,
            vis_leafs: 2,
        }],
        // A one-plane world: everything at x > 100 is inside the wall,
        // everything at x < 100 is the room in front of it. Enough to make
        // the leaf descent and the segment trace answerable by hand.
        nodes: vec![Node {
            plane: 0,
            // Front of the plane is solid, back is the open room. Children
            // encode a leaf as -1 - index, so -1 is leaf 0 and -2 is leaf 1.
            children: [-1, -2],
        }],
        leaves: vec![
            Leaf {
                contents: CONTENTS_SOLID,
                vis_offset: -1,
            },
            Leaf {
                contents: -1,
                vis_offset: 0,
            },
        ],
        // One row, one byte: leaf 1 can see itself and not leaf 2.
        visibility: vec![0b0000_0001],
        head_node: 0,
        vis_leaf_count: 2,
        bounds: Vec::new(),
    };
    bsp.bounds = (0..bsp.faces.len())
        .map(|i| bsp.compute_bounds(i))
        .collect();
    bsp
}

#[test]
fn a_polygon_is_recovered_in_winding_order() {
    let bsp = synthetic_bsp();
    let poly = bsp.face_polygon(0);
    assert_eq!(poly.len(), 4);
    assert!(poly.iter().all(|v| (v[0] - 100.0).abs() < 0.001));
}

#[test]
fn a_negative_surfedge_traverses_its_edge_backwards() {
    // Getting this wrong yields the right vertices in the wrong order, which
    // point-in-polygon silently gets wrong rather than rejecting.
    let mut bsp = synthetic_bsp();
    // Edge 1 joins vertices 1 and 2, so the two directions are
    // distinguishable. (Edge 0 would not be: -0 == 0.)
    bsp.surfedges = vec![1, 1, 1, 1];
    let forward = bsp.face_polygon(0);
    bsp.surfedges = vec![-1, -1, -1, -1];
    let backward = bsp.face_polygon(0);

    assert_eq!(
        forward[0], bsp.vertices[1],
        "a positive surfedge starts at its edge's first vertex"
    );
    assert_eq!(
        backward[0], bsp.vertices[2],
        "a negative one starts at its edge's second vertex"
    );
}

#[test]
fn a_point_on_the_face_is_found() {
    let bsp = synthetic_bsp();
    let hit = bsp.nearest_face(&[100.0, 32.0, 32.0], 4.0);
    assert_eq!(hit.map(|(i, _)| i), Some(0));
    assert!(hit.unwrap().1 < 0.001);
}

#[test]
fn a_point_off_the_end_of_the_face_is_not() {
    // The whole risk of tiling past a wall's edge: the coordinate looks
    // plausible and creates no decal.
    let bsp = synthetic_bsp();
    assert!(bsp.nearest_face(&[100.0, 400.0, 32.0], 4.0).is_none());
}

#[test]
fn a_point_off_the_plane_beyond_tolerance_is_not() {
    let bsp = synthetic_bsp();
    assert!(bsp.nearest_face(&[100.0, 32.0, 32.0], 4.0).is_some());
    assert!(bsp.nearest_face(&[120.0, 32.0, 32.0], 4.0).is_none());
}

#[test]
fn sky_and_trigger_faces_are_rejected() {
    let mut bsp = synthetic_bsp();
    assert!(bsp.face_takes_decals(0));

    bsp.texture_names = vec!["sky".to_string()];
    assert!(!bsp.face_takes_decals(0));

    bsp.texture_names = vec!["aaatrigger".to_string()];
    assert!(!bsp.face_takes_decals(0));

    bsp.texture_names = vec!["brick_wall".to_string()];
    bsp.texinfo[0].flags = TEX_SPECIAL;
    assert!(!bsp.face_takes_decals(0), "TEX_SPECIAL holds no decal");
}

#[test]
fn area_is_measured_in_world_units() {
    let bsp = synthetic_bsp();
    assert!((bsp.face_area(0) - 64.0 * 64.0).abs() < 1.0);
}

#[test]
fn only_the_world_model_is_walked() {
    let mut bsp = synthetic_bsp();
    bsp.models = vec![
        Model {
            first_face: 0,
            num_faces: 0,
            head_node: 0,
            vis_leafs: 0,
        },
        Model {
            first_face: 0,
            num_faces: 1,
            head_node: 0,
            vis_leafs: 0,
        },
    ];
    assert!(
        bsp.nearest_face(&[100.0, 32.0, 32.0], 4.0).is_none(),
        "a face owned by a brush entity must not count as world surface"
    );
}

#[test]
fn a_non_goldsrc_file_is_rejected() {
    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(&46_i32.to_le_bytes());
    assert!(Bsp::parse(&bytes).is_err());
}

#[test]
fn a_truncated_file_is_rejected_rather_than_panicking() {
    let mut bytes = vec![0u8; HEADER_SIZE];
    bytes[0..4].copy_from_slice(&BSP_VERSION.to_le_bytes());
    // Every lump claims a huge length that runs past the end.
    for i in 0..LUMP_COUNT {
        let at = 4 + i * 8;
        bytes[at..at + 4].copy_from_slice(&(HEADER_SIZE as i32).to_le_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&1_000_000_i32.to_le_bytes());
    }
    assert!(Bsp::parse(&bytes).is_err());
}

#[test]
fn a_point_descends_to_the_leaf_it_is_in() {
    let bsp = synthetic_bsp();
    // In front of the wall is the open room, behind its plane is solid.
    assert_eq!(bsp.leaf_at(&[50.0, 32.0, 32.0]), 1, "the room");
    assert_eq!(bsp.leaf_at(&[150.0, 32.0, 32.0]), 0, "inside the wall");
}

#[test]
fn a_wall_between_two_points_blocks_the_line() {
    // The whole point of the occlusion work: a spot the cone test would
    // reject as "in front of the camera" is fine if this says blocked.
    let bsp = synthetic_bsp();
    assert!(
        bsp.line_blocked(&[50.0, 32.0, 32.0], &[150.0, 32.0, 32.0]),
        "a segment crossing into solid must be blocked"
    );
}

#[test]
fn an_open_line_is_not_blocked() {
    let bsp = synthetic_bsp();
    assert!(!bsp.line_blocked(&[20.0, 32.0, 32.0], &[60.0, 32.0, 32.0]));
}

#[test]
fn a_line_is_blocked_from_either_end() {
    // The trace splits at the plane and walks the near half first, so the
    // two directions take different paths through the recursion and both
    // have to agree.
    let bsp = synthetic_bsp();
    let a = [50.0, 32.0, 32.0];
    let b = [150.0, 32.0, 32.0];
    assert_eq!(bsp.line_blocked(&a, &b), bsp.line_blocked(&b, &a));
}

#[test]
fn sky_blocks_a_line_the_way_solid_does() {
    let mut bsp = synthetic_bsp();
    bsp.leaves[0].contents = CONTENTS_SKY;
    assert!(bsp.line_blocked(&[50.0, 32.0, 32.0], &[150.0, 32.0, 32.0]));
}

#[test]
fn water_does_not_block_a_line() {
    // Water is transparent: a decal behind it is visible through it, so
    // treating it as an occluder would hide a spot that is actually in shot.
    let mut bsp = synthetic_bsp();
    bsp.leaves[0].contents = -3; // CONTENTS_WATER
    assert!(!bsp.line_blocked(&[50.0, 32.0, 32.0], &[150.0, 32.0, 32.0]));
}

#[test]
fn a_pvs_row_decompresses_and_reads_back() {
    let bsp = synthetic_bsp();
    let row = bsp.pvs_row(1).expect("leaf 1 has vis data");
    assert!(Bsp::pvs_contains(&row, 1), "leaf 1 sees itself");
    assert!(!Bsp::pvs_contains(&row, 2), "and not leaf 2");
    assert!(
        !Bsp::pvs_contains(&row, 0),
        "leaf 0 is the solid leaf and is never in a vis row"
    );
}

#[test]
fn a_zero_run_skips_the_leaves_it_covers() {
    // The lump is run-length encoded: a zero byte is followed by a count of
    // zero bytes to skip. Reading that as a literal byte would shift every
    // later leaf's bit and quietly mis-answer visibility across the map.
    let mut bsp = synthetic_bsp();
    bsp.vis_leaf_count = 24;
    // Skip two zero bytes (leaves 1-16), then set bit 0 of the third byte,
    // which is leaf 17.
    bsp.visibility = vec![0x00, 0x02, 0b0000_0001];
    let row = bsp.pvs_row(1).unwrap();

    assert_eq!(row.len(), 3);
    assert!(Bsp::pvs_contains(&row, 17), "leaf 17 should be visible");
    for leaf in 1..=16 {
        assert!(
            !Bsp::pvs_contains(&row, leaf),
            "leaf {} should not be",
            leaf
        );
    }
}

#[test]
fn a_leaf_without_vis_data_has_no_row() {
    let bsp = synthetic_bsp();
    assert!(bsp.pvs_row(0).is_none(), "the solid leaf carries no vis");
}

#[test]
fn a_map_compiled_without_vis_reports_it() {
    let mut bsp = synthetic_bsp();
    bsp.visibility.clear();
    assert!(!bsp.has_vis());
    assert!(bsp.pvs_row(1).is_none());
    assert!(bsp.pvs_union(&[1]).is_none());
}

#[test]
fn a_union_that_cannot_be_completed_is_refused() {
    // If any camera leaf has no vis data it could potentially see anywhere,
    // so the union rules nothing out and the caller must fall back to
    // tracing rather than trusting a partial answer.
    let bsp = synthetic_bsp();
    assert!(bsp.pvs_union(&[1]).is_some());
    assert!(
        bsp.pvs_union(&[0, 1]).is_none(),
        "a leaf without vis must void the union, not be skipped"
    );
}

#[test]
fn a_union_covers_every_leaf_any_camera_can_see() {
    let mut bsp = synthetic_bsp();
    bsp.vis_leaf_count = 8;
    bsp.visibility = vec![0b0000_0001, 0b0000_0010];
    bsp.leaves = vec![
        Leaf {
            contents: CONTENTS_SOLID,
            vis_offset: -1,
        },
        Leaf {
            contents: -1,
            vis_offset: 0,
        },
        Leaf {
            contents: -1,
            vis_offset: 1,
        },
    ];

    let union = bsp.pvs_union(&[1, 2]).unwrap();
    assert!(Bsp::pvs_contains(&union, 1), "from leaf 1");
    assert!(Bsp::pvs_contains(&union, 2), "from leaf 2");
    assert!(!Bsp::pvs_contains(&union, 3));
}

/// The defect this exists for: the engine draws on the surface, not at the
/// coordinate, so the coordinate is not what a camera test may judge.
#[test]
fn decal_draw_point_projects_onto_the_face_and_steps_off_it() {
    let bsp = synthetic_bsp();
    // Two units off the plane, well inside the face's own square.
    let drawn = bsp
        .decal_draw_point(&[98.0, 32.0, 32.0], 4.0, 1.0)
        .expect("a face two units away is within reach");
    // Projected back onto x = 100, then lifted one unit along the normal.
    assert!((drawn[0] - 101.0).abs() < 1e-3, "{:?}", drawn);
    assert!((drawn[1] - 32.0).abs() < 1e-3, "{:?}", drawn);
    assert!((drawn[2] - 32.0).abs() < 1e-3, "{:?}", drawn);
}

/// Tiling lays a grid across a fitted plane, and the plane runs on past the
/// brush that proved it. Tiles landing in open air are not decal spots and
/// must resolve to nothing rather than to some distant face.
#[test]
fn decal_draw_point_gives_up_beyond_its_reach() {
    let bsp = synthetic_bsp();
    assert!(
        bsp.decal_draw_point(&[80.0, 32.0, 32.0], 4.0, 1.0)
            .is_none()
    );
    // The same point is answerable if the reach is widened to cover it,
    // which is what makes the constant the thing that decides, not the map.
    assert!(
        bsp.decal_draw_point(&[80.0, 32.0, 32.0], 32.0, 1.0)
            .is_some()
    );
}

/// Off the end of the face: on the plane, but there is no surface there.
#[test]
fn decal_draw_point_needs_the_face_not_just_its_plane() {
    let bsp = synthetic_bsp();
    assert!(
        bsp.decal_draw_point(&[98.0, 500.0, 32.0], 4.0, 1.0)
            .is_none()
    );
}

/// The whole bug in one assertion.
///
/// `synthetic_bsp` puts solid in front of the plane, so its face normal
/// points into the wall. Flipping `side` gives the real-map arrangement —
/// normal facing the room — which is what makes the difference visible: a
/// coordinate buried in the wall traces as blocked from anywhere in the
/// room, while the point the decal is actually drawn at traces clear.
#[test]
fn a_coordinate_inside_a_wall_is_blocked_but_the_decal_on_it_is_not() {
    let mut bsp = synthetic_bsp();
    bsp.faces[0].side = 1;
    assert_eq!(
        bsp.face_normal(0),
        Some([-1.0, 0.0, 0.0]),
        "normal must face the room"
    );

    let eye = [50.0, 32.0, 32.0];
    let buried = [101.0, 32.0, 32.0];

    // What the old test asked, and why it always said "safe": the wall the
    // decal renders on is itself the thing standing in the way.
    assert!(
        bsp.line_blocked(&eye, &buried),
        "a coordinate inside solid is occluded from everywhere, by construction"
    );

    // What the engine actually draws, and what the camera actually sees.
    let drawn = bsp
        .decal_draw_point(&buried, 4.0, 1.0)
        .expect("one unit inside a wall is well within reach of its face");
    assert!(
        (drawn[0] - 99.0).abs() < 1e-3,
        "drawn on the room side: {:?}",
        drawn
    );
    assert!(
        !bsp.line_blocked(&eye, &drawn),
        "nothing stands between the room and the face it is looking at"
    );
}

/// The fixture's one face is 64x64 at x=100, and its plane's front side is
/// SOLID — so its normal points *into* the wall, the opposite of a real
/// map. Flipping `side` puts the normal back where a room would be, which
/// is the only orientation these tests mean anything in: sampling along the
/// unflipped normal would bury every candidate in the brush and still
/// return the right *count*.
fn room_bsp() -> Bsp {
    let bsp = super::one_wall_room();
    assert_eq!(bsp.face_normal(0), Some([-1.0, 0.0, 0.0]));
    bsp
}

#[test]
fn face_candidates_land_off_the_surface_in_open_space() {
    let bsp = room_bsp();
    let pts = bsp.face_candidates(&FaceSampling::default());

    // 64 units at a 32 pitch samples at 16 and 48 on both axes; the third
    // step lands at 80, off the polygon.
    assert_eq!(pts.len(), 4, "{:?}", pts);
    for p in &pts {
        assert!(
            (p[0] - 98.0).abs() < 1e-3,
            "lifted 2 units into the room: {:?}",
            p
        );
        assert_ne!(
            bsp.leaf_contents(bsp.leaf_at(p)),
            CONTENTS_SOLID,
            "a candidate inside solid is exactly what the flush must never place: {:?}",
            p
        );
    }
}

#[test]
fn a_sampled_point_projects_back_onto_the_face_it_came_from() {
    // The whole source is worthless if the engine's projection sends these
    // somewhere other than the face they were measured on.
    let bsp = room_bsp();
    for p in bsp.face_candidates(&FaceSampling::default()) {
        let drawn = bsp
            .decal_draw_point(&p, 4.0, 1.0)
            .expect("a point 2 units off a face is within a 4-unit reach");
        assert!((drawn[0] - 99.0).abs() < 1e-3, "{:?} -> {:?}", p, drawn);
    }
}

#[test]
fn candidates_keep_their_distance_from_the_polygon_edges() {
    let bsp = room_bsp();
    // The four samples sit 16 units from the nearest edge, so an inset just
    // above that must clear the face out entirely — proving the check is
    // measuring the edge and not merely the bounding box.
    let tight = FaceSampling {
        inset: 20.0,
        ..FaceSampling::default()
    };
    assert!(bsp.face_candidates(&tight).is_empty());

    let loose = FaceSampling {
        inset: 15.0,
        ..FaceSampling::default()
    };
    assert_eq!(bsp.face_candidates(&loose).len(), 4);
}

#[test]
fn a_face_too_small_to_hold_a_decal_is_never_sampled() {
    let bsp = room_bsp();
    let opts = FaceSampling {
        min_area: 5000.0, // the face is 64x64 = 4096
        ..FaceSampling::default()
    };
    assert!(bsp.face_candidates(&opts).is_empty());
}

#[test]
fn one_face_cannot_spend_the_whole_budget() {
    let bsp = room_bsp();
    let opts = FaceSampling {
        pitch: 8.0,
        per_face: 3,
        ..FaceSampling::default()
    };
    assert_eq!(bsp.face_candidates(&opts).len(), 3);
}

#[test]
fn the_overall_limit_stops_the_scan() {
    let bsp = room_bsp();
    let opts = FaceSampling {
        limit: 2,
        ..FaceSampling::default()
    };
    assert_eq!(bsp.face_candidates(&opts).len(), 2);
}

#[test]
fn a_face_whose_front_is_sealed_yields_nothing() {
    // The unflipped fixture is a face whose outward normal points into
    // solid — a brush sealed against another one, or one facing the void
    // outside the hull. Real maps are full of them: sampling five DoD maps
    // put between 0.5% and 21% of raw samples inside solid. Offering those
    // to the flush would hand it the projection bug back, since a decal
    // aimed inside a wall is drawn on whichever face the engine reaches.
    let bsp = synthetic_bsp();
    assert_eq!(
        bsp.face_normal(0),
        Some([1.0, 0.0, 0.0]),
        "into the solid side"
    );
    assert!(bsp.face_candidates(&FaceSampling::default()).is_empty());
}

#[test]
fn a_face_that_holds_no_decal_is_never_sampled() {
    // Sky, liquid and trigger brushes take no decal at all, so a candidate
    // on one costs the sweep a ring slot and reports nothing.
    for name in ["sky_day", "!water", "aaatrigger", "clipbrush"] {
        let mut bsp = room_bsp();
        bsp.texture_names = vec![name.to_string()];
        assert!(
            bsp.face_candidates(&FaceSampling::default()).is_empty(),
            "sampled a {} face",
            name
        );
    }
}
