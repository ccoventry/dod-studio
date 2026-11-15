// patch/map_fetch.rs
// Fetch a map the local library is missing, and refuse to install one that is
// not the build the demo asked for.
//
// The mirror lays out every file at the same path it occupies under `dod/`, so
// `dod/maps/dod_railyard_s9a.bsp` is `<base>/maps/dod_railyard_s9a.bsp`. That
// makes the URL a pure function of the map name — no index to consult, nothing
// to keep in step.
//
// The order here is the whole point: download to a scratch file, read the
// checksum out of what actually arrived, and only then move it into the map
// library. A wrong build installed by an updater is precisely the failure
// `map_check` exists to catch, so this must never be able to cause it. An
// existing file is moved aside rather than overwritten — a map that turned out
// to be the wrong build is still the map the user had, and it is not this
// code's call to destroy it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::bsp;
use super::map_check::is_safe_map_name;

/// KTP's mirror. Every file under `dod/` at the same relative path.
pub const DEFAULT_MIRROR: &str = "https://fastdl.ktpdod.com/dod/";

/// A GoldSrc BSP the engine will load is far below this. The cap exists so a
/// mirror serving something else — an error page, a redirect loop, the wrong
/// file entirely — cannot be read into memory indefinitely.
pub const MAX_MAP_BYTES: u64 = 96 * 1024 * 1024;

const SCRATCH_SUFFIX: &str = ".bsp.part";

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub map_name: String,
    pub installed: PathBuf,
    pub checksum: u32,
    pub bytes: u64,
    /// Where an existing file was moved to, when one was in the way.
    pub replaced: Option<PathBuf>,
    /// True when the map was already present and already correct, so nothing
    /// was downloaded.
    pub already_correct: bool,
}

/// Where a map would be fetched from. Exposed so a prompt can show the user the
/// exact URL before anything reaches the network.
pub fn map_url(mirror: &str, map_name: &str) -> Result<String, String> {
    if !is_safe_map_name(map_name) {
        return Err(crate::messages::not_a_usable_map_name(map_name));
    }
    let base = mirror.trim_end_matches('/');
    if !base.starts_with("https://") {
        // The file is going into the folder the game loads code-adjacent data
        // from. Fetching it over a channel anyone can rewrite in flight is not
        // a trade worth offering, even with the checksum check behind it —
        // demos that state no checksum would have nothing behind it at all.
        return Err(crate::messages::MAP_MIRRORS_MUST_BE_HTTPS.to_string());
    }
    Ok(format!("{}/maps/{}.bsp", base, map_name))
}

/// Where a fetch is, for a progress line (#957).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchStep {
    /// `chunk` more bytes arrived; `total` is the size the mirror stated.
    Downloaded { chunk: u64, total: Option<u64> },
    /// Everything arrived: reading the checksum and installing it.
    Checking,
}

/// Download one map into `maps_dir`, verifying before it lands.
///
/// `expected` is the checksum the demo asked for. `None` — an HLTV demo, which
/// records no checksum — means the download can only be checked for being a
/// readable BSP at all.
pub fn fetch_map(
    map_name: &str,
    expected: Option<u32>,
    maps_dir: &Path,
    mirror: &str,
) -> Result<FetchOutcome, String> {
    fetch_map_with_progress(map_name, expected, maps_dir, mirror, &mut |_| {})
}

/// [`fetch_map`], telling `on_step` about every chunk read and when the check
/// starts. Unthrottled: the caller decides how often to show it.
pub fn fetch_map_with_progress(
    map_name: &str,
    expected: Option<u32>,
    maps_dir: &Path,
    mirror: &str,
    on_step: &mut dyn FnMut(FetchStep),
) -> Result<FetchOutcome, String> {
    let url = map_url(mirror, map_name)?;
    let target = maps_dir.join(format!("{}.bsp", map_name));

    // Already here and already right: say so and touch nothing.
    if target.is_file()
        && let (Some(want), Ok(found)) = (expected, bsp::map_checksum_of_file(&target))
        && want == found
    {
        return Ok(FetchOutcome {
            map_name: map_name.to_string(),
            installed: target,
            checksum: found,
            bytes: 0,
            replaced: None,
            already_correct: true,
        });
    }

    std::fs::create_dir_all(maps_dir)
        .map_err(|e| crate::messages::labeled(maps_dir.display(), e))?;

    let body = download(&url, on_step)?;
    on_step(FetchStep::Checking);

    // What arrived, not what was asked for. A mirror can serve an error page
    // with a 200, and an error page is not a map.
    let checksum =
        bsp::map_checksum(&body).map_err(|e| crate::messages::served_unreadable_bsp(&url, e))?;
    bsp::Bsp::parse(&body).map_err(|e| crate::messages::served_unparseable_map(&url, e))?;

    if let Some(want) = expected
        && checksum != want
    {
        return Err(crate::messages::served_wrong_build(&url, checksum, want));
    }

    // Write beside the target so the rename cannot cross a volume, then move
    // the old file aside before putting the new one in place.
    let scratch = maps_dir.join(format!("{}{}", map_name, SCRATCH_SUFFIX));
    {
        let mut file = std::fs::File::create(&scratch)
            .map_err(|e| crate::messages::labeled(scratch.display(), e))?;
        file.write_all(&body)
            .and_then(|_| file.sync_all())
            .map_err(|e| {
                let _ = std::fs::remove_file(&scratch);
                crate::messages::labeled(scratch.display(), e)
            })?;
    }

    let mut replaced = None;
    if target.is_file() {
        let previous = bsp::map_checksum_of_file(&target).unwrap_or(0);
        let aside = maps_dir.join(format!("{}.{:08x}.bsp.bak", map_name, previous));
        std::fs::rename(&target, &aside).map_err(|e| {
            let _ = std::fs::remove_file(&scratch);
            crate::messages::could_not_move_existing_aside(target.display(), e)
        })?;
        replaced = Some(aside);
    }

    std::fs::rename(&scratch, &target).map_err(|e| {
        let _ = std::fs::remove_file(&scratch);
        crate::messages::labeled(target.display(), e)
    })?;

    Ok(FetchOutcome {
        map_name: map_name.to_string(),
        installed: target,
        checksum,
        bytes: body.len() as u64,
        replaced,
        already_correct: false,
    })
}

