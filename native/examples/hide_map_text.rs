//! Hides a map's own on-screen text in a demo, so the result can be watched
//! before any of this is wired into the capture pipeline.
//!
//! The strings come from the map's entity lump -- `dod_score_ent`'s `message`
//! for the round result, `env_message`'s for a hint like the anzio mortar
//! warning -- and are matched against the `HudText` messages the demo carries.
//! See `native/src/patch/map_text.rs` for why that is the channel, and
//! `analysis/examples/map_text_probe.rs` for the measurement.
//!
//!     cargo run --release -p native --example hide_map_text -- \
//!         demo.dem --maps-dir "<hl dir>/dod/maps" --out quiet.dem
//!
//! `--list` stops after printing what the map declares, without writing.
//! Pair it with `mute_map_sounds` for a demo that is quiet in both senses.

use std::collections::BTreeSet;
use std::path::PathBuf;

use native::patch::{hide_map_text, map_text, TextSelection};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut demo: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut maps_dir: Option<PathBuf> = None;
    let mut list_only = false;
    let mut selection = TextSelection::default();
    let mut selection_stated = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--maps-dir" => maps_dir = args.next().map(PathBuf::from),
            "--list" => list_only = true,
            "--round-result" => {
                selection.round_result = true;
                selection_stated = true;
            }
            "--hints" => {
                selection.hints = true;
                selection_stated = true;
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}");
                std::process::exit(2);
            }
            other => demo = Some(PathBuf::from(other)),
        }
    }

    if !selection_stated {
        selection = TextSelection { round_result: true, hints: true };
    }

    let Some(demo_path) = demo else {
        eprintln!(
            "usage: hide_map_text <demo.dem> [--maps-dir DIR] [--out OUT.dem] \
             [--round-result] [--hints] [--list]"
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
        eprintln!("--maps-dir is required: the strings come from the map, not from the demo");
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

    let text = map_text(&entities);
    print_set("round-result text", &text.round_result);
    print_set("hints and warnings", &text.hints);

    let wanted = text.selected(selection);
    println!(
        "\nselected to hide ({}): round result={} hints={}",
        wanted.len(),
        selection.round_result,
        selection.hints
    );
    if list_only {
        return;
    }
    if wanted.is_empty() {
        println!("nothing to hide; leaving the demo alone");
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

    let stats = hide_map_text(&mut parsed, &wanted);
    println!("\nnopped {} HudText messages", stats.hidden);
    print_set("strings the demo actually carried", &stats.matched);

    if stats.hidden == 0 {
        println!("\nnone of them appeared in this demo; not writing an output file");
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
