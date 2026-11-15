//! The recording encoder DoD Studio's OBS profile uses (#1060).
//!
//! OBS's Simple output mode fixes NVENC at preset P5 with two-pass
//! (quarter-resolution) encoding, 8 frames of lookahead and 2 B-frames: on an
//! RTX 3070 Ti that encodes 2560x1440 at about 120-145 fps with nothing else
//! running, and with the game running OBS skipped 32-95% of a 120 fps clip's
//! frames "due to encoding lag". Advanced output mode lets the profile pick a
//! faster setup, but keeps its encoder settings in the profile folder's
//! `recordEncoder.json`, which obs-websocket can't write. This module finds
//! that folder and writes the file; `provision` then rebuilds OBS's outputs so
//! it reads it.
//!
//! Only for NVENC on OBS 31 and later (the `obs-nvenc` plugin and its setting
//! names). Any other encoder, or a profile folder that can't be found, keeps
//! Simple mode: OBS's own settings for that hardware, unmeasured here.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

pub use crate::patch::ObsRecordingQuality;

/// What the profile records with, when it's an Advanced-mode NVENC setup.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvancedEncoder {
    /// OBS's encoder id (`AdvOut/RecEncoder`).
    pub id: &'static str,
    /// What goes into `recordEncoder.json`.
    pub settings: Value,
    /// OBS's video colour format (`Video/ColorFormat`) the encoder is fed.
    pub color_format: &'static str,
}

/// The Advanced-mode encoder for `quality`, or `None` to stay in Simple mode.
///
/// `simple_encoder` is OBS's own pick for this PC (Simple mode's
/// `RecEncoder` default): `nvenc`, `amd`, `qsv`, `x264`, ...
pub fn advanced_encoder(
    quality: ObsRecordingQuality,
    simple_encoder: &str,
    obs_version: &str,
) -> Option<AdvancedEncoder> {
    // obs-nvenc (and its `preset`/`tune`/`multipass` names) arrived in OBS 31;
    // 30's NVENC encoder named them differently.
    if !super::provision::version_at_least(obs_version, (31, 0)) {
        return None;
    }
    if !simple_encoder.starts_with("nvenc") && simple_encoder != "jim_nvenc" {
        return None;
    }
    // H.264 throughout: at these quantisers NVENC's HEVC made the same size
    // file at the same quality, no faster (measured, docs/obs_alternate_capture.md).
    let (profile, cqp, color_format) = match quality {
        ObsRecordingQuality::Small => ("high", 20, "NV12"),
        ObsRecordingQuality::Standard => ("high", 16, "NV12"),
        ObsRecordingQuality::High => ("high", 12, "NV12"),
        // Full colour resolution: sharp coloured HUD text and edges, which
        // 4:2:0 halves whatever the quantiser. obs-nvenc picks the 4:4:4
        // profile itself from the I444 format.
        ObsRecordingQuality::Max => ("high", 12, "I444"),
        ObsRecordingQuality::ObsSimple => return None,
    };
    Some(AdvancedEncoder {
        id: "obs_nvenc_h264_tex",
        color_format,
        settings: json!({
            "rate_control": "CQP",
            "cqp": cqp,
            "preset": "p4",
            "tune": "hq",
            "multipass": "disabled",
            "lookahead": false,
            "bf": 0,
            "profile": profile,
        }),
    })
}

/// Settings OBS's config folder for the running OBS could be in: the
/// per-user one, and a portable install's `config\obs-studio` beside each
/// running `obs64.exe`.
pub fn config_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(dir) = dirs::config_dir() {
        roots.push(dir.join("obs-studio"));
    }
    let sys = crate::sys::process::snapshot();
    for pid in crate::sys::process::pids_named_in(&sys, &["obs64.exe", "obs.exe"]) {
        let exe = sys
            .process(sysinfo::Pid::from_u32(pid))
            .and_then(|p| p.exe())
            .map(Path::to_path_buf);
        // <install>\bin\64bit\obs64.exe -> <install>\config\obs-studio
        if let Some(install) = exe.as_deref().and_then(|e| e.ancestors().nth(3)) {
            let portable = install.join("config").join("obs-studio");
            if !roots.contains(&portable) {
                roots.push(portable);
            }
        }
    }
    roots
}

/// The profile folder whose `basic.ini` holds `marker` (a line
/// `<key>=<marker>` that `provision` has just written through
/// obs-websocket, which saves `basic.ini` on every write). Matching on a fresh
/// marker rather than the profile's name finds the folder the running OBS
/// actually uses, never a stale copy from another install.
pub fn find_profile_dir(roots: &[PathBuf], key: &str, marker: &str) -> Option<PathBuf> {
    let line = format!("{key}={marker}");
    roots.iter().find_map(|root| {
        std::fs::read_dir(root.join("basic").join("profiles"))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|dir| {
                std::fs::read_to_string(dir.join("basic.ini"))
                    .is_ok_and(|ini| ini.lines().any(|l| l.trim() == line))
            })
    })
}

