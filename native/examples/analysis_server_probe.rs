//! Serves Studio's analysis pipe (#565) without the app, for testing the
//! game's Highlights tab against it:
//!
//!     set DOD_STUDIO_LOG_DIR=<scratch dir>
//!     cargo run -p native --release --example analysis_server_probe -- [seconds]
//!
//! (`DOD_STUDIO_LOG_DIR` keeps its log lines out of your real activity log.)
//! Exits after `seconds` (default 300).

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    native::sys::analysis_server::start();
    println!(
        "serving {} for {seconds} s",
        native::sys::analysis_server::PIPE_NAME
    );
    std::thread::sleep(std::time::Duration::from_secs(seconds));
}
