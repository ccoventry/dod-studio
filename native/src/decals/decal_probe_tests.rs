use super::*;

fn opts_with(offsets: &[f32]) -> ProbeOptions {
    ProbeOptions {
        offsets: offsets.to_vec(),
        ..Default::default()
    }
}

#[test]
fn cluster_splits_only_on_gaps_wider_than_tolerance() {
    // Two surfaces 40 units apart, each with slight coordinate jitter.
    let values = [100.0, 100.5, 99.8, 140.0, 140.2];
    let groups = cluster(&values, 2.0);
    assert_eq!(groups.len(), 2);
    let sizes: Vec<usize> = groups.iter().map(|(_, m)| m.len()).collect();
    assert!(sizes.contains(&3) && sizes.contains(&2));
}

#[test]
fn connected_patches_separates_coplanar_but_distant_surfaces() {
    // The failure this exists to prevent: two floors at the same height in
    // different rooms read as one plane, and a grid centred between them
    // would hang in mid-air over neither.
    let members = [
        [0.0, 0.0, -384.0],
        [40.0, 0.0, -384.0],
        [80.0, 0.0, -384.0],
        [2000.0, 0.0, -384.0],
        [2040.0, 0.0, -384.0],
    ];
    let patches = connected_patches(&members, 160.0);
    assert_eq!(patches.len(), 2);
    assert_eq!(patches.iter().map(|p| p.len()).sum::<usize>(), 5);
}

#[test]
fn connected_patches_keeps_a_chain_together() {
    // Links are transitive: end-to-end distance may exceed the radius as
    // long as each hop does not, which is what lets one long wall stay one
    // patch.
    let members: Vec<[f32; 3]> = (0..6).map(|i| [i as f32 * 100.0, 0.0, 0.0]).collect();
    let patches = connected_patches(&members, 160.0);
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].len(), 6);
}

#[test]
fn densest_band_centres_on_the_busiest_window() {
    let values = [0.0, 200.0, 205.0, 210.0, 215.0, 500.0];
    let centre = densest_band(&values, 20.0).unwrap();
    assert!((200.0..=215.0).contains(&centre), "got {}", centre);
}

#[test]
fn lay_columns_spaces_evenly_and_reports_backing() {
    // Decals every 40 units across 320 units of wall, with a 120-unit hole
    // in the middle where a doorway would be.
    let mut band: Vec<[f32; 3]> = Vec::new();
    for i in 0..5 {
        band.push([i as f32 * 40.0, 0.0, 0.0]);
    }
    for i in 0..4 {
        band.push([320.0 + i as f32 * 40.0, 0.0, 0.0]);
    }

    let layout = lay_columns(&band, 0, 5, 40.0).expect("a layout exists");
    assert_eq!(layout.columns.len(), 5);
    assert_eq!(layout.evidence.len(), 5);

    // Evenly spaced, whatever pitch it settled on.
    for w in layout.columns.windows(2) {
        assert!(
            (w[1] - w[0] - layout.pitch).abs() < 0.01,
            "columns {:?} are not evenly spaced at pitch {}",
            layout.columns,
            layout.pitch
        );
    }
    // The left run alone seats all five columns, so it should find full
    // backing rather than straddling the gap.
    assert_eq!(layout.backed, 5);
    assert!(layout.evidence.iter().all(|e| *e <= layout.pitch / 2.0));
}

#[test]
fn lay_columns_may_interleave_between_decals_and_still_count_as_backed() {
    // Four decals over 120 units, five columns wanted. The row does not
    // have to land on them: sliding half a pitch puts every column between
    // two decals, which evidences surface either side of it just as well.
    let band: Vec<[f32; 3]> = (0..4).map(|i| [i as f32 * 40.0, 0.0, 0.0]).collect();
    let layout = lay_columns(&band, 0, 5, 40.0).expect("a layout exists");
    assert_eq!(layout.backed, 5);
    assert!(layout.evidence.iter().all(|e| *e <= layout.pitch / 2.0));
    assert!(layout.pitch >= MIN_COLUMN_SPACING);
}

