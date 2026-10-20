//! Players tab, "Add a crosshair file…" (#842): copies a crosshair sheet the
//! user made (Vertex's crosshair creator gives a `customxhair.spr`, alone or
//! in a ZIP with a preview, a README and a `.cfg`) into the install's
//! `<game>_addon\sprites\` as `customxhair-<name>.spr`, so several coexist and
//! the crosshair picker lists them.
//!
//! The addon folder, never `<game>\`: Studio's own files go where `-addons`
//! (which Studio passes) finds them first, and the game's folder stays the
//! user's (#783). An existing file is never replaced: a name already taken in
//! either folder is refused, and the file is opened `create_new`.
//!
//! The `customxhair-` prefix is the one `goldsrc-hooks`' texture hook already
//! knows as a crosshair sheet, so the HD path leaves these stock too.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::player_overrides::{addon_of, decode_sprite, list_sprites};

/// Every imported sheet's file name starts with this.
pub const PREFIX: &str = "customxhair-";
/// The file name the creator exports, and the one taken from a ZIP that holds
/// more than one sprite.
pub const CREATOR_FILE: &str = "customxhair.spr";
/// The longest name kept, before the prefix and extension.
pub const MAX_NAME_CHARS: usize = 40;
/// A picked file larger than this is not read: a 256x256 sheet is ~66 KB, a
/// ZIP of it with a preview image a few hundred.
const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
/// A sprite larger than this is refused, whether picked or inside a ZIP.
const MAX_SPRITE_BYTES: u64 = 4 * 1024 * 1024;

/// The part of `raw` kept for a file name: lower-case ASCII letters, digits,
/// `-` and `_`; spaces, dots and other punctuation become one `-`; other
/// characters are dropped. A trailing `.spr` and a leading `customxhair` are
/// dropped too (the file name adds both back), and the result is at most
/// [`MAX_NAME_CHARS`].
pub fn sanitize_name(raw: &str) -> String {
    let raw = raw.trim();
    let raw = if raw.to_ascii_lowercase().ends_with(".spr") {
        &raw[..raw.len() - 4]
    } else {
        raw
    };
    let mut out = String::new();
    for c in raw.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if (c == '-' || c.is_ascii()) && !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut name = out.as_str();
    loop {
        let trimmed = name.trim_matches(['-', '_']);
        match trimmed.strip_prefix("customxhair") {
            Some(rest) => name = rest,
            None => {
                name = trimmed;
                break;
            }
        }
    }
    let mut name: String = name.chars().take(MAX_NAME_CHARS).collect();
    while name.ends_with(['-', '_']) {
        name.pop();
    }
    name
}

/// `customxhair-<name>.spr`, or an error when nothing of `name` is usable.
pub fn file_name_for(name: &str) -> Result<String, String> {
    let name = sanitize_name(name);
    if name.is_empty() {
        return Err("type a name for the crosshair (letters or numbers)".into());
    }
    Ok(format!("{PREFIX}{name}.spr"))
}

/// Whether `bytes` are a Half-Life sprite the crosshair can use: `IDSP`,
/// version 2, no bigger than [`MAX_SPRITE_BYTES`], and a first frame that
/// reads to the end (the same check the picker's preview makes).
pub fn validate_sprite(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_SPRITE_BYTES {
        return Err(format!(
            "the sprite is {} KB, too big for a crosshair",
            bytes.len() / 1024
        ));
    }
    decode_sprite(bytes).map(|_| ())
}

fn base_name(entry: &str) -> &str {
    entry.rsplit(['/', '\\']).next().unwrap_or(entry)
}

/// Whether a ZIP entry is a sprite worth taking: a `.spr`, not a macOS
/// resource-fork copy (`__MACOSX/`, `._name`).
fn is_sprite_entry(entry: &str) -> bool {
    let base = base_name(entry);
    base.to_ascii_lowercase().ends_with(".spr")
        && !base.starts_with("._")
        && !entry.starts_with("__MACOSX")
}

/// Which of a ZIP's sprite entries to take: the creator's `customxhair.spr`,
/// else the only one. Several others are refused with their names, so the
/// user can unzip and pick one.
fn pick_entry(sprites: &[String]) -> Result<&str, String> {
    if let Some(e) = sprites
        .iter()
        .find(|e| base_name(e).eq_ignore_ascii_case(CREATOR_FILE))
    {
        return Ok(e);
    }
    match sprites {
        [] => Err("the ZIP holds no crosshair (.spr) file".into()),
        [only] => Ok(only),
        many => Err(format!(
            "the ZIP holds {} sprites ({}): unzip it and add the one you want",
            many.len(),
            many.join(", ")
        )),
    }
}

