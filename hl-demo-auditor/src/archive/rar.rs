//! Demos inside `.rar` files (#929), through RARLAB's UnRAR library
//! (`unrar_sys`, which builds it from source), read and never changed: every
//! entry is opened in UnRAR's test mode, which decompresses and checks it
//! without writing anything, and its bytes reach the audit through a
//! callback.
//!
//! UnRAR source code may be used in any software to handle RAR archives
//! without limitations free of charge, but cannot be used to develop RAR
//! (WinRAR) compatible archiver and to re-create RAR compression algorithm,
//! which is proprietary. Distribution of modified UnRAR source code in
//! separate form or as a part of other software is permitted, provided that
//! full text of this paragraph, starting from "UnRAR source code" words, is
//! included in license, or in documentation if license is not available, and
//! in source code comments of resulting package.
//!
//! (That paragraph is UnRAR's licence, quoted whole as it asks: `unrar_sys`
//! carries its own patches to the source. docs/third_party_notices.md quotes
//! it too.)
//!
//! This wraps UnRAR's C API itself; `unrar_sys` only builds and links the
//! library. Its Rust structs leave out the `#pragma pack(1)` of UnRAR's
//! `dll.hpp`, so every field after the first pointer is read from the wrong
//! place (`HashType`, and in the open call `CmtBufW`, which UnRAR writes
//! through); the `unrar` crate above it also unwraps return codes and has no
//! case for `ERAR_LARGE_DICT`, so one archive asking for a large dictionary
//! would abort Studio (release builds abort on a panic). Here the structs
//! are packed as `dll.hpp` declares them and every code is an error string.
//! A password, the next volume of a split archive and a dictionary over
//! UnRAR's limit are all refused through the callback, so nothing waits on
//! a prompt.

use super::{ContentHasher, ZipDemoEntry, is_demo_name};
use crate::ContentHashes;
use std::collections::HashMap;
use std::ffi::c_void;
use std::io::Read;
use std::os::raw::{c_char, c_int, c_uint};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};

// Loads `unrar_sys`, whose build script compiles UnRAR into the static
// library the declarations below link against.
extern crate unrar_sys;

// From UnRAR's dll.hpp.
const ERAR_SUCCESS: c_int = 0;
const ERAR_END_ARCHIVE: c_int = 10;
const ERAR_NO_MEMORY: c_int = 11;
const ERAR_BAD_DATA: c_int = 12;
const ERAR_BAD_ARCHIVE: c_int = 13;
const ERAR_UNKNOWN_FORMAT: c_int = 14;
const ERAR_EOPEN: c_int = 15;
const ERAR_EREAD: c_int = 18;
const ERAR_MISSING_PASSWORD: c_int = 22;
const ERAR_BAD_PASSWORD: c_int = 24;
const ERAR_LARGE_DICT: c_int = 25;
const RAR_OM_LIST: c_uint = 0;
const RAR_OM_EXTRACT: c_uint = 1;
const RAR_SKIP: c_int = 0;
const RAR_TEST: c_int = 1;
const RAR_HASH_CRC32: c_uint = 1;
const RHDF_SPLITBEFORE: c_uint = 0x01;
const RHDF_SPLITAFTER: c_uint = 0x02;
const RHDF_ENCRYPTED: c_uint = 0x04;
const RHDF_DIRECTORY: c_uint = 0x20;
const UCM_PROCESSDATA: c_uint = 1;
const ROADF_VOLUME: c_uint = 0x0001;
const ROADF_ENCHEADERS: c_uint = 0x0080;
const ROADF_FIRSTVOLUME: c_uint = 0x0100;

type Callback = extern "system" fn(c_uint, isize, isize, isize) -> c_int;

/// `RAROpenArchiveDataEx`, every field as dll.hpp has it.
#[repr(C, packed)]
#[allow(dead_code)]
struct OpenArchiveDataEx {
    arc_name: *const c_char,
    arc_name_w: *const u16,
    open_mode: c_uint,
    open_result: c_uint,
    cmt_buf: *mut c_char,
    cmt_buf_size: c_uint,
    cmt_size: c_uint,
    cmt_state: c_uint,
    flags: c_uint,
    callback: Option<Callback>,
    user_data: isize,
    op_flags: c_uint,
    cmt_buf_w: *mut u16,
    reserved: [c_uint; 25],
}

