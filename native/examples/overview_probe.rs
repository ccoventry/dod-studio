//! Builds the Overviews page's scene for some maps and writes each as JSON,
//! with how long it took and how much is reachable -- for checking the port
//! against `local/overview-rnd` and for drawing the scene outside the app.
//!
//! cargo run -p native --release --example overview_probe -- <install> <out dir> <map>...

use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: overview_probe <install> <out dir> <map>...");
        std::process::exit(2);
    }
    let install = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    std::fs::create_dir_all(out).expect("out dir");
    for map in &args[2..] {
        let started = Instant::now();
        let bsp = install.join("dod").join("maps").join(format!("{map}.bsp"));
        let level = match native::overview::level::Level::from_file(&bsp) {
            Ok(level) => level,
            Err(e) => {
                println!("{map}: {e}");
                continue;
            }
        };
        let reach = native::overview::reach::build(&level).expect("reach");
        let reached = reach.reached.iter().filter(|r| **r).count();
        let fits = reach.fits.iter().filter(|f| **f).count();
        match native::overview::scene_for(install, map) {
            Ok(scene) => {
                let t = scene.transform;
                println!(
                    "{map}: {} floors, {fits} of {} nodes fit, {reached} reachable; {} faces drawn, {} areas, {} cap zones, {} flags; ZOOM {} ORIGIN {} {} ROTATED {} -- {:.1}s",
                    reach.floors.len(),
                    reach.cell.len(),
                    scene.faces.len(),
                    scene.areas.len(),
                    scene.cap_zones.len(),
                    scene.flags.len(),
                    t.zoom,
                    t.origin[0],
                    t.origin[1],
                    t.rotated as i32,
                    started.elapsed().as_secs_f32()
                );
                let json = serde_json::to_string(&scene).expect("json");
                std::fs::write(out.join(format!("{map}.json")), json).expect("write");
            }
            Err(e) => println!("{map}: {e}"),
        }
    }
}
