//! This build's id (`build.rs`), kept in the DLL where Studio can find it
//! both in the file on disk and in the copy a running game has loaded
//! (#1143): `native::sys::hook_build` scans for [`MARKER`]'s prefix, so the
//! same path holding another build reads as another hook DLL.

/// The id alone, for the hook's log: `0.10.0+1a2b3c4d5`, with
/// `.dirty.<UTC time>` for a build with uncommitted changes.
pub const ID: &str = env!("DS_HOOK_BUILD_ID");

/// The prefix, the id and a NUL. The prefix must match
/// `native::sys::hook_build::MARKER_PREFIX` (a test there checks this file).
const MARKER: &str = concat!("DSHOOKBUILD:", env!("DS_HOOK_BUILD_ID"), "\0");

const fn marker_bytes<const N: usize>() -> [u8; N] {
    let source = MARKER.as_bytes();
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = source[i];
        i += 1;
    }
    out
}

/// Exported so the linker keeps it: nothing in the DLL reads it.
#[unsafe(no_mangle)]
#[used]
pub static DODSTUDIO_HOOK_BUILD_ID: [u8; MARKER.len()] = marker_bytes::<{ MARKER.len() }>();
