//! Surveys how much camera data demos carry (#920): per demo, how many of its
//! playback network frames name a view entity or a view origin, how many
//! ClientData frames carry a field of view, and how far the frame clock moves.
//! A demo rebuilt from server traffic has every one of them zero.
//!
//!     cargo run --release -p analysis --example view_data_survey -- <folder-or-demo>...
//!
//! Prints one tab-separated line per demo.

use dem::open_demo_from_bytes;
use dem::types::FrameData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn demos_in(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(read) = std::fs::read_dir(path) else {
        return;
    };
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_dir() {
            demos_in(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")) {
            out.push(p);
        }
    }
}

fn nonzero(v: &[f32]) -> bool {
    v.iter().any(|x| *x != 0.0)
}

fn measure(path: &Path) -> String {
    let name = path.display();
    let Ok(bytes) = std::fs::read(path) else {
        return format!("{name}\tunreadable");
    };
    let Ok(demo) = open_demo_from_bytes(&bytes) else {
        return format!("{name}\tdoes not parse");
    };
    let info = analysis::DemoInfo::from(&demo);
    let (mut net, mut with_entity, mut with_origin) = (0usize, 0usize, 0usize);
    let (mut client, mut client_fov) = (0usize, 0usize);
    let (mut first, mut last) = (f32::MAX, f32::MIN);
    // Entry 0 is the LOADING segment, whose frames carry no view yet.
    for entry in demo.directory.entries.iter().skip(1) {
        for frame in &entry.frames {
            first = first.min(frame.time);
            last = last.max(frame.time);
            match &frame.frame_data {
                FrameData::NetworkMessage(boxed) => {
                    net += 1;
                    let rp = &boxed.1.info.refparams;
                    with_entity += usize::from(rp.view_entity != 0);
                    with_origin += usize::from(nonzero(&rp.view_origin));
                }
                FrameData::ClientData(cd) => {
                    client += 1;
                    client_fov += usize::from(cd.fov != 0.0);
                }
                _ => {}
            }
        }
    }
    let span = if last >= first { last - first } else { 0.0 };
    format!(
        "{name}\t{}\tplayback {:.1}s/{} frames\tspan {:.1}s\tnet {net} entity {with_entity} origin {with_origin}\tclientdata {client} fov {client_fov}",
        info.demo_type, info.playback_time, info.playback_frames, span
    )
}

fn main() {
    let mut demos = Vec::new();
    for arg in std::env::args().skip(1) {
        demos_in(Path::new(&arg), &mut demos);
    }
    let queue = Mutex::new(demos.into_iter());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some(path) = queue.lock().unwrap().next() else {
                        break;
                    };
                    println!("{}", measure(&path));
                }
            });
        }
    });
}
