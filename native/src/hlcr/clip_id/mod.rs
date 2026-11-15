//! A DoD Studio ID on every rendered clip, so a project finds its videos
//! again after the user moves or renames them (#1442).
//!
//! One render gets one [`ClipTag`], built from what the project already
//! records for it (the take key, when the render started and the stream), so
//! the project can work out every tag it owns without storing a new field. The
//! tag goes on the video three ways, read back in this order:
//!
//! 1. inside the file ([`mov_tags`]): an FFmpeg metadata key written at render
//!    time, which survives renames, moves and copies to any drive;
//! 2. an NTFS alternate data stream ([`stream_tag`]) on the file, written on
//!    every new render and by a scan onto clips the project already knows,
//!    without touching the video;
//! 3. the file's size plus a hash of its first and last MB
//!    ([`fingerprint`]), for a clip that lost both.
//!
//! [`relink`] runs the scan that uses them.

pub mod fingerprint;
pub mod mov_tags;
pub mod relink;
pub mod stream_tag;

use std::fmt;

/// The metadata key a render writes its tag under (`-metadata
/// dodstudio_clip=...` with `-movflags use_metadata_tags`, so MOV and MP4 keep
/// a custom key; MKV keeps it as `DODSTUDIO_CLIP`).
pub const METADATA_KEY: &str = "dodstudio_clip";

/// The format version every tag starts with, so a later format can be told
/// apart from this one.
const VERSION_PREFIX: &str = "v1/";

/// Which render a video came from: `v1/<take key>/<start ms>/<stream>`.
///
/// The take key is `session/take` (`shared::paths::take_key`), so it holds one
/// `/` itself; the start time and the stream are the last two segments. A
/// stream is a folder name (`all`, or `hud:<folder>` for a HUD pass) and never
/// holds a `/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipTag {
    take_key: String,
    started_unix_ms: u64,
    stream: String,
}

impl ClipTag {
    /// The tag of the render of `take_key`'s `stream` that started at
    /// `started_unix_ms` (the render history's own key for an attempt, #438).
    pub fn new(take_key: &str, started_unix_ms: u64, stream: &str) -> Self {
        Self {
            take_key: take_key.to_string(),
            started_unix_ms,
            stream: stream.to_string(),
        }
    }

    /// Reads a tag back from a file, or `None` when `text` is not one this
    /// build writes (another program's comment, a later format, garbage).
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.trim().strip_prefix(VERSION_PREFIX)?;
        let (rest, stream) = rest.rsplit_once('/')?;
        let (take_key, started) = rest.rsplit_once('/')?;
        if take_key.is_empty() || stream.is_empty() {
            return None;
        }
        Some(Self::new(take_key, started.parse().ok()?, stream))
    }
}

impl fmt::Display for ClipTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{VERSION_PREFIX}{}/{}/{}",
            self.take_key, self.started_unix_ms, self.stream
        )
    }
}

/// What tagging a finished render did.
#[derive(Debug)]
pub struct RenderStamp {
    /// The video's fingerprint, for the project to keep.
    pub content_hash: Option<String>,
    /// Why the stream tag could not be written (a FAT/exFAT export drive),
    /// when it could not.
    pub stream_error: Option<std::io::Error>,
}

/// Writes `tag` as the stream tag of the render just finished at `file` and
/// takes its fingerprint. Reads 2 MB of the video at most; writes only the
/// stream.
pub fn stamp_new_render(file: &std::path::Path, tag: &ClipTag) -> RenderStamp {
    RenderStamp {
        stream_error: stream_tag::write_stream_tag(file, &tag.to_string()).err(),
        content_hash: fingerprint::content_hash(file).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_render_gets_its_stream_tag_and_fingerprint() {
        let dir = crate::test_support::Scratch::new("clip_id_stamp");
        let video = dir.path().join("clip.mov");
        std::fs::write(&video, b"rendered").unwrap();
        let tag = ClipTag::new("s/t", 9, "all");
        let stamp = stamp_new_render(&video, &tag);
        assert!(stamp.stream_error.is_none());
        assert_eq!(stamp.content_hash, fingerprint::content_hash(&video).ok());
        assert_eq!(stream_tag::read_stream_tag(&video), Some(tag.to_string()));
    }

    #[test]
    fn a_tag_reads_back_as_the_render_it_names() {
        let tag = ClipTag::new("session_1/take_0003", 1_731_700_000_000, "hud:all");
        let text = tag.to_string();
        assert_eq!(text, "v1/session_1/take_0003/1731700000000/hud:all");
        assert_eq!(ClipTag::parse(&text), Some(tag));
    }

    #[test]
    fn text_that_is_not_a_tag_reads_as_none() {
        for text in [
            "",
            "Made with VEGAS",
            "v2/session/take/1/all",
            "v1/session/take/not-a-number/all",
            "v1/1/all",
            "v1/session/take/1/",
        ] {
            assert_eq!(ClipTag::parse(text), None, "{text}");
        }
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert_eq!(
            ClipTag::parse(" v1/s/t/5/all\r\n"),
            Some(ClipTag::new("s/t", 5, "all"))
        );
    }
}
