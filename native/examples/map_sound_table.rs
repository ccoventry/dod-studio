//! Builds the two name tables behind `ds_sound_mute_victory_music` and
//! `ds_sound_mute_flag_capture` (`goldsrc-hooks/src/map_sounds.rs`, #284)
//! from the maps on disk.
//!
//! The hook classifies each map's own sounds as it loads, and keeps these
//! lists as the fallback for a map it can't read. This walks every `.bsp` in
//! the folders given, runs the same structural selection the hook and the
//! demo patcher use (`dod::map_sounds`, re-exported as
//! `native::patch::map_sounds`: the win music the round-end wiring fires, and
//! capture keyvalues), and
//! prints each family with a map count per name, the maps too when there are
//! few. A name in both families is printed separately, since it can only sit
//! in one list.
//!
//! It also prints what fires each victory name (the master's Allies or Axis
//! target, the round timer's, or a relay in between), so a name the round
//! timer fires can be checked by ear, and every name that some *other* map
//! places as plain ambience, since muting one of those could silence that
//! map's atmosphere: check what each is there before it goes in a table. The
//! three it prints today (dod_vicenza's `uswin` / `germanwin`, dod_escape's
//! `usobjectivesecure`) are victory music and an objective announcement there
//! too, wired another way, so they stay in their lists.
//!
//!     cargo run --release -p native --example map_sound_table -- \
//!         "<PRE>/dod/maps" "<POST>/dod/maps"

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use native::patch::bsp_entities::{MapEntity, parse_entities};
use native::patch::map_sounds;

/// Maps listed by name when a sound is used by this many or fewer.
const LIST_MAPS_UP_TO: usize = 3;

type ByName = BTreeMap<String, BTreeSet<String>>;

fn normalise(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

/// What fires each `ambient_generic` the round-end wiring reaches directly:
/// sound name -> "allies", "axis" or "timer". A name reached only through a
/// relay is missing here.
fn direct_firers(entities: &[MapEntity]) -> BTreeMap<String, BTreeSet<&'static str>> {
    let mut targets: Vec<(&str, &'static str)> = Vec::new();
    for entity in entities {
        match entity.classname() {
            "dod_control_point_master" => {
                targets.extend(entity.get("allies_capture_target").map(|t| (t, "allies")));
                targets.extend(entity.get("axis_capture_target").map(|t| (t, "axis")));
                targets.extend(entity.get("target").map(|t| (t, "master")));
            }
            "dod_round_timer" => targets.extend(entity.get("target").map(|t| (t, "timer"))),
            _ => {}
        }
    }
    let mut out: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for entity in entities {
        if entity.classname() != "ambient_generic" {
            continue;
        }
        let (Some(name), Some(message)) = (entity.get("targetname"), entity.get("message")) else {
            continue;
        };
        for (target, by) in &targets {
            if *target == name {
                out.entry(normalise(message)).or_default().insert(by);
            }
        }
    }
    out
}

fn print_family(title: &str, names: &ByName) {
    println!("\n== {title} ({} names)", names.len());
    for (name, maps) in names {
        if maps.len() <= LIST_MAPS_UP_TO {
            let maps: Vec<&str> = maps.iter().map(String::as_str).collect();
            println!("{:4}  {name}  ({})", maps.len(), maps.join(", "));
        } else {
            println!("{:4}  {name}", maps.len());
        }
    }
}

fn print_table(constant: &str, names: &ByName) {
    println!("\n== table {constant}");
    for name in names.keys() {
        println!("    \"{name}\",");
    }
}

fn main() {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: map_sound_table <maps dir>...");
        std::process::exit(2);
    }

    // name -> maps, by family
    let mut capture: ByName = BTreeMap::new();
    let mut win: ByName = BTreeMap::new();
    // victory name -> what fires it -> maps
    let mut fired_by: BTreeMap<String, BTreeMap<&'static str, BTreeSet<String>>> = BTreeMap::new();
    // every ambient_generic message, with the maps that use it
    let mut ambient: ByName = BTreeMap::new();
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
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
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
            let firers = direct_firers(&entities);
            for name in sounds.round_win {
                let by = firers
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| ["relay"].into());
                for firer in by {
                    fired_by
                        .entry(name.clone())
                        .or_default()
                        .entry(firer)
                        .or_default()
                        .insert(map.clone());
                }
                win.entry(name).or_default().insert(map.clone());
            }
            for name in sounds.capture {
                capture.entry(name).or_default().insert(map.clone());
            }
            for entity in &entities {
                if entity.classname() == "ambient_generic"
                    && let Some(message) = entity.get("message")
                {
                    ambient
                        .entry(normalise(message))
                        .or_default()
                        .insert(map.clone());
                }
            }
        }
    }

    println!("maps read: {} ({failed} unreadable)", maps_read.len());
    let wanted: BTreeSet<&String> = capture.keys().chain(win.keys()).collect();

    print_family("victory music", &win);
    print_family("capture announcements", &capture);

    println!("\n== in both families (pick one list)");
    for name in win.keys().filter(|n| capture.contains_key(*n)) {
        println!("{name}");
    }

    // The master's own targets are the winner's music; anything else is worth
    // a listen before it goes in the victory list.
    println!("\n== victory names not fired by the master's allies/axis targets");
    for (name, by) in &fired_by {
        let other: Vec<String> = by
            .iter()
            .filter(|(firer, _)| !matches!(**firer, "allies" | "axis"))
            .map(|(firer, maps)| {
                let maps: Vec<&str> = maps.iter().map(String::as_str).collect();
                format!("{firer}: {}", maps.join(", "))
            })
            .collect();
        if !other.is_empty() {
            println!("{name}  {}", other.join("; "));
        }
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
            println!(
                "{name}: {} maps, e.g. {:?}",
                other.len(),
                &other[..other.len().min(5)]
            );
        }
    }

    print_table("VICTORY_MUSIC", &win);
    print_table("FLAG_CAPTURE", &capture);
}
