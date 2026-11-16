//! The Demo Analyzer's Convert to protocol 48 (#239): a protocol-47 demo
//! (2004 and a little after) gets a protocol-48 copy beside it, which today's
//! game plays. The work is `native::patch::protocol47`; this is the IPC and
//! the activity log around it.

use std::path::Path;
use std::time::Instant;

use native::utils::duration_text::duration_text;

/// Converts the protocol-47 demo at `path` into `<name>_p48.dem` beside it
/// and returns the copy's path. The demo itself is only read. Reads and
/// writes the whole file once, off the main thread.
#[tauri::command]
pub async fn convert_protocol_47(path: String) -> Result<String, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        convert_with_log(Path::new(&path))
    }))
    .await
}

/// One conversion with its activity-log lines: started, then done or failed,
/// with how long it took.
fn convert_with_log(path: &Path) -> Result<String, String> {
    use crate::messages::protocol47 as m;
    let shown = path.display().to_string();
    let started = Instant::now();
    native::log_markdown(&m::convert_started(&shown));
    let outcome = native::patch::protocol47::convert_file(path, None);
    let took = duration_text(started.elapsed().as_secs_f64());
    match outcome {
        Ok((written, converted)) => {
            let written = written.display().to_string();
            native::log_markdown(&m::convert_done(
                &shown,
                &written,
                converted.server_infos,
                &took,
            ));
            Ok(written)
        }
        Err(e) => {
            let e = e.to_string();
            native::log_markdown(&m::convert_failed(&shown, &e, &took));
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_demo_that_is_not_protocol_47_is_refused_with_the_reason() {
        let scratch = crate::test_support::Scratch::new("convert47");
        let dir = scratch.path();
        let demo = dir.join("new.dem");
        let mut header = vec![0u8; 544];
        header[..8].copy_from_slice(b"HLDEMO\0\0");
        header[12..16].copy_from_slice(&48i32.to_le_bytes());
        std::fs::write(&demo, &header).unwrap();
        let err = convert_with_log(&demo).unwrap_err();
        assert!(err.contains("already a protocol 48 demo"), "{err}");
        assert!(!dir.join("new_p48.dem").exists());
    }
}
