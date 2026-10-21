//! Did the recording player have DoD's overview map up? (issue #897)
//!
//! Every network frame of a demo carries the `ref_params_s` the recorder's
//! `V_CalcRefdef` worked out for the main view. `client.dll` sets its
//! `nextView` to 1 whenever `_cl_minimap` asks for a map (`+map`, key `m`), so
//! that the engine renders a second, map-only view; `hw.dll` records the
//! struct after that call. So the share of frames with `nextView` 1 is the
//! share of the recording with the map up.
//!
//! In POV playback `V_RenderView` copies the recorded struct over the live one
//! and then zeroes `nextView`, which is why no map is drawn there.
//!
//!     cargo run --release -p analysis --example nextview_probe -- <demo>...

use dem::open_demo_from_bytes;
use dem::types::FrameData;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: nextview_probe <demo>...");
        std::process::exit(2);
    }
    println!("frames\tnext_view\tonly_client\tspectator\tdemo");
    for path in &paths {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        let demo = match open_demo_from_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{path}: could not parse ({e:?})");
                continue;
            }
        };
        let (mut frames, mut next, mut only, mut spec) = (0u64, 0u64, 0u64, 0u64);
        for entry in &demo.directory.entries {
            for frame in &entry.frames {
                let FrameData::NetworkMessage(bt) = &frame.frame_data else {
                    continue;
                };
                let p = &bt.1.info.refparams;
                frames += 1;
                next += u64::from(p.next_view != 0);
                only += u64::from(p.only_client_draw != 0);
                spec += u64::from(p.spectator != 0);
            }
        }
        let name = std::path::Path::new(path)
            .file_name()
            .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
        println!("{frames}\t{next}\t{only}\t{spec}\t{name}");
    }
}
