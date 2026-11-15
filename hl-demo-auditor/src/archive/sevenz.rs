//! Demos inside `.7z` files (#929), through `sevenz-rust2` (pure Rust,
//! Apache-2.0), read and never changed.
//!
//! A 7z packs its files into blocks, and a solid archive puts many files in
//! one block: an entry there can only be reached by decompressing every
//! entry before it in its block. So each read walks the archive's blocks in
//! order, skips the blocks that hold nothing wanted, and in the others
//! decompresses up to the last wanted entry, once for all of them.

use super::{ZipDemoEntry, is_demo_name};
use crate::ContentHashes;
use sevenz_rust2::{Archive, BlockDecoder, Coder, EncoderMethod, Password};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// The largest dictionary a block may ask for. Decoding allocates it whole,
/// and a damaged header can claim 4 GiB; 1.5 GiB is the most 7-Zip offers,
/// and its Ultra preset uses 64 MiB.
const MAX_DICTIONARY: u64 = 1536 << 20;

/// `e` in plain words: `sevenz_rust2`'s `Display` prints its `Debug` form
/// (`Other("next header offset out of range")`).
fn error_text(e: &sevenz_rust2::Error) -> String {
    use sevenz_rust2::Error as E;
    match e {
        E::BadSignature(_) => "it is not a 7z file".into(),
        E::UnsupportedVersion { major, minor } => {
            format!("its 7z version ({major}.{minor}) isn't supported")
        }
        E::ChecksumVerificationFailed | E::NextHeaderCrcMismatch => {
            "it is damaged (a checksum doesn't match)".into()
        }
        E::Io(e, context) if context.is_empty() => io_error_text(e),
        E::Io(e, context) => format!("{context}: {}", io_error_text(e)),
        E::FileOpen(e, _) => format!("it couldn't be opened: {}", io_error_text(e)),
        E::Other(why) => format!("it is damaged ({why})"),
        E::BadTerminatedStreamsInfo(_)
        | E::BadTerminatedUnpackInfo
        | E::BadTerminatedPackInfo(_)
        | E::BadTerminatedSubStreamsInfo
        | E::BadTerminatedHeader(_) => "it is damaged (its header is malformed)".into(),
        E::ExternalUnsupported => "it uses a compression method that isn't supported".into(),
        E::UnsupportedCompressionMethod(method) => {
            format!("it uses a compression method that isn't supported ({method})")
        }
        E::MaxMemLimited { .. } => "it needs more memory than allowed".into(),
        E::PasswordRequired | E::MaybeBadPassword(_) => "it is encrypted (needs a password)".into(),
        E::Unsupported(what) => format!("it uses something not supported ({what})"),
        E::FileNotFound => "it is not in the archive".into(),
    }
}

/// `e` in plain words, through [`error_text`] when it wraps a 7z error, as
/// the block reader's checksum failure does.
fn io_error_text(e: &std::io::Error) -> String {
    match e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<sevenz_rust2::Error>())
    {
        Some(inner) => error_text(inner),
        None => e.to_string(),
    }
}

fn open(path: &Path) -> Result<(BufReader<File>, Archive), String> {
    let mut file = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let archive = Archive::read(&mut file, &Password::empty()).map_err(|e| error_text(&e))?;
    Ok((file, archive))
}

/// The dictionary an LZMA or LZMA2 coder asks for, from its properties.
fn dictionary_size(id: &[u8], props: &[u8]) -> Option<u64> {
    if id == EncoderMethod::ID_LZMA {
        props
            .get(1..5)
            .map(|b| u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])))
    } else if id == EncoderMethod::ID_LZMA2 {
        props.first().map(|&d| match d & 0x3f {
            d if d > 40 => u64::MAX,
            40 => u64::from(u32::MAX),
            d => (2 | u64::from(d & 1)) << (d / 2 + 11),
        })
    } else {
        None
    }
}

