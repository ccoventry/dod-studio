//! Whether a demo carries a camera to play back (#920).
//!
//! The client's view during playback comes from each network frame's own
//! header (`refparams`: view origin, angles, view entity), not from the
//! server messages inside it. A file rebuilt from server traffic, rather than
//! recorded by a game client, keeps every server message but has all of that
//! zeroed: the kill feed and scoreboard play, while the camera sits at the
//! map origin with no gun, and a capture of it is a frozen view.
//!
//! An HLTV proxy's own recording has every view zeroed too, and plays fine:
//! the spectator camera follows players from the entities, not from these
//! headers. So only a POV demo is flagged. (Surveyed over the 1,723 demos in
//! the PRE, POST and stock installs' `dod\`, many of them copies between
//! installs, `examples/view_data_survey.rs`: 171 of 191 HLTV demos have no
//! view in any frame, the other 20 being clients' recordings of an HLTV
//! stream; of 1,510 POV demos, every playback frame of each carries one
//! except in the #920 file and `primer_test.dem`, made from it.)
//!
//! Pure over a parsed [`Demo`], so it also builds for the wasm web analyzer.

use dem::types::{Demo, FrameData};

/// How many of a demo's playback network frames carry a view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CameraFrames {
    /// An HLTV proxy's recording, whose frames carry no view by design.
    pub hltv: bool,
    /// Network frames after the LOADING segment, whose frames carry no view
    /// yet in any demo.
    pub network_frames: usize,
    /// Those that name a view entity or a view origin.
    pub with_view: usize,
}

impl CameraFrames {
    /// A POV demo none of whose playback frames carries a view. A demo with
    /// no playback network frames at all has nothing to say, and isn't
    /// flagged.
    pub fn is_missing(&self) -> bool {
        !self.hltv && self.network_frames > 0 && self.with_view == 0
    }
}

/// Counts the playback network frames of `demo` that carry a view.
pub fn camera_frames(demo: &Demo) -> CameraFrames {
    let mut count = CameraFrames {
        hltv: crate::is_hltv_demo(demo),
        ..Default::default()
    };
    for entry in demo.directory.entries.iter().skip(1) {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
                continue;
            };
            count.network_frames += 1;
            let view = &boxed.1.info.refparams;
            if view.view_entity != 0 || view.view_origin.iter().any(|v| *v != 0.0) {
                count.with_view += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Demo {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        dem::open_demo_from_bytes(&bytes).unwrap()
    }

    /// Zeroes every playback frame's view, as a demo rebuilt from server
    /// traffic has it.
    fn without_views(demo: &mut Demo) {
        for entry in demo.directory.entries.iter_mut().skip(1) {
            for frame in &mut entry.frames {
                if let FrameData::NetworkMessage(boxed) = &mut frame.frame_data {
                    let view = &mut boxed.1.info.refparams;
                    view.view_entity = 0;
                    view.view_origin.iter_mut().for_each(|v| *v = 0.0);
                }
            }
        }
    }

    #[test]
    fn a_recorded_demo_carries_a_view_in_every_playback_frame() {
        let count = camera_frames(&fixture());
        assert!(count.network_frames > 0);
        assert_eq!(count.with_view, count.network_frames);
        assert!(!count.is_missing());
    }

    #[test]
    fn a_demo_with_every_view_zeroed_has_no_camera_data() {
        let mut demo = fixture();
        without_views(&mut demo);
        let count = camera_frames(&demo);
        assert_eq!(count.with_view, 0);
        assert!(count.is_missing());
    }

    #[test]
    fn one_frame_with_a_view_is_enough() {
        let mut demo = fixture();
        without_views(&mut demo);
        let frame = demo.directory.entries[1]
            .frames
            .iter_mut()
            .find_map(|f| match &mut f.frame_data {
                FrameData::NetworkMessage(boxed) => Some(boxed),
                _ => None,
            })
            .unwrap();
        frame.1.info.refparams.view_entity = 1;
        assert!(!camera_frames(&demo).is_missing());
    }

    #[test]
    fn no_playback_frames_is_not_flagged() {
        assert!(!CameraFrames::default().is_missing());
    }

    #[test]
    fn an_hltv_recording_with_no_views_is_not_flagged() {
        let hltv = CameraFrames {
            hltv: true,
            network_frames: 41_000,
            with_view: 0,
        };
        assert!(!hltv.is_missing());
        assert!(
            CameraFrames {
                hltv: false,
                ..hltv
            }
            .is_missing()
        );
    }

    /// The pair from #920: a WSOD25 POV demo rebuilt from server traffic, and
    /// the client's own recording of the same half, with the HLTV recording
    /// of it. None is in the tree (78 MB and more); set `DOD_CAMERA_DEMOS` to
    /// the folder holding them, or keep them in the PRE install's `dod\`.
    #[test]
    #[ignore = "needs the #920 wsod25_ply2_m3_h2 demos"]
    fn the_rebuilt_copy_from_920_has_no_camera_data_and_the_recording_has() {
        const REBUILT: &str = "wsod25_ply2_m3_h2_m00cat.dem";
        const RECORDED: &str = "wsod25_ply2_m3_h2_m00cat-1_dod_armory_b6.dem";
        const HLTV: &str = "wsod25_ply2_m3_h2_hltv.dem";
        let folder = std::env::var_os("DOD_CAMERA_DEMOS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
                )
            });
        let count = |name: &str| {
            let path = folder.join(name);
            let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                panic!(
                    "{}: {e} (set DOD_CAMERA_DEMOS to the folder with the #920 demos)",
                    path.display()
                )
            });
            camera_frames(&dem::open_demo_from_bytes(&bytes).unwrap())
        };
        let rebuilt = count(REBUILT);
        assert!(rebuilt.is_missing(), "{rebuilt:?}");
        assert_eq!(rebuilt.network_frames, 118_291);
        let recorded = count(RECORDED);
        assert!(!recorded.is_missing(), "{recorded:?}");
        assert_eq!(recorded.with_view, recorded.network_frames);
        // The HLTV proxy's recording of the same half: no views, by design.
        let hltv = count(HLTV);
        assert!(hltv.hltv && hltv.with_view == 0, "{hltv:?}");
        assert!(!hltv.is_missing());
    }
}
