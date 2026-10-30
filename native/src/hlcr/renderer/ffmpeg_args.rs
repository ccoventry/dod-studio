//! The FFmpeg command line a render job runs: its inputs for each take
//! shape, the encoder settings and container for each codec, and the output
//! options every render shares. Pure: nothing here reads the disk or spawns.

use std::path::Path;

use crate::hlcr::config::{RenderCodec, RenderConfig};
use crate::hlcr::scanner::ClipData;

/// The picture encoder's arguments and the container they go in.
///
/// Decided in the same match, not looked up separately, so the two can't
/// silently drift apart the way the old string-keyed get_codec_preset()
/// already had (it matched "h264" for .mp4, but RenderCodec::NvencH264 has
/// always mapped to the distinct "h264_nvenc" id — dead code, removed, never
/// actually wired to a render).
#[derive(Debug, PartialEq)]
pub(super) struct Encode<'a> {
    /// Not `'static`: the Custom arm borrows from `config.custom_codec_args`.
    pub(super) video_args: Vec<&'a str>,
    /// With its dot: `.mp4`, `.mov`, `.avi`, `.mkv`.
    pub(super) file_ext: &'static str,
}

/// The encoder and container for a job, or why it can't be rendered.
/// Alpha/HUD output always needs a MOV container regardless of the selected
/// codec (QuickTime-style alpha), matching dev's own behaviour and the
/// dropdown's "MP4" label applying only to the non-alpha H.264 variants.
pub(super) fn encode_settings(is_hud: bool, config: &RenderConfig) -> Result<Encode<'_>, String> {
    let mut codec_args: Vec<&str> = Vec::new();
    let file_ext = if is_hud {
        codec_args.extend_from_slice(&[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4444",
            "-pix_fmt",
            "yuva444p10le",
        ]);
        ".mov"
    } else {
        match config.target_codec {
            RenderCodec::NvencH264 => {
                codec_args.extend_from_slice(&[
                    "-c:v",
                    "h264_nvenc",
                    "-preset",
                    "p6",
                    "-tune",
                    "hq",
                    "-cq",
                    "15",
                    "-pix_fmt",
                    "yuv420p",
                ]);
                ".mp4"
            }
            RenderCodec::H264Software => {
                codec_args.extend_from_slice(&[
                    "-c:v", "libx264", "-preset", "fast", "-crf", "16", "-pix_fmt", "yuv420p",
                ]);
                ".mp4"
            }
            codec @ (RenderCodec::ProRes
            | RenderCodec::ProRes422
            | RenderCodec::ProResLt
            | RenderCodec::ProRes4444) => {
                codec_args.extend(prores_args(codec));
                ".mov"
            }
            RenderCodec::DnxHr => {
                codec_args.extend_from_slice(&[
                    "-c:v",
                    "dnxhd",
                    "-profile:v",
                    "dnxhr_hq",
                    "-pix_fmt",
                    "yuv422p",
                ]);
                ".mov"
            }
            RenderCodec::HuffYuv => {
                codec_args.extend_from_slice(&["-c:v", "huffyuv", "-pix_fmt", "yuv422p"]);
                ".avi"
            }
            RenderCodec::Uncompressed => {
                codec_args.extend_from_slice(&["-c:v", "rawvideo", "-pix_fmt", "yuv422p"]);
                ".avi"
            }
            // No allow-list, no parsing beyond the whitespace split --
            // FFmpeg itself is what validates this, exactly as it would a
            // codec typed directly at a terminal.
            RenderCodec::Custom => {
                if config.custom_codec_args.trim().is_empty() {
                    return Err(
                        "Custom codec selected but no FFmpeg arguments were entered.".to_string(),
                    );
                }
                codec_args.extend(config.custom_codec_args.split_whitespace());
                ".mkv"
            }
            // A Skip job returns before it gets here — it has its own
            // extension and never runs an FFmpeg encode at all. Not
            // `unreachable!()`: release builds run with `panic = "abort"`
            // (see `obs::session::ObsSession::stop_handle`'s doc comment), so
            // a panic here would kill the whole render batch, not just this
            // job, if some future edit ever broke that invariant. Failing
            // loudly through the normal update channel costs nothing today
            // and is safe if it ever turns out to be reachable.
            RenderCodec::SourceCopy => {
                return Err("Internal error: SourceCopy reached the FFmpeg codec path.".to_string());
            }
        }
    };
    Ok(Encode {
        video_args: codec_args,
        file_ext,
    })
}

