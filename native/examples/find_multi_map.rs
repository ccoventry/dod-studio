//! #624: list the demos under folders that recorded more than one map, the
//! way the Demo Auditor's Split tab does.
//!
//!     cargo run --release -p native --example find_multi_map -- <folder>... [--check]
//!
//! `--check` also fully parses each one found and compares the map lists.

use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let cancel = AtomicBool::new(false);
    for folder in args.iter().filter(|a| !a.starts_with("--")) {
        let started = std::time::Instant::now();
        let mut seen = 0;
        let found = native::demo_maps_scan::find_multi_map_demos(
            std::path::Path::new(folder),
            true,
            &native::analyzer_cache_root(),
            &cancel,
            &mut |done, _, _| seen = done,
        );
        println!(
            "== {folder}: {seen} demos, {} with more than one map, {:.1}s",
            found.len(),
            started.elapsed().as_secs_f32()
        );
        for d in &found {
            print!("  {} [{}] {:?}", d.path, d.source, d.maps);
            if check {
                match native::demo_split::demo_segments(std::path::Path::new(&d.path)) {
                    Ok(segs) => {
                        let maps: Vec<String> = segs.iter().map(|s| s.map.clone()).collect();
                        let lens: Vec<String> = segs
                            .iter()
                            .map(|s| format!("{:.0}s", s.seconds()))
                            .collect();
                        print!(
                            " parse {} {:?}",
                            if maps == d.maps { "AGREES" } else { "DIFFERS" },
                            lens
                        );
                    }
                    Err(e) => print!(" parse FAILED {e}"),
                }
            }
            println!();
        }
    }
}
