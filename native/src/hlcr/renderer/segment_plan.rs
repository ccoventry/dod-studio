//! Which renders are cut into segments, and where (#1451): a long take
//! renders as several segments, each its own FFmpeg run and file, joined at
//! the end by stream copy, so a render stopped partway (Studio closed or
//! crashed, the PC restarted) resumes from its finished segments. Also the
//! FFmpeg command for one segment and for the join, and the key that tells
//! a segment of these settings from one of others. Pure: nothing here reads
//! the disk or spawns.
//!
//! Measured with FFmpeg 7.1.1 on synthetic takes (BMP sequence and utvideo
//! AVI, 300 fps, 22,050 Hz wav, 14 joins): a segmented ProRes 422 or HuffYUV
//! render joined with the concat demuxer matched the one-piece render frame
//! for frame (framemd5) and its PCM sound sample for sample. What makes that
//! hold, each found by a join that failed without it:
//!
//! - Each segment's sound is cut from the whole wav with `atrim` at the
//!   segment's start and end times, written the same way for the segment
//!   that ends at a frame and the one that starts there, so FFmpeg rounds
//!   both to the same sample: no gap and no overlap at a join. `-ss` on the
//!   wav or `-shortest` lost up to 1,820 samples per segment, and
//!   `-frames:v` ends the audio with the video's last packet.
//! - The picture is cut with `trim=end_frame`. `setpts` after it drops the
//!   last frame's duration and a MOV then hides that frame (999 of 1,000).
//! - A video take is seeked to a quarter frame before the segment's first
//!   frame. A seek to the half frame rounded to the frame before it at one
//!   join in four: FFmpeg keeps the time to the microsecond.
//!
//! H.264 renders (MP4, AAC sound) render in one piece: every AAC segment
//! carries its own encoder priming and padding, measured at about 1,400
//! samples (65 ms at 22,050 Hz) per segment, so the joined sound drifts out
//! of sync and clicks at every join. Custom renders do too: their codec and
//! container can be anything.

use std::path::Path;

use analysis::fnv1a::Fnv1a;

use super::ffmpeg_args::{Encode, audio_codec_args, tag_args};
use crate::hlcr::config::RenderCodec;
use crate::hlcr::scanner::ClipData;

/// Seconds of take per segment. At the measured ~15x real time for a
/// 2560x1440 300 fps take to ProRes 422 (a 10 s clip in about 2.5 min), a
/// segment is about 2.5 min of work: the most a stop can lose. Each segment
/// costs one more FFmpeg start and one read of the wav, well under a second,
/// and the join is a stream copy.
pub(super) const SEGMENT_SECONDS: u32 = 10;

/// A take of up to this many segments' length renders in one piece, as
/// before: a stop loses little, and segments would only add joins.
const ONE_PIECE_UP_TO_SEGMENTS: usize = 2;

/// What a segment's finished file is called, before its number.
pub(super) const SEGMENT_EXT: &str = "seg";

/// A frame rate as a fraction: `num / den` frames per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrameRate {
    pub(super) num: u32,
    pub(super) den: u32,
}

impl FrameRate {
    /// When frame `frame` (counted from 0, may be fractional) starts.
    fn seconds_at(self, frame: f64) -> f64 {
        frame * f64::from(self.den) / f64::from(self.num)
    }

    /// Whole frames in `seconds`, at least 1.
    fn frames_in(self, seconds: u32) -> usize {
        ((u64::from(seconds) * u64::from(self.num) / u64::from(self.den)) as usize).max(1)
    }

    /// As FFmpeg's `-framerate` takes it.
    fn to_arg(self) -> String {
        if self.den == 1 {
            self.num.to_string()
        } else {
            format!("{}/{}", self.num, self.den)
        }
    }
}

/// How the take's picture is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pictures {
    /// HLAE's numbered BMP files: a segment starts at its first file.
    Sequence,
    /// One AVI per stream folder: a segment seeks to its first frame.
    Video,
}

