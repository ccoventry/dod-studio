//! Demos inside `.zip` files (#929), read and never changed: nothing is
//! extracted to disk, and an archive is never written or deleted. A demo in
//! a zip counts as a copy, so the Duplicates audit groups it with loose
//! copies of the same demo, and keeps it, since one entry can't be removed
//! without rewriting the whole archive.
//!
//! Only `.dem` entries count. A zip inside a zip is not opened. 7z and rar
//! are not read.

use crate::{ContentHashes, Fnv1a, KEY_BYTES};
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

/// Between an archive's path and an entry's name wherever one is shown,
/// e.g. `C:\demos\pack.zip › match/a.dem`.
pub const SEPARATOR: &str = " \u{203a} ";

/// One demo inside a zip.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ZipRef {
    /// The archive, canonical as the walk names loose demos.
    pub zip: PathBuf,
    /// The entry's name as the archive stores it (`/` between folders).
    pub entry: String,
}

impl ZipRef {
    /// `<archive path> › <entry name>`.
    pub fn display(&self) -> String {
        format!("{}{SEPARATOR}{}", self.zip.display(), self.entry)
    }
}

/// A zip's `.dem` entry as its central directory lists it, plus what the
/// audit learned reading it ([`crate::KeyCache`] keeps both).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ZipDemoEntry {
    pub name: String,
    pub size: u64,
    pub crc32: u32,
    /// Once the entry was read in full: its key, tail and whole-file hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hashes: Option<ContentHashes>,
    /// Why it can't be read (encrypted, corrupt): it is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn is_demo_name(name: &str) -> bool {
    name.len() > 4
        && name.is_char_boundary(name.len() - 4)
        && name[name.len() - 4..].eq_ignore_ascii_case(".dem")
}

fn open(zip: &Path) -> Result<zip::ZipArchive<BufReader<fs::File>>, String> {
    let file = fs::File::open(zip).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(BufReader::new(file)).map_err(|e| e.to_string())
}

/// The `.dem` entries `zip` lists, from its central directory and each
/// entry's local header; nothing is decompressed. An error when it isn't a
/// zip that can be read. An entry named twice is listed once; an encrypted
/// one carries an error.
pub fn list_zip_demos(zip: &Path) -> Result<Vec<ZipDemoEntry>, String> {
    let mut archive = open(zip)?;
    let mut demos: Vec<ZipDemoEntry> = Vec::new();
    for i in 0..archive.len() {
        let Some(name) = archive.name_for_index(i).map(str::to_owned) else {
            continue;
        };
        if !is_demo_name(&name) || demos.iter().any(|d| d.name == name) {
            continue;
        }
        let entry = match archive.by_index_raw(i) {
            Ok(file) if file.is_dir() => continue,
            Ok(file) => ZipDemoEntry {
                size: file.size(),
                crc32: file.crc32(),
                hashes: None,
                error: file
                    .encrypted()
                    .then(|| "it is encrypted (needs a password)".to_string()),
                name,
            },
            Err(e) => ZipDemoEntry {
                size: 0,
                crc32: 0,
                hashes: None,
                error: Some(e.to_string()),
                name,
            },
        };
        demos.push(entry);
    }
    Ok(demos)
}

/// `f` with entry `name` of `zip` opened for reading, decompressed as it is
/// read, and the size the archive gives it. Reading it to its end also checks
/// its CRC-32: a corrupt entry fails there.
pub fn with_zip_entry<T>(
    zip: &Path,
    name: &str,
    f: impl FnOnce(&mut dyn Read, u64) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let mut archive = open(zip).map_err(std::io::Error::other)?;
    let mut file = archive.by_name(name).map_err(std::io::Error::other)?;
    let size = file.size();
    f(&mut file, size)
}