/// The main picture's `-c:v` args for a ProRes choice; empty for any other
/// codec. The HUD stream never comes here: it is always 4444 with alpha.
pub(super) fn prores_args(codec: RenderCodec) -> Vec<&'static str> {
    match codec.prores_profile() {
        Some((profile, pix_fmt, _)) => vec![
            "-c:v",
            "prores_ks",
            "-profile:v",
            profile,
            "-pix_fmt",
            pix_fmt,
        ],
        None => Vec::new(),
    }
}

/// The audio encoder for a container.
///
/// MP4 has never had solid cross-player support for raw PCM audio — ffmpeg's
/// MP4 muxer boxes it as `ipcm`, which FFmpeg/VLC read fine but which Vegas
/// Pro (and likely other strict NLEs) can't decode, producing garbled/static
/// audio despite the samples themselves being intact (confirmed: the exact
/// same PCM bytes remuxed into a .mov container instead, where ffmpeg uses
/// the older/broadly-supported `sowt` tag, play back correctly everywhere).
/// AAC sidesteps the whole box-tag mess and is the standard choice for MP4
/// audio; every other codec here (ProRes/DNxHD's .mov, HuffYUV/rawvideo's
/// .avi) stays lossless PCM since none of them are .mp4.
pub(super) fn audio_codec_args(file_ext: &str) -> &'static [&'static str] {
    if file_ext == ".mp4" {
        &["-c:a", "aac", "-b:a", "192k"]
    } else {
        &["-c:a", "pcm_s16le"]
    }
}

/// FFmpeg's `-threads` for one job: the machine's threads shared between
/// the renders allowed at once, at least 1; 2 when the machine won't say.
pub(super) fn threads_per_process(available: Option<usize>, max_concurrent: usize) -> usize {
    match available {
        Some(val) => std::cmp::max(1, val / max_concurrent),
        None => 2,
    }
}