/// Why a block can't be read, before any of it is decompressed.
fn block_problem(coders: &[Coder]) -> Option<String> {
    for coder in coders {
        let id = coder.encoder_method_id();
        if id == EncoderMethod::ID_AES256_SHA256 {
            return Some("it is encrypted (needs a password)".to_string());
        }
        if dictionary_size(id, coder.properties()).is_some_and(|d| d > MAX_DICTIONARY) {
            return Some(format!(
                "it asks for a dictionary over {} MiB",
                MAX_DICTIONARY >> 20
            ));
        }
    }
    None
}

fn block_of(archive: &Archive, file: usize) -> Option<usize> {
    archive
        .stream_map
        .file_block_index
        .get(file)
        .copied()
        .flatten()
}

/// The `.dem` entries `path` lists, from its header alone.
pub(super) fn list(path: &Path) -> Result<Vec<ZipDemoEntry>, String> {
    let (_, archive) = open(path)?;
    let mut demos: Vec<ZipDemoEntry> = Vec::new();
    for (i, file) in archive.files.iter().enumerate() {
        if file.is_directory
            || file.is_anti_item
            || !is_demo_name(&file.name)
            || demos.iter().any(|d| d.name == file.name)
        {
            continue;
        }
        let error = block_of(&archive, i)
            .and_then(|b| archive.blocks.get(b))
            .and_then(|block| block_problem(&block.coders));
        demos.push(ZipDemoEntry {
            name: file.name.clone(),
            size: file.size,
            crc32: file.crc as u32,
            no_crc: !file.has_crc,
            hashes: None,
            error,
        });
    }
    Ok(demos)
}

/// Calls `each(i, reader, size)` for every `names[i]` found in `path`, in
/// archive order, with the entry decompressed as it is read; `each` returns
/// whether it read the entry to its end. The names not reached get the
/// reason, the archive's or their block's.
fn for_each_named(
    path: &Path,
    names: &[String],
    mut each: impl FnMut(usize, &mut dyn Read, u64) -> bool,
) -> Vec<Option<String>> {
    let mut missing: Vec<Option<String>> =
        vec![Some("it is not in the archive".into()); names.len()];
    let (mut source, archive) = match open(path) {
        Ok(opened) => opened,
        Err(e) => return vec![Some(e); names.len()],
    };
    let mut wanted: HashMap<&str, usize> = HashMap::new();
    for (i, name) in names.iter().enumerate() {
        wanted.entry(name.as_str()).or_insert(i);
    }
    // An entry with no data (an empty file) is in no block.
    for (i, file) in archive.files.iter().enumerate() {
        if block_of(&archive, i).is_none()
            && !file.is_directory
            && let Some(at) = wanted.remove(file.name.as_str())
        {
            if each(at, &mut std::io::empty(), 0) {
                missing[at] = None;
            } else {
                missing[at] = Some("it couldn't be read".into());
            }
        }
    }
    let password = Password::empty();
    for block in 0..archive.blocks.len() {
        if wanted.is_empty() {
            break;
        }
        let decoder = BlockDecoder::new(1, block, &archive, &password, &mut source);
        let mut left = decoder
            .entries()
            .iter()
            .filter(|e| wanted.contains_key(e.name.as_str()))
            .count();
        if left == 0 {
            continue;
        }
        if let Some(problem) = block_problem(&archive.blocks[block].coders) {
            for e in decoder.entries() {
                if let Some(at) = wanted.remove(e.name.as_str()) {
                    missing[at] = Some(problem.clone());
                }
            }
            continue;
        }
        let in_block: Vec<String> = decoder.entries().iter().map(|e| e.name.clone()).collect();
        let mut failed: Option<String> = None;
        let result = decoder.for_each_entries(&mut |entry, reader| {
            match wanted.remove(entry.name.as_str()) {
                Some(at) => {
                    left -= 1;
                    if each(at, reader, entry.size) {
                        missing[at] = None;
                    } else {
                        // The block's stream is out of step after a failed
                        // read: nothing after it can be trusted.
                        missing[at] = Some("it couldn't be read".into());
                        return Ok(false);
                    }
                }
                // Decompressed only to get past it.
                None => {
                    if let Err(e) = std::io::copy(reader, &mut std::io::sink()) {
                        failed = Some(io_error_text(&e));
                        return Ok(false);
                    }
                }
            }
            Ok(left > 0)
        });
        let reason = match result {
            Err(e) => Some(error_text(&e)),
            Ok(_) => failed,
        };
        // Wanted entries of this block the walk never reached.
        for name in &in_block {
            if let Some(at) = wanted.remove(name.as_str()) {
                missing[at] = Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| "it couldn't be read".into()),
                );
            }
        }
    }
    missing
}

