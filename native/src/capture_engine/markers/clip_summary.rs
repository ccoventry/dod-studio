//! The hook DLL's one-line summary of each captured clip (#1221), read off
//! the events pipe and written to the activity log.
//!
//! The hook sends `[dod-studio] CLIP_SUMMARY` then tab-separated
//! `key=value` fields when a clip's recording stops (`goldsrc-hooks`'
//! `clip_log.rs` documents them). Any field may be missing, and keys this
//! does not know are skipped, so either side can add one. A hook DLL from
//! before #1221 never sends the line, and the log just has no per-clip line.

/// What the summary's label starts with.
const LABEL: &str = "CLIP_SUMMARY";

/// One clip, as the hook summed it up.
#[derive(Debug, Default, PartialEq)]
pub(super) struct ClipSummary {
    pub number: Option<u32>,
    pub demo: Option<(u32, u32)>,
    pub demo_clip: Option<(u32, u32)>,
    pub start: Option<f64>,
    pub stop: Option<f64>,
    pub frames: Option<u32>,
    pub markers: Vec<String>,
}

fn pair(value: &str) -> Option<(u32, u32)> {
    let (n, of) = value.split_once('/')?;
    Some((n.parse().ok()?, of.parse().ok()?))
}

/// The summary in `label` (the text after the tag), if it is one.
pub(super) fn parse(label: &str) -> Option<ClipSummary> {
    let mut fields = label.split('\t');
    if fields.next()?.trim() != LABEL {
        return None;
    }
    let mut clip = ClipSummary::default();
    for field in fields {
        let Some((key, value)) = field.trim().split_once('=') else {
            continue;
        };
        match key {
            "clip" => clip.number = value.parse().ok(),
            "demo" => clip.demo = pair(value),
            "demo_clip" => clip.demo_clip = pair(value),
            "start" => clip.start = value.parse().ok().filter(|t: &f64| t.is_finite()),
            "stop" => clip.stop = value.parse().ok().filter(|t: &f64| t.is_finite()),
            "frames" => clip.frames = value.parse().ok(),
            "markers" => {
                clip.markers = value
                    .split(',')
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            _ => {}
        }
    }
    Some(clip)
}

impl ClipSummary {
    fn saw(&self, marker: &str) -> bool {
        self.markers.iter().any(|m| m == marker)
    }

    /// The activity log's line for this clip.
    pub(super) fn log_line(&self) -> String {
        let mut head = match self.number {
            Some(n) => format!("Clip {n}"),
            None => "A clip".to_string(),
        };
        let mut place = Vec::new();
        if let Some((n, of)) = self.demo {
            place.push(format!("demo {n} of {of}"));
        }
        if let Some((n, of)) = self.demo_clip {
            place.push(format!("clip {n} of {of}"));
        }
        if !place.is_empty() {
            head.push_str(&format!(" ({})", place.join(", ")));
        }
        let mut parts = Vec::new();
        match (self.start, self.stop) {
            (Some(start), Some(stop)) => parts.push(format!(
                "demo time {start:.2}–{stop:.2} s ({:.2} s)",
                stop - start
            )),
            (Some(start), None) => parts.push(format!("from demo time {start:.2} s")),
            (None, Some(stop)) => parts.push(format!("to demo time {stop:.2} s")),
            (None, None) => {}
        }
        if let Some(frames) = self.frames {
            parts.push(format!("{frames} frames"));
        }
        let markers = if self.markers.is_empty() {
            "none".to_string()
        } else {
            self.markers.join(", ")
        };
        parts.push(format!("markers {markers}"));
        let missing: Vec<&str> = ["START_RECORD", "STOP_RECORD"]
            .into_iter()
            .filter(|m| !self.saw(m))
            .collect();
        if missing.is_empty() {
            format!("[HLAE] {head} recorded: {}", parts.join("; "))
        } else {
            format!(
                "⚠️ [HLAE] {head} has no {}: {}",
                missing.join(" or "),
                parts.join("; ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "CLIP_SUMMARY\tclip=3\tdemo=2/2\tdemo_clip=1/2\tstart=412.100\t\
                        stop=424.533\tframes=745\tmarkers=AUDIO_SYNC,START_RECORD,STOP_RECORD";

    #[test]
    fn a_whole_summary_reads_back_and_logs_one_line() {
        let clip = parse(FULL).expect("a summary");
        assert_eq!(clip.number, Some(3));
        assert_eq!(clip.demo, Some((2, 2)));
        assert_eq!(clip.frames, Some(745));
        assert_eq!(
            clip.log_line(),
            "[HLAE] Clip 3 (demo 2 of 2, clip 1 of 2) recorded: demo time 412.10–424.53 s \
             (12.43 s); 745 frames; markers AUDIO_SYNC, START_RECORD, STOP_RECORD"
        );
    }

    #[test]
    fn a_clip_that_never_stopped_is_flagged() {
        let clip =
            parse("CLIP_SUMMARY\tclip=1\tstart=11.000\tframes=300\tmarkers=START_RECORD").unwrap();
        assert_eq!(
            clip.log_line(),
            "⚠️ [HLAE] Clip 1 has no STOP_RECORD: from demo time 11.00 s; 300 frames; \
             markers START_RECORD"
        );
    }

    /// Either side may add a field: unknown keys and bad values are skipped.
    #[test]
    fn unknown_keys_and_bad_values_are_skipped() {
        let clip = parse(
            "CLIP_SUMMARY\tclip=2\tsomething_new=7\tframes=many\tstart=NaN\tmarkers=STOP_RECORD",
        )
        .unwrap();
        assert_eq!(clip.number, Some(2));
        assert_eq!(clip.frames, None);
        assert_eq!(clip.start, None);
        assert!(clip.log_line().contains("no START_RECORD"));
    }

    #[test]
    fn other_markers_are_not_summaries() {
        assert!(parse("STOP_RECORD - Tick 900").is_none());
        assert!(parse("CLIP_SUMMARYX\tclip=1").is_none());
        assert!(parse("BATCH_HELD").is_none());
    }
}
