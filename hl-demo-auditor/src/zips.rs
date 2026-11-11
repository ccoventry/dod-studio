//! The archives in a pass (#929): each one's `.dem` entries, remembered
//! while the archive is unchanged, and the entries read in full.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use crate::lanes::{Readers, drive_of};
use crate::{
    AuditProgress, ContentHashes, KeyCache, Reads, Stamp, ZipDemoEntry, ZipRef, archive, shown_path,
};

/// A `.dem` entry of one of the pass's archives.
pub(crate) struct ZipDemo {
    pub(crate) zip: usize,
    pub(crate) entry: ZipDemoEntry,
}

impl ZipDemo {
    pub(crate) fn at(&self, zips: &[(PathBuf, Option<Stamp>)]) -> ZipRef {
        ZipRef {
            zip: zips[self.zip].0.clone(),
            entry: self.entry.name.clone(),
        }
    }
}

/// Each archive's `.dem` entries (#929): remembered while the archive's
/// size and modified time hold, else read from its listing. An archive that
/// can't be read lists none and is named in `skipped`.
pub(crate) fn list_zips(
    zips: &[(PathBuf, Option<Stamp>)],
    readers: &Readers,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    cache: &mut KeyCache,
    reads: &mut Reads,
    skipped: &mut Vec<String>,
) -> Vec<ZipDemo> {
    let reading = |path: &Path| {
        if let Some(tx) = progress {
            let _ = tx.send(AuditProgress::ReadingArchive(
                path.to_string_lossy().into_owned(),
            ));
        }
    };
    let mut listings: Vec<Option<Result<Vec<ZipDemoEntry>, String>>> = vec![None; zips.len()];
    let mut to_read = Vec::new();
    for (i, (path, stamp)) in zips.iter().enumerate() {
        match stamp.and_then(|s| cache.archive(path, s)) {
            Some(entry) => {
                listings[i] = Some(match &entry.error {
                    Some(e) => Err(e.clone()),
                    None => Ok(entry.demos.clone()),
                });
                reading(path);
            }
            None => to_read.push(i),
        }
    }
    let read = readers.map(
        &to_read,
        |&i| drive_of(&zips[i].0),
        cancel,
        |&i| {
            let path = &zips[i].0;
            let listing = archive::list_archive_demos(path);
            reading(path);
            listing
        },
    );
    for (&i, listing) in to_read.iter().zip(read) {
        let Some(listing) = listing else { continue };
        reads.zips += 1;
        let (path, stamp) = &zips[i];
        if let Some(stamp) = stamp {
            cache.put_archive(path, *stamp, listing.clone());
        }
        listings[i] = Some(listing);
    }
    let mut demos = Vec::new();
    for (i, listing) in listings.into_iter().enumerate() {
        let path = &zips[i].0;
        match listing {
            None => {}
            Some(Err(e)) => skipped.push(format!("{}: {e}", shown_path(path))),
            Some(Ok(entries)) => {
                for entry in entries {
                    match &entry.error {
                        Some(e) => {
                            let at = ZipRef {
                                zip: path.clone(),
                                entry: entry.name.clone(),
                            };
                            skipped.push(format!("{}: {e}", at.shown()));
                        }
                        None => demos.push(ZipDemo { zip: i, entry }),
                    }
                }
            }
        }
    }
    demos
}

/// Reads each of `picks` (indices into `demos`) in full, one archive per
/// thread at a time and each archive opened once; sends one `Hashing` per
/// demo.
pub(crate) fn read_zip_demos(
    picks: &[usize],
    demos: &[ZipDemo],
    zips: &[(PathBuf, Option<Stamp>)],
    readers: &Readers,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> Vec<(usize, Result<ContentHashes, String>)> {
    let mut by_zip: HashMap<usize, Vec<usize>> = HashMap::new();
    for &j in picks {
        by_zip.entry(demos[j].zip).or_default().push(j);
    }
    let mut jobs: Vec<(usize, Vec<usize>)> = by_zip.into_iter().collect();
    jobs.sort();
    let drive = |(zip, _): &(usize, Vec<usize>)| drive_of(&zips[*zip].0);
    let done = readers.map(&jobs, drive, cancel, |(zip, members)| {
        let names: Vec<String> = members
            .iter()
            .map(|&j| demos[j].entry.name.clone())
            .collect();
        let found = archive::hash_archive_entries(&zips[*zip].0, &names);
        if let Some(tx) = progress {
            for &j in members {
                let _ = tx.send(AuditProgress::Hashing(demos[j].at(zips).display()));
            }
        }
        members.iter().copied().zip(found).collect::<Vec<_>>()
    });
    done.into_iter().flatten().flatten().collect()
}