#[test]
fn lay_columns_reports_a_column_it_cannot_back() {
    // Two decals 40 units apart cannot back five columns at any pitch down
    // to the floor, so the shortfall must be visible rather than silent.
    let band = [[0.0, 0.0, 0.0], [40.0, 0.0, 0.0]];
    let layout = lay_columns(&band, 0, 5, 40.0).expect("a layout exists");
    assert!(
        layout.backed < 5,
        "expected a shortfall, got {} at pitch {}",
        layout.backed,
        layout.pitch
    );
}

fn test_target() -> Target {
    Target {
        axis: 1,
        value: 448.0,
        col_axis: 0,
        row_axis: 2,
        columns: vec![100.0, 140.0, 180.0],
        evidence: vec![0.0; 3],
        backed: 3,
        pitch: 40.0,
        row_center: -200.0,
        members: vec![[100.0, 448.0, -200.0]],
        spread: (80.0, 0.0),
        witness_time: None,
    }
}

#[test]
fn build_probes_puts_the_rows_on_opposite_sides_of_the_surface() {
    let opts = opts_with(&[0.0, 8.0, 32.0]);
    let target = test_target();
    let probes = build_probes(&target, -1.0, &opts);
    assert_eq!(probes.len(), 9);

    for p in &probes {
        match p.row {
            // Outward is -Y here, so OUT must sit below the plane value
            // and IN above it — the sign is taken from where the camera
            // was, not assumed.
            ProbeRow::Out => assert!(p.position[1] <= target.value),
            ProbeRow::In => assert!(p.position[1] >= target.value),
            ProbeRow::Control => assert_eq!(p.position[1], target.value),
        }
        if p.offset == 0.0 {
            assert_eq!(
                p.position[1], target.value,
                "every row's zero column must sit on the surface"
            );
        }
    }
}

#[test]
fn build_probes_separates_the_rows_by_a_full_row_gap() {
    let opts = opts_with(&[0.0, 8.0]);
    let target = test_target();
    let probes = build_probes(&target, 1.0, &opts);

    let z_of = |row: ProbeRow| probes.iter().find(|p| p.row == row).unwrap().position[2];
    assert_eq!(z_of(ProbeRow::Control), target.row_center);
    assert!((z_of(ProbeRow::Out) - z_of(ProbeRow::Control) - opts.row_gap).abs() < 0.01);
    assert!((z_of(ProbeRow::Control) - z_of(ProbeRow::In) - opts.row_gap).abs() < 0.01);
}

#[test]
fn target_anchor_sits_on_the_surface_at_the_middle_of_the_grid() {
    let target = test_target();
    let anchor = target.anchor();
    assert_eq!(anchor[target.axis], target.value);
    assert_eq!(anchor[target.col_axis], 140.0);
    assert_eq!(anchor[target.row_axis], target.row_center);
}

#[test]
fn sightings_rank_near_and_centred_ahead_of_far_and_peripheral() {
    let close_centred = Sighting {
        svc_time: 0.0,
        distance: 80.0,
        off_axis_degrees: 3.0,
        yaw_degrees: -3.0,
        pitch_degrees: 0.0,
    };
    let far_peripheral = Sighting {
        svc_time: 0.0,
        distance: 700.0,
        off_axis_degrees: 22.0,
        yaw_degrees: 20.0,
        pitch_degrees: 8.0,
    };
    assert!(quality(&close_centred) > quality(&far_peripheral));
}

#[test]
fn descending_offsets_are_rejected() {
    let demo: &[u8] = b"not a demo";
    let opts = opts_with(&[8.0, 4.0]);
    let err = probe_decal_offsets(demo, &opts).unwrap_err();
    assert!(err.contains("ascending"), "got: {}", err);
}
