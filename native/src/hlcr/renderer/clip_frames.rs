//! How many video frames a rendered file holds, read from its container's
//! own index rather than from FFmpeg's `-progress` output (#1451): FFmpeg
//! 6.1 and 7.0 print `frame=` only for filtered streams, so a stream-copy
//! join reports none there. A segmented render checks each segment and the
//! joined clip by it.
//!
//! A MOV's count is its video track's sample count (`moov/trak/mdia/minf/
//! stbl/stsz`, the track whose `hdlr` says `vide`); an AVI's is its video
//! stream header's length (`scanner::avi_video_frames`). Only headers are
//! read: the walk seeks past `mdat`.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::hlcr::clip_id::mov_tags::{Atom, atom_at, find_child};

/// Tracks looked at in one `moov` before giving up.
const MAX_TRACKS: usize = 64;

/// The video frames in `file`, written as `file_ext`'s container (`.avi`,
/// else a MOV), or `None` when its header can't be read.
pub(super) fn count_frames(file: &Path, file_ext: &str) -> Option<usize> {
    if file_ext.eq_ignore_ascii_case(".avi") {
        return crate::hlcr::scanner::avi_video_frames(file);
    }
    let mut reader = BufReader::new(File::open(file).ok()?);
    mov_video_frames(&mut reader).ok().flatten()
}

/// The sample count of the first video track in a MOV.
fn mov_video_frames<R: Read + Seek>(reader: &mut R) -> io::Result<Option<usize>> {
    let file_end = reader.seek(SeekFrom::End(0))?;
    let Some(moov) = find_child(reader, 0, file_end, b"moov")? else {
        return Ok(None);
    };
    let mut pos = moov.body;
    for _ in 0..MAX_TRACKS {
        let Some(atom) = atom_at(reader, pos, moov.end)? else {
            return Ok(None);
        };
        pos = atom.end;
        if &atom.kind != b"trak" {
            continue;
        }
        if let Some(frames) = video_track_frames(reader, atom)? {
            return Ok(Some(frames));
        }
    }
    Ok(None)
}

/// A `trak`'s sample count when it is a video track.
fn video_track_frames<R: Read + Seek>(reader: &mut R, trak: Atom) -> io::Result<Option<usize>> {
    let Some(mdia) = find_child(reader, trak.body, trak.end, b"mdia")? else {
        return Ok(None);
    };
    let Some(hdlr) = find_child(reader, mdia.body, mdia.end, b"hdlr")? else {
        return Ok(None);
    };
    // Version and flags, then pre_defined, then the handler type.
    if read_u32_at(reader, hdlr, 8)?.map(u32::to_be_bytes) != Some(*b"vide") {
        return Ok(None);
    }
    let mut parent = mdia;
    for kind in [b"minf", b"stbl", b"stsz"] {
        match find_child(reader, parent.body, parent.end, kind)? {
            Some(child) => parent = child,
            None => return Ok(None),
        }
    }
    // Version and flags, then sample_size, then sample_count.
    Ok(read_u32_at(reader, parent, 8)?.map(|count| count as usize))
}

/// The big-endian `u32` at `offset` into `atom`'s body, if it has one there.
fn read_u32_at<R: Read + Seek>(reader: &mut R, atom: Atom, offset: u64) -> io::Result<Option<u32>> {
    if atom.body + offset + 4 > atom.end {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(atom.body + offset))?;
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(Some(u32::from_be_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// A track of handler `handler` with `samples` samples.
    fn track(handler: &[u8; 4], samples: u32) -> Vec<u8> {
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(handler);
        hdlr.extend_from_slice(&[0u8; 12]);
        let mut stsz = vec![0u8; 8];
        stsz.extend_from_slice(&samples.to_be_bytes());
        let stbl = atom(b"stbl", &atom(b"stsz", &stsz));
        let minf = atom(b"minf", &stbl);
        let mut mdia = atom(b"mdhd", &[0u8; 24]);
        mdia.extend(atom(b"hdlr", &hdlr));
        mdia.extend(minf);
        atom(b"trak", &atom(b"mdia", &mdia))
    }

    /// Laid out as FFmpeg writes a MOV: ftyp, mdat, then moov, its sound
    /// track first here so the walk has to pass it.
    fn mov(tracks: &[Vec<u8>]) -> Vec<u8> {
        let mut file = atom(b"ftyp", b"qt  ");
        file.extend(atom(b"mdat", &[7u8; 64]));
        let mut moov = atom(b"mvhd", &[0u8; 100]);
        for t in tracks {
            moov.extend_from_slice(t);
        }
        file.extend(atom(b"moov", &moov));
        file
    }

    fn frames_of(file: Vec<u8>) -> Option<usize> {
        mov_video_frames(&mut Cursor::new(file)).ok().flatten()
    }

    #[test]
    fn a_movs_frames_are_its_video_tracks_sample_count() {
        let file = mov(&[track(b"soun", 22050), track(b"vide", 1000)]);
        assert_eq!(frames_of(file), Some(1000));
    }

    #[test]
    fn a_mov_without_a_video_track_or_moov_has_no_count() {
        assert_eq!(frames_of(mov(&[track(b"soun", 5)])), None);
        assert_eq!(frames_of(atom(b"mdat", &[0u8; 32])), None);
        // A zero-filled file, like a damaged segment.
        assert_eq!(frames_of(vec![0u8; 4096]), None);
    }

    #[test]
    fn a_truncated_mov_reads_as_no_count_without_panicking() {
        let file = mov(&[track(b"vide", 1000)]);
        for cut in [file.len() - 1, file.len() / 2, 9] {
            assert_eq!(frames_of(file[..cut].to_vec()), None, "cut at {cut}");
        }
    }
}