/// What a render's segments are decided from, read off the take by the
/// caller.
#[derive(Clone, Debug)]
pub(super) struct TakeFacts {
    pub(super) codec: RenderCodec,
    pub(super) is_hud: bool,
    /// The take's frames (`ClipData::frame_count`).
    pub(super) frame_count: usize,
    /// How the picture is read, and at what rate: the BMP sequence's
    /// `-framerate`, or the AVI's own header rate. `None` for a picture a
    /// segment can't start at a frame of (an OBS MP4, an AVI with no rate).
    pub(super) pictures: Option<(Pictures, FrameRate)>,
    /// The take's wav length; `None` without a wav (an OBS take, whose sound
    /// is in its video) or with one whose header can't be read.
    pub(super) wav_seconds: Option<f64>,
}

/// One render's segments: every `segment_frames` frames of the take, the
/// last one shorter.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SegmentPlan {
    pub(super) total_frames: usize,
    pub(super) segment_frames: usize,
    pub(super) pictures: Pictures,
    pub(super) rate: FrameRate,
}

/// One segment: its number and the frames it renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Segment {
    pub(super) index: usize,
    pub(super) start_frame: usize,
    pub(super) frames: usize,
}

impl SegmentPlan {
    /// Every segment in order.
    pub(super) fn segments(&self) -> Vec<Segment> {
        (0..self.total_frames)
            .step_by(self.segment_frames)
            .enumerate()
            .map(|(index, start_frame)| Segment {
                index,
                start_frame,
                frames: self.segment_frames.min(self.total_frames - start_frame),
            })
            .collect()
    }
}

/// Whether a render with this codec can be cut and joined without a seam:
/// an intra-frame picture (every frame stands alone) with PCM sound. The HUD
/// pass is always ProRes 4444 with PCM.
fn joins_cleanly(codec: RenderCodec, is_hud: bool) -> bool {
    is_hud
        || matches!(
            codec,
            RenderCodec::ProRes
                | RenderCodec::ProRes422
                | RenderCodec::ProResLt
                | RenderCodec::ProRes4444
                | RenderCodec::DnxHr
                | RenderCodec::HuffYuv
                | RenderCodec::Uncompressed
        )
}

/// The segments this render is cut into, or `None` to render it in one
/// piece: a codec that doesn't join cleanly, a picture a segment can't
/// start at a frame of, no wav (an OBS take), a take of up to
/// [`ONE_PIECE_UP_TO_SEGMENTS`] segments, or a wav shorter than the
/// picture by more than a frame (one piece trims the picture to the sound
/// with `-shortest`; segments would keep it all).
pub(super) fn plan(facts: &TakeFacts) -> Option<SegmentPlan> {
    if !joins_cleanly(facts.codec, facts.is_hud) {
        return None;
    }
    let (pictures, rate) = facts.pictures?;
    let wav_seconds = facts.wav_seconds?;
    let segment_frames = rate.frames_in(SEGMENT_SECONDS);
    if facts.frame_count <= segment_frames * ONE_PIECE_UP_TO_SEGMENTS {
        return None;
    }
    let picture_seconds = rate.seconds_at(facts.frame_count as f64);
    if wav_seconds < picture_seconds - rate.seconds_at(1.0) {
        return None;
    }
    Some(SegmentPlan {
        total_frames: facts.frame_count,
        segment_frames,
        pictures,
        rate,
    })
}

/// A time as FFmpeg is given it: to the microsecond, which is what it keeps.
fn seconds_arg(seconds: f64) -> String {
    format!("{seconds:.6}")
}

/// The FFmpeg muxer for a container, given by name because a segment's file
/// doesn't end in a video extension (so a folder walk for finished clips
/// never takes one for a clip).
fn muxer_for(file_ext: &str) -> &'static str {
    match file_ext {
        ".avi" => "avi",
        _ => "mov",
    }
}

