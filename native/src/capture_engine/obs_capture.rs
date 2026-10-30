//! OBS capture mode's part of a batch: connecting before the game starts,
//! and stopping and reporting once the batch's loop ends.

use super::EngineEvent;
use super::workspace::CaptureCleanupGuard;
use crate::log_markdown;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender},
};

/// Connects to OBS and starts its live drop rate, before the game is
/// spawned, on purpose: every failure here otherwise produces a batch that
/// runs to completion and captures nothing — the worst outcome this path
/// has, because it looks exactly like success until someone opens the
/// folder. The error is the text the batch aborts with.
pub(super) fn start(
    config: &crate::patch::PatcherConfig,
    obs_take_folders: &[std::path::PathBuf],
    cancel_token: &Arc<AtomicBool>,
    tx: &Sender<EngineEvent>,
    cleanup: &mut CaptureCleanupGuard,
) -> Result<(crate::obs::ObsSession, Option<crate::obs::DropPoller>), String> {
    if obs_take_folders.is_empty() {
        return Err("OBS capture was requested but no blocks were planned".into());
    }
    match crate::obs::ObsSession::start(
        &config.obs,
        obs_take_folders.to_vec(),
        config.resolution_width,
        config.resolution_height,
        config.obs_capture_fps,
    ) {
        Ok((session, preflight)) => {
            log_markdown(&format!(
                "🎥 **OBS capture** — connected to OBS {} (obs-websocket {}) at {}. \
                 Canvas {}x{} @ {:.0}fps, scene `{}`. HLAE will record nothing; OBS is \
                 driven from the engine's own console markers.",
                preflight.obs_version,
                preflight.websocket_version,
                config.obs.redacted(),
                preflight.canvas_width,
                preflight.canvas_height,
                preflight.fps,
                preflight.current_scene,
            ));
            // Hand the connection to the guard first, so that from
            // this point on every exit path stops the recorder.
            cleanup.obs = Some(session.stop_handle());
            // The live drop rate (#172), on its own connection so
            // it never holds up a start or stop. Advice only: a
            // poller that can't start just means no figure.
            let drop_tx = tx.clone();
            let drop_poller = crate::obs::DropPoller::spawn(
                config.obs.clone(),
                obs_take_folders.to_vec(),
                config.obs_capture_fps,
                Arc::clone(cancel_token),
                move |report| {
                    let _ = drop_tx.send(EngineEvent::ObsDropRate(report));
                },
            )
            .ok();
            Ok((session, drop_poller))
        }
        Err(e) => Err(format!("OBS capture could not start — {}", e)),
    }
}

/// Stops anything still recording and puts the scene back before the
/// teardown starts removing junctions. The readers are stopped first, so
/// the markers still queued are the last; with `late` they reach the
/// session before it finishes.
pub(super) fn finish(
    mut session: crate::obs::ObsSession,
    late: &[crate::obs::Marker],
    marker_rx: &Receiver<crate::obs::Marker>,
    tail_cancel: &AtomicBool,
    drop_poller: Option<crate::obs::DropPoller>,
    tx: &Sender<EngineEvent>,
) {
    tail_cancel.store(true, Ordering::Relaxed);
    let rest: Vec<_> = marker_rx.try_iter().collect();
    for marker in late.iter().chain(&rest) {
        if let Some(late) = session.on_marker(marker) {
            let _ = tx.send(EngineEvent::ObsStartedLate(late));
        }
    }
    session.finish();
    // Before the batch reports it ended, so the last clip's
    // totals reach the Last Batch panel.
    if let Some(poller) = drop_poller {
        poller.stop_and_wait();
    }
    log_markdown(&format!(
        "🎥 **OBS capture** — {} block(s) recorded, {} skipped.",
        session.recorded.len(),
        session.skipped.len()
    ));
    for problem in &session.skipped {
        log_markdown(&format!("⚠️ **OBS** — {problem}"));
    }
    for block in &session.recorded {
        // The duration is wall-clock, which tracks the file on a
        // clean stop and overstates it on a salvaged one — the
        // recording was cut off, so the seconds after that are not
        // in the file. Said out loud rather than silently printed
        // as if it were a length.
        log_markdown(&format!(
            "- [obs] {} ({}{}) -> {}",
            block.take_folder.display(),
            crate::utils::duration_text::duration_text(block.seconds),
            if block.salvaged {
                " of recording, salvaged — the file is shorter"
            } else {
                ""
            },
            block.video.display()
        ));
    }
}
