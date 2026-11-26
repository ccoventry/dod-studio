//! Reading the clip tag back out of a rendered MOV or MP4 (#1442).
//!
//! FFmpeg's `-metadata <key>=<value> -movflags use_metadata_tags` writes a
//! QuickTime `mdta` key: `moov/udta/meta` holds a `keys` atom naming each key
//! and an `ilst` atom whose children (numbered by key, from 1) each hold a
//! `data` atom with the value. Only that path is read: the walk seeks past
//! `mdat` (the video) and every `trak`, so a multi-GB clip costs a few small
//! reads. Checked against FFmpeg 7.1's output for `.mov` and `.mp4`. MKV and
//! AVI renders are found by their stream tag or fingerprint instead.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Atoms looked at in one container before the file is taken as not a MOV.
const MAX_ATOMS_PER_LEVEL: usize = 4096;

/// A `keys` or `ilst` atom bigger than this is not FFmpeg's few tags.
const MAX_TAG_ATOM_BYTES: u64 = 1024 * 1024;

/// Whether `file`'s name says it is a MOV or MP4.
pub fn is_mov_family(file: &Path) -> bool {
    file.extension().is_some_and(|ext| {
        ["mov", "mp4", "m4v"]
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// The text value of metadata key `key` in the MOV/MP4 `file`, or `None` when
/// it has none, is not a MOV/MP4 or cannot be read.
pub fn read_mov_tag(file: &Path, key: &str) -> Option<String> {
    if !is_mov_family(file) {
        return None;
    }
    let mut reader = BufReader::new(File::open(file).ok()?);
    read_tag(&mut reader, key).ok().flatten()
}

/// One atom: its four-letter type and where its body starts and ends. Also
/// read by the renderer's frame count of a joined clip (#1451).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Atom {
    pub(crate) kind: [u8; 4],
    pub(crate) body: u64,
    pub(crate) end: u64,
}

/// [`read_mov_tag`] over any reader: the walk `moov` → `udta` → `meta` (or
/// `moov` → `meta`), then the key's index in `keys` and its value in `ilst`.
fn read_tag<R: Read + Seek>(reader: &mut R, key: &str) -> io::Result<Option<String>> {
    let file_end = reader.seek(SeekFrom::End(0))?;
    let Some(moov) = find_child(reader, 0, file_end, b"moov")? else {
        return Ok(None);
    };
    let in_udta = match find_child(reader, moov.body, moov.end, b"udta")? {
        Some(udta) => find_child(reader, udta.body, udta.end, b"meta")?,
        None => None,
    };
    let meta = match in_udta {
        Some(meta) => meta,
        None => match find_child(reader, moov.body, moov.end, b"meta")? {
            Some(meta) => meta,
            None => return Ok(None),
        },
    };
    let body = meta_children_start(reader, meta)?;
    let (Some(keys), Some(ilst)) = (
        find_child(reader, body, meta.end, b"keys")?,
        find_child(reader, body, meta.end, b"ilst")?,
    ) else {
        return Ok(None);
    };
    let Some(index) = key_index(&read_body(reader, keys)?, key) else {
        return Ok(None);
    };
    Ok(value_of(&read_body(reader, ilst)?, index))
}

/// Where a `meta` atom's children start: QuickTime's `meta` has none of the
/// ISO version/flags word that MP4 readers expect, and FFmpeg writes either
/// depending on the brand, so the first child (`hdlr`) says which.
fn meta_children_start<R: Read + Seek>(reader: &mut R, meta: Atom) -> io::Result<u64> {
    reader.seek(SeekFrom::Start(meta.body))?;
    let mut peek = [0u8; 8];
    if meta.end - meta.body >= 8 && reader.read_exact(&mut peek).is_ok() && &peek[4..8] == b"hdlr" {
        Ok(meta.body)
    } else {
        Ok(meta.body + 4)
    }
}

/// The first child of type `kind` between `start` and `end`.
pub(crate) fn find_child<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    end: u64,
    kind: &[u8; 4],
) -> io::Result<Option<Atom>> {
    let mut pos = start;
    for _ in 0..MAX_ATOMS_PER_LEVEL {
        let Some(atom) = atom_at(reader, pos, end)? else {
            return Ok(None);
        };
        if &atom.kind == kind {
            return Ok(Some(atom));
        }
        pos = atom.end;
    }
    Ok(None)
}

/// The atom whose header starts at `pos`, or `None` when no whole atom fits
/// before `end` (the container's last child has been passed, or the header
/// is garbage).
pub(crate) fn atom_at<R: Read + Seek>(
    reader: &mut R,
    pos: u64,
    end: u64,
) -> io::Result<Option<Atom>> {
    if pos.saturating_add(8) > end {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(pos))?;
    let mut header = [0u8; 8];
    reader.read_exact(&mut header)?;
    let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
    let kind = [header[4], header[5], header[6], header[7]];
    let (size, header_len) = match size32 {
        // To the end of the container (only the last top-level atom does this).
        0 => (end - pos, 8),
        1 => {
            let mut large = [0u8; 8];
            reader.read_exact(&mut large)?;
            (u64::from_be_bytes(large), 16)
        }
        n => (n, 8),
    };
    if size < header_len || pos.saturating_add(size) > end {
        return Ok(None);
    }
    Ok(Some(Atom {
        kind,
        body: pos + header_len,
        end: pos + size,
    }))
}

/// An atom's body, when it is small enough to be FFmpeg's tag list.
fn read_body<R: Read + Seek>(reader: &mut R, atom: Atom) -> io::Result<Vec<u8>> {
    let len = atom.end - atom.body;
    if len > MAX_TAG_ATOM_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "tag atom too large",
        ));
    }
    reader.seek(SeekFrom::Start(atom.body))?;
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body)?;
    Ok(body)
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// `key`'s number (from 1) in a `keys` body: version/flags, a count, then
/// `size, namespace, name` per key.
fn key_index(keys: &[u8], key: &str) -> Option<u32> {
    let count = be_u32(keys, 4)?;
    let mut pos = 8usize;
    for index in 1..=count {
        let size = be_u32(keys, pos)? as usize;
        if size < 8 {
            return None;
        }
        let name = keys.get(pos + 8..pos.checked_add(size)?)?;
        if name == key.as_bytes() {
            return Some(index);
        }
        pos += size;
    }
    None
}