fn sprite_from_zip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("the ZIP can't be read: {e}"))?;
    let sprites: Vec<String> = zip
        .file_names()
        .filter(|n| is_sprite_entry(n))
        .map(str::to_string)
        .collect();
    let name = pick_entry(&sprites)?.to_string();
    let entry = zip
        .by_name(&name)
        .map_err(|e| format!("{name} in the ZIP can't be read: {e}"))?;
    if entry.size() > MAX_SPRITE_BYTES {
        return Err(format!("{name} is too big for a crosshair"));
    }
    // The header's size can lie: never read past the limit whatever it says.
    let mut out = Vec::new();
    entry
        .take(MAX_SPRITE_BYTES + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("{name} in the ZIP can't be read: {e}"))?;
    validate_sprite(&out).map_err(|e| format!("{name} in the ZIP: {e}"))?;
    Ok(out)
}

/// The sprite in a picked file: the file itself when it is one, the
/// sprite inside when it is a ZIP. Validated either way.
pub fn extract_sprite(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return sprite_from_zip(bytes);
    }
    validate_sprite(bytes)?;
    Ok(bytes.to_vec())
}

/// Copies the sprite in `source` (a `.spr` or a `.zip`) into
/// `<game_dir>_addon\sprites\customxhair-<name>.spr` and returns the file
/// name. `game_dir` is `<hl.exe's folder>\<game folder>`. Refuses a name
/// either sprites folder already has, and never replaces a file.
pub fn import_crosshair(game_dir: &Path, source: &Path, name: &str) -> Result<String, String> {
    let file = file_name_for(name)?;
    let taken = |file: &str| {
        list_sprites(game_dir)
            .iter()
            .any(|n| n.eq_ignore_ascii_case(file))
    };
    let already = || format!("there's already a crosshair called {file}: pick another name");
    if taken(&file) {
        return Err(already());
    }
    let size = std::fs::metadata(source)
        .map_err(|e| format!("{}: {e}", source.display()))?
        .len();
    if size > MAX_SOURCE_BYTES {
        return Err(format!(
            "{} is {} MB, too big to be a crosshair file",
            source.display(),
            size / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    let sprite = extract_sprite(&bytes).map_err(|e| format!("{}: {e}", source.display()))?;
    let dir = addon_of(game_dir).join("sprites");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let path = dir.join(&file);
    let mut out = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(already()),
        Err(e) => return Err(format!("could not create {}: {e}", path.display())),
    };
    if let Err(e) = out.write_all(&sprite).and_then(|()| out.flush()) {
        drop(out);
        let _ = std::fs::remove_file(&path);
        return Err(format!("could not write {}: {e}", path.display()));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A 256x256 one-frame sheet, the creator's layout.
    fn sheet() -> Vec<u8> {
        let (w, h) = (256i32, 256i32);
        let mut out = b"IDSP".to_vec();
        for v in [2, 2, 3] {
            out.extend_from_slice(&i32::to_le_bytes(v));
        }
        out.extend_from_slice(&0f32.to_le_bytes());
        for v in [w, h, 1] {
            out.extend_from_slice(&i32::to_le_bytes(v));
        }
        out.extend_from_slice(&0f32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&256u16.to_le_bytes());
        out.extend(std::iter::repeat_n(0u8, 768));
        out.extend_from_slice(&0i32.to_le_bytes());
        for v in [-w / 2, h / 2, w, h] {
            out.extend_from_slice(&i32::to_le_bytes(v));
        }
        out.extend(std::iter::repeat_n(255u8, (w * h) as usize));
        out
    }

    fn zip_of(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let method = if deflate {
                zip::CompressionMethod::Deflated
            } else {
                zip::CompressionMethod::Stored
            };
            let options = zip::write::SimpleFileOptions::default().compression_method(method);
            for (name, bytes) in files {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("Red Dot"), "red-dot");
        assert_eq!(sanitize_name("  dicE[: :]m00cat :D "), "dice-m00cat-d");
        assert_eq!(sanitize_name("customxhair (3)"), "3");
        assert_eq!(sanitize_name("customxhair"), "");
        assert_eq!(sanitize_name("customXHair-blue_v2"), "blue_v2");
        assert_eq!(sanitize_name("..\\..\\evil/name"), "evil-name");
        assert_eq!(sanitize_name("crème brûlée"), "crme-brle");
        assert_eq!(sanitize_name("---"), "");
        assert_eq!(sanitize_name(&"x".repeat(100)).len(), MAX_NAME_CHARS);
        assert_eq!(file_name_for("Red Dot").unwrap(), "customxhair-red-dot.spr");
        assert!(file_name_for("  ").is_err());
        assert!(file_name_for("customxhair.spr").is_err());
    }

    /// The JS copy (`crosshair_file.js`) is tested against the same cases.
    #[test]
    fn names_match_the_frontend_cases() {
        for (raw, want) in [
            ("customxhair.spr", ""),
            ("customxhair-pack.zip", "pack-zip"),
            ("My Crosshair", "my-crosshair"),
            ("__a__", "a"),
        ] {
            assert_eq!(sanitize_name(raw), want, "{raw}");
        }
    }

    #[test]
    fn takes_a_sprite_alone_or_from_a_zip() {
        let spr = sheet();
        assert_eq!(extract_sprite(&spr).unwrap(), spr);
        for deflate in [false, true] {
            let zip = zip_of(
                &[
                    ("preview.png", b"\x89PNG"),
                    ("README.txt", b"hi"),
                    ("customxhair.cfg", b"bind x y"),
                    ("customxhair.spr", &spr),
                ],
                deflate,
            );
            assert_eq!(extract_sprite(&zip).unwrap(), spr, "deflate {deflate}");
        }
        // In a folder, beside a macOS resource fork.
        let zip = zip_of(
            &[
                ("__MACOSX/pack/._mine.spr", b"junk"),
                ("pack/mine.spr", &spr),
            ],
            true,
        );
        assert_eq!(extract_sprite(&zip).unwrap(), spr);
    }

    #[test]
    fn a_zip_with_several_sprites_takes_the_creators_or_refuses() {
        let spr = sheet();
        let zip = zip_of(&[("a.spr", &spr), ("CustomXHair.spr", &spr)], false);
        assert!(extract_sprite(&zip).is_ok());
        let zip = zip_of(&[("a.spr", &spr), ("b.spr", &spr)], false);
        let err = extract_sprite(&zip).unwrap_err();
        assert!(err.contains("a.spr, b.spr"), "{err}");
        let zip = zip_of(&[("README.txt", b"hi")], false);
        assert!(extract_sprite(&zip).unwrap_err().contains("no crosshair"));
    }

    #[test]
    fn bad_files_are_errors_not_panics() {
        let spr = sheet();
        for len in [0, 3, 4, 8, 40, 100, spr.len() - 1] {
            assert!(extract_sprite(&spr[..len]).is_err(), "{len}");
        }
        assert!(extract_sprite(b"\x89PNG\r\n\x1a\n").is_err());
        assert!(extract_sprite(b"PK\x03\x04garbage").is_err());
        let zip = zip_of(&[("customxhair.spr", b"IDSP\x02\0\0\0")], true);
        assert!(extract_sprite(&zip).is_err());
        let mut v1 = spr.clone();
        v1[4] = 1;
        assert!(extract_sprite(&v1).is_err());
        let huge = vec![0u8; MAX_SPRITE_BYTES as usize + 1];
        assert!(validate_sprite(&huge).unwrap_err().contains("too big"));
    }

    #[test]
    fn copies_into_the_addon_folder_and_never_overwrites() {
        let dir = Scratch::new("crosshair_import");
        let game = dir.join("dod");
        std::fs::create_dir_all(game.join("sprites")).unwrap();
        std::fs::write(game.join("sprites").join("customxhair-taken.spr"), b"x").unwrap();
        let source = dir.join("customxhair (2).zip");
        std::fs::write(&source, zip_of(&[("customxhair.spr", &sheet())], true)).unwrap();

        let file = import_crosshair(&game, &source, "Red Dot").unwrap();
        assert_eq!(file, "customxhair-red-dot.spr");
        let written = dir.join("dod_addon").join("sprites").join(&file);
        assert_eq!(std::fs::read(&written).unwrap(), sheet());
        assert!(!game.join("sprites").join(&file).exists());
        // The picker lists it.
        assert!(list_sprites(&game).contains(&file));

        // Taken in either folder, whatever the case: refused, nothing changed.
        let err = import_crosshair(&game, &source, "RED dot").unwrap_err();
        assert!(err.contains("already"), "{err}");
        let err = import_crosshair(&game, &source, "taken").unwrap_err();
        assert!(err.contains("already"), "{err}");
        assert_eq!(
            std::fs::read(game.join("sprites").join("customxhair-taken.spr")).unwrap(),
            b"x"
        );

        // A bad file writes nothing.
        let bad = dir.join("bad.spr");
        std::fs::write(&bad, b"not a sprite").unwrap();
        assert!(import_crosshair(&game, &bad, "bad").is_err());
        assert!(!dir.join("dod_addon/sprites/customxhair-bad.spr").exists());
    }
}