/// The input arguments for a take's shape: a BMP sequence or a video per
/// stream folder, one picture or the HUD's colour and alpha pair, with its
/// wav or (an OBS take) the audio already in the video. Paths are relative:
/// FFmpeg runs in the take folder.
///
/// `-framerate` is given only for a BMP sequence. It tells FFmpeg how to
/// interpret an untimed image sequence; a video carries its own timing, and
/// forcing a rate onto it would re-time the clip. That also means a video
/// take cannot suffer the FPS mismatch that BMP takes can — there is no
/// second number to disagree with.
///
/// The wav `expect`s hold because `check_inputs` refuses a HUD take
/// without one, and the scanner never admits a BMP take without one
/// (`take_shape_is_renderable`).
pub(super) fn input_args(clip: &ClipData, fps: &str) -> Vec<String> {
    // The alpha half of a HUD pair, as the scanner recorded it. Older autosaves
    // predate the field, so fall back to the literal the renderer used to
    // hardcode — that is exactly what those takes were scanned as.
    let alpha_folder = clip.alpha_folder.as_deref().unwrap_or("hudalpha");
    let is_hud = clip.clip_type == "hud_only";
    let mut args: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(|s| s.to_string()));

    if let Some(video) = clip.video_file.as_deref() {
        if is_hud {
            let wav = clip
                .wav_file
                .as_deref()
                .expect("validated by check_inputs: HUD clips require a wav");
            let hud_color_input = format!("{}/{}", clip.img_folder, video);
            let hud_alpha_input = format!("{}/{}", alpha_folder, video);
            push(&[
                "-i",
                &hud_color_input,
                "-i",
                &hud_alpha_input,
                "-thread_queue_size",
                "512",
                "-i",
                wav,
                "-filter_complex",
                "[1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge[hud]",
                "-map",
                "[hud]",
                "-map",
                "2:a",
            ]);
        } else if let Some(wav) = clip.wav_file.as_deref() {
            let video_input = format!("{}/{}", clip.img_folder, video);
            push(&["-i", &video_input, "-thread_queue_size", "512", "-i", wav]);
        } else {
            // An OBS take: the video already carries its own audio stream, so
            // there is nothing to mux against. A single input with no `-map`
            // needed — FFmpeg's default stream selection already picks one
            // video and one audio stream out of it.
            let video_input = format!("{}/{}", clip.img_folder, video);
            push(&["-i", &video_input]);
        }
    } else if is_hud {
        let wav = clip
            .wav_file
            .as_deref()
            .expect("validated by check_inputs: HUD clips require a wav");
        let hud_color_input = format!("{}/%05d.bmp", clip.img_folder);
        let hud_alpha_input = format!("{}/%05d.bmp", alpha_folder);
        push(&[
            // Skip probe/analyze on known BMP sequences; add read-ahead buffering.
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-thread_queue_size",
            "512",
            "-framerate",
            fps,
            "-i",
            &hud_color_input,
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-thread_queue_size",
            "512",
            "-framerate",
            fps,
            "-i",
            &hud_alpha_input,
            "-thread_queue_size",
            "512",
            "-i",
            wav,
            "-filter_complex",
            "[1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge[hud]",
            "-map",
            "[hud]",
            "-map",
            "2:a",
        ]);
    } else {
        let wav = clip
            .wav_file
            .as_deref()
            .expect("admitted by take_shape_is_renderable: BMP takes require a wav");
        let img_input = format!("{}/%05d.bmp", clip.img_folder);
        push(&[
            // Skip probe/analyze on known BMP sequences; add read-ahead buffering.
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-thread_queue_size",
            "512",
            "-framerate",
            fps,
            "-i",
            &img_input,
            "-thread_queue_size",
            "512",
            "-i",
            wav,
        ]);
    }
    args
}

/// Everything one FFmpeg render is told, in order: overwrite and quiet
/// banner, the inputs, the picture encoder, threads, the audio encoder, and
/// the output options with the output file last.
pub(super) struct FfmpegArgs<'a> {
    pub(super) clip: &'a ClipData,
    /// The rate the BMP frames are read at (`take_meta::render_fps`).
    pub(super) fps: u32,
    pub(super) encode: &'a Encode<'a>,
    pub(super) threads: usize,
    pub(super) out_file: &'a Path,
}

