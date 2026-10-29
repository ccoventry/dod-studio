//! OBS's live drop rate (#172): how many frames OBS is losing while it
//! records, read once a second, so a capture set past what the PC's encoder
//! keeps up with shows on the status line while it happens rather than in the
//! activity log after each clip.
//!
//! OBS loses frames two ways, and `GetStats` counts both: frames **skipped**
//! because the encoder lagged (`outputSkippedFrames`, the log's "skipped
//! frames due to encoding lag"), and frames **missed** because rendering
//! lagged (`renderSkippedFrames`, "missed frames due to rendering lag"). Both
//! counters run for the whole OBS process, so every figure here is the
//! difference between two reads.
//!
//! The poller runs on its own thread with its own connection, so a slow or
//! stuck `GetStats` can never hold up the session's `StartRecord` and
//! `StopRecord`, which run off console markers on the engine's thread. OBS
//! answers `GetStats` fine while recording: a second client polled it every
//! 250 ms through whole batches with no effect on the capture (#645).
//!
//! It tells clips apart by what OBS reports (`GetRecordStatus`), not by the
//! session's markers, and names each clip's take folder from the record
//! directory the session pointed OBS at, so it needs nothing from the
//! session.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::patch::ObsConfig;

use super::client::ObsClient;
use super::session::{STREAM_FOLDER, TAKE_FOLDER};

/// How often OBS is asked. Once a second is enough for a line people read, and
/// three small requests a second cost OBS nothing measurable.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The stretch the live warning looks at, in seconds. Long enough that one
/// hitch (a map's first frames, a window switch) doesn't flag a clip, short
/// enough that an encoder that can't keep up shows within a few seconds of
/// the recording starting: at 120 fps it lost a third or more of every
/// second (#1060).
pub const RECENT_WINDOW_SECS: f64 = 5.0;

/// Share of frames lost before Studio warns: the same 5% the per-clip
/// activity-log warning uses (`ObsSession`). A frame or two now and then is
/// normal on real hardware; 5% is a stutter people see, and every lagging run
/// measured so far was far past it (32-98%), while clean runs were at 0-0.4%.
pub const LAG_WARNING_SHARE: f64 = 0.05;

/// How long to wait before connecting again after the connection failed.
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

/// How long `stop_and_wait` waits for the poller's last report. A request in
/// flight can take up to its own timeout; the batch never waits that long for
/// advice.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// `GetStats`'s frame counters at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameCounters {
    pub output_total: i64,
    /// Skipped because the encoder lagged.
    pub output_skipped: i64,
    pub render_total: i64,
    /// Missed because rendering lagged.
    pub render_skipped: i64,
}

impl FrameCounters {
    /// What happened between `base` and `self`. A counter that went down
    /// (OBS restarted mid-batch) reads as nothing rather than as negative.
    pub fn since(self, base: Self) -> Self {
        Self {
            output_total: (self.output_total - base.output_total).max(0),
            output_skipped: (self.output_skipped - base.output_skipped).max(0),
            render_total: (self.render_total - base.render_total).max(0),
            render_skipped: (self.render_skipped - base.render_skipped).max(0),
        }
    }

    /// Share of frames skipped because the encoder lagged, 0..=1.
    pub fn skipped_share(&self) -> f64 {
        share(self.output_skipped, self.output_total)
    }

    /// Share of frames missed because rendering lagged, 0..=1.
    pub fn missed_share(&self) -> f64 {
        share(self.render_skipped, self.render_total)
    }

    /// Past the warning share, on a stretch of at least `min_frames`, so the
    /// first frame of a clip being late doesn't read as 100%.
    pub fn lagging(&self, min_frames: i64) -> bool {
        let min = min_frames.max(1);
        (self.output_total >= min && self.skipped_share() >= LAG_WARNING_SHARE)
            || (self.render_total >= min && self.missed_share() >= LAG_WARNING_SHARE)
    }
}

fn share(part: i64, total: i64) -> f64 {
    if total <= 0 {
        0.0
    } else {
        (part.max(0) as f64 / total as f64).min(1.0)
    }
}