/// `RARHeaderDataEx`, every field as dll.hpp has it.
#[repr(C, packed)]
#[allow(dead_code)]
struct HeaderDataEx {
    arc_name: [c_char; 1024],
    arc_name_w: [u16; 1024],
    file_name: [c_char; 1024],
    file_name_w: [u16; 1024],
    flags: c_uint,
    pack_size: c_uint,
    pack_size_high: c_uint,
    unp_size: c_uint,
    unp_size_high: c_uint,
    host_os: c_uint,
    file_crc: c_uint,
    file_time: c_uint,
    unp_ver: c_uint,
    method: c_uint,
    file_attr: c_uint,
    cmt_buf: *mut c_char,
    cmt_buf_size: c_uint,
    cmt_size: c_uint,
    cmt_state: c_uint,
    dict_size: c_uint,
    hash_type: c_uint,
    hash: [c_char; 32],
    redir_type: c_uint,
    redir_name: *mut u16,
    redir_name_size: c_uint,
    dir_target: c_uint,
    mtime_low: c_uint,
    mtime_high: c_uint,
    ctime_low: c_uint,
    ctime_high: c_uint,
    atime_low: c_uint,
    atime_high: c_uint,
    arc_name_ex: *mut u16,
    arc_name_ex_size: c_uint,
    file_name_ex: *mut u16,
    file_name_ex_size: c_uint,
    reserved: [c_uint; 982],
}

// The layouts `dll.hpp` gives them on 64-bit Windows.
const _: () = assert!(std::mem::size_of::<OpenArchiveDataEx>() == 176);
const _: () = assert!(std::mem::size_of::<HeaderDataEx>() == 10244);

unsafe extern "system" {
    fn RAROpenArchiveEx(data: *mut OpenArchiveDataEx) -> *mut c_void;
    fn RARCloseArchive(handle: *mut c_void) -> c_int;
    fn RARReadHeaderEx(handle: *mut c_void, header: *mut HeaderDataEx) -> c_int;
    fn RARProcessFileW(
        handle: *mut c_void,
        operation: c_int,
        dest_path: *const u16,
        dest_name: *const u16,
    ) -> c_int;
    fn RARSetCallback(handle: *mut c_void, callback: Option<Callback>, user_data: isize);
}

// UnRAR calls the registry, token and crypto functions of advapi32, which
// `unrar_sys` doesn't link and Rust's std no longer pulls in.
#[link(name = "advapi32")]
unsafe extern "system" {}

/// What the callback hands each decompressed chunk to; `false` stops.
type OnData<'a> = &'a mut dyn FnMut(&[u8]) -> bool;

extern "system" fn callback(msg: c_uint, user: isize, p1: isize, p2: isize) -> c_int {
    if msg != UCM_PROCESSDATA {
        // A password, the next volume, a dictionary over the limit: no.
        return -1;
    }
    if user == 0 || p1 == 0 || p2 <= 0 {
        // Data while skipping (a solid archive decompresses what it
        // passes): nothing wants it.
        return 1;
    }
    // SAFETY: `user` is the `OnData` that `Rar::process` set for this call
    // and clears after it; UnRAR calls back on the calling thread, inside
    // `RARProcessFileW`. `p1`/`p2` are UnRAR's buffer and its length.
    let on_data = unsafe { &mut *(user as *mut OnData) };
    let data = unsafe { std::slice::from_raw_parts(p1 as *const u8, p2 as usize) };
    if on_data(data) { 1 } else { -1 }
}

fn describe(code: c_int) -> String {
    match code {
        ERAR_NO_MEMORY => "there wasn't enough memory to read it".into(),
        ERAR_BAD_DATA => "it is damaged (a checksum doesn't match)".into(),
        ERAR_BAD_ARCHIVE => "it is not a valid RAR archive".into(),
        ERAR_UNKNOWN_FORMAT => "its format is unknown".into(),
        ERAR_EOPEN => "it couldn't be opened (or a part of it is missing)".into(),
        ERAR_EREAD => "it couldn't be read".into(),
        ERAR_MISSING_PASSWORD | ERAR_BAD_PASSWORD => "it is encrypted (needs a password)".into(),
        ERAR_LARGE_DICT => "it asks for a dictionary larger than UnRAR allows".into(),
        code => format!("UnRAR error {code}"),
    }
}

/// An open archive; closed on drop.
struct Rar(*mut c_void);

// SAFETY: an UnRAR handle may be used from any thread, one at a time; `Rar`
// is moved, never shared.
unsafe impl Send for Rar {}

impl Drop for Rar {
    fn drop(&mut self) {
        // SAFETY: opened by `RAROpenArchiveEx`, closed once.
        unsafe { RARCloseArchive(self.0) };
    }
}