impl FfmpegArgs<'_> {
    pub(super) fn to_args(&self) -> Vec<String> {
        let mut args: Vec<String> = vec!["-y".to_string(), "-hide_banner".to_string()];
        args.extend(input_args(self.clip, &self.fps.to_string()));
        args.extend(self.encode.video_args.iter().map(|s| s.to_string()));
        args.extend(["-threads".to_string(), self.threads.to_string()]);
        args.extend(
            audio_codec_args(self.encode.file_ext)
                .iter()
                .map(|s| s.to_string()),
        );
        args.extend(
            [
                "-shortest",
                // +faststart is only needed for HTTP streaming; omitting it avoids the
                // post-render moov-atom rewrite pass on what can be multi-GB files.
                "-progress",
                "pipe:1",
                "-loglevel",
                "error",
            ]
            .map(str::to_string),
        );
        args.push(self.out_file.to_string_lossy().into_owned());
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn config(codec: RenderCodec, custom: &str) -> RenderConfig {
        RenderConfig {
            ffmpeg_path: "ffmpeg".to_string(),
            source_folder: String::new(),
            export_directories: Vec::new(),
            fps: 300,
            target_codec: codec,
            custom_codec_args: custom.to_string(),
            max_concurrent_renders: 1,
        }
    }

    fn clip(
        clip_type: &str,
        img: &str,
        wav: Option<&str>,
        video: Option<&str>,
        alpha: Option<&str>,
    ) -> ClipData {
        ClipData {
            clip_name: None,
            take_folder: String::new(),
            clip_type: clip_type.to_string(),
            img_folder: img.to_string(),
            wav_file: wav.map(str::to_string),
            base_name: "t".to_string(),
            frame_count: 10,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: video.map(str::to_string),
            alpha_folder: alpha.map(str::to_string),
        }
    }

    fn encode_line(is_hud: bool, codec: RenderCodec, custom: &str) -> String {
        let config = config(codec, custom);
        let e = encode_settings(is_hud, &config).unwrap();
        format!("{} {}", e.video_args.join(" "), e.file_ext)
    }

    #[test]
    fn each_prores_choice_renders_with_its_own_profile() {
        let args = |c: RenderCodec| prores_args(c).join(" ");
        assert_eq!(
            args(RenderCodec::ProRes),
            "-c:v prores_ks -profile:v hq -pix_fmt yuv422p10le"
        );
        assert_eq!(
            args(RenderCodec::ProRes422),
            "-c:v prores_ks -profile:v standard -pix_fmt yuv422p10le"
        );
        assert_eq!(
            args(RenderCodec::ProResLt),
            "-c:v prores_ks -profile:v lt -pix_fmt yuv422p10le"
        );
        assert_eq!(
            args(RenderCodec::ProRes4444),
            "-c:v prores_ks -profile:v 4444 -pix_fmt yuv444p10le"
        );
        assert_eq!(args(RenderCodec::DnxHr), "");
    }

    /// Every codec's encoder arguments and container, as renders ran them
    /// before #1177 split `run_render_job` (captured from its command lines).
    #[test]
    fn each_codec_has_its_encoder_and_container() {
        use RenderCodec::*;
        let cases = [
            (
                NvencH264,
                "-c:v h264_nvenc -preset p6 -tune hq -cq 15 -pix_fmt yuv420p .mp4",
            ),
            (
                H264Software,
                "-c:v libx264 -preset fast -crf 16 -pix_fmt yuv420p .mp4",
            ),
            (
                ProRes,
                "-c:v prores_ks -profile:v hq -pix_fmt yuv422p10le .mov",
            ),
            (
                ProRes422,
                "-c:v prores_ks -profile:v standard -pix_fmt yuv422p10le .mov",
            ),
            (
                ProResLt,
                "-c:v prores_ks -profile:v lt -pix_fmt yuv422p10le .mov",
            ),
            (
                ProRes4444,
                "-c:v prores_ks -profile:v 4444 -pix_fmt yuv444p10le .mov",
            ),
            (
                DnxHr,
                "-c:v dnxhd -profile:v dnxhr_hq -pix_fmt yuv422p .mov",
            ),
            (HuffYuv, "-c:v huffyuv -pix_fmt yuv422p .avi"),
            (Uncompressed, "-c:v rawvideo -pix_fmt yuv422p .avi"),
        ];
        for (codec, expected) in cases {
            assert_eq!(encode_line(false, codec, ""), expected, "{codec:?}");
            // The HUD stream is always ProRes 4444 with alpha in a MOV.
            assert_eq!(
                encode_line(true, codec, ""),
                "-c:v prores_ks -profile:v 4444 -pix_fmt yuva444p10le .mov",
                "{codec:?}"
            );
        }
        assert_eq!(
            encode_line(false, Custom, "  -c:v   ffv1 -level 3 "),
            "-c:v ffv1 -level 3 .mkv"
        );
    }

    #[test]
    fn custom_without_arguments_and_skip_have_no_encoder() {
        assert_eq!(
            encode_settings(false, &config(RenderCodec::Custom, "  ")),
            Err("Custom codec selected but no FFmpeg arguments were entered.".to_string())
        );
        assert_eq!(
            encode_settings(false, &config(RenderCodec::SourceCopy, "")),
            Err("Internal error: SourceCopy reached the FFmpeg codec path.".to_string())
        );
        // The HUD stream ignores the choice, so neither is refused for it.
        assert!(encode_settings(true, &config(RenderCodec::Custom, "")).is_ok());
    }

    #[test]
    fn mp4_gets_aac_and_every_other_container_pcm() {
        assert_eq!(audio_codec_args(".mp4"), ["-c:a", "aac", "-b:a", "192k"]);
        for ext in [".mov", ".avi", ".mkv"] {
            assert_eq!(audio_codec_args(ext), ["-c:a", "pcm_s16le"]);
        }
    }

    #[test]
    fn threads_are_shared_between_concurrent_renders() {
        assert_eq!(threads_per_process(Some(16), 2), 8);
        assert_eq!(threads_per_process(Some(16), 3), 5);
        assert_eq!(threads_per_process(Some(2), 4), 1);
        assert_eq!(threads_per_process(None, 4), 2);
    }

    /// Each take shape's inputs, as renders ran them before #1177 split
    /// `run_render_job` (captured from its command lines).
    #[test]
    fn each_take_shape_has_its_inputs() {
        let bmp_in = "-probesize 32 -analyzeduration 0 -thread_queue_size 512 -framerate 30 -i";
        let merge = "-filter_complex [1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge[hud] -map [hud] -map 2:a";
        let cases = [
            (
                clip("single", "all", Some("sound.wav"), None, None),
                format!("{bmp_in} all/%05d.bmp -thread_queue_size 512 -i sound.wav"),
            ),
            (
                clip(
                    "hud_only",
                    "hudcolor",
                    Some("sound.wav"),
                    None,
                    Some("hudalpha"),
                ),
                format!(
                    "{bmp_in} hudcolor/%05d.bmp {bmp_in} hudalpha/%05d.bmp -thread_queue_size 512 -i sound.wav {merge}"
                ),
            ),
            // An older autosave with no alpha folder recorded.
            (
                clip("hud_only", "hudcolor", Some("sound.wav"), None, None),
                format!(
                    "{bmp_in} hudcolor/%05d.bmp {bmp_in} hudalpha/%05d.bmp -thread_queue_size 512 -i sound.wav {merge}"
                ),
            ),
            (
                clip("single", "all", Some("alt.wav"), Some("video.avi"), None),
                "-i all/video.avi -thread_queue_size 512 -i alt.wav".to_string(),
            ),
            (
                clip(
                    "hud_only",
                    "hudcolor",
                    Some("sound.wav"),
                    Some("video.avi"),
                    Some("hudalpha"),
                ),
                format!(
                    "-i hudcolor/video.avi -i hudalpha/video.avi -thread_queue_size 512 -i sound.wav {merge}"
                ),
            ),
            (
                clip("single", "all", None, Some("video.mp4"), None),
                "-i all/video.mp4".to_string(),
            ),
        ];
        for (clip, expected) in cases {
            assert_eq!(input_args(&clip, "30").join(" "), expected, "{clip:?}");
        }
    }

    /// The whole command line for a BMP take to H.264, as captured from a
    /// render before #1177 split `run_render_job`.
    #[test]
    fn a_whole_command_line_runs_inputs_encoder_threads_audio_then_output() {
        let clip = clip("single", "all", Some("sound.wav"), None, None);
        let config = config(RenderCodec::H264Software, "");
        let encode = encode_settings(false, &config).unwrap();
        let out_file = PathBuf::from("export").join("bmp_h264.mp4");
        let args = FfmpegArgs {
            clip: &clip,
            fps: 30,
            encode: &encode,
            threads: 8,
            out_file: &out_file,
        }
        .to_args();
        let mut expected: Vec<String> = "-y -hide_banner -probesize 32 -analyzeduration 0 -thread_queue_size 512 -framerate 30 -i all/%05d.bmp -thread_queue_size 512 -i sound.wav -c:v libx264 -preset fast -crf 16 -pix_fmt yuv420p -threads 8 -c:a aac -b:a 192k -shortest -progress pipe:1 -loglevel error"
            .split(' ')
            .map(str::to_string)
            .collect();
        expected.push(out_file.to_string_lossy().into_owned());
        assert_eq!(args, expected);
    }
}
