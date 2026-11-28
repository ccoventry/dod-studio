//! Prints every brush entity's spawn baseline in a demo as TSV: its `*N`
//! model, bounds, origin and render settings, so the demo can be matched
//! piece by piece against a map file on disk. Used to tell whether a demo was
//! recorded on a different build of the map, since HLTV demos zero the header's
//! map checksum.
//!
//! With a map file as well, prints what the hook DLL's `ds_map_fix_old_build`
//! does with each entity on that map instead (`dod::brush_pieces`, #1540):
//! the piece it fits, the action, and a summary line.
//!
//!     cargo run --release -p analysis --example brush_bounds_probe -- <demo> [<bsp>]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "dev-only probe run by hand: a panic on bad input is its error report (#1179)"
)]

use analysis::brush_baselines::{BrushBaseline, brush_baselines};
use dod::brush_pieces::{self, Action, Fit, Piece, Spawned};

fn vec3(v: [f32; 3]) -> String {
    format!("{} {} {}", v[0], v[1], v[2])
}

fn print_baselines(rows: &[BrushBaseline]) {
    println!(
        "ent\tmodel\tmins\tmaxs\torigin\trendermode\trenderamt\trenderfx\tskin\tsolid\teffects\tpackets"
    );
    for r in rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.entity,
            r.spawned.named,
            vec3(r.spawned.bounds.mins),
            vec3(r.spawned.bounds.maxs),
            vec3(r.spawned.origin),
            r.rendermode,
            r.renderamt,
            r.renderfx,
            r.skin,
            r.solid,
            r.effects,
            r.packets
        );
    }
}

fn print_plan(rows: &[BrushBaseline], pieces: &[Piece]) {
    let spawned: Vec<Spawned> = rows.iter().map(|r| r.spawned).collect();
    let plan = brush_pieces::plan(&spawned, pieces);
    println!("ent\tnamed\tfit\taction\trendermode\trenderamt\tpackets");
    for ((r, action), s) in rows.iter().zip(&plan.actions).zip(&spawned) {
        let fit = match brush_pieces::fit(s, pieces) {
            Fit::Named => "named".to_string(),
            Fit::Other(n) => format!("*{n}"),
            Fit::Several => "several".to_string(),
            Fit::Nothing => "nothing".to_string(),
            Fit::NoBounds => "no bounds".to_string(),
        };
        let action = match action {
            Action::Keep => "keep".to_string(),
            Action::Draw(n) => format!("draw *{n}"),
            Action::Hide => "hide".to_string(),
        };
        println!(
            "{}\t*{}\t{fit}\t{action}\t{}\t{}\t{}",
            r.entity, s.named, r.rendermode, r.renderamt, r.packets
        );
    }
    let c = plan.counts;
    println!(
        "# {} pieces in the map (with the world); {} brush entities: {} fit their own piece, \
         {} another, {} several, {} none, {} carry no bounds; other build: {}",
        pieces.len(),
        rows.len(),
        c.named,
        c.other,
        c.several,
        c.nothing,
        c.no_bounds,
        c.is_other_build()
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let demo = args
        .next()
        .expect("usage: brush_bounds_probe <demo> [<bsp>]");
    let bytes = std::fs::read(&demo).expect("read demo");
    let rows = brush_baselines(&dem::open_demo_from_bytes(&bytes).expect("parse demo"));
    match args.next() {
        None => print_baselines(&rows),
        Some(bsp) => {
            let map = std::fs::read(&bsp).expect("read map");
            print_plan(
                &rows,
                &brush_pieces::pieces_from_bsp(&map).expect("parse map"),
            );
        }
    }
}