struct Header {
    name: String,
    size: u64,
    crc: u32,
    no_crc: bool,
    flags: u32,
}

impl Header {
    fn is_dir(&self) -> bool {
        self.flags & RHDF_DIRECTORY != 0
    }
}

impl Rar {
    /// `path` opened to list (`RAR_OM_LIST`) or read (`RAR_OM_EXTRACT`), and
    /// the archive's `ROADF_*` flags.
    fn open(path: &Path, mode: c_uint) -> Result<(Rar, u32), String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: all-zero is a valid `OpenArchiveDataEx` (null pointers,
        // no callback), and the reserved words must be zero.
        let mut data: OpenArchiveDataEx = unsafe { std::mem::zeroed() };
        data.arc_name_w = wide.as_ptr();
        data.open_mode = mode;
        data.callback = Some(callback);
        // SAFETY: `data` and the name it points to outlive the call.
        let handle = unsafe { RAROpenArchiveEx(&mut data) };
        let result = data.open_result as c_int;
        if handle.is_null() {
            return Err(describe(result));
        }
        let rar = Rar(handle);
        if result != ERAR_SUCCESS {
            return Err(describe(result));
        }
        Ok((rar, data.flags))
    }

    /// The next entry's header, or `None` at the end.
    fn next(&self) -> Result<Option<Header>, String> {
        // SAFETY: all-zero is a valid `HeaderDataEx`; the `*Ex` name
        // pointers stay null, so UnRAR writes only into the struct.
        let mut h: Box<HeaderDataEx> = Box::new(unsafe { std::mem::zeroed() });
        // SAFETY: an open handle and a header UnRAR fills in.
        match unsafe { RARReadHeaderEx(self.0, &mut *h) } {
            ERAR_SUCCESS => {}
            ERAR_END_ARCHIVE => return Ok(None),
            code => return Err(describe(code)),
        }
        let name_w = h.file_name_w;
        let len = name_w.iter().position(|&c| c == 0).unwrap_or(name_w.len());
        let name = String::from_utf16_lossy(&name_w[..len]).replace(std::path::MAIN_SEPARATOR, "/");
        Ok(Some(Header {
            name,
            size: (u64::from(h.unp_size_high) << 32) | u64::from(h.unp_size),
            crc: h.file_crc,
            no_crc: h.hash_type != RAR_HASH_CRC32,
            flags: h.flags,
        }))
    }

    /// Moves past the entry `next` returned: `RAR_SKIP`, or `RAR_TEST`
    /// with its bytes handed to `on_data`. Test mode decompresses and checks
    /// the entry and writes nothing.
    fn process(&self, op: c_int, on_data: Option<OnData>) -> Result<(), String> {
        let mut on_data = on_data;
        let user = match on_data.as_mut() {
            Some(f) => f as *mut OnData as isize,
            None => 0,
        };
        // SAFETY: `on_data` outlives the call, and is cleared from the
        // handle before it goes.
        let code = unsafe {
            RARSetCallback(self.0, Some(callback), user);
            let code = RARProcessFileW(self.0, op, std::ptr::null(), std::ptr::null());
            RARSetCallback(self.0, Some(callback), 0);
            code
        };
        match code {
            ERAR_SUCCESS => Ok(()),
            code => Err(describe(code)),
        }
    }
}

/// Held while UnRAR is in use: its error state is one global
/// (`ErrHandler`) that every handle's open and process calls set and read,
/// so two archives read at once could take each other's errors. Rar
/// archives are read one at a time; zips and 7z files still run alongside.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static UNRAR: Mutex<()> = Mutex::new(());
    UNRAR.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The `.dem` entries `path` lists, from its headers alone.
pub(super) fn list(path: &Path) -> Result<Vec<ZipDemoEntry>, String> {
    let _one = one_at_a_time();
    let (rar, flags) = Rar::open(path, RAR_OM_LIST)?;
    if flags & ROADF_ENCHEADERS != 0 {
        return Err("its file list is encrypted (needs a password)".into());
    }
    if flags & ROADF_VOLUME != 0 && flags & ROADF_FIRSTVOLUME == 0 {
        // A later part of a split archive: its demos are listed, as split,
        // with the first part.
        return Ok(Vec::new());
    }
    let mut demos: Vec<ZipDemoEntry> = Vec::new();
    loop {
        let header = match rar.next() {
            Ok(Some(header)) => header,
            Ok(None) => break,
            // A split archive ends where its first part does.
            Err(_) if flags & ROADF_VOLUME != 0 => break,
            Err(e) => return Err(e),
        };
        if !header.is_dir()
            && is_demo_name(&header.name)
            && !demos.iter().any(|d| d.name == header.name)
        {
            let error = if header.flags & RHDF_ENCRYPTED != 0 {
                Some("it is encrypted (needs a password)".to_string())
            } else if header.flags & (RHDF_SPLITBEFORE | RHDF_SPLITAFTER) != 0 {
                Some("it is split across several .rar files".to_string())
            } else {
                None
            };
            demos.push(ZipDemoEntry {
                name: header.name,
                size: header.size,
                crc32: header.crc,
                no_crc: header.no_crc,
                hashes: None,
                error,
            });
        }
        if let Err(e) = rar.process(RAR_SKIP, None) {
            if flags & ROADF_VOLUME != 0 {
                break;
            }
            return Err(e);
        }
    }
    Ok(demos)
}

