use super::*;
use crate::test_support::Scratch;

fn make_preview(dir: &Path, stem: &str) -> (PathBuf, PathBuf) {
    let demo = dir.join(format!("{stem}_preview.dem"));
    let sidecar = demo.with_extension(PREVIEW_SIDECAR_EXT);
    std::fs::write(&demo, b"demo").unwrap();
    std::fs::write(&sidecar, b"").unwrap();
    (demo, sidecar)
}

fn scratch(name: &str) -> Scratch {
    let root = Scratch::new(format_args!("acp_{name}"));
    std::fs::create_dir_all(root.join("dod")).unwrap();
    root
}

#[test]
fn a_preview_with_its_sidecar_is_removed() {
    let root = scratch("pair");
    let (demo, sidecar) = make_preview(&root.join("dod"), "match1");

    clear_capture_scratch(&root, false, false, true, false);

    assert!(!demo.exists(), "the preview demo should be gone");
    assert!(!sidecar.exists(), "its sidecar should be gone with it");
}

#[test]
fn a_user_named_preview_without_a_sidecar_is_left_alone() {
    let root = scratch("nosidecar");
    let demo = root.join("dod").join("my_own_preview.dem");
    std::fs::write(&demo, b"demo").unwrap();

    clear_capture_scratch(&root, false, false, true, false);

    assert!(
        demo.exists(),
        "a demo the user named this way is not ours to delete"
    );
}

/// The sidecar is the only thing marking a `_preview.dem` as ours. If it is
/// removed while the demo itself could not be, the demo stops being
/// recognisable and sits in the game folder permanently -- cleanup can
/// never pick it up again on any later batch. So a failed demo removal must
/// leave the sidecar in place.
///
/// Windows only, for the same reason `remove_file_retrying`'s own
/// lock test is: `File::open`'s default share mode includes
/// FILE_SHARE_DELETE, so a plain open would not block the delete and this
/// would pass without testing anything.
#[cfg(target_os = "windows")]
#[test]
fn a_sidecar_outlives_a_demo_that_could_not_be_removed() {
    use std::os::windows::fs::OpenOptionsExt;

    let root = scratch("orphan");
    let (demo, sidecar) = make_preview(&root.join("dod"), "locked");

    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&demo)
        .unwrap();
    assert!(
        std::fs::remove_file(&demo).is_err(),
        "the fixture itself is wrong if a plain remove_file succeeds here"
    );

    clear_capture_scratch(&root, false, false, true, false);

    assert!(demo.exists(), "the locked demo could not have been removed");
    assert!(
        sidecar.exists(),
        "the sidecar must survive, or this demo can never be cleaned up again"
    );

    drop(handle);
}