fn download(url: &str, on_step: &mut dyn FnMut(FetchStep)) -> Result<Vec<u8>, String> {
    let mut response = crate::utils::http::get(url)?;
    let total = response.body().content_length();
    let reader = response
        .body_mut()
        .with_config()
        .limit(MAX_MAP_BYTES)
        .reader();
    read_counted(reader, total, on_step).map_err(|e| crate::messages::labeled(url, e))
}

/// Reads `reader` to the end, reporting each chunk as it arrives.
fn read_counted(
    mut reader: impl Read,
    total: Option<u64>,
    on_step: &mut dyn FnMut(FetchStep),
) -> std::io::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(total.unwrap_or(0).min(MAX_MAP_BYTES) as usize);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(body),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        body.extend_from_slice(&buf[..n]);
        on_step(FetchStep::Downloaded {
            chunk: n as u64,
            total,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn the_url_mirrors_the_path_under_dod() {
        assert_eq!(
            map_url(DEFAULT_MIRROR, "dod_railyard_s9a").unwrap(),
            "https://fastdl.ktpdod.com/dod/maps/dod_railyard_s9a.bsp"
        );
    }

    #[test]
    fn a_trailing_slash_on_the_mirror_does_not_double_up() {
        assert_eq!(
            map_url("https://example.com/dod/", "dod_anzio").unwrap(),
            "https://example.com/dod/maps/dod_anzio.bsp"
        );
    }

    #[test]
    fn a_map_name_that_could_escape_the_url_is_refused() {
        // The name comes out of a demo header, which is a file anyone can hand
        // over. It reaches a URL and a file path, so it is checked at both.
        assert!(map_url(DEFAULT_MIRROR, "../../etc/passwd").is_err());
        assert!(map_url(DEFAULT_MIRROR, "dod anzio").is_err());
        assert!(map_url(DEFAULT_MIRROR, "").is_err());
    }

    #[test]
    fn a_mirror_that_is_not_https_is_refused() {
        // The file lands in the folder the engine loads maps from, and a demo
        // that records no checksum has nothing else standing behind it.
        assert!(map_url("http://fastdl.ktpdod.com/dod/", "dod_anzio").is_err());
    }

    #[test]
    fn reading_a_download_reports_every_chunk_and_keeps_every_byte() {
        let data: Vec<u8> = (0..200_000u32).map(|i| i as u8).collect();
        let mut steps = Vec::new();
        let body = read_counted(
            std::io::Cursor::new(data.clone()),
            Some(200_000),
            &mut |s| steps.push(s),
        )
        .unwrap();
        assert_eq!(body, data);
        let mut sum = 0;
        for s in &steps {
            match s {
                FetchStep::Downloaded { chunk, total } => {
                    assert_eq!(*total, Some(200_000));
                    sum += chunk;
                }
                FetchStep::Checking => panic!("reading never reports the check"),
            }
        }
        assert_eq!(sum, 200_000);
        assert!(steps.len() >= 4, "64 KB chunks: {}", steps.len());
    }

    #[test]
    fn a_map_already_present_and_already_correct_is_left_alone() {
        // No download, no rename, and no backup file left behind — the common
        // case must be free and must not touch the library.
        let dir = Scratch::new("map_fetch");

        // A map whose checksum we can state without a real BSP: an empty lump
        // table checksums to the CRC's initial value.
        let mut bytes = vec![0u8; 4 + 15 * 8];
        bytes[0] = 30;
        let sum = bsp::map_checksum(&bytes).unwrap();
        std::fs::write(dir.join("dod_test.bsp"), &bytes).unwrap();

        let outcome = fetch_map("dod_test", Some(sum), &dir, DEFAULT_MIRROR).unwrap();
        assert!(outcome.already_correct);
        assert_eq!(outcome.bytes, 0);
        assert!(outcome.replaced.is_none());
    }
}