/// Each of `names` in `path` read in full, in one walk of the archive.
pub(super) fn hash_entries(path: &Path, names: &[String]) -> Vec<Result<ContentHashes, String>> {
    let _one = one_at_a_time();
    let rar = match Rar::open(path, RAR_OM_EXTRACT) {
        Ok((rar, _)) => rar,
        Err(e) => return names.iter().map(|_| Err(e.clone())).collect(),
    };
    let mut wanted: HashMap<&str, usize> = HashMap::new();
    for (i, name) in names.iter().enumerate() {
        wanted.entry(name.as_str()).or_insert(i);
    }
    let mut found: Vec<Option<Result<ContentHashes, String>>> = vec![None; names.len()];
    let mut stopped: Option<String> = None;
    while !wanted.is_empty() {
        let header = match rar.next() {
            Ok(Some(header)) => header,
            Ok(None) => break,
            Err(e) => {
                stopped = Some(e);
                break;
            }
        };
        let at = (!header.is_dir())
            .then(|| wanted.remove(header.name.as_str()))
            .flatten();
        let Some(at) = at else {
            if let Err(e) = rar.process(RAR_SKIP, None) {
                stopped = Some(e);
                break;
            }
            continue;
        };
        let mut hasher = ContentHasher::new(header.size);
        let read = {
            let mut on_data = |chunk: &[u8]| {
                hasher.update(chunk);
                true
            };
            rar.process(RAR_TEST, Some(&mut on_data))
        };
        found[at] = Some(read.and_then(|()| hasher.finish().map_err(|e| e.to_string())));
    }
    found
        .into_iter()
        .map(|read| {
            read.unwrap_or_else(|| {
                Err(stopped
                    .clone()
                    .unwrap_or_else(|| "it is not in the archive".into()))
            })
        })
        .collect()
}

/// Reads what [`with_entry`]'s reading thread sends: chunks, then an error
/// if the entry couldn't be read to its end.
struct ChannelReader {
    rx: mpsc::Receiver<Result<Vec<u8>, String>>,
    chunk: Vec<u8>,
    at: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.at == self.chunk.len() {
            match self.rx.recv() {
                Ok(Ok(chunk)) => {
                    self.chunk = chunk;
                    self.at = 0;
                }
                Ok(Err(e)) => return Err(std::io::Error::other(e)),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.at);
        buf[..n].copy_from_slice(&self.chunk[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// `f` with entry `name` of `path` open for reading, as
/// [`super::with_archive_entry`]. UnRAR pushes the bytes from a thread of
/// its own, a few chunks ahead of `f`, so nothing is held whole in memory.
pub(super) fn with_entry<T>(
    path: &Path,
    name: &str,
    f: impl FnOnce(&mut dyn Read, u64) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let _one = one_at_a_time();
    let (rar, _) = Rar::open(path, RAR_OM_EXTRACT).map_err(std::io::Error::other)?;
    let size = loop {
        match rar.next().map_err(std::io::Error::other)? {
            None => return Err(std::io::Error::other("it is not in the archive")),
            Some(h) if !h.is_dir() && h.name == name => break h.size,
            Some(_) => rar.process(RAR_SKIP, None).map_err(std::io::Error::other)?,
        }
    };
    let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(4);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut on_data = |chunk: &[u8]| tx.send(Ok(chunk.to_vec())).is_ok();
            if let Err(e) = rar.process(RAR_TEST, Some(&mut on_data)) {
                // Dropped by now when `f` stopped early: nobody to tell.
                let _ = tx.send(Err(e));
            }
        });
        let mut reader = ChannelReader {
            rx,
            chunk: Vec::new(),
            at: 0,
        };
        // Dropping the reader ends the thread's sends when `f` stops early.
        f(&mut reader, size)
    })
}

