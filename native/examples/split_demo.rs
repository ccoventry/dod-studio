//! Split a multi-map demo into one demo per map (#624), and check each.
//!
//!     cargo run --release -p native --example split_demo -- <in.dem> <out dir> [map index...]
//!
//! With no indices it only lists the maps (from the quick byte scan and the
//! full parse, which must agree).

use native::demo_split::{extract, header_protocol, quick_maps, segments, split_name};

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .expect("usage: split_demo <in.dem> <out dir> [map index...]");
    let out_dir = args.next();
    let keep: Vec<usize> = args.filter_map(|a| a.parse().ok()).collect();
    let bytes = std::fs::read(&input).expect("read");
    let t = std::time::Instant::now();
    let quick = quick_maps(&bytes, header_protocol(&bytes).unwrap_or(48));
    println!(
        "quick scan: {quick:?} in {:.0} ms",
        t.elapsed().as_secs_f32() * 1000.0
    );
    let demo = dem::open_demo_from_bytes(&bytes).expect("parse");
    let segs = segments(&demo);
    for s in &segs {
        println!(
            "map {}: {} crc {:#x}, {:.1}s to {:.1}s ({:.0}s), {} frames",
            s.index,
            s.map,
            s.map_checksum,
            s.start_seconds,
            s.end_seconds,
            s.seconds(),
            s.frames
        );
    }
    let Some(out_dir) = out_dir else { return };
    let stem = std::path::Path::new(&input)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    for i in keep {
        let find_bsp = |map: &str| {
            let dir = std::path::Path::new(&input).parent()?;
            let bytes = std::fs::read(dir.join("maps").join(format!("{map}.bsp"))).ok()?;
            native::patch::bsp::map_checksum(&bytes).ok()
        };
        let Some(part) = extract(&demo, i, &find_bsp) else {
            println!("map {i}: no such map");
            continue;
        };
        let out = part
            .write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
            .expect("write");
        let name = split_name(&stem, &segs, &segs[i]);
        let path = std::path::Path::new(&out_dir).join(format!("{name}.dem"));
        std::fs::write(&path, &out).expect("write file");
        let back = dem::open_demo_from_bytes(&out).expect("re-parse");
        let back_segs = segments(&back);
        let analysis = analysis::Analysis::try_from_bytes(&out);
        println!(
            "wrote {} ({:.1} MB): {} map(s) {:?}, header {:?}; analysis: {}",
            path.display(),
            out.len() as f64 / 1e6,
            back_segs.len(),
            back_segs
                .iter()
                .map(|s| (&s.map, s.seconds().round()))
                .collect::<Vec<_>>(),
            String::from_utf8_lossy(&back.header.map_name.0).trim_end_matches('\0'),
            match analysis {
                Ok(a) => format!(
                    "ok, map {:?}, {} players, {} kills, {:.0}s, type {}",
                    a.state.initial_map_name,
                    a.state.players.len(),
                    a.state
                        .players
                        .iter()
                        .map(|p| p.stats.0.max(0))
                        .sum::<i32>(),
                    a.demo_info.playback_time,
                    a.demo_info.demo_type
                ),
                Err(e) => format!("FAILED {e}"),
            }
        );
    }
}