/// Everything one segment's FFmpeg run is told.
pub(super) struct SegmentArgs<'a> {
    pub(super) clip: &'a ClipData,
    pub(super) plan: &'a SegmentPlan,
    pub(super) segment: Segment,
    pub(super) encode: &'a Encode<'a>,
    pub(super) threads: usize,
    pub(super) out_file: &'a Path,
}

impl SegmentArgs<'_> {
    /// The command line: the picture inputs started at the segment's first
    /// frame, the whole wav, a graph that cuts both to the segment, the
    /// encoder, and the output with its muxer named.
    pub(super) fn to_args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec!["-y".to_string(), "-hide_banner".to_string()];
        let pictures = self.picture_inputs();
        for picture in &pictures {
            args.extend(self.picture_input_args(picture));
        }
        // `plan` refuses a take without a wav, so there is one here.
        let wav = self.clip.wav_file.as_deref().unwrap_or("sound.wav");
        args.extend(["-thread_queue_size", "512", "-i", wav].map(str::to_string));
        args.extend([
            "-filter_complex".to_string(),
            self.graph(pictures.len()),
            "-map".to_string(),
            "[v]".to_string(),
            "-map".to_string(),
            "[a]".to_string(),
        ]);
        args.extend(self.encode.video_args.iter().map(|s| s.to_string()));
        args.extend(["-threads".to_string(), self.threads.to_string()]);
        args.extend(
            audio_codec_args(self.encode.file_ext)
                .iter()
                .map(|s| s.to_string()),
        );
        args.extend(
            [
                "-progress",
                "pipe:1",
                "-loglevel",
                "error",
                "-f",
                muxer_for(self.encode.file_ext),
            ]
            .map(str::to_string),
        );
        args.push(self.out_file.to_string_lossy().into_owned());
        args
    }

    /// The picture inputs, relative to the take folder: the main stream, or
    /// the HUD's colour and alpha pair.
    fn picture_inputs(&self) -> Vec<String> {
        let clip = self.clip;
        let mut folders = vec![clip.img_folder.as_str()];
        if clip.clip_type == "hud_only" {
            // Older autosaves predate the field; see `input_args`.
            folders.push(clip.alpha_folder.as_deref().unwrap_or("hudalpha"));
        }
        folders
            .into_iter()
            .map(
                |folder| match (self.plan.pictures, clip.video_file.as_deref()) {
                    (Pictures::Video, Some(video)) => format!("{folder}/{video}"),
                    _ => format!("{folder}/%05d.bmp"),
                },
            )
            .collect()
    }

    /// One picture input, started at the segment's first frame.
    fn picture_input_args(&self, input: &str) -> Vec<String> {
        let start = self.segment.start_frame;
        let mut args: Vec<String> = Vec::new();
        match self.plan.pictures {
            Pictures::Sequence => {
                args.extend(
                    [
                        "-probesize",
                        "32",
                        "-analyzeduration",
                        "0",
                        "-thread_queue_size",
                        "512",
                    ]
                    .map(str::to_string),
                );
                args.extend([
                    "-start_number".to_string(),
                    start.to_string(),
                    "-framerate".to_string(),
                    self.plan.rate.to_arg(),
                ]);
            }
            Pictures::Video if start > 0 => {
                let seek = self.plan.rate.seconds_at(start as f64 - 0.25);
                args.extend(["-ss".to_string(), seconds_arg(seek)]);
            }
            Pictures::Video => {}
        }
        args.extend(["-i".to_string(), input.to_string()]);
        args
    }

    /// The filter graph: the picture (the HUD's colour and alpha merged)
    /// cut to the segment's frames as `[v]`, the wav cut to the segment's
    /// times as `[a]`. The wav is the input after the pictures.
    fn graph(&self, pictures: usize) -> String {
        let segment = self.segment;
        let picture = if pictures == 2 {
            "[1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge"
        } else {
            "[0:v]null"
        };
        let start = self.plan.rate.seconds_at(segment.start_frame as f64);
        let end = self
            .plan
            .rate
            .seconds_at((segment.start_frame + segment.frames) as f64);
        format!(
            "{picture},trim=end_frame={}[v];[{pictures}:a]atrim=start={}:end={},asetpts=PTS-STARTPTS[a]",
            segment.frames,
            seconds_arg(start),
            seconds_arg(end),
        )
    }
}

