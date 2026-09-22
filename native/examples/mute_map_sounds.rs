//! Silences a map's own capture and round-win sounds in a demo, so the result
//! can be played back and listened to before any of this is wired into the
//! capture pipeline.
//!
//! The selection is read from the map, not from a filename list -- see
//! `native/src/patch/sound_mute.rs` for why, and for the measurements behind
//! it. `--list` stops after printing what it found, which is the fast way to
//! ask what a given map would lose.
//!
//!     cargo run --release -p native --example mute_map_sounds -- \
//!         demo.dem --maps-dir "<hl dir>/dod/maps" --out muted.dem
//!
//! Verify the result with the sound probe, which reports both carriers:
//!
//!     cargo run --release -p analysis --example svc_sound_probe -- muted.dem ambience

use std::collections::BTreeSet;
use std::path::PathBuf;

use native::patch::{MuteSelection, map_sounds, mute_sounds};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut demo: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut maps_dir: Option<PathBuf> = None;
    let mut list_only = false;
    let mut selection = MuteSelection::default();
    let mut selection_stated = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--maps-dir" => maps_dir = args.next().map(PathBuf::from),
            "--list" => list_only = true,
            "--captures" => {
                selection.capture = true;
                selection_stated = true;
            }
            "--win-music" => {
                selection.round_win = true;
                selection_stated = true;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}");
                std::process::exit(2);
            }
            other => demo = Some(PathBuf::from(other)),
        }
    }

    // Neither flag means both, which is what someone asking for a quiet demo
    // almost always wants.
    if !selection_stated {
        selection = MuteSelection {
            capture: true,
            round_win: true,
        };
    }

    let Some(demo_path) = demo else {
        eprintln!(
            "usage: mute_map_sounds <demo.dem> [--maps-dir DIR] [--out OUT.dem] \
             [--captures] [--win-music] [--list]"
        );
        std::process::exit(2);
    };

    let reference = match native::patch::map_reference(&demo_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("map: {}", reference.map_name);

    let Some(maps_dir) = maps_dir else {
        eprintln!("--maps-dir is required: the sounds come from the map, not from the demo");
        std::process::exit(2);
    };
    let bsp_path = native::patch::map_check::map_path(&maps_dir, &reference.map_name);
    let bsp_bytes = match std::fs::read(&bsp_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: {e}", bsp_path.display());
            std::process::exit(1);
        }
    };
    let entities = match native::patch::bsp_entities::parse_entities(&bsp_bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}: {e}", bsp_path.display());
            std::process::exit(1);
        }
    };

    let sounds = map_sounds(&entities);
    print_set("capture sounds", &sounds.capture);
    print_set("round-win sounds", &sounds.round_win);

    let wanted = sounds.selected(selection);
    println!(
        "\nselected to mute ({}): captures={} win music={}",
        wanted.len(),
        selection.capture,
        selection.round_win
    );
    if list_only {
        return;
    }
    if wanted.is_empty() {
        println!("nothing to mute; leaving the demo alone");
        return;
    }

    let Some(out_path) = out else {
        eprintln!("--out is required unless --list is given");
        std::process::exit(2);
    };

    let demo_bytes = match std::fs::read(&demo_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: {e}", demo_path.display());
            std::process::exit(1);
        }
    };
    let mut parsed = match dem::open_demo_from_bytes(&demo_bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: could not parse demo: {e}", demo_path.display());
            std::process::exit(1);
        }
    };

    let stats = mute_sounds(&mut parsed, &wanted);
    println!(
        "\nnopped {} svc_sound and {} svc_spawnstaticsound ({} total), \
         left {} in the signon block alone",
        stats.sounds,
        stats.static_sounds,
        stats.total(),
        stats.signon_skipped
    );
    print_set("names the demo actually precached", &stats.matched_names);

    if stats.total() == 0 {
        println!("\nnothing was playing in this demo; not writing an output file");
        return;
    }

    let bytes = parsed.write_to_bytes();
    match std::fs::write(&out_path, &bytes) {
        Ok(()) => println!("\nwrote {} ({} bytes)", out_path.display(), bytes.len()),
        Err(e) => {
            eprintln!("{}: {e}", out_path.display());
            std::process::exit(1);
        }
    }
}

fn print_set(label: &str, names: &BTreeSet<String>) {
    if names.is_empty() {
        println!("{label}: none");
        return;
    }
    println!("{label}:");
    for name in names {
        println!("  {name}");
    }
}