/// One poll of OBS.
#[derive(Clone, Debug)]
pub struct Sample {
    /// Seconds since the poller started.
    pub at_secs: f64,
    pub active: bool,
    /// `GetRecordStatus`'s `outputDuration`, in ms.
    pub duration_ms: i64,
    /// Where OBS is recording to; only read while it records.
    pub directory: Option<String>,
    pub counters: FrameCounters,
}

/// What the status line and the Last Batch panel show.
#[derive(Clone, Debug)]
pub struct ObsDropReport {
    /// Which OBS recording of the batch, from 1.
    pub clip: u32,
    /// The block's take folder, as the batch planned it, when OBS's record
    /// directory matched one.
    pub take_folder: Option<PathBuf>,
    /// `true` while the clip records; `false` once: the clip ended and
    /// `clip_counts` are its totals.
    pub recording: bool,
    /// Since the clip started.
    pub clip_counts: FrameCounters,
    /// Over the last [`RECENT_WINDOW_SECS`] (empty in the clip's last report).
    pub recent: FrameCounters,
    /// Past [`LAG_WARNING_SHARE`]: over the recent stretch while recording,
    /// over the whole clip at its end.
    pub lagging: bool,
    /// The OBS Capture FPS the batch asked for, for the advice.
    pub capture_fps: i32,
}

struct Clip {
    number: u32,
    take_folder: Option<PathBuf>,
    directory: Option<String>,
    start: FrameCounters,
    last: FrameCounters,
    last_duration_ms: i64,
}

/// Turns polls into reports. Pure, so the clip boundaries and the window are
/// tested without OBS.
pub struct DropTracker {
    take_folders: Vec<PathBuf>,
    capture_fps: i32,
    clips: u32,
    clip: Option<Clip>,
    /// `(at_secs, counters)` of this clip's recent polls; the front one is the
    /// newest at or before the window's start, the base of `recent`.
    window: VecDeque<(f64, FrameCounters)>,
}

impl DropTracker {
    pub fn new(take_folders: Vec<PathBuf>, capture_fps: i32) -> Self {
        Self {
            take_folders,
            capture_fps,
            clips: 0,
            clip: None,
            window: VecDeque::new(),
        }
    }

    /// One second's worth of frames: the least a stretch needs before its
    /// share means anything.
    fn min_frames(&self) -> i64 {
        i64::from(self.capture_fps.max(1))
    }

    pub fn on_sample(&mut self, s: &Sample) -> Vec<ObsDropReport> {
        let mut out = Vec::new();
        let new_recording = match &self.clip {
            None => s.active,
            Some(c) => {
                s.active
                    && (s.duration_ms < c.last_duration_ms
                        || (s.directory.is_some() && s.directory != c.directory))
            }
        };
        if !s.active || new_recording {
            out.extend(self.end_clip());
        }
        if !s.active {
            return out;
        }
        if self.clip.is_none() {
            self.clips += 1;
            self.window.clear();
            self.clip = Some(Clip {
                number: self.clips,
                take_folder: s
                    .directory
                    .as_deref()
                    .and_then(|d| take_folder_for(d, &self.take_folders)),
                directory: s.directory.clone(),
                start: s.counters,
                last: s.counters,
                last_duration_ms: s.duration_ms,
            });
        }

        self.window.push_back((s.at_secs, s.counters));
        let window_start = s.at_secs - RECENT_WINDOW_SECS;
        while self.window.len() > 1 && self.window[1].0 <= window_start {
            self.window.pop_front();
        }
        let recent = s.counters.since(self.window[0].1);
        let min_frames = self.min_frames();
        let capture_fps = self.capture_fps;

        let Some(clip) = self.clip.as_mut() else {
            return out;
        };
        clip.last = s.counters;
        clip.last_duration_ms = s.duration_ms;
        out.push(ObsDropReport {
            clip: clip.number,
            take_folder: clip.take_folder.clone(),
            recording: true,
            clip_counts: s.counters.since(clip.start),
            recent,
            lagging: recent.lagging(min_frames),
            capture_fps,
        });
        out
    }

