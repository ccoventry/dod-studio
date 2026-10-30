//! Which demos in a folder can actually be played, and on the right map build.
//!
//! ```text
//! cargo run --release -p native --bin check_maps -- <demo-or-folder> [maps-dir] [--fetch]
//! ```
//!
//! `maps-dir` defaults to `<folder>/maps`, because demos live inside the `dod`
//! folder and the maps sit beside them. `--fetch` downloads whatever is missing
//! or is the wrong build, verifying each one against the demo that wants it
//! before it is installed.

use native::messages;
use native::patch::map_check::{MapStatus, check_demo};
use native::patch::map_fetch::{DEFAULT_MIRROR, fetch_map, map_url};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fetch = args.iter().any(|a| a == "--fetch");
    let mut args = args.into_iter().filter(|a| !a.starts_with("--"));
    let target = std::path::PathBuf::from(args.next().expect(messages::CHECK_MAPS_USAGE));

    let demos: Vec<std::path::PathBuf> = if target.is_dir() {
        let mut found: Vec<_> = std::fs::read_dir(&target)
            .expect(messages::MAPS_READ_FOLDER_FAILED)
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
            .collect();
        found.sort();
        found
    } else {
        vec![target.clone()]
    };

    let maps_dir = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let base = if target.is_dir() {
                target.clone()
            } else {
                target.parent().map(|p| p.to_path_buf()).unwrap_or_default()
            };
            base.join("maps")
        });

    println!("{}", messages::maps_dir_line(maps_dir.display()));
    println!("{}", messages::maps_demo_count(demos.len()));

    let (mut ok, mut wrong, mut missing, mut unverifiable, mut unreadable, mut failed) =
        (0, 0, 0, 0, 0, 0);
    // Map name -> how many demos want it, and the build they want. Demos of the
    // same map should agree; if two disagree the first is kept and the clash is
    // reported, because installing either would leave the other broken.
    let mut needed: std::collections::BTreeMap<String, (usize, Option<u32>)> =
        std::collections::BTreeMap::new();

    for demo in &demos {
        let name = demo.file_name().unwrap_or_default().to_string_lossy();
        match check_demo(demo, &maps_dir) {
            Ok((reference, status)) => {
                match &status {
                    MapStatus::Ok { .. } => ok += 1,
                    MapStatus::WrongBuild { .. } => {
                        wrong += 1;
                        let e = needed.entry(reference.map_name.clone()).or_default();
                        e.0 += 1;
                        e.1 = e.1.or(reference.expected_checksum);
                    }
                    MapStatus::Missing => {
                        missing += 1;
                        let e = needed.entry(reference.map_name.clone()).or_default();
                        e.0 += 1;
                        e.1 = e.1.or(reference.expected_checksum);
                    }
                    MapStatus::Unverifiable => unverifiable += 1,
                    MapStatus::Unreadable { .. } => unreadable += 1,
                }
                if !matches!(status, MapStatus::Ok { .. }) {
                    println!(
                        "{}",
                        messages::maps_demo_status(&name, status.summary(&reference.map_name))
                    );
                }
            }
            Err(e) => {
                failed += 1;
                println!("{}", messages::maps_unreadable_demo(&name, e));
            }
        }
    }

    println!(
        "{}",
        messages::maps_totals(ok, wrong, missing, unverifiable, unreadable, failed)
    );
    if needed.is_empty() {
        return;
    }

    println!("{}", messages::MAPS_TO_FETCH);
    for (map, (count, want)) in &needed {
        let url = map_url(DEFAULT_MIRROR, map).unwrap_or_else(|e| e);
        match want {
            Some(sum) => println!("{}", messages::maps_to_fetch_row(map, *sum, *count, url)),
            None => println!("{}", messages::maps_to_fetch_row_unstated(map, *count, url)),
        }
    }

    if !fetch {
        println!("{}", messages::MAPS_PASS_FETCH);
        return;
    }

    println!();
    for (map, (_, want)) in &needed {
        match fetch_map(map, *want, &maps_dir, DEFAULT_MIRROR) {
            Ok(o) if o.already_correct => println!("{}", messages::maps_already_correct(map)),
            Ok(o) => {
                println!(
                    "{}",
                    messages::maps_installed(
                        map,
                        o.checksum,
                        o.bytes as f64 / (1024.0 * 1024.0),
                        &o.replaced
                            .map(|p| messages::maps_previous_kept_at(p.display()))
                            .unwrap_or_default()
                    )
                );
            }
            Err(e) => println!("{}", messages::maps_fetch_failed(map, e)),
        }
    }
}