/// The concat demuxer's list of a render's finished segments, in order.
/// Names only: the list sits beside them, and FFmpeg reads names in it
/// relative to the list.
pub(super) fn concat_list(segment_names: &[String]) -> String {
    segment_names
        .iter()
        .map(|name| format!("file '{name}'\n"))
        .collect()
}

/// The join's command line: the segments listed in `list_file` (a bare name,
/// read in the segment folder FFmpeg runs in) copied into `out_file` as they
/// are, with the render's DoD Studio tag (#1442).
/// `-xerror` ends it with an error at a segment it can't read: without it the
/// concat demuxer stops at a missing or damaged segment and FFmpeg still
/// exits 0 with a short clip. The caller checks the joined frame count too.
pub(super) fn join_args(
    list_file: &Path,
    out_file: &Path,
    file_ext: &str,
    clip_tag: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = ["-y", "-hide_banner", "-xerror", "-f", "concat", "-i"]
        .map(str::to_string)
        .to_vec();
    args.push(list_file.to_string_lossy().into_owned());
    args.extend(
        [
            "-map",
            "0",
            "-c",
            "copy",
            "-progress",
            "pipe:1",
            "-loglevel",
            "error",
        ]
        .map(str::to_string),
    );
    if let Some(tag) = clip_tag {
        args.extend(tag_args(tag, file_ext));
    }
    args.push(out_file.to_string_lossy().into_owned());
    args
}

/// What names a render's segment folder: which take and stream it is of,
/// and everything the segments' content depends on. Segments of the same
/// take and stream under other settings are thrown away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SegmentKey {
    pub(super) take: String,
    pub(super) settings: String,
}

impl SegmentKey {
    /// The segment folder's name.
    pub(super) fn folder_name(&self) -> String {
        format!("{}_{}", self.take, self.settings)
    }

    /// Whether `folder_name` is a segment folder of this take and stream
    /// under other settings.
    pub(super) fn is_other_settings(&self, folder_name: &str) -> bool {
        folder_name
            .strip_prefix(&self.take)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|settings| settings != self.settings)
    }
}

/// What a segment's content depends on, beyond the take and stream.
pub(super) struct KeySettings<'a> {
    pub(super) encode: &'a Encode<'a>,
    pub(super) plan: &'a SegmentPlan,
    pub(super) ffmpeg_path: &'a str,
    /// The take's wav and video sizes and modified times: a take written
    /// over in place changes them, even at the same length.
    pub(super) source_stamp: &'a str,
}

/// Bumped when a segment's command line changes in a way the key's parts
/// don't show, so segments of an older build are thrown away.
const KEY_FORMAT: &str = "1";

/// The key of `clip`'s segments under `settings`. FFmpeg's thread count is
/// left out: it doesn't change what an intra-frame encoder writes, and it
/// follows Max Concurrent Renders, which may change between the stop and
/// the resume.
pub(super) fn segment_key(clip: &ClipData, settings: &KeySettings) -> SegmentKey {
    let plan = settings.plan;
    let settings = hex(&[
        KEY_FORMAT,
        &clip.frame_count.to_string(),
        settings.source_stamp,
        &settings.encode.video_args.join(" "),
        settings.encode.file_ext,
        &format!("{:?}", plan.pictures),
        &plan.rate.to_arg(),
        &plan.segment_frames.to_string(),
        settings.ffmpeg_path,
    ]);
    SegmentKey {
        take: take_key(clip),
        settings,
    }
}

