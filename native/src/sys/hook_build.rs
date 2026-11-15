//! Which DoD Studio hook DLL a running game has, and which build it is
//! (#1143), against the file Studio would launch with.
//!
//! The path comes off the game's module list (`sys::modules`), since HLAE
//! injects the DLL and exits. The build is the id `goldsrc-hooks/build.rs`
//! bakes into the DLL behind [`MARKER_PREFIX`], read from the file on disk
//! and from the image loaded in the game. Windows refuses to overwrite or
//! delete a loaded DLL but lets it be renamed and a new file be put at its
//! path, and the game's module list keeps the old path: so the same path can
//! hold another build than the game runs, and only the id tells them apart.

use std::path::Path;

use crate::patch::launch_settings::{HookDll, LoadedHook};

/// What `goldsrc-hooks/src/build_id.rs` puts in front of the id, which ends
/// at a NUL.
pub const MARKER_PREFIX: &[u8] = b"DSHOOKBUILD:";

/// The longest id read: a version, a commit and a dirty build's time fit in
/// well under this, and a stray prefix with no NUL near it is not an id.
const MAX_ID_BYTES: usize = 128;

/// The build id in a hook DLL's bytes (the file or its loaded image), or
/// `None` for a build from before ids were baked in.
pub fn build_id_in(bytes: &[u8]) -> Option<String> {
    let start = bytes
        .windows(MARKER_PREFIX.len())
        .position(|w| w == MARKER_PREFIX)?
        + MARKER_PREFIX.len();
    let rest = &bytes[start..bytes.len().min(start + MAX_ID_BYTES)];
    let id = &rest[..rest.iter().position(|&b| b == 0)?];
    let printable = !id.is_empty() && id.iter().all(|b| b.is_ascii_graphic());
    printable.then(|| String::from_utf8_lossy(id).into_owned())
}

/// The hook DLL at `path` on disk and its build id. The id is `None` when
/// the file has none or can't be read: a launch with a missing file fails on
/// its own, and comparing it against the game's still reads as different.
pub fn hook_on_disk(path: &Path) -> HookDll {
    HookDll {
        path: path.to_path_buf(),
        build: std::fs::read(path).ok().and_then(|b| build_id_in(&b)),
    }
}

/// Which hook DLL (by file name, `hook_name`) game `pid` has loaded and its
/// build id, read from the game's memory. `Unreadable` when its module list
/// or the DLL's image can't be read, so the caller asks rather than reusing a
/// game it knows nothing about.
pub fn loaded_hook(pid: u32, hook_name: &str) -> LoadedHook {
    let Some(images) = crate::sys::modules::module_images(pid) else {
        return LoadedHook::Unreadable;
    };
    let Some(image) = images.into_iter().find(|m| {
        m.path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(hook_name))
    }) else {
        return LoadedHook::Absent;
    };
    match crate::sys::modules::read_module_image(pid, &image) {
        Some(bytes) => LoadedHook::Loaded(HookDll {
            path: image.path,
            build: build_id_in(&bytes),
        }),
        None => LoadedHook::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dll_with(marker: &[u8]) -> Vec<u8> {
        let mut bytes = b"MZ\x90\0 code and data ".to_vec();
        bytes.extend_from_slice(marker);
        bytes.extend_from_slice(b"more data");
        bytes
    }

    #[test]
    fn the_id_after_the_prefix_up_to_the_nul_is_read() {
        let bytes = dll_with(b"DSHOOKBUILD:0.10.0+1a2b3c4d5\0");
        assert_eq!(build_id_in(&bytes).as_deref(), Some("0.10.0+1a2b3c4d5"));
    }

    #[test]
    fn a_dll_without_the_prefix_has_no_id() {
        assert_eq!(build_id_in(&dll_with(b"nothing here\0")), None);
    }

    #[test]
    fn a_prefix_with_no_nul_or_unprintable_bytes_after_it_is_not_an_id() {
        assert_eq!(build_id_in(b"xx DSHOOKBUILD:0.10.0"), None);
        assert_eq!(build_id_in(&dll_with(b"DSHOOKBUILD:\x01\x02\0")), None);
        assert_eq!(build_id_in(&dll_with(b"DSHOOKBUILD:\0")), None);
        let long = [b"DSHOOKBUILD:".as_slice(), &[b'a'; 200], b"\0"].concat();
        assert_eq!(build_id_in(&dll_with(&long)), None);
    }

    #[test]
    fn the_prefix_matches_the_one_the_hook_bakes_in() {
        let source = include_str!("../../../goldsrc-hooks/src/build_id.rs");
        let prefix = std::str::from_utf8(MARKER_PREFIX).unwrap();
        assert!(
            source.contains(&format!("concat!(\"{prefix}\"")),
            "goldsrc-hooks/src/build_id.rs no longer starts its marker with {prefix}"
        );
    }

    #[test]
    fn a_missing_file_has_no_id() {
        let hook = hook_on_disk(Path::new("Z:/no/such/dodstudio_goldsrc_hooks.dll"));
        assert_eq!(hook.build, None);
    }

    #[cfg(windows)]
    #[test]
    fn a_game_without_the_dll_reads_as_absent() {
        // This test process has no hook DLL loaded.
        assert_eq!(
            loaded_hook(std::process::id(), "dodstudio_goldsrc_hooks.dll"),
            LoadedHook::Absent
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_game_whose_modules_cannot_be_read_is_unreadable() {
        // No process has this PID (0 would mean this process to Windows).
        assert_eq!(
            loaded_hook(0xFFFF_FFF0, "dodstudio_goldsrc_hooks.dll"),
            LoadedHook::Unreadable
        );
    }
}
