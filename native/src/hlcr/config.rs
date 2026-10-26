#![cfg(not(target_arch = "wasm32"))]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum RenderCodec {
    NvencH264,
    /// Software libx264 — not in dev's original 3-variant enum. Added to
    /// preserve the Tauri rebuild's deliberate choice (see
    /// docs/archive/tauri_parity_audit.md Area 5) to keep both software and NVENC
    /// H.264 as separate, explicit options rather than picking one.
    H264Software,
    /// ProRes 422 HQ. Keeps the plain name (and the `"prores"` id) because
    /// every settings file, preset and autosave written before the other
    /// profiles existed means HQ by it.
    ProRes,
    /// ProRes 422 (Apple's "standard"): about 2/3 of HQ's size.
    ProRes422,
    /// ProRes 422 LT: about half of HQ's size.
    ProResLt,
    /// ProRes 4444 without alpha, for the main picture. The HUD stream's
    /// 4444 with alpha is separate and doesn't follow this choice.
    ProRes4444,
    DnxHr,
    /// Lossless. Pairs with the AVI container, HuffYUV's native home and the
    /// one every decoder actually expects it in.
    HuffYuv,
    /// `-c:v rawvideo` — no compression at all, not even the intra-frame
    /// kind HuffYUV does. Same AVI container as HuffYUV, for the same reason.
    Uncompressed,
    /// An escape hatch: `RenderConfig::custom_codec_args` is split on
    /// whitespace and passed to FFmpeg verbatim in place of the `-c:v ...`
    /// block every other variant hardcodes here. For a codec this dropdown
    /// doesn't have yet -- no allow-list, no parsing beyond the split;
    /// FFmpeg itself is the validator, the same way a bad codec typed
    /// directly at a terminal would be. Container is always `.mkv`, which
    /// accepts nearly anything, since there is no way to know what a given
    /// custom codec needs.
    Custom,
    /// "Skip" — leave an OBS take exactly as OBS wrote it: no FFmpeg pass at
    /// all, just a copy into the export pool under the pipeline's naming.
    /// Only meaningful for a clip with its own muxed-in audio (`ClipData.wav_file
    /// == None`); `run_render_job` rejects it for anything else.
    SourceCopy,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RenderConfig {
    pub ffmpeg_path: String,
    pub source_folder: String,
    #[serde(default)]
    pub export_directories: Vec<PathBuf>,
    pub fps: u32,
    pub target_codec: RenderCodec,
    /// Raw FFmpeg video-codec args (e.g. `-c:v mpeg4 -q:v 3`), only read
    /// when `target_codec` is `RenderCodec::Custom`. `#[serde(default)]` so
    /// a config file written before this field existed still loads.
    #[serde(default)]
    pub custom_codec_args: String,
    pub max_concurrent_renders: usize,
}

impl RenderCodec {
    /// Maps the frontend's codec-select string values to the enum used by
    /// `run_render_job`. Unrecognized values fall back to ProRes, matching
    /// `studio`'s prior `codec_args_and_ext()` default.
    pub fn from_str_id(id: &str) -> Self {
        match id {
            "h264" => Self::H264Software,
            "h264_nvenc" => Self::NvencH264,
            "prores_422" => Self::ProRes422,
            "prores_lt" => Self::ProResLt,
            "prores_4444" => Self::ProRes4444,
            "dnxhr" => Self::DnxHr,
            "huffyuv" => Self::HuffYuv,
            "uncompressed" => Self::Uncompressed,
            "custom" => Self::Custom,
            "source_copy" => Self::SourceCopy,
            _ => Self::ProRes,
        }
    }

