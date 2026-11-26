//! Refusals `start_capture_batch_impl` makes before it claims the batch, for
//! payloads the frontend's launch guard (`capture_pane.js`'s
//! `refreshLaunchGuard`) should never have sent.

use native::log_markdown;
use native::patch::{CaptureMode, PatcherConfig};

/// An AGR batch needs its own AGR FPS: AGR's rate never follows Capture FPS,
/// and a high default would make Blender renders far slower. The Start button
/// is the only start path and the UI blocks a blank box, so this catches a
/// payload from an older build or a direct IPC call. `DEFAULT_AGR_FPS` stays
/// as the fallback for readers that start no batch (catch-up of old
/// manifests). Takes the resolved config so the legacy `ffmpeg_capture` flag
/// and an empty `capture_mode` map to a mode exactly as the batch does.
pub fn require_agr_rate(config: &PatcherConfig) -> Result<(), String> {
    if config.capture_mode == CaptureMode::Agr && config.agr_fps <= 0 {
        log_markdown(&format!(
            "**Capture batch refused:** {}",
            crate::messages::AGR_FPS_REQUIRED
        ));
        return Err(crate::messages::AGR_FPS_REQUIRED.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(mode: CaptureMode, agr_fps: i32) -> PatcherConfig {
        let mut cfg = PatcherConfig::default();
        cfg.capture_mode = mode;
        cfg.agr_fps = agr_fps;
        cfg
    }

    #[test]
    fn an_agr_batch_without_its_own_rate_is_refused() {
        for fps in [0, -5] {
            let err = require_agr_rate(&config(CaptureMode::Agr, fps)).unwrap_err();
            assert_eq!(err, crate::messages::AGR_FPS_REQUIRED);
        }
        assert!(require_agr_rate(&config(CaptureMode::Agr, 30)).is_ok());
    }

    #[test]
    fn other_modes_ignore_the_field() {
        for mode in [
            CaptureMode::FrameSequence,
            CaptureMode::DirectToVideo,
            CaptureMode::Obs,
        ] {
            assert!(require_agr_rate(&config(mode, 0)).is_ok());
        }
    }
}
