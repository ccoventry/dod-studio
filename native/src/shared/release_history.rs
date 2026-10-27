//! Earlier versions' release notes for Studio's What's new window (#1013):
//! GitHub's release list, kept in the app's data folder so a second open, or
//! one with no network, still shows what the last successful read found.
//! The notes themselves are a build product (`.github/scripts/release_notes.py`);
//! the app only reads them.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The newest 30 releases, stable and experimental, newest first.
pub const RELEASES_URL: &str =
    "https://api.github.com/repos/ccoventry/dod-studio/releases?per_page=30";

/// The cache's file name in the app's data folder.
pub const CACHE_FILE: &str = "release_notes_cache.json";

/// One published release, as the What's new window shows it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Release {
    /// `v0.2.0`, or `experimental-latest` for the experimental channel's build.
    pub tag: String,
    /// `DoD Studio v0.2.0`, `DoD Studio 0.2.1-10 (experimental)`.
    pub name: String,
    pub prerelease: bool,
    /// RFC 3339, as GitHub gives it.
    pub published_at: Option<String>,
    /// The release body: the notes `release_notes.py` wrote, in Markdown.
    pub body: String,
}

/// What the window gets: the releases, and whether they came from the cache
/// because GitHub couldn't be read (`error` says why).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct History {
    pub releases: Vec<Release>,
    pub from_cache: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    published_at: Option<String>,
    body: Option<String>,
}

/// GitHub's `/releases` reply, drafts left out (only the repo's owner sees
/// them, and they aren't out).
pub fn parse_github_releases(json: &str) -> Result<Vec<Release>, String> {
    let list: Vec<GithubRelease> = serde_json::from_str(json)
        .map_err(|e| format!("GitHub's release list could not be read: {e}"))?;
    Ok(list
        .into_iter()
        .filter(|r| !r.draft)
        .map(|r| Release {
            name: r
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| r.tag_name.clone()),
            tag: r.tag_name,
            prerelease: r.prerelease,
            published_at: r.published_at,
            body: r.body.unwrap_or_default(),
        })
        .collect())
}

/// The releases the last successful read saved in `dir`, or none when there
/// is no cache or it can't be read (it is only ever a copy of GitHub's).
pub fn read_cache(dir: &Path) -> Vec<Release> {
    std::fs::read_to_string(dir.join(CACHE_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Saves `releases` in `dir` for the next open.
pub fn write_cache(dir: &Path, releases: &[Release]) -> Result<(), String> {
    let path = dir.join(CACHE_FILE);
    let text = serde_json::to_string(releases).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&path, text))
        .map_err(|e| format!("Could not save {}: {e}", path.display()))
}

/// Reads `url`'s release list and caches it in `dir`; when the read fails,
/// hands back the cache with the reason. A failed cache write is not an
/// error: the window still shows what was read.
pub fn fetch_or_cached(url: &str, dir: &Path) -> History {
    match fetch(url) {
        Ok(releases) => {
            let _ = write_cache(dir, &releases);
            History {
                releases,
                from_cache: false,
                error: None,
            }
        }
        Err(error) => History {
            releases: read_cache(dir),
            from_cache: true,
            error: Some(error),
        },
    }
}

fn fetch(url: &str) -> Result<Vec<Release>, String> {
    let mut response = crate::utils::http::get(url)?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| crate::messages::labeled(url, e))?;
    parse_github_releases(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dodstudio-release-history-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const REPLY: &str = r###"[
      {"tag_name": "experimental-latest", "name": "DoD Studio 0.2.1-10 (experimental)", "prerelease": true, "draft": false,
       "published_at": "2026-10-26T05:10:11Z", "body": "## New\n\n- A thing (#1)", "assets": []},
      {"tag_name": "v0.2.1", "name": "", "prerelease": false, "draft": true, "published_at": null, "body": "draft"},
      {"tag_name": "v0.1.0", "name": null, "prerelease": false, "draft": false, "published_at": "2026-09-01T07:14:53Z", "body": null}
    ]"###;

    #[test]
    fn drafts_are_left_out_and_missing_names_and_bodies_filled() {
        let releases = parse_github_releases(REPLY).unwrap();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag, "experimental-latest");
        assert!(releases[0].prerelease);
        assert_eq!(releases[0].body, "## New\n\n- A thing (#1)");
        assert_eq!(releases[1].name, "v0.1.0");
        assert_eq!(releases[1].body, "");
    }

    #[test]
    fn a_reply_that_is_not_a_list_is_an_error_naming_github() {
        let err = parse_github_releases(r#"{"message": "API rate limit exceeded"}"#).unwrap_err();
        assert!(err.contains("GitHub"), "{err}");
    }

    #[test]
    fn the_cache_reads_back_what_was_written() {
        let dir = scratch("roundtrip");
        let releases = parse_github_releases(REPLY).unwrap();
        write_cache(&dir, &releases).unwrap();
        assert_eq!(read_cache(&dir), releases);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_cache_or_a_broken_one_reads_as_empty() {
        let dir = scratch("broken");
        assert!(read_cache(&dir).is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(CACHE_FILE), "{not json").unwrap();
        assert!(read_cache(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_read_falls_back_to_the_cache_with_the_reason() {
        let dir = scratch("fallback");
        let releases = parse_github_releases(REPLY).unwrap();
        write_cache(&dir, &releases).unwrap();
        let url = "http://127.0.0.1:1/nothing-listens-here";
        let history = fetch_or_cached(url, &dir);
        assert!(history.from_cache);
        assert_eq!(history.releases, releases);
        assert!(history.error.unwrap().contains(url));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