/// [`ContentHashes`] of everything `reader` gives, which must be `size`
/// bytes: the same hashes [`crate::content_hashes`] finds in a file.
pub fn hashes_from_reader(reader: &mut dyn Read, size: u64) -> std::io::Result<ContentHashes> {
    let head_end = size.min(KEY_BYTES);
    let tail_start = if size > KEY_BYTES {
        size - KEY_BYTES
    } else {
        u64::MAX
    };
    let (mut head, mut tail, mut full) = (Fnv1a::new(), Fnv1a::new(), Fnv1a::new());
    let mut buf = vec![0u8; 1 << 20];
    let mut offset = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        let end = offset + n as u64;
        if offset < head_end {
            head.update(&chunk[..(head_end.min(end) - offset) as usize]);
        }
        if end > tail_start {
            tail.update(&chunk[(tail_start.saturating_sub(offset)) as usize..]);
        }
        full.update(chunk);
        offset = end;
    }
    if offset != size {
        return Err(std::io::Error::other(format!(
            "it holds {offset} bytes, not the {size} it should"
        )));
    }
    Ok(ContentHashes {
        size,
        head: head.finish(),
        tail: if size > KEY_BYTES { tail.finish() } else { 0 },
        full: full.finish(),
    })
}

/// Each of `names` in `zip` read in full, opening the archive once.
pub fn hash_zip_entries(zip: &Path, names: &[String]) -> Vec<Result<ContentHashes, String>> {
    let mut archive = match open(zip) {
        Ok(archive) => archive,
        Err(e) => return names.iter().map(|_| Err(e.clone())).collect(),
    };
    names
        .iter()
        .map(|name| {
            let mut file = archive.by_name(name).map_err(|e| e.to_string())?;
            let size = file.size();
            hashes_from_reader(&mut file, size).map_err(|e| e.to_string())
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a zip at `path` with `entries` (name, bytes, deflated?).
    pub(crate) fn write_zip(path: &Path, entries: &[(&str, &[u8], bool)]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes, deflate) in entries {
            let method = if *deflate {
                zip::CompressionMethod::Deflated
            } else {
                zip::CompressionMethod::Stored
            };
            let options = zip::write::SimpleFileOptions::default().compression_method(method);
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("auditor_zip_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_dem_entries_are_listed_with_their_sizes_and_crcs() {
        let dir = scratch("list");
        let zip = dir.join("pack.zip");
        write_zip(
            &zip,
            &[
                ("a.dem", b"stored demo", false),
                ("sub/B.DEM", b"deflated demo, deflated demo", true),
                ("readme.txt", b"not a demo", true),
                ("inner.zip", b"PK not opened", false),
            ],
        );
        let demos = list_zip_demos(&zip).unwrap();
        let names: Vec<_> = demos.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["a.dem", "sub/B.DEM"]);
        assert_eq!(demos[0].size, 11);
        assert_eq!(demos[1].size, 28);
        assert!(demos.iter().all(|d| d.error.is_none() && d.crc32 != 0));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entrys_hashes_match_the_same_bytes_on_disk() {
        let dir = scratch("hashes");
        let bytes: Vec<u8> = (0..(3 * KEY_BYTES as usize + 77))
            .map(|i| (i * 13 % 251) as u8)
            .collect();
        let loose = dir.join("loose.dem");
        fs::write(&loose, &bytes).unwrap();
        let zip = dir.join("pack.zip");
        write_zip(&zip, &[("s.dem", &bytes, false), ("d.dem", &bytes, true)]);
        let on_disk = crate::content_hashes(&loose).unwrap();
        let found = hash_zip_entries(&zip, &["s.dem".into(), "d.dem".into(), "gone.dem".into()]);
        assert_eq!(found[0], Ok(on_disk));
        assert_eq!(found[1], Ok(on_disk));
        assert!(found[2].is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_an_error() {
        let dir = scratch("corrupt");
        let zip = dir.join("broken.zip");
        fs::write(&zip, b"PK\x03\x04 this is not really a zip").unwrap();
        assert!(list_zip_demos(&zip).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_demo_name_needs_the_extension() {
        assert!(is_demo_name("a.dem"));
        assert!(is_demo_name("x/Y.DeM"));
        assert!(!is_demo_name(".dem"));
        assert!(!is_demo_name("a.dem.zip"));
        assert!(!is_demo_name("démo"));
    }
}
