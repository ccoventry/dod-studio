use super::*;
use crate::test_support::Scratch;

#[test]
fn a_missing_file_is_not_an_error() {
    let dir = Scratch::new("rfr_missing");
    assert!(remove_file_retrying(&dir.join("nope.dem")).is_none());
}

#[test]
fn an_unlocked_file_is_removed_on_the_first_attempt() {
    let dir = Scratch::new("rfr_plain");
    let file = dir.join("dodstudio_chain_01.dem");
    std::fs::write(&file, b"demo").unwrap();

    assert!(remove_file_retrying(&file).is_none());
    assert!(!file.exists());
}

// Only on Windows: `std::fs::File::open`'s default share mode there
// already includes FILE_SHARE_DELETE, so holding a plain handle open
// would not actually block a delete and this test would pass without the
// retry loop doing anything. `share_mode(1)` (FILE_SHARE_READ only,
// matching the pattern already used in `capture_engine.rs`'s demo-copy
// path) withholds delete sharing, which is what reproduces a file hl.exe
// still has open the way this function exists to wait out.
#[cfg(target_os = "windows")]
#[test]
fn a_file_that_becomes_removable_partway_through_succeeds() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = Scratch::new("rfr_delayed");
    let file = dir.join("dodstudio_chain_02.dem");
    std::fs::write(&file, b"demo").unwrap();

    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&file)
        .unwrap();
    assert!(
        std::fs::remove_file(&file).is_err(),
        "the fixture itself is wrong if a plain remove_file succeeds while this handle is open"
    );

    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(350));
        drop(handle);
    });

    let result = remove_file_retrying(&file);
    releaser.join().unwrap();

    assert!(result.is_none(), "{result:?}");
    assert!(!file.exists());
}
