//! A duplicate group is keyed by size and the first 64 KiB only
//! (`hl_demo_auditor`), so two different demos can share one. Before the
//! Demo Auditor's Duplicates delete or Consolidate recycles a copy, it is
//! compared byte for byte with the copy its group keeps (#871, #872).

use std::io::Read;
use std::path::Path;

/// A path as Explorer shows it, without the audit's `\\?\` prefix: the
/// messages here use it, and so does Studio's delete error.
pub use crate::messages::shown;

/// Why [`check_same_contents`] refused a copy.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    /// The bytes differ: a different demo. The table marks it
    /// "Not an exact copy".
    NotACopy(String),
    /// Anything else: the kept copy is gone, is the same file, can't be read.
    Other(String),
}

impl Refusal {
    /// The plain-words reason, and whether it's a different demo.
    pub fn into_parts(self) -> (String, bool) {
        match self {
            Refusal::NotACopy(e) => (e, true),
            Refusal::Other(e) => (e, false),
        }
    }
}

/// Bytes read from each file per comparison step.
const COMPARE_CHUNK: usize = 1 << 20;

/// Fills `buf` from `file` until it is full or the file ends; how many bytes.
fn read_chunk(file: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Whether `copy` is a true copy of `keep`: another file with every byte
/// the same. The audit's key reads only the first 64 KiB, so two different
/// demos can share a group; this keeps a delete from trusting that.
pub fn check_same_contents(copy: &Path, keep: &Path) -> Result<(), Refusal> {
    use crate::messages::{
        copy_compare_failed, copy_differs_from_kept, copy_is_the_kept_copy, kept_copy_missing,
    };
    let kept_is_there = keep.symlink_metadata().is_ok_and(|m| m.is_file());
    if !kept_is_there {
        return Err(Refusal::Other(kept_copy_missing(copy, keep)));
    }
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(copy), std::fs::canonicalize(keep))
        && a == b
    {
        return Err(Refusal::Other(copy_is_the_kept_copy(copy)));
    }
    let unreadable = |e: std::io::Error| Refusal::Other(copy_compare_failed(copy, keep, e));
    let differs = |at: u64| Refusal::NotACopy(copy_differs_from_kept(copy, keep, at));
    let mut a = std::fs::File::open(copy).map_err(unreadable)?;
    let mut b = std::fs::File::open(keep).map_err(unreadable)?;
    let (len_a, len_b) = (
        a.metadata().map_err(unreadable)?.len(),
        b.metadata().map_err(unreadable)?.len(),
    );
    if len_a != len_b {
        return Err(differs(len_a.min(len_b)));
    }
    let mut buf_a = vec![0u8; COMPARE_CHUNK];
    let mut buf_b = vec![0u8; COMPARE_CHUNK];
    let mut offset: u64 = 0;
    loop {
        let n_a = read_chunk(&mut a, &mut buf_a).map_err(unreadable)?;
        let n_b = read_chunk(&mut b, &mut buf_b).map_err(unreadable)?;
        if let Some(i) = buf_a[..n_a.min(n_b)]
            .iter()
            .zip(&buf_b[..n_a.min(n_b)])
            .position(|(x, y)| x != y)
        {
            return Err(differs(offset + i as u64));
        }
        if n_a != n_b {
            // The file changed size while it was read.
            return Err(differs(offset + n_a.min(n_b) as u64));
        }
        if n_a == 0 {
            return Ok(());
        }
        offset += n_a as u64;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// Same size and first 64 KiB, so the audit groups them, but not the
    /// same demo past that.
    pub(crate) fn demo_bytes(tail: u8) -> Vec<u8> {
        let mut bytes = vec![7u8; COMPARE_CHUNK + 70_000];
        *bytes.last_mut().unwrap() = tail;
        bytes
    }

    fn file(dir: &Scratch, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn messages_show_paths_without_the_verbatim_prefix() {
        assert_eq!(
            crate::messages::copy_is_the_kept_copy(Path::new(r"\\?\C:\d\a.dem")),
            r"Not deleted: C:\d\a.dem is the copy being kept"
        );
        assert_eq!(shown(Path::new(r"\\?\UNC\nas\d\a.dem")), r"\\nas\d\a.dem");
    }

    #[test]
    fn a_byte_for_byte_copy_can_be_deleted() {
        let s = Scratch::new("compare_same");
        let keep = file(&s, "keep.dem", &demo_bytes(1));
        let copy = file(&s, "copy.dem", &demo_bytes(1));
        assert_eq!(check_same_contents(&copy, &keep), Ok(()));
    }

    #[test]
    fn a_copy_that_differs_past_the_key_is_not_deleted() {
        let s = Scratch::new("compare_differs");
        let keep = file(&s, "keep.dem", &demo_bytes(1));
        let copy = file(&s, "copy.dem", &demo_bytes(2));
        let Err(Refusal::NotACopy(err)) = check_same_contents(&copy, &keep) else {
            panic!("a different file must be refused as not a copy");
        };
        let last = (COMPARE_CHUNK + 70_000 - 1).to_string();
        assert!(err.contains(&format!("differ from byte {last}")), "{err}");
        assert!(Refusal::NotACopy(err).into_parts().1);
    }

    #[test]
    fn a_copy_of_another_size_is_not_deleted() {
        let s = Scratch::new("compare_size");
        let keep = file(&s, "keep.dem", b"abcdef");
        let copy = file(&s, "copy.dem", b"abc");
        let Err(Refusal::NotACopy(err)) = check_same_contents(&copy, &keep) else {
            panic!("another size must be refused as not a copy");
        };
        assert!(err.contains("differ from byte 3"), "{err}");
    }

    #[test]
    fn the_kept_copy_itself_or_a_missing_one_is_never_a_reason_to_delete() {
        let s = Scratch::new("compare_self");
        let keep = file(&s, "keep.dem", b"abc");
        let Err(Refusal::Other(err)) = check_same_contents(&keep, &keep) else {
            panic!("the kept file itself must be refused");
        };
        assert!(err.contains("is the copy being kept"), "{err}");
        let gone = s.path().join("gone.dem");
        let Err(Refusal::Other(err)) = check_same_contents(&keep, &gone) else {
            panic!("a missing kept copy must be refused");
        };
        assert!(err.contains("no longer there"), "{err}");
        assert!(!Refusal::Other(err).into_parts().1);
    }
}