/// Writes `settings` into `dir`'s `recordEncoder.json`, keeping any other
/// keys already there (OBS's own Settings dialog writes the full set).
/// Returns whether the file changed.
pub fn write_record_encoder(dir: &Path, settings: &Value) -> std::io::Result<bool> {
    let path = dir.join("recordEncoder.json");
    let mut current: Map<String, Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let Some(wanted) = settings.as_object() else {
        return Ok(false);
    };
    if wanted.iter().all(|(k, v)| current.get(k) == Some(v)) {
        return Ok(false);
    }
    for (k, v) in wanted {
        current.insert(k.clone(), v.clone());
    }
    std::fs::write(&path, Value::Object(current).to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dodstudio-obs-encoder-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Every NVENC choice is the fast setup: single pass, no lookahead, no
    /// B-frames, preset P4 (#1060).
    #[test]
    fn nvenc_choices_are_single_pass_with_no_lookahead_or_b_frames() {
        for q in [
            ObsRecordingQuality::Small,
            ObsRecordingQuality::Standard,
            ObsRecordingQuality::High,
            ObsRecordingQuality::Max,
        ] {
            let enc = advanced_encoder(q, "nvenc", "32.2.2").expect("NVENC gets Advanced mode");
            assert_eq!(enc.settings["multipass"], "disabled");
            assert_eq!(enc.settings["lookahead"], false);
            assert_eq!(enc.settings["bf"], 0);
            assert_eq!(enc.settings["preset"], "p4");
            assert_eq!(enc.settings["rate_control"], "CQP");
        }
        let std = advanced_encoder(ObsRecordingQuality::Standard, "nvenc", "32.2.2").unwrap();
        assert_eq!(std.id, "obs_nvenc_h264_tex");
        assert_eq!(std.settings["cqp"], 16);
        assert_eq!(std.color_format, "NV12");
        let max = advanced_encoder(ObsRecordingQuality::Max, "nvenc", "32.2.2").unwrap();
        assert_eq!(max.color_format, "I444");
    }

    /// Simple mode stays for other encoders, old OBS, and the "OBS's own"
    /// choice.
    #[test]
    fn simple_mode_where_the_fast_setup_is_unmeasured_or_unwanted() {
        assert!(advanced_encoder(ObsRecordingQuality::Standard, "amd", "32.2.2").is_none());
        assert!(advanced_encoder(ObsRecordingQuality::Standard, "qsv", "32.2.2").is_none());
        assert!(advanced_encoder(ObsRecordingQuality::Standard, "x264", "32.2.2").is_none());
        assert!(advanced_encoder(ObsRecordingQuality::Standard, "", "32.2.2").is_none());
        assert!(advanced_encoder(ObsRecordingQuality::Standard, "nvenc", "30.2.3").is_none());
        assert!(advanced_encoder(ObsRecordingQuality::ObsSimple, "nvenc", "32.2.2").is_none());
        assert!(advanced_encoder(ObsRecordingQuality::High, "nvenc_hevc", "31.0.0").is_some());
    }

    /// Settings files keep their unknown keys, and are left alone when they
    /// already match.
    #[test]
    fn writes_only_what_differs_and_keeps_other_keys() {
        let dir = temp_dir("write");
        std::fs::write(
            dir.join("recordEncoder.json"),
            r#"{"cqp":20,"keyint_sec":2}"#,
        )
        .unwrap();
        let enc = advanced_encoder(ObsRecordingQuality::Standard, "nvenc", "32.2.2").unwrap();
        assert!(write_record_encoder(&dir, &enc.settings).unwrap());
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("recordEncoder.json")).unwrap())
                .unwrap();
        assert_eq!(written["cqp"], 16);
        assert_eq!(written["keyint_sec"], 2);
        assert!(!write_record_encoder(&dir, &enc.settings).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The folder is the one whose basic.ini carries the fresh marker, not
    /// just any profile by our name.
    #[test]
    fn finds_the_profile_folder_by_its_marker() {
        let root = temp_dir("find");
        let profiles = root.join("basic").join("profiles");
        for (name, ini) in [
            (
                "Old",
                "[General]\nName=[DoD-Studio]\n[DoDStudio]\nMarker=111\n",
            ),
            (
                "DoDStudio",
                "[General]\nName=[DoD-Studio]\n[DoDStudio]\nMarker=222\n",
            ),
        ] {
            std::fs::create_dir_all(profiles.join(name)).unwrap();
            std::fs::write(profiles.join(name).join("basic.ini"), ini).unwrap();
        }
        let roots = vec![root.join("missing"), root.clone()];
        assert_eq!(
            find_profile_dir(&roots, "Marker", "222"),
            Some(profiles.join("DoDStudio"))
        );
        assert_eq!(find_profile_dir(&roots, "Marker", "333"), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
