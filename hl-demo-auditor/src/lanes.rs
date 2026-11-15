//! Which drive each file is on, and how many reads each drive gets at once:
//! the SSDs share one pool of threads, and each hard disk gets a reader of
//! its own so its head isn't sent back and forth (#941).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::par_map;
use crate::walk::comparable_path;

/// Threads a walk or key pass shares between the SSDs: separate drives list
/// and read at once, and an SSD answers several requests at a time even on
/// one drive. A hard disk gets one of its own ([`Readers`]).
fn worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

/// The drive a path is on, as `c:`, for sharing reads out by drive (#941).
/// `None` for a network share or a relative path.
pub(crate) fn drive_of(path: &Path) -> Option<String> {
    let text = comparable_path(path);
    let bytes = text.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        .then(|| text[..2].to_string())
}

/// Whether Windows says `drive` (`c:`) has a seek penalty: a spinning hard
/// disk. `false` when it can't say, and off Windows, so an unknown drive
/// keeps the pool, as before #941.
fn drive_seeks(drive: &str) -> bool {
    seek_penalty::query(drive).unwrap_or(false)
}

/// `IOCTL_STORAGE_QUERY_PROPERTY` with `StorageDeviceSeekPenaltyProperty` on
/// `\\.\X:`, opened with no access rights, which needs no admin.
#[cfg(windows)]
pub(crate) mod seek_penalty {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *const c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn DeviceIoControl(
            device: *mut c_void,
            io_control_code: u32,
            in_buffer: *const c_void,
            in_buffer_size: u32,
            out_buffer: *mut c_void,
            out_buffer_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const STORAGE_DEVICE_SEEK_PENALTY_PROPERTY: u32 = 7;
    const PROPERTY_STANDARD_QUERY: u32 = 0;

    /// `STORAGE_PROPERTY_QUERY`.
    #[repr(C)]
    struct Query {
        property_id: u32,
        query_type: u32,
        additional: [u8; 1],
    }

    /// `DEVICE_SEEK_PENALTY_DESCRIPTOR`.
    #[repr(C)]
    #[derive(Default)]
    struct SeekPenalty {
        _version: u32,
        _size: u32,
        incurs_seek_penalty: u8,
    }

    /// `None` when the drive can't be opened or doesn't answer (a volume
    /// spread over several disks, a virtual drive).
    pub fn query(drive: &str) -> Option<bool> {
        let name: Vec<u16> = format!(r"\\.\{drive}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle.is_null() || handle as isize == -1 {
            return None;
        }
        let query = Query {
            property_id: STORAGE_DEVICE_SEEK_PENALTY_PROPERTY,
            query_type: PROPERTY_STANDARD_QUERY,
            additional: [0],
        };
        let mut answer = SeekPenalty::default();
        let mut returned = 0u32;
        // SAFETY: both buffers are live, sized as passed, and the handle is
        // open until the CloseHandle below.
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                (&raw const query).cast(),
                size_of::<Query>() as u32,
                (&raw mut answer).cast(),
                size_of::<SeekPenalty>() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: opened above and closed once.
        unsafe { CloseHandle(handle) };
        // The answer is in byte 8, after `version` and `size`.
        (ok != 0 && returned >= 9).then_some(answer.incurs_seek_penalty != 0)
    }
}

#[cfg(not(windows))]
pub(crate) mod seek_penalty {
    pub fn query(_drive: &str) -> Option<bool> {
        None
    }
}

/// One share of a pass's items: their indices, and how many are read at once.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Lane {
    pub(crate) threads: usize,
    pub(crate) items: Vec<usize>,
}

/// Items shared out by drive (#941): each hard disk (`seeks` says so) gets a
/// lane of its own with one reader, so its head never seeks back and forth
/// between two files; every other item (SSDs, unknown drives, shares) is in
/// one lane of `threads` readers, as before. [`Readers::map`] runs the lanes
/// side by side, so a hard disk doesn't hold up the SSDs. In item order: the
/// shared lane first, then the hard disks by drive.
pub(crate) fn plan_lanes(
    drives: impl Iterator<Item = Option<String>>,
    threads: usize,
    seeks: &dyn Fn(&str) -> bool,
) -> Vec<Lane> {
    let mut known: HashMap<String, bool> = HashMap::new();
    let mut shared = Vec::new();
    let mut disks: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for (i, drive) in drives.enumerate() {
        match drive {
            Some(drive) if *known.entry(drive.clone()).or_insert_with(|| seeks(&drive)) => {
                disks.entry(drive).or_default().push(i)
            }
            _ => shared.push(i),
        }
    }
    let mut lanes = Vec::new();
    if !shared.is_empty() {
        lanes.push(Lane {
            threads: threads.max(1),
            items: shared,
        });
    }
    lanes.extend(disks.into_values().map(|items| Lane { threads: 1, items }));
    lanes
}

/// How a pass reads: `threads` at once over the SSDs, one per hard disk.
pub(crate) struct Readers<'a> {
    pub(crate) threads: usize,
    /// Whether a drive (`c:`) is a hard disk; [`drive_seeks`] outside tests.
    pub(crate) seeks: &'a (dyn Fn(&str) -> bool + Sync),
}

impl Readers<'static> {
    pub(crate) fn system() -> Self {
        Readers {
            threads: worker_threads(),
            seeks: &drive_seeks,
        }
    }
}

impl Readers<'_> {
    /// [`par_map`] with the items shared out by drive ([`plan_lanes`]);
    /// `drive` says which drive an item reads from.
    pub(crate) fn map<T: Sync, R: Send>(
        &self,
        items: &[T],
        drive: impl Fn(&T) -> Option<String>,
        cancel: &AtomicBool,
        f: impl Fn(&T) -> R + Sync,
    ) -> Vec<Option<R>> {
        let lanes = plan_lanes(items.iter().map(drive), self.threads, self.seeks);
        if let [lane] = lanes.as_slice() {
            return par_map(items, lane.threads, cancel, f);
        }
        let mut out: Vec<Option<R>> = std::iter::repeat_with(|| None).take(items.len()).collect();
        let f = &f;
        let done: Vec<Vec<Option<R>>> = std::thread::scope(|scope| {
            let workers: Vec<_> = lanes
                .iter()
                .map(|lane| {
                    scope.spawn(move || {
                        par_map(&lane.items, lane.threads, cancel, |&i| f(&items[i]))
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|w| w.join().unwrap_or_default())
                .collect()
        });
        for (lane, results) in lanes.iter().zip(done) {
            for (&i, r) in lane.items.iter().zip(results) {
                out[i] = r;
            }
        }
        out
    }
}