/// The text of the `ilst` entry numbered `index`: each entry is an atom whose
/// type is its key's number, holding a `data` atom (type and locale words,
/// then the value).
fn value_of(ilst: &[u8], index: u32) -> Option<String> {
    let mut pos = 0usize;
    while pos + 8 <= ilst.len() {
        let size = be_u32(ilst, pos)? as usize;
        if size < 8 || pos.checked_add(size)? > ilst.len() {
            return None;
        }
        if be_u32(ilst, pos + 4)? == index {
            let entry = &ilst[pos + 8..pos + size];
            let data_size = be_u32(entry, 0)? as usize;
            if entry.get(4..8)? != b"data" || data_size < 16 {
                return None;
            }
            let value = entry.get(16..data_size)?;
            return Some(String::from_utf8_lossy(value).into_owned());
        }
        pos += size;
    }
    None
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

    /// A file laid out as FFmpeg writes one: ftyp, a big mdat, then moov with
    /// a track and `udta/meta` holding `keys` and `ilst`.
    fn mov_with_tags(tags: &[(&str, &str)], iso_meta: bool) -> Vec<u8> {
        let mut keys = vec![0, 0, 0, 0];
        keys.extend_from_slice(&(tags.len() as u32).to_be_bytes());
        let mut ilst = Vec::new();
        for (i, (key, value)) in tags.iter().enumerate() {
            keys.extend_from_slice(&((key.len() + 8) as u32).to_be_bytes());
            keys.extend_from_slice(b"mdta");
            keys.extend_from_slice(key.as_bytes());
            let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
            data.extend_from_slice(value.as_bytes());
            ilst.extend(atom(&(i as u32 + 1).to_be_bytes(), &atom(b"data", &data)));
        }
        let mut meta = if iso_meta {
            vec![0, 0, 0, 0]
        } else {
            Vec::new()
        };
        meta.extend(atom(b"hdlr", &[0u8; 25]));
        meta.extend(atom(b"keys", &keys));
        meta.extend(atom(b"ilst", &ilst));
        let mut moov = atom(b"mvhd", &[0u8; 100]);
        moov.extend(atom(b"trak", &[0u8; 300]));
        moov.extend(atom(b"udta", &atom(b"meta", &meta)));
        let mut file = atom(b"ftyp", b"qt  \0\0\x02\0qt  ");
        file.extend(atom(b"mdat", &vec![0xAB; 50_000]));
        file.extend(atom(b"moov", &moov));
        file
    }

    fn tag_of(file: Vec<u8>, key: &str) -> Option<String> {
        read_tag(&mut Cursor::new(file), key).unwrap()
    }

    #[test]
    fn the_tag_is_read_from_moov_udta_meta() {
        let file = mov_with_tags(
            &[("dodstudio_clip", "v1/s/t/1/all"), ("encoder", "Lavf")],
            false,
        );
        assert_eq!(
            tag_of(file, "dodstudio_clip").as_deref(),
            Some("v1/s/t/1/all")
        );
    }

    #[test]
    fn an_iso_meta_with_version_flags_reads_the_same() {
        let file = mov_with_tags(
            &[("encoder", "Lavf"), ("dodstudio_clip", "v1/s/t/2/all")],
            true,
        );
        assert_eq!(
            tag_of(file, "dodstudio_clip").as_deref(),
            Some("v1/s/t/2/all")
        );
    }

    #[test]
    fn a_file_without_the_key_or_without_moov_has_no_tag() {
        assert_eq!(
            tag_of(
                mov_with_tags(&[("encoder", "Lavf")], false),
                "dodstudio_clip"
            ),
            None
        );
        assert_eq!(tag_of(atom(b"mdat", &[0u8; 64]), "dodstudio_clip"), None);
        assert_eq!(tag_of(Vec::new(), "dodstudio_clip"), None);
    }

    #[test]
    fn a_truncated_file_reads_as_no_tag_without_panicking() {
        let file = mov_with_tags(&[("dodstudio_clip", "v1/s/t/1/all")], false);
        for cut in [10, 100, 50_100, file.len() - 30, file.len() - 1] {
            let short = file[..cut].to_vec();
            assert_eq!(
                read_tag(&mut Cursor::new(short), "dodstudio_clip")
                    .ok()
                    .flatten(),
                None
            );
        }
    }

    #[test]
    fn only_mov_family_names_are_read() {
        assert!(is_mov_family(Path::new("a/clip.MOV")));
        assert!(is_mov_family(Path::new("clip.mp4")));
        assert!(!is_mov_family(Path::new("clip.mkv")));
        assert!(!is_mov_family(Path::new("clip")));
    }
}