/// The take-and-stream part of `clip`'s segment keys, whatever the
/// settings: every segment folder of this take and stream starts with it.
pub(super) fn take_key(clip: &ClipData) -> String {
    hex(&[
        &clip.take_folder,
        &clip.clip_type,
        &clip.img_folder,
        clip.alpha_folder.as_deref().unwrap_or(""),
        clip.wav_file.as_deref().unwrap_or(""),
        clip.video_file.as_deref().unwrap_or(""),
    ])
}

/// `parts` hashed together, as 16 hex digits.
fn hex(parts: &[&str]) -> String {
    let mut hash = Fnv1a::new();
    for part in parts {
        hash.update(part.as_bytes());
        // A separator no part holds, so ("ab", "c") and ("a", "bc") differ.
        hash.update(&[0]);
    }
    format!("{:016x}", hash.finish())
}

/// The clip size a resume reserves for: what its finished segments weigh
/// per frame, over the whole take, with `margin` (the learned rates' head
/// room, `LEARNED_RATE_MARGIN`), and never more than `clip_estimate`. Right
/// after a restart the estimate is the raw size, and twice that seldom fits.
pub(super) fn resume_clip_bytes(
    clip_estimate: u64,
    plan: &SegmentPlan,
    frames_on_disk: usize,
    bytes_on_disk: u64,
    margin: f64,
) -> u64 {
    if frames_on_disk == 0 {
        return clip_estimate;
    }
    let per_frame = bytes_on_disk as f64 / frames_on_disk as f64;
    let learned = (per_frame * plan.total_frames as f64 * margin).ceil() as u64;
    learned.min(clip_estimate)
}