    /// The clip in progress, closed with its totals; `None` when none is.
    pub fn end_clip(&mut self) -> Option<ObsDropReport> {
        let clip = self.clip.take()?;
        self.window.clear();
        let totals = clip.last.since(clip.start);
        Some(ObsDropReport {
            clip: clip.number,
            take_folder: clip.take_folder,
            recording: false,
            clip_counts: totals,
            recent: FrameCounters::default(),
            lagging: totals.lagging(self.min_frames()),
            capture_fps: self.capture_fps,
        })
    }
}

/// The planned take folder OBS is recording into, from its record directory
/// (`<take>/take0000/all`, set by `ObsSession` before each block). Compared
/// without case and with either slash, since OBS may hand the path back in
/// its own spelling.
fn take_folder_for(directory: &str, take_folders: &[PathBuf]) -> Option<PathBuf> {
    let want = normalised(directory);
    take_folders
        .iter()
        .find(|tf| normalised(&tf.join(TAKE_FOLDER).join(STREAM_FOLDER).to_string_lossy()) == want)
        .cloned()
}

fn normalised(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// One poll, or `None` when any request failed (the connection is dropped and
/// made again).
fn read_sample(client: &mut ObsClient, at_secs: f64) -> Option<Sample> {
    let (active, duration_ms) = client.record_status()?;
    let directory = if active {
        Some(client.record_directory().ok()?)
    } else {
        None
    };
    let counters = client.frame_counters()?;
    Some(Sample {
        at_secs,
        active,
        duration_ms,
        directory,
        counters,
    })
}

/// The running poller: [`DropPoller::stop_and_wait`] ends it and waits briefly
/// for the last clip's totals.
pub struct DropPoller {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

impl DropPoller {
    /// Polls OBS on its own thread until `stop_and_wait` or `cancel`, handing
    /// each report to `on_report`. Never fails: an OBS that can't be reached
    /// or doesn't answer `GetStats` just means no live figure.
    pub fn spawn(
        cfg: ObsConfig,
        take_folders: Vec<PathBuf>,
        capture_fps: i32,
        cancel: Arc<AtomicBool>,
        on_report: impl FnMut(ObsDropReport) + Send + 'static,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("obs_drop_rate".into())
            .spawn(move || {
                let mut tracker = DropTracker::new(take_folders, capture_fps);
                run(
                    &cfg.address(),
                    &cfg.password,
                    POLL_INTERVAL,
                    &mut tracker,
                    &stop_flag,
                    &cancel,
                    on_report,
                );
            })?;
        Ok(Self { stop, handle })
    }

    /// Stops polling and gives the thread a moment to send the last clip's
    /// totals. Doesn't wait out a request OBS is slow to answer.
    pub fn stop_and_wait(self) {
        self.stop.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + STOP_WAIT;
        while !self.handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A batch that returns early (a failure, a crash) stops the poller without
/// waiting for it.
impl Drop for DropPoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(
    address: &str,
    password: &str,
    every: Duration,
    tracker: &mut DropTracker,
    stop: &AtomicBool,
    cancel: &AtomicBool,
    mut on_report: impl FnMut(ObsDropReport),
) {
    let started = Instant::now();
    let stopped = || stop.load(Ordering::Relaxed) || cancel.load(Ordering::Relaxed);
    let mut client: Option<ObsClient> = None;
    let mut last_connect: Option<Instant> = None;
    while !stopped() {
        let poll_started = Instant::now();
        if client.is_none() && last_connect.is_none_or(|t| t.elapsed() >= RECONNECT_DELAY) {
            last_connect = Some(Instant::now());
            client = ObsClient::connect(address, password).ok();
        }
        if let Some(c) = client.as_mut() {
            match read_sample(c, started.elapsed().as_secs_f64()) {
                Some(sample) => tracker
                    .on_sample(&sample)
                    .into_iter()
                    .for_each(&mut on_report),
                None => client = None,
            }
        }
        // Slept in slices so a stop is seen within a few tens of ms.
        while !stopped() && poll_started.elapsed() < every {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    if let Some(report) = tracker.end_clip() {
        on_report(report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counters(total: i64, skipped: i64) -> FrameCounters {
        FrameCounters {
            output_total: total,
            output_skipped: skipped,
            render_total: total,
            render_skipped: 0,
        }
    }

    fn sample(
        at: f64,
        active: bool,
        duration_ms: i64,
        dir: Option<&str>,
        c: FrameCounters,
    ) -> Sample {
        Sample {
            at_secs: at,
            active,
            duration_ms,
            directory: dir.map(str::to_string),
            counters: c,
        }
    }

    #[test]
    fn shares_need_a_second_of_frames_before_they_warn() {
        let c = counters(60, 30);
        assert!((c.skipped_share() - 0.5).abs() < 1e-9);
        assert!(
            !c.lagging(120),
            "half a second at 120 fps is too little to judge"
        );
        assert!(c.lagging(60));
        assert!(!counters(1200, 59).lagging(120), "4.9% stays quiet");
        assert!(counters(1200, 60).lagging(120), "5% warns");
        let render = FrameCounters {
            render_total: 600,
            render_skipped: 60,
            ..FrameCounters::default()
        };
        assert!(render.lagging(120), "render lag warns too");
        assert_eq!(FrameCounters::default().skipped_share(), 0.0);
    }

    #[test]
    fn a_counter_that_went_down_reads_as_nothing() {
        let d = counters(10, 5).since(counters(500, 100));
        assert_eq!(d, FrameCounters::default());
    }

    #[test]
    fn a_clip_is_one_recording_with_totals_at_its_end() {
        let folders = vec![
            PathBuf::from(r"D:\raw\s1\b0"),
            PathBuf::from(r"D:\raw\s1\b1"),
        ];
        let mut t = DropTracker::new(folders.clone(), 120);
        assert!(
            t.on_sample(&sample(0.0, false, 0, None, counters(1000, 0)))
                .is_empty()
        );

        // OBS may spell the directory with forward slashes.
        let r = t.on_sample(&sample(
            1.0,
            true,
            100,
            Some("D:/raw/s1/b0/take0000/all"),
            counters(1100, 0),
        ));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].clip, 1);
        assert_eq!(r[0].take_folder.as_deref(), Some(folders[0].as_path()));
        assert!(r[0].recording && !r[0].lagging);

        let r = t.on_sample(&sample(
            2.0,
            true,
            1100,
            Some("D:/raw/s1/b0/take0000/all"),
            counters(1220, 60),
        ));
        assert_eq!(r[0].clip_counts, counters(120, 60));
        assert!(r[0].lagging, "half the last second skipped");

        let r = t.on_sample(&sample(3.0, false, 0, None, counters(1300, 60)));
        assert_eq!(r.len(), 1);
        assert!(!r[0].recording);
        assert_eq!(
            r[0].clip_counts,
            counters(120, 60),
            "totals stop at the last poll while recording"
        );
        assert!(r[0].lagging);

        let r = t.on_sample(&sample(
            4.0,
            true,
            50,
            Some(r"D:\raw\s1\b1\take0000\all"),
            counters(1400, 60),
        ));
        assert_eq!(r[0].clip, 2);
        assert_eq!(r[0].take_folder.as_deref(), Some(folders[1].as_path()));
        assert_eq!(r[0].clip_counts, FrameCounters::default());
    }

    #[test]
    fn a_recording_that_restarted_between_polls_is_a_new_clip() {
        let mut t = DropTracker::new(Vec::new(), 60);
        t.on_sample(&sample(0.0, true, 100, Some("a"), counters(0, 0)));
        t.on_sample(&sample(1.0, true, 5000, Some("a"), counters(300, 0)));
        // Same folder, but the duration went back down: OBS stopped and
        // started again inside one poll.
        let r = t.on_sample(&sample(2.0, true, 200, Some("a"), counters(400, 0)));
        assert_eq!(r.len(), 2);
        assert!(!r[0].recording && r[0].clip == 1);
        assert!(r[1].recording && r[1].clip == 2);
        // A new folder is a new clip too.
        let r = t.on_sample(&sample(3.0, true, 1200, Some("b"), counters(500, 0)));
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].clip, 3);
    }

    #[test]
    fn the_warning_follows_the_last_five_seconds_not_the_whole_clip() {
        let mut t = DropTracker::new(Vec::new(), 100);
        let mut skipped = 0;
        let mut last = Vec::new();
        // 5 s of heavy skipping, then 10 s clean.
        for s in 0..=15 {
            let lost = if (1..=5).contains(&s) { 50 } else { 0 };
            skipped += lost;
            last = t.on_sample(&sample(
                s as f64,
                true,
                s * 1000 + 1,
                Some("x"),
                counters(s * 100, skipped),
            ));
            if s == 5 {
                assert!(last[0].lagging);
            }
        }
        assert!(!last[0].lagging, "recovered for the last 5 s");
        assert!(
            last[0].clip_counts.skipped_share() > 0.1,
            "the clip still carries its loss"
        );
        let end = t.end_clip().unwrap();
        assert!(end.lagging, "250 of 1500 frames over the clip");
        assert!(t.end_clip().is_none());
    }

    #[test]
    fn a_directory_outside_the_plan_has_no_take_folder() {
        let folders = vec![PathBuf::from(r"D:\raw\s1\b0")];
        assert!(take_folder_for(r"D:\raw\s1\b0\take0000\all\", &folders).is_some());
        assert!(take_folder_for(r"d:\RAW\s1\b0\take0000\all", &folders).is_some());
        assert!(take_folder_for(r"D:\raw\s1\b0", &folders).is_none());
        assert!(take_folder_for(r"C:\Videos", &folders).is_none());
    }

    /// Against a stand-in OBS: the poller reads the three requests, reports
    /// a clip while it records and its totals once OBS stops, then stops.
    #[test]
    fn the_poller_reports_a_clip_from_obs_and_its_totals() {
        use super::super::fake_obs::{Reply, serve};
        use serde_json::json;
        use std::sync::Mutex;

        let polls = Arc::new(Mutex::new(0i64));
        let p = Arc::clone(&polls);
        let obs = serve(move |kind, _| {
            let mut n = p.lock().unwrap();
            match kind {
                "GetRecordStatus" => {
                    *n += 1;
                    // Records for polls 2 and 3, then stops.
                    let active = (2..=3).contains(&*n);
                    Reply::ok(json!({ "outputActive": active, "outputDuration": *n * 1000 }))
                }
                "GetRecordDirectory" => {
                    Reply::ok(json!({ "recordDirectory": "D:/raw/b0/take0000/all" }))
                }
                "GetStats" => Reply::ok(json!({
                    "outputTotalFrames": *n * 120,
                    "outputSkippedFrames": *n * 40,
                    "renderTotalFrames": *n * 120,
                    "renderSkippedFrames": 0,
                })),
                _ => Reply::ok(json!({})),
            }
        });
        let reports = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&reports);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let url = obs.url.clone();
        let poller = std::thread::spawn(move || {
            let mut tracker = DropTracker::new(vec![PathBuf::from(r"D:\raw\b0")], 120);
            let cancel = AtomicBool::new(false);
            run(
                &url,
                "",
                Duration::from_millis(30),
                &mut tracker,
                &stop_flag,
                &cancel,
                |r| sink.lock().unwrap().push(r),
            );
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while *polls.lock().unwrap() < 6 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
        poller.join().unwrap();

        let reports = reports.lock().unwrap();
        assert_eq!(reports.len(), 3, "{reports:?}");
        assert!(reports[0].recording && reports[1].recording && !reports[2].recording);
        assert_eq!(
            reports[2].take_folder.as_deref(),
            Some(std::path::Path::new(r"D:\raw\b0"))
        );
        assert_eq!(
            reports[2].clip_counts,
            FrameCounters {
                output_total: 120,
                output_skipped: 40,
                render_total: 120,
                render_skipped: 0,
            }
        );
        assert!(reports[2].lagging, "a third skipped");
    }
}