    /// Inverse of `from_str_id` — round-trips through the same string ids
    /// (not `Debug` formatting) so autosave recovery can parse a persisted
    /// codec back into the enum without a second, drifting mapping.
    pub fn to_str_id(self) -> &'static str {
        match self {
            Self::H264Software => "h264",
            Self::NvencH264 => "h264_nvenc",
            Self::DnxHr => "dnxhr",
            Self::HuffYuv => "huffyuv",
            Self::Uncompressed => "uncompressed",
            Self::Custom => "custom",
            Self::ProRes => "prores",
            Self::ProRes422 => "prores_422",
            Self::ProResLt => "prores_lt",
            Self::ProRes4444 => "prores_4444",
            Self::SourceCopy => "source_copy",
        }
    }

    /// `prores_ks`'s `-profile:v` and the pixel format for a ProRes choice,
    /// with its expected size next to HQ's at the same frame size, for the
    /// render reservation before a render of that profile has finished
    /// (`renderer::job_reservation_estimate`). Measured on a 300 fps
    /// 2560x1440 game capture: 422 0.67x, LT 0.47x, 4444 1.47x (from an HQ
    /// copy, so slightly low); Apple's target rates give the same ratios.
    pub fn prores_profile(self) -> Option<(&'static str, &'static str, f64)> {
        match self {
            Self::ProRes => Some(("hq", "yuv422p10le", 1.0)),
            Self::ProRes422 => Some(("standard", "yuv422p10le", 0.67)),
            Self::ProResLt => Some(("lt", "yuv422p10le", 0.47)),
            Self::ProRes4444 => Some(("4444", "yuv444p10le", 1.5)),
            _ => None,
        }
    }

    /// Short human-readable label for UI display (e.g. the render job
    /// table's per-job Settings column).
    pub fn label(self) -> &'static str {
        match self {
            Self::H264Software => "H.264 (Software)",
            Self::NvencH264 => "H.264 (NVENC)",
            Self::DnxHr => "DNxHR",
            Self::HuffYuv => "HuffYUV",
            Self::Uncompressed => "Uncompressed",
            Self::Custom => "Custom",
            Self::ProRes => "ProRes HQ",
            Self::ProRes422 => "ProRes 422",
            Self::ProResLt => "ProRes LT",
            Self::ProRes4444 => "ProRes 4444",
            Self::SourceCopy => "Skip (Keep Original)",
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            ffmpeg_path: "ffmpeg".to_string(),
            source_folder: "".to_string(),
            export_directories: Vec::new(),
            fps: 300,
            target_codec: RenderCodec::ProRes,
            custom_codec_args: String::new(),
            max_concurrent_renders: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_str_id_round_trips_through_every_codec() {
        // to_str_id/from_str_id must stay inverses — autosave recovery
        // (render_manager.rs's recover_render_batch) round-trips a persisted
        // codec through exactly this pair.
        for codec in [
            RenderCodec::NvencH264,
            RenderCodec::H264Software,
            RenderCodec::ProRes,
            RenderCodec::ProRes422,
            RenderCodec::ProResLt,
            RenderCodec::ProRes4444,
            RenderCodec::DnxHr,
            RenderCodec::HuffYuv,
            RenderCodec::Uncompressed,
            RenderCodec::Custom,
            RenderCodec::SourceCopy,
        ] {
            assert_eq!(RenderCodec::from_str_id(codec.to_str_id()), codec);
            // The enum's own serde form (RenderConfig, autosaves) too.
            let json = serde_json::to_string(&codec).unwrap();
            assert_eq!(serde_json::from_str::<RenderCodec>(&json).unwrap(), codec);
        }
    }

    /// Settings, presets and autosaves written before the other profiles
    /// existed say "prores" (or `ProRes` in serde form): that stays HQ.
    #[test]
    fn old_prores_values_still_mean_hq() {
        assert_eq!(RenderCodec::from_str_id("prores"), RenderCodec::ProRes);
        assert_eq!(
            serde_json::from_str::<RenderCodec>("\"ProRes\"").unwrap(),
            RenderCodec::ProRes
        );
        assert_eq!(RenderCodec::ProRes.prores_profile().unwrap().0, "hq");
    }

    #[test]
    fn each_prores_choice_has_its_own_profile() {
        let profile = |c: RenderCodec| c.prores_profile().map(|(p, f, _)| (p, f));
        assert_eq!(profile(RenderCodec::ProRes), Some(("hq", "yuv422p10le")));
        assert_eq!(
            profile(RenderCodec::ProRes422),
            Some(("standard", "yuv422p10le"))
        );
        assert_eq!(profile(RenderCodec::ProResLt), Some(("lt", "yuv422p10le")));
        // No alpha: the main picture is opaque; the HUD stream has its own.
        assert_eq!(
            profile(RenderCodec::ProRes4444),
            Some(("4444", "yuv444p10le"))
        );
        assert_eq!(profile(RenderCodec::DnxHr), None);
        let ratio = |c: RenderCodec| c.prores_profile().unwrap().2;
        assert!(ratio(RenderCodec::ProResLt) < ratio(RenderCodec::ProRes422));
        assert!(ratio(RenderCodec::ProRes422) < ratio(RenderCodec::ProRes));
        assert!(ratio(RenderCodec::ProRes) < ratio(RenderCodec::ProRes4444));
    }

    #[test]
    fn test_from_str_id_unrecognized_falls_back_to_prores() {
        assert_eq!(
            RenderCodec::from_str_id("not_a_real_codec"),
            RenderCodec::ProRes
        );
    }
}