/// The most a segmented render has on its export drive at once, for a clip
/// of `clip_bytes`: the joined clip, plus the segments still to render (the
/// finished ones are on the disk already, so its free space counts them).
/// Twice the clip for a render starting from nothing.
pub(super) fn peak_bytes(clip_bytes: u64, plan: &SegmentPlan, frames_on_disk: usize) -> u64 {
    let total = plan.total_frames.max(1) as u128;
    let to_render = plan.total_frames.saturating_sub(frames_on_disk) as u128;
    let segments = (u128::from(clip_bytes) * to_render).div_ceil(total);
    clip_bytes.saturating_add(u64::try_from(segments).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlcr::config::RenderConfig;
    use crate::hlcr::renderer::ffmpeg_args::encode_settings;
    use std::path::PathBuf;

    const RATE_300: FrameRate = FrameRate { num: 300, den: 1 };

    fn facts(codec: RenderCodec, frame_count: usize) -> TakeFacts {
        TakeFacts {
            codec,
            is_hud: false,
            frame_count,
            pictures: Some((Pictures::Sequence, RATE_300)),
            wav_seconds: Some(frame_count as f64 / 300.0),
        }
    }

    #[test]
    fn a_take_up_to_two_segments_long_renders_in_one_piece() {
        assert_eq!(plan(&facts(RenderCodec::ProRes422, 6000)), None);
        let long = plan(&facts(RenderCodec::ProRes422, 6001)).unwrap();
        assert_eq!(long.segment_frames, 3000);
        assert_eq!(long.segments().len(), 3);
    }

    #[test]
    fn intra_frame_codecs_with_pcm_sound_are_segmented() {
        for codec in [
            RenderCodec::ProRes,
            RenderCodec::ProRes422,
            RenderCodec::ProResLt,
            RenderCodec::ProRes4444,
            RenderCodec::DnxHr,
            RenderCodec::HuffYuv,
            RenderCodec::Uncompressed,
        ] {
            assert!(plan(&facts(codec, 9000)).is_some(), "{codec:?}");
        }
    }

    /// H.264 renders carry AAC sound, which drifts at every join; Custom
    /// can be anything.
    #[test]
    fn h264_custom_and_skip_render_in_one_piece() {
        for codec in [
            RenderCodec::NvencH264,
            RenderCodec::H264Software,
            RenderCodec::Custom,
            RenderCodec::SourceCopy,
        ] {
            assert_eq!(plan(&facts(codec, 9000)), None, "{codec:?}");
        }
    }

    #[test]
    fn the_hud_pass_is_segmented_whatever_the_main_codec() {
        let hud = TakeFacts {
            is_hud: true,
            ..facts(RenderCodec::H264Software, 9000)
        };
        assert!(plan(&hud).is_some());
    }

    #[test]
    fn a_take_with_no_wav_or_no_seekable_picture_renders_in_one_piece() {
        let no_wav = TakeFacts {
            wav_seconds: None,
            ..facts(RenderCodec::ProRes, 9000)
        };
        assert_eq!(plan(&no_wav), None);
        let no_picture = TakeFacts {
            pictures: None,
            ..facts(RenderCodec::ProRes, 9000)
        };
        assert_eq!(plan(&no_picture), None);
    }

    #[test]
    fn a_wav_shorter_than_the_picture_by_more_than_a_frame_renders_in_one_piece() {
        let mut take = facts(RenderCodec::ProRes, 9000);
        take.wav_seconds = Some(30.0 - 0.5 / 300.0);
        assert!(plan(&take).is_some());
        take.wav_seconds = Some(29.9);
        assert_eq!(plan(&take), None);
    }

    #[test]
    fn segments_cover_every_frame_once_with_a_shorter_last_one() {
        let plan = SegmentPlan {
            total_frames: 7,
            segment_frames: 3,
            pictures: Pictures::Sequence,
            rate: RATE_300,
        };
        let spans: Vec<(usize, usize, usize)> = plan
            .segments()
            .iter()
            .map(|s| (s.index, s.start_frame, s.frames))
            .collect();
        assert_eq!(spans, [(0, 0, 3), (1, 3, 3), (2, 6, 1)]);
    }

    #[test]
    fn segment_frames_follow_the_rate() {
        assert_eq!(FrameRate { num: 60, den: 1 }.frames_in(10), 600);
        assert_eq!(
            FrameRate {
                num: 60000,
                den: 1001
            }
            .frames_in(10),
            599
        );
        assert_eq!(FrameRate { num: 1, den: 100 }.frames_in(10), 1);
    }

    fn clip(video: Option<&str>, hud: bool) -> ClipData {
        ClipData {
            clip_name: None,
            take_folder: "D:/takes/s/take0000".to_string(),
            clip_type: if hud { "hud_only" } else { "single" }.to_string(),
            img_folder: if hud { "hudcolor" } else { "all" }.to_string(),
            wav_file: Some("sound.wav".to_string()),
            base_name: "t".to_string(),
            frame_count: 900,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: video.map(str::to_string),
            alpha_folder: hud.then(|| "hudalpha".to_string()),
        }
    }

    fn config(codec: RenderCodec) -> RenderConfig {
        RenderConfig {
            ffmpeg_path: "ffmpeg".to_string(),
            source_folder: String::new(),
            export_directories: Vec::new(),
            fps: 300,
            target_codec: codec,
            custom_codec_args: String::new(),
            max_concurrent_renders: 1,
            clip_tag: None,
            resumable_renders: true,
            live_thread_share: Default::default(),
        }
    }

    fn plan_of(pictures: Pictures) -> SegmentPlan {
        SegmentPlan {
            total_frames: 900,
            segment_frames: 300,
            pictures,
            rate: RATE_300,
        }
    }

    fn args_line(clip: &ClipData, pictures: Pictures, index: usize) -> String {
        let config = config(RenderCodec::ProRes422);
        let encode = encode_settings(clip.clip_type == "hud_only", &config).unwrap();
        let plan = plan_of(pictures);
        let out = PathBuf::from("seg").join("0001.partial.seg");
        SegmentArgs {
            clip,
            plan: &plan,
            segment: plan.segments()[index],
            encode: &encode,
            threads: 4,
            out_file: &out,
        }
        .to_args()
        .join(" ")
    }

    #[test]
    fn a_sequence_segment_starts_at_its_first_file_and_cuts_the_wav_to_its_times() {
        let line = args_line(&clip(None, false), Pictures::Sequence, 1);
        let out = PathBuf::from("seg").join("0001.partial.seg");
        assert_eq!(
            line,
            format!(
                "-y -hide_banner -probesize 32 -analyzeduration 0 -thread_queue_size 512 -start_number 300 -framerate 300 -i all/%05d.bmp \
                 -thread_queue_size 512 -i sound.wav \
                 -filter_complex [0:v]null,trim=end_frame=300[v];[1:a]atrim=start=1.000000:end=2.000000,asetpts=PTS-STARTPTS[a] -map [v] -map [a] \
                 -c:v prores_ks -profile:v standard -pix_fmt yuv422p10le -threads 4 -c:a pcm_s16le -progress pipe:1 -loglevel error -f mov {}",
                out.display()
            )
        );
    }

    #[test]
    fn a_video_segment_seeks_a_quarter_frame_before_its_first_frame() {
        let line = args_line(&clip(Some("video.avi"), false), Pictures::Video, 2);
        assert!(
            line.starts_with(
                "-y -hide_banner -ss 1.999167 -i all/video.avi -thread_queue_size 512 -i sound.wav"
            ),
            "{line}"
        );
        assert!(line.contains("atrim=start=2.000000:end=3.000000"), "{line}");
        // The first segment needs no seek.
        let first = args_line(&clip(Some("video.avi"), false), Pictures::Video, 0);
        assert!(
            first.starts_with("-y -hide_banner -i all/video.avi "),
            "{first}"
        );
    }

    #[test]
    fn a_hud_segment_starts_both_streams_and_merges_the_alpha_before_the_cut() {
        let line = args_line(&clip(Some("video.avi"), true), Pictures::Video, 1);
        assert!(
            line.contains("-ss 0.999167 -i hudcolor/video.avi -ss 0.999167 -i hudalpha/video.avi -thread_queue_size 512 -i sound.wav"),
            "{line}"
        );
        assert!(
            line.contains("[1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge,trim=end_frame=300[v];[2:a]atrim="),
            "{line}"
        );
        assert!(
            line.contains("-profile:v 4444 -pix_fmt yuva444p10le"),
            "{line}"
        );
    }

    /// The end of one segment and the start of the next are written alike,
    /// so FFmpeg rounds them to the same sample.
    #[test]
    fn adjacent_segments_share_their_boundary_time_exactly() {
        let rate = FrameRate {
            num: 60000,
            den: 1001,
        };
        let plan = SegmentPlan {
            total_frames: 3000,
            segment_frames: 599,
            pictures: Pictures::Sequence,
            rate,
        };
        let segments = plan.segments();
        for pair in segments.windows(2) {
            let end = rate.seconds_at((pair[0].start_frame + pair[0].frames) as f64);
            let start = rate.seconds_at(pair[1].start_frame as f64);
            assert_eq!(seconds_arg(end), seconds_arg(start));
        }
    }

    #[test]
    fn an_avi_segment_is_muxed_as_avi() {
        let clip = clip(None, false);
        let config = config(RenderCodec::HuffYuv);
        let encode = encode_settings(false, &config).unwrap();
        let plan = plan_of(Pictures::Sequence);
        let out = PathBuf::from("0000.partial.seg");
        let args = SegmentArgs {
            clip: &clip,
            plan: &plan,
            segment: plan.segments()[0],
            encode: &encode,
            threads: 1,
            out_file: &out,
        }
        .to_args();
        assert_eq!(args[args.len() - 3..args.len() - 1], ["-f", "avi"]);
    }

    #[test]
    fn the_join_copies_the_listed_segments_and_writes_the_tag() {
        let list = PathBuf::from("seg").join("join.txt");
        let out = PathBuf::from("out").join("clip.partial.mov");
        let args = join_args(&list, &out, ".mov", Some("v1/s/t/1/all"));
        assert_eq!(
            args.join(" "),
            format!(
                "-y -hide_banner -xerror -f concat -i {} -map 0 -c copy -progress pipe:1 -loglevel error -metadata dodstudio_clip=v1/s/t/1/all -movflags use_metadata_tags {}",
                list.display(),
                out.display()
            )
        );
        assert_eq!(
            concat_list(&["0000.seg".to_string(), "0001.seg".to_string()]),
            "file '0000.seg'\nfile '0001.seg'\n"
        );
    }

    fn key_for(clip: &ClipData, codec: RenderCodec, source_stamp: &str) -> SegmentKey {
        let config = config(codec);
        let encode = encode_settings(false, &config).unwrap();
        let plan = plan_of(Pictures::Sequence);
        segment_key(
            clip,
            &KeySettings {
                encode: &encode,
                plan: &plan,
                ffmpeg_path: "ffmpeg",
                source_stamp,
            },
        )
    }

    #[test]
    fn the_key_changes_with_the_settings_but_not_the_take_part() {
        let clip = clip(None, false);
        let base = key_for(&clip, RenderCodec::ProRes422, "10");
        assert_eq!(base, key_for(&clip, RenderCodec::ProRes422, "10"));
        let other_codec = key_for(&clip, RenderCodec::ProResLt, "10");
        assert_eq!(other_codec.take, base.take);
        assert_ne!(other_codec.settings, base.settings);
        assert!(base.is_other_settings(&other_codec.folder_name()));
        assert!(!base.is_other_settings(&base.folder_name()));
        let rewritten = key_for(&clip, RenderCodec::ProRes422, "11");
        assert_ne!(rewritten.settings, base.settings);
    }

    #[test]
    fn every_key_of_a_take_starts_with_its_take_key() {
        let clip = clip(None, false);
        let key = key_for(&clip, RenderCodec::ProRes422, "10");
        assert_eq!(key.take, take_key(&clip));
        assert!(key.folder_name().starts_with(&take_key(&clip)));
    }

    #[test]
    fn a_fresh_segmented_render_needs_twice_the_clip_and_a_resume_less() {
        let plan = plan_of(Pictures::Sequence);
        assert_eq!(peak_bytes(1000, &plan, 0), 2000);
        // Two of three segments on the disk: the clip and one more segment.
        assert_eq!(peak_bytes(1000, &plan, 600), 1334);
        assert_eq!(peak_bytes(1000, &plan, 900), 1000);
        assert_eq!(peak_bytes(u64::MAX, &plan, 0), u64::MAX);
    }

    /// Two of three segments (600 of 900 frames) weigh 6000 bytes: 10 a
    /// frame, so the clip is 900 x 10 x 1.5.
    #[test]
    fn a_resume_sizes_the_clip_from_its_finished_segments() {
        let plan = plan_of(Pictures::Sequence);
        assert_eq!(resume_clip_bytes(1_000_000, &plan, 600, 6000, 1.5), 13_500);
        assert_eq!(resume_clip_bytes(10_000, &plan, 600, 6000, 1.5), 10_000);
        assert_eq!(resume_clip_bytes(10_000, &plan, 0, 0, 1.5), 10_000);
    }

    #[test]
    fn another_take_or_stream_is_never_other_settings_of_this_one() {
        let main = key_for(&clip(None, false), RenderCodec::ProRes422, "10");
        let mut other = clip(None, false);
        other.take_folder = "D:/takes/s/take0001".to_string();
        let other = key_for(&other, RenderCodec::ProRes422, "10");
        assert_ne!(other.take, main.take);
        assert!(!main.is_other_settings(&other.folder_name()));
        let mut alt_wav = clip(None, false);
        alt_wav.wav_file = Some("alt.wav".to_string());
        let alt_wav = key_for(&alt_wav, RenderCodec::ProRes422, "10");
        assert!(!main.is_other_settings(&alt_wav.folder_name()));
    }
}
