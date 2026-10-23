//! Builds the name table behind `ds_sound_mute_win_and_capture`
//! (`goldsrc-hooks/src/map_sounds.rs`, #284) from the maps on disk.
//!
//! The hook matches a sample name against a fixed list, because the DLL has
//! no BSP parser. This walks every `.bsp` in the folders given, runs the same
//! structural selection the demo patcher uses (`native::patch::map_sounds`:
//! capture keyvalues, and the win music the round-end wiring fires), and
//! prints the union with a map count per name. It also prints every name that
//! some *other* map places as plain ambience, since muting one of those would
//! silence that map's atmosphere: such a name must not go in the table.
//!
//!     cargo run --release -p native --example map_sound_table -- \
//!         "<PRE>/dod/maps" "<POST>/dod/maps"

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use native::patch::bsp_entities::parse_entities;
use native::patch::map_sounds;

fn main() {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: map_sound_table <maps dir>...");
        std::process::exit(2);
    }

    // name -> maps, by kind
    let mut capture: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut win: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // every ambient_generic message, with the maps that use it
    let mut ambient: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut maps_read = BTreeSet::new();
    let mut failed = 0;

    for dir in &dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("{}: {e}", dir.display());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
                != Some("bsp".into())
            {
                continue;
            }
            let map = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !maps_read.insert(map.clone()) {
                continue; // the same map in a second folder
            }
            let entities = match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|b| parse_entities(&b))
            {
                Ok(entities) => entities,
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    failed += 1;
                    continue;
                }
            };
            let sounds = map_sounds(&entities);
            for name in sounds.capture {
                capture.entry(name).or_default().insert(map.clone());
            }
            for name in sounds.round_win {
                win.entry(name).or_default().insert(map.clone());
            }
            for entity in &entities {
                if entity.classname() == "ambient_generic"
                    && let Some(message) = entity.get("message")
                {
                    let name = message.replace('\\', "/").to_ascii_lowercase();
                    ambient.entry(name).or_default().insert(map.clone());
                }
            }
        }
    }

    println!("maps read: {} ({failed} unreadable)", maps_read.len());
    let wanted: BTreeSet<&String> = capture.keys().chain(win.keys()).collect();

    println!("\n== round-win music ({} names)", win.len());
    for (name, maps) in &win {
        println!("{:4}  {name}", maps.len());
    }
    println!("\n== capture sounds ({} names)", capture.len());
    for (name, maps) in &capture {
        println!("{:4}  {name}", maps.len());
    }

    // A wanted name some map uses as an ambient_generic that is *not* its
    // win music: that map would lose atmosphere if the name were muted.
    println!("\n== wanted names also placed as other ambience");
    for name in &wanted {
        let Some(maps) = ambient.get(*name) else {
            continue;
        };
        let as_win = win.get(*name).cloned().unwrap_or_default();
        let other: Vec<&String> = maps.difference(&as_win).collect();
        if !other.is_empty() {
            println!("{name}: {} maps, e.g. {:?}", other.len(), &other[..other.len().min(5)]);
        }
    }

    println!("\n== table");
    for name in &wanted {
        println!("    \"{name}\",");
    }
}
