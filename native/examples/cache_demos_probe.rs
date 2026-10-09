//! Runs the Demo Analyzer's Cache all (#569) on a folder from the command
//! line, printing each step:
//!
//!     cargo run -p native --release --example cache_demos_probe -- <folder> [limit]
//!
//! Writes into the real analyzer cache, as the button does.

fn main() {
    let mut args = std::env::args().skip(1);
    let folder = std::path::PathBuf::from(
        args.next()
            .expect("usage: cache_demos_probe <folder> [limit]"),
    );
    let limit: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let mut demos: Vec<_> = std::fs::read_dir(&folder)
        .expect("folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
        .collect();
    demos.sort();
    demos.truncate(limit);
    let started = std::time::Instant::now();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let last = native::demo_cache::cache_demos(demos, cancel, |p| println!("{p:?}"));
    println!("{:.1} s: {last:?}", started.elapsed().as_secs_f32());
}