#[cfg(test)]
mod tests {
    use super::super::{hash_archive_entries, list_archive_demos, with_archive_entry};
    use std::path::PathBuf;

    /// A stored (uncompressed) RAR 4 archive, written by hand from RARLAB's
    /// published RAR 4 format notes, not with RAR: `match/a.dem` (3000 bytes
    /// of [`demo_bytes`]`(5, 3000)`), `readme.txt`, and `locked.dem`, whose
    /// encrypted flag is set over 16 plain bytes.
    pub(crate) const PACK_RAR: &[u8] = include_bytes!("testdata/pack.rar");

    pub(crate) fn demo_bytes(seed: usize, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| ((i * 31 + seed * 7) % 251) as u8)
            .collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("auditor_rar_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dem_entries_are_listed_and_an_encrypted_one_carries_an_error() {
        let dir = scratch("list");
        let path = dir.join("pack.rar");
        std::fs::write(&path, PACK_RAR).unwrap();
        let demos = list_archive_demos(&path).unwrap();
        let names: Vec<_> = demos.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["match/a.dem", "locked.dem"]);
        assert_eq!(demos[0].size, 3000);
        assert!(
            demos[0].error.is_none() && !demos[0].no_crc && demos[0].crc32 != 0,
            "{demos:?}"
        );
        assert!(
            demos[1]
                .error
                .as_deref()
                .is_some_and(|e| e.contains("encrypted"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_hashes_like_the_same_bytes_on_disk_and_reads_back_whole() {
        let dir = scratch("hashes");
        let path = dir.join("pack.rar");
        std::fs::write(&path, PACK_RAR).unwrap();
        let bytes = demo_bytes(5, 3000);
        let loose = dir.join("loose.dem");
        std::fs::write(&loose, &bytes).unwrap();
        let found = hash_archive_entries(&path, &["match/a.dem".into(), "gone.dem".into()]);
        assert_eq!(found[0], Ok(crate::content_hashes(&loose).unwrap()));
        assert!(found[1].is_err());
        let read = with_archive_entry(&path, "match/a.dem", |r, size| {
            let mut out = Vec::new();
            r.read_to_end(&mut out)?;
            Ok((out, size))
        })
        .unwrap();
        assert_eq!(read, (bytes.clone(), 3000));
        // Stopping early ends the read without hanging.
        let first = with_archive_entry(&path, "match/a.dem", |r, _| {
            let mut one = [0u8; 1];
            r.read_exact(&mut one)?;
            Ok(one[0])
        })
        .unwrap();
        assert_eq!(first, bytes[0]);
        assert!(with_archive_entry(&path, "gone.dem", |_, _| Ok(())).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_byte_fails_the_entrys_checksum() {
        let dir = scratch("crc");
        let mut bytes = PACK_RAR.to_vec();
        let start = bytes
            .windows(16)
            .position(|w| w == &demo_bytes(5, 16)[..])
            .unwrap();
        bytes[start + 100] ^= 0xff;
        let path = dir.join("bad.rar");
        std::fs::write(&path, &bytes).unwrap();
        assert!(list_archive_demos(&path).is_ok(), "its headers are intact");
        let found = hash_archive_entries(&path, &["match/a.dem".into()]);
        assert!(found[0].is_err(), "{found:?}");
        let read = with_archive_entry(&path, "match/a.dem", |r, _| {
            let mut out = Vec::new();
            r.read_to_end(&mut out)?;
            Ok(())
        });
        assert!(read.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Damaged copies (cut short, or a byte changed) give errors, never a
    /// crash or a hang.
    #[test]
    fn damaged_archives_fail_without_crashing() {
        let dir = scratch("damaged");
        let bad = dir.join("bad.rar");
        let names = ["match/a.dem".to_string()];
        for cut in (0..PACK_RAR.len()).step_by(13) {
            std::fs::write(&bad, &PACK_RAR[..cut]).unwrap();
            let _ = list_archive_demos(&bad);
            let _ = hash_archive_entries(&bad, &names);
        }
        for at in 0..120 {
            let mut changed = PACK_RAR.to_vec();
            changed[at] ^= 0x5a;
            std::fs::write(&bad, &changed).unwrap();
            let _ = list_archive_demos(&bad);
            let _ = hash_archive_entries(&bad, &names);
        }
        std::fs::write(&bad, b"Rar!\x1a\x07\x00 not really a rar").unwrap();
        assert!(list_archive_demos(&bad).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
