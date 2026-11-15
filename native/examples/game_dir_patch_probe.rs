//! R&D probe (#402): writes one patched copy of a demo with no highlights, so
//! the copy's header and svc_serverinfo name a given game folder, for an
//! in-game check that a copy of DoD plays it.
//!
//!     cargo run -p native --example game_dir_patch_probe -- <folder> <in.dem> <out.dem>

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: game_dir_patch_probe <game folder> <in.dem> <out.dem>");
        std::process::exit(2);
    }
    native::shared::paths::set_game_folder(&args[1]).expect("game folder");
    let job = native::patch::types::PatchJob {
        source_demo: args[2].clone(),
        output_demo: std::path::PathBuf::from(&args[3]),
        streaks: Vec::new(),
        target_player: None,
        init_commands: Vec::new(),
        scheduled_commands: Vec::new(),
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };
    let config = native::patch::types::PatcherConfig::default();
    let cancel = Arc::new(AtomicBool::new(false));
    native::patch::engine::StreamPatcher::new(&args[2], &args[3])
        .patch(&job, &config, &cancel)
        .expect("patch");
    let bytes = std::fs::read(&args[3]).expect("read output");
    let header_dir = native::patch::game_dir::header_game_dir(&bytes).unwrap_or(b"");
    let protocol = native::patch::game_dir::header_network_protocol(&bytes).unwrap_or(0);
    let still_dod =
        native::patch::game_dir::rewrite_server_info_game_dir(&bytes, protocol, &args[1]);
    println!(
        "header game dir: {}; svc_serverinfo still needing a rename: {}; maps: {:?}",
        String::from_utf8_lossy(header_dir),
        still_dod.is_some(),
        native::demo_split::quick_maps(&bytes, protocol)
    );
}