/// Each of `names` in `path` read in full, in one walk of the archive.
pub(super) fn hash_entries(path: &Path, names: &[String]) -> Vec<Result<ContentHashes, String>> {
    let mut found: Vec<Option<Result<ContentHashes, String>>> = vec![None; names.len()];
    let missing = for_each_named(path, names, |at, reader, size| {
        let read = super::hashes_from_reader(reader, size).map_err(|e| io_error_text(&e));
        let ok = read.is_ok();
        found[at] = Some(read);
        ok
    });
    found
        .into_iter()
        .zip(missing)
        .map(|(read, missing)| match (read, missing) {
            (Some(read), _) => read,
            (None, Some(why)) => Err(why),
            (None, None) => Err("it couldn't be read".into()),
        })
        .collect()
}

/// `f` with entry `name` of `path` open for reading, as
/// [`super::with_archive_entry`].
pub(super) fn with_entry<T>(
    path: &Path,
    name: &str,
    f: impl FnOnce(&mut dyn Read, u64) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let mut f = Some(f);
    let mut out: Option<std::io::Result<T>> = None;
    let missing = for_each_named(path, &[name.to_string()], |_, reader, size| {
        let Some(f) = f.take() else { return false };
        let result = f(reader, size);
        let ok = result.is_ok();
        out = Some(result);
        ok
    });
    match (out, missing.into_iter().next().flatten()) {
        (Some(result), _) => result,
        (None, why) => Err(std::io::Error::other(
            why.unwrap_or_else(|| "it couldn't be read".into()),
        )),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, SourceReader};
    use std::path::PathBuf;

    /// Writes a 7z at `path`: each entry (name, bytes) in its own block, or
    /// all in one solid block.
    pub(crate) fn write_7z(path: &Path, entries: &[(&str, &[u8])], solid: bool) {
        let mut writer = ArchiveWriter::create(path).unwrap();
        writer.set_content_methods(vec![EncoderConfiguration::new(EncoderMethod::LZMA2)]);
        if solid {
            let files = entries
                .iter()
                .map(|(name, _)| ArchiveEntry::new_file(name))
                .collect();
            let readers = entries
                .iter()
                .map(|(_, bytes)| SourceReader::new(*bytes))
                .collect();
            writer.push_archive_entries(files, readers).unwrap();
        } else {
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(ArchiveEntry::new_file(name), Some(*bytes))
                    .unwrap();
            }
        }
        writer.finish().unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("auditor_7z_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn demo_bytes(seed: usize, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| ((i * 31 + seed * 7) % 251) as u8)
            .collect()
    }

    #[test]
    fn only_dem_entries_are_listed_with_sizes_and_crcs() {
        let dir = scratch("list");
        let path = dir.join("pack.7z");
        write_7z(
            &path,
            &[
                ("a.dem", b"a small demo"),
                ("sub/B.DEM", b"another demo, a little longer"),
                ("readme.txt", b"not a demo"),
                ("inner.zip", b"PK never opened"),
            ],
            true,
        );
        let demos = super::super::list_archive_demos(&path).unwrap();
        let names: Vec<_> = demos.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["a.dem", "sub/B.DEM"]);
        assert_eq!((demos[0].size, demos[1].size), (12, 29));
        assert!(demos.iter().all(|d| d.error.is_none() && !d.no_crc));
        assert_eq!(demos[0].crc32, crc32fast_like(b"a small demo"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CRC-32 (IEEE), as zip and 7z store it.
    fn crc32fast_like(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn entries_hash_like_the_same_bytes_on_disk_solid_or_not() {
        let dir = scratch("hashes");
        let big = demo_bytes(1, 3 * crate::KEY_BYTES as usize + 77);
        let small = demo_bytes(2, 500);
        let loose = dir.join("loose.dem");
        std::fs::write(&loose, &big).unwrap();
        let on_disk = crate::content_hashes(&loose).unwrap();
        for solid in [false, true] {
            let path = dir.join(format!("pack_{solid}.7z"));
            write_7z(
                &path,
                &[
                    ("first.dem", &small),
                    ("x/big.dem", &big),
                    ("last.dem", &small),
                ],
                solid,
            );
            let names = ["x/big.dem".to_string(), "gone.dem".to_string()];
            let found = super::super::hash_archive_entries(&path, &names);
            assert_eq!(found[0], Ok(on_disk), "solid {solid}");
            assert!(found[1].is_err());
            let same = super::super::with_archive_entry(&path, "x/big.dem", |r, size| {
                let mut bytes = Vec::new();
                r.read_to_end(&mut bytes)?;
                Ok((bytes == big, size))
            })
            .unwrap();
            assert_eq!(same, (true, big.len() as u64));
            assert!(super::super::with_archive_entry(&path, "gone.dem", |_, _| Ok(())).is_err());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_7z_is_an_error() {
        let dir = scratch("corrupt");
        let path = dir.join("broken.7z");
        std::fs::write(&path, b"7z\xbc\xaf\x27\x1c this is not really a 7z").unwrap();
        assert!(super::super::list_archive_demos(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Damaged copies of a real archive (cut short, or a byte changed) give
    /// errors, never a panic: release builds abort on one (#655).
    #[test]
    fn damaged_archives_fail_without_panicking() {
        let dir = scratch("damaged");
        let good = dir.join("good.7z");
        let demo = demo_bytes(3, 3000);
        write_7z(&good, &[("a.dem", &demo), ("b.dem", &demo[..1000])], true);
        let bytes = std::fs::read(&good).unwrap();
        let bad = dir.join("bad.7z");
        let names = ["a.dem".to_string(), "b.dem".to_string()];
        let mut damaged = 0;
        for cut in (0..bytes.len()).step_by(7) {
            std::fs::write(&bad, &bytes[..cut]).unwrap();
            assert!(super::super::list_archive_demos(&bad).is_err() || cut > 32);
            let _ = super::super::hash_archive_entries(&bad, &names);
        }
        for at in 0..bytes.len() {
            for flip in [0x01u8, 0x80, 0xff] {
                let mut changed = bytes.clone();
                changed[at] ^= flip;
                std::fs::write(&bad, &changed).unwrap();
                if super::super::list_archive_demos(&bad).is_err()
                    || super::super::hash_archive_entries(&bad, &names)
                        .iter()
                        .any(Result::is_err)
                {
                    damaged += 1;
                }
            }
        }
        assert!(damaged > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dictionary_sizes_come_from_the_coder_properties() {
        let lzma2 = |d: u8| dictionary_size(EncoderMethod::ID_LZMA2, &[d]);
        assert_eq!(lzma2(0), Some(4 << 10));
        assert_eq!(lzma2(18), Some(2 << 20));
        assert_eq!(lzma2(19), Some(3 << 20));
        assert_eq!(lzma2(40), Some(u64::from(u32::MAX)));
        assert_eq!(lzma2(41), Some(u64::MAX));
        let lzma = |size: u32| {
            let mut props = vec![0x5d];
            props.extend(size.to_le_bytes());
            dictionary_size(EncoderMethod::ID_LZMA, &props)
        };
        assert_eq!(lzma(64 << 20), Some(64 << 20));
        assert_eq!(dictionary_size(EncoderMethod::ID_LZMA, &[0x5d, 1]), None);
        assert_eq!(dictionary_size(EncoderMethod::ID_COPY, &[]), None);
        assert!(lzma(1536 << 20).is_some_and(|d| d <= MAX_DICTIONARY));
        assert!(lzma(u32::MAX).is_some_and(|d| d > MAX_DICTIONARY));
    }
}
