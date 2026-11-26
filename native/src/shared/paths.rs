use lock_recover::{read_or_recover, write_or_recover};
use std::path::{Path, PathBuf};

/// Name of this app's directory under `%APPDATA%`.
pub const APPDATA_DIR_NAME: &str = "dod-studio";

/// What it was called before #257. Migrated from, never written to.
pub const LEGACY_APPDATA_DIR_NAME: &str = "dod-tools";

// A tripwire against the obvious way this breaks. Both pairs below exist only
// so the *old* spelling stays readable after #257, and both were briefly
// clobbered by the tree-wide `dod-tools` -> `dod-studio` sweep that renamed
// everything else -- silently, because a legacy constant equal to the current
// one just makes the compatibility path a no-op. It is not a runtime check
// because there is no runtime in which it could ever be true.
const _: () = {
    assert!(
        !const_str_eq(APPDATA_DIR_NAME, LEGACY_APPDATA_DIR_NAME),
        "LEGACY_APPDATA_DIR_NAME must keep the pre-#257 spelling -- equal to the current one, the migration silently does nothing"
    );
    assert!(
        !const_str_eq(PREVIEW_SIDECAR_EXT, LEGACY_PREVIEW_SIDECAR_EXT),
        "LEGACY_PREVIEW_SIDECAR_EXT must keep the pre-#257 spelling -- equal to the current one, existing previews become unrecognisable"
    );
};

/// `==` on `&str` is not const, and these have to be compared at compile time.
const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `%APPDATA%\dod-studio`, created if absent.
///
/// Deliberately does no migration of its own. This is called from logging, and
/// logging is reachable from inside a migration, so a self-migrating getter
/// would be a recursion waiting to happen. See [`migrate_legacy_appdata_dir`],
/// which the app calls once, explicitly, before anything reads settings.
pub fn get_appdata_dir() -> PathBuf {
    let path = appdata_base().join(APPDATA_DIR_NAME);
    let _ = std::fs::create_dir_all(&path);
    path
}

fn appdata_base() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// `%LOCALAPPDATA%\dod-studio`, created if absent: files that belong to this
/// PC and have no reason to travel with a roaming Windows profile, such as
/// the hook DLL copies the game launches from (#1146).
pub fn get_local_appdata_dir() -> PathBuf {
    let path = local_appdata_base().join(APPDATA_DIR_NAME);
    let _ = std::fs::create_dir_all(&path);
    path
}

fn local_appdata_base() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(appdata_base)
}

/// What a migration run did, for the caller to log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AppDataMigration {
    /// Entries moved out of the legacy directory.
    pub moved: Vec<String>,
    /// Entries left behind because the new directory already had that name.
    /// Never overwritten -- whatever is already in the new location wins.
    pub skipped: Vec<String>,
    /// Entries that could not be moved, with the reason.
    pub failed: Vec<(String, String)>,
    /// Whether the now-empty legacy directory was removed.
    pub legacy_removed: bool,
}

impl AppDataMigration {
    pub fn did_something(&self) -> bool {
        !self.moved.is_empty() || !self.skipped.is_empty() || !self.failed.is_empty()
    }
}

/// Moves `%APPDATA%\dod-tools`'s contents into `%APPDATA%\dod-studio` (#257).
///
/// Moves **entry by entry** rather than renaming the directory, which matters
/// more than it looks: any binary that logs -- `preview_cli`, a test, the
/// companion DLL -- creates `dod-studio/logs` the first time it runs. A
/// whole-directory rename would see the destination already exists, decline,
/// and strand `settings.json` in the old folder forever. Per-entry means the
/// order things happen to run in cannot lose the user's settings.
///
/// An entry that already exists at the destination and is not itself a
/// directory colliding with a directory is left alone, so this is safe to
/// call repeatedly and never overwrites newer state with older.
pub fn migrate_legacy_appdata_dir() -> AppDataMigration {
    let base = appdata_base();
    migrate_appdata_dir_between(
        &base.join(LEGACY_APPDATA_DIR_NAME),
        &base.join(APPDATA_DIR_NAME),
    )
}

fn migrate_appdata_dir_between(legacy: &Path, current: &Path) -> AppDataMigration {
    let mut report = AppDataMigration::default();
    if !legacy.is_dir() || legacy == current {
        return report;
    }
    if std::fs::create_dir_all(current).is_err() {
        return report;
    }

    merge_dir_entries(legacy, current, "", &mut report);

    // Only ever removes an empty directory -- `remove_dir` fails rather than
    // recursing, so anything that could not be moved is still there afterwards.
    report.legacy_removed = std::fs::remove_dir(legacy).is_ok();
    report
}

/// Moves every entry of `source` into `destination`, recursing into a
/// subdirectory that collides with one already at the destination instead of
/// skipping it wholesale.
///
/// `logs/` is the collision that motivated this: the companion DLL computes
/// `%APPDATA%\dod-studio\logs` directly, independent of this migration, so the
/// very first time it runs it creates that folder on its own -- almost always
/// before this migration gets a turn. A plain top-level skip then treats the
/// whole `logs` entry as "already present" and strands the user's entire log
/// history (potentially months of it) in the old location permanently. A file
/// colliding with a file is still a plain skip; nothing here overwrites
/// existing content, only merges past a directory that happens to already
/// exist.
fn merge_dir_entries(
    source: &Path,
    destination: &Path,
    prefix: &str,
    report: &mut AppDataMigration,
) {
    let Ok(entries) = std::fs::read_dir(source) else {
        return;
    };

    for entry in entries.flatten() {
        let entry_name = entry.file_name().to_string_lossy().into_owned();
        let name = if prefix.is_empty() {
            entry_name.clone()
        } else {
            format!("{prefix}/{entry_name}")
        };
        let source_path = entry.path();
        let destination_path = destination.join(&entry_name);

        if !destination_path.exists() {
            match std::fs::rename(&source_path, &destination_path) {
                Ok(()) => report.moved.push(name),
                Err(e) => report.failed.push((name, e.to_string())),
            }
            continue;
        }

        // `file_type` never follows a junction, so a link in the old folder
        // is skipped like a file collision, never merged through (#376).
        if entry.file_type().is_ok_and(|t| t.is_dir()) && destination_path.is_dir() {
            merge_dir_entries(&source_path, &destination_path, &name, report);
            // Only succeeds once the merge above has moved everything out --
            // anything left behind (a skipped file-vs-file collision) keeps
            // this directory around too, the same "nothing lost silently"
            // guarantee the top-level legacy dir gets.
            let _ = std::fs::remove_dir(&source_path);
        } else {
            report.skipped.push(name);
        }
    }
}

/// Extension of the hidden sidecar that marks a `_preview.dem` as this app's.
pub const PREVIEW_SIDECAR_EXT: &str = "dodstudio_preview";

/// The pre-#257 spelling. Read, never written.
///
/// #257 renamed the console commands outright, on the grounds that no released
/// build ever carried the old spelling. These sidecars are different: they are
/// **on disk already**, in the user's own demo folders, and the sidecar is the
/// only thing marking its `_preview.dem` as ours. A scan taught only the new
/// name would leave every existing preview permanently unrecognisable -- both
/// invisible to the app's own cleanup and indistinguishable from a demo the
/// user named that way themselves. That is exactly the orphaning #218/#250
/// fixed, and renaming carelessly would reintroduce it wholesale.
const LEGACY_PREVIEW_SIDECAR_EXT: &str = "dodtools_preview";

/// The sidecar beside `demo` in whichever spelling it uses, if one exists.
///
/// New sidecars are always written as [`PREVIEW_SIDECAR_EXT`]; this is only
/// about reading what is already there.
pub fn preview_sidecar(demo: &Path) -> Option<PathBuf> {
    for ext in [PREVIEW_SIDECAR_EXT, LEGACY_PREVIEW_SIDECAR_EXT] {
        let candidate = demo.with_extension(ext);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Stem of the primer demo `build_batch_queue` writes into the game folder
/// ([`mod_dir`]) when the batch launches without the hook DLL (#365). The
/// cleanup still removes one left by such a batch. The `dodstudio_` prefix matches every other file this app writes
/// there (`dodstudio_helper.cfg`, `dodstudio_capture_done.cfg`) and keeps the
/// name out of the space a player's own demo could plausibly occupy (#197).
pub const PRIMER_DEMO_STEM: &str = "dodstudio_primer";

/// Prefix of every patched chain demo `build_batch_queue` writes into the game
/// folder, followed by a zero-padded job number. See [`PRIMER_DEMO_STEM`] for
/// why the `dodstudio_` part is there.
pub const CHAIN_DEMO_PREFIX: &str = "dodstudio_chain_";

/// True for exactly the filenames `build_batch_queue` gives patched chain
/// demos (`dodstudio_chain_01.dem`, `dodstudio_chain_9999.dem`, ...). No cap on
/// digit count -- a batch of over a hundred thousand demos is implausible, but
/// nothing here assumes an upper bound either. A plain
/// `starts_with(CHAIN_DEMO_PREFIX)` would also match a source demo that
/// happens to share the prefix -- requiring the rest of the name to be all
/// digits rules that out.
///
/// Bare `chain_NN.dem` names, which this app wrote before #197, are
/// deliberately *not* matched: the whole point of the prefix is that a name
/// without it might be the user's own demo, and leaving a stale file behind is
/// the strictly safer failure than deleting someone's recording.
pub fn is_chain_demo_filename(filename: &str) -> bool {
    filename
        .strip_prefix(CHAIN_DEMO_PREFIX)
        .and_then(|rest| rest.strip_suffix(".dem"))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Stable identity for one capture take, shared by the capture and render
/// pipelines so they can correlate without either knowing about the other.
///
/// Takes land at `<capture_dir>/<session_id>/dodstudio_chain_JJ_bN/`, so the
/// key is normally the last two path components lowercased:
/// `session_20260818_142233/dodstudio_chain_01_b0`. Deliberately *not* the absolute
/// path — capture output routinely gets moved onto a different drive before
/// rendering, which would invalidate it — and deliberately not the take name
/// alone, which repeats every batch.
///
/// HLAE's `mirv_movie` plugin auto-numbers each recording into a `take0000`,
/// `take0001`, ... subfolder *under* the block folder (`hlcr::scanner`'s own
/// `is_renderable_take` has to account for the same nesting) — Render
/// Studio's real folder scanner finds takes at that nested path, not the
/// block folder itself, so a trailing `take*` component is skipped to keep
/// both sides keying off the same block folder regardless of which literal
/// path was passed in.
pub fn take_key(take_folder: &Path) -> Option<String> {
    let is_take_number_folder = take_folder
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase().starts_with("take"))
        .unwrap_or(false);
    let block_folder = if is_take_number_folder {
        take_folder.parent()?
    } else {
        take_folder
    };
    let take_name = block_folder.file_name()?.to_string_lossy().to_lowercase();
    let session = block_folder
        .parent()?
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    Some(format!("{}/{}", session, take_name))
}

/// The game folder a launch names with `-game` when the user has set none
/// (#402).
pub const DEFAULT_GAME_FOLDER: &str = "dod";

/// The user's game folder, empty for [`DEFAULT_GAME_FOLDER`] (#402).
///
/// Process-wide, not passed down, because most of the paths it decides are
/// built from nothing but an `hl.exe` path, deep in code (map checks, HD
/// builds, overviews, Blender's map list) that has no settings at hand.
/// Studio sets it when settings load and on every save
/// ([`set_game_folder`]); everything else (the CLIs, tests) gets `dod`.
/// Read on every call rather than cached by callers, so a changed setting
/// applies to the next launch, scan or build.
static GAME_FOLDER: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Why `name` cannot be a game folder, or `Ok` when it can: a single folder
/// name beside `hl.exe`, the `<mod>` of `-game <mod>` (#402). Anything with a
/// path separator, a drive colon, `..` or a character Windows refuses in a
/// file name would build paths outside the install, or a launch line the
/// engine reads differently from the folder we write to.
pub fn validate_game_folder(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(
            "The game folder is empty: it is the folder beside hl.exe, such as dod".to_string(),
        );
    }
    if name != name.trim() {
        return Err(format!(
            "The game folder \"{name}\" starts or ends with a space; the engine would not find it"
        ));
    }
    if name == "." || name == ".." || name.contains("..") {
        return Err(format!(
            "The game folder \"{name}\" contains \"..\": give the name of a folder beside hl.exe"
        ));
    }
    let bad = |c: char| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || c.is_control()
            || c.is_whitespace()
    };
    if let Some(c) = name.chars().find(|&c| bad(c)) {
        return Err(format!(
            "The game folder \"{name}\" contains \"{c}\": give just the name of a folder beside hl.exe, such as dod"
        ));
    }
    if name.starts_with(['-', '+']) {
        return Err(format!(
            "The game folder \"{name}\" starts with \"{}\", which the engine reads as another launch option",
            &name[..1]
        ));
    }
    Ok(())
}

/// Sets the game folder for the rest of this process (#402). A blank `name`
/// restores [`DEFAULT_GAME_FOLDER`]; an invalid one is refused and the old
/// value stays.
pub fn set_game_folder(name: &str) -> Result<(), String> {
    let name = name.trim();
    if !name.is_empty() {
        validate_game_folder(name)?;
    }
    let mut guard = write_or_recover(&GAME_FOLDER);
    *guard = name.to_string();
    Ok(())
}

/// The game folder's name: the `<mod>` in `-game <mod>`, `dod` by default
/// (#402).
pub fn game_folder() -> String {
    let guard = read_or_recover(&GAME_FOLDER);
    if guard.is_empty() {
        DEFAULT_GAME_FOLDER.to_string()
    } else {
        guard.clone()
    }
}

/// `<game_root>\<game folder>`, where the game reads its configs, maps and
/// demos, and where a batch writes its helper `.cfg` and chain demos (#402).
/// `game_root` is the folder holding `hl.exe`.
pub fn mod_dir(game_root: &Path) -> PathBuf {
    game_root.join(game_folder())
}

/// [`mod_dir`] for the folder holding `hl_exe`, `None` when it has no parent.
pub fn mod_dir_for_exe(hl_exe: &Path) -> Option<PathBuf> {
    hl_exe.parent().map(mod_dir)
}

/// `<game_root>\<game folder>_addon`: the engine searches it ahead of the game
/// folder with `-addons`, and DoD Studio's own UI, HD and overview files go
/// there (#408, #415). The engine names it from the game folder, so a
/// `dod_movies` install's is `dod_movies_addon` (#402).
pub fn addon_dir(game_root: &Path) -> PathBuf {
    game_root.join(format!("{}_addon", game_folder()))
}

/// `<game_root>\<game folder>_downloads`, where the engine saves what it
/// downloads from servers (maps, sprays). Named from the game folder like
/// [`addon_dir`] (#402).
pub fn downloads_dir(game_root: &Path) -> PathBuf {
    game_root.join(format!("{}_downloads", game_folder()))
}

/// The folders beside `hl.exe` (`game_root`) that hold a `liblist.gam` and a
/// `cl_dlls/client.dll`, the files that make a folder a mod `-game` can
/// start, sorted by name. The Game folder setting offers these (#402).
/// `_addon`/`_downloads` siblings have no `liblist.gam`; the movie installs'
/// `valve` has one but no `client.dll`, and starting it ends in the engine's
/// "could not load library" error (#768).
pub fn list_game_folders(game_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(game_root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| {
            let dir = e.path();
            dir.join("liblist.gam").is_file() && dir.join("cl_dlls").join("client.dll").is_file()
        })
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|name| validate_game_folder(name).is_ok())
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// A game folder copy that lacks the text file VGUI2 needs (#925).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MissingLanguageFile {
    pub folder: String,
    pub language: String,
    /// `<folder>_<language>.txt`, the file `resource\` needs.
    pub needed: String,
    /// The file in the folder's own `resource\` to rename to `needed`: its
    /// `dod_<language>.txt`, else its `dod_english.txt`. `None` when it has
    /// neither, and the file has to come from the `dod` folder.
    pub rename_from: Option<String>,
    /// What Explorer opens on for the user to fix it: the file to rename,
    /// selected in `resource\`; else `resource\` itself; else the folder.
    pub show: String,
}

/// What a copy of the `dod` folder needs so its in-game text isn't raw
/// `#Dod_…` keys: VGUI2 loads `resource\<game folder>_<language>.txt`, and a
/// copy only has `dod_<language>.txt` (#925). `None` for `dod` itself and for
/// a folder that already has the file. Only looks; Studio never writes the
/// user's folder.
pub fn missing_language_file(
    game_root: &Path,
    folder: &str,
    language: &str,
) -> Option<MissingLanguageFile> {
    if folder.eq_ignore_ascii_case(DEFAULT_GAME_FOLDER) {
        return None;
    }
    let resource = game_root.join(folder).join("resource");
    let needed = format!("{folder}_{language}.txt");
    if resource.join(&needed).is_file() {
        return None;
    }
    let rename_from = [
        format!("{DEFAULT_GAME_FOLDER}_{language}.txt"),
        format!("{DEFAULT_GAME_FOLDER}_english.txt"),
    ]
    .into_iter()
    .find(|name| resource.join(name).is_file());
    let show = match &rename_from {
        Some(name) => resource.join(name),
        None if resource.is_dir() => resource.clone(),
        None => game_root.join(folder),
    };
    Some(MissingLanguageFile {
        folder: folder.to_string(),
        language: language.to_string(),
        needed,
        rename_from,
        show: show.display().to_string(),
    })
}

/// Deletes the engine's `qconsole.log`.
///
/// `-condebug` writes it to `hl.exe`'s own folder, not the mod folder, so
/// cleanup that named `dod/qconsole.log` never matched anything and the log
/// accumulated across every session indefinitely. (`condump` is the separate
/// console command that drops a numbered `condump_NNN.txt`; nothing here
/// issues it.)
///
/// Beside `hl.exe` specifically, not in the launching process's working
/// directory: `build_hlae_process` runs HLAE with its CWD set to HLAE's own
/// folder, and the log still appears next to `hl.exe` with nothing in HLAE's
/// folder. So deriving the path from the game path is always right.
pub fn console_log_path(game_root: &Path) -> PathBuf {
    game_root.join("qconsole.log")
}

/// Best-effort removal of the console log, for callers outside a capture
/// batch. `clear_capture_scratch` does not go through this -- it needs the
/// retry-and-report treatment every other scratch file there gets -- but both
/// resolve the path through [`console_log_path`], so neither can drift from
/// the other on *where* the log lives.
pub fn remove_console_log(game_root: &Path) {
    let _ = std::fs::remove_file(console_log_path(game_root));
}

/// Every scratch file a capture batch leaves in the game folder, cleared
/// according to the four auto-clear settings.
///
/// `game_root` is the folder holding `hl.exe`; the batch writes into it and
/// into its game folder ([`mod_dir`], `dod/` by default). Nothing here is fatal — a capture that finished
/// should not fail because a leftover could not be deleted — but nothing here
/// is silent either: every removal goes through `remove_scratch_file`, which
/// retries and then reports a file it could not delete against the checkbox
/// that asked for it. See #218.
///
/// This is one function because it used to be two verbatim copies, in
/// `CaptureCleanupGuard::drop` (capture_engine.rs) and `WorkspaceGuard::drop`
/// (patch/builder.rs). Both guards run for the same batch and clear the same
/// files; adding a scratch file to one and forgetting the other left it in the
/// user's game folder with no error and no log line. Add new scratch filenames
/// here and both paths get them.
pub fn clear_capture_scratch(
    game_root: &Path,
    auto_clear_logs: bool,
    auto_clear_temp_demos: bool,
    auto_clear_previews: bool,
    save_local_patched_copy: bool,
) {
    let dod_dir = mod_dir(game_root);

    if auto_clear_logs {
        remove_scratch_file(&console_log_path(game_root), "Auto-clear Logs");
        remove_scratch_file(&dod_dir.join("dodstudio_helper.cfg"), "Auto-clear Logs");
        remove_scratch_file(
            &dod_dir.join("dodstudio_capture_done.cfg"),
            "Auto-clear Logs",
        );
        remove_scratch_file(&dod_dir.join("dod_quit.cfg"), "Auto-clear Logs");
        // Legacy only: nothing has written a per-chain cfg since the helper cfg
        // absorbed those aliases. Kept so a user upgrading from a build that
        // did write them still gets them cleared, not because anything current
        // produces one.
        if let Ok(entries) = std::fs::read_dir(&dod_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if filename.starts_with("dodstudio_chain_") && filename.ends_with(".cfg") {
                    remove_scratch_file(&entry.path(), "Auto-clear Logs");
                }
            }
        }
    }

    if auto_clear_temp_demos && !save_local_patched_copy {
        // Retried, and logged loudly on final failure -- unlike the best-effort
        // removals above, the last demo in a batch can still have hl.exe's file
        // handle attached here (see #198's investigation), and a leftover chain
        // file after auto-clear went completely unnoticed the first time this
        // happened.
        let primer_demo = format!("{PRIMER_DEMO_STEM}.dem");
        if let Some(e) = remove_file_retrying(&dod_dir.join(&primer_demo)) {
            crate::log_markdown(&crate::messages::cleanup_primer_demo_failed(
                &primer_demo,
                e,
            ));
        }
        if let Ok(entries) = std::fs::read_dir(&dod_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if is_chain_demo_filename(&filename) {
                    remove_scratch_file(&entry.path(), "Auto-clear Temp Demos");
                }
            }
        }
    }

    if auto_clear_previews {
        // Only a `_preview.dem` with its hidden `.dodstudio_preview` sidecar is
        // ours. A demo the user named that way themselves has no sidecar and is
        // left alone.
        for scan_dir in [dod_dir.clone(), game_root.to_path_buf()] {
            let Ok(entries) = std::fs::read_dir(scan_dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !filename.ends_with("_preview.dem") {
                    continue;
                }
                let Some(sidecar) = preview_sidecar(&path) else {
                    continue;
                };
                // Order matters, and the sidecar goes second on purpose. The
                // sidecar is the *only* thing marking this demo as ours; drop
                // it while the demo itself is still on disk and the demo can
                // never be recognised for cleanup again, so it would sit in
                // the game folder forever. Removing it only once the demo is
                // actually gone means a failure here is retried next batch
                // instead of becoming permanent.
                if remove_scratch_file(&path, "Auto-clear Previews") {
                    remove_scratch_file(&sidecar, "Auto-clear Previews");
                }
            }
        }
    }
}

/// Removes one scratch file, retrying, and says so in the app log if it still
/// could not be removed. Returns whether the file is gone.
///
/// Every removal here used to be a bare `let _ = std::fs::remove_file(..)`,
/// which is doubly silent: the result is discarded, and the `log::warn!` the
/// surrounding code reached for has no registered backend in this app. #198
/// fixed that for the temp demos only. The same silence applies to every other
/// file cleared here for a reason that has nothing to do with hl.exe -- a
/// `qconsole.log` the user has open in a text editor to read is the obvious
/// one -- and the symptom is identical: the checkbox is on, the file is still
/// there, and nothing anywhere says why.
///
/// `setting` names the auto-clear checkbox that asked for this, so the log line
/// points at the control the user would go looking for.
fn remove_scratch_file(path: &Path, setting: &str) -> bool {
    match remove_file_retrying(path) {
        None => true,
        Some(e) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            crate::log_markdown(&crate::messages::cleanup_scratch_file_failed(
                &name, e, setting,
            ));
            false
        }
    }
}

/// Where `dodstudio_goldsrc_hooks.dll` lives if nothing overrides it:
/// `studio/src-tauri/tauri.conf.json` maps this crate's release build to
/// `goldsrc-hooks/dodstudio_goldsrc_hooks.dll` among `bundle.resources`, and
/// Tauri v2's NSIS installer puts those beside the app's own executable
/// (Tauri's `resource_dir`). Installs made before that kept them under a
/// `resources` folder there, so that spot is checked second. Looking only
/// in `resources` found no DLL in a current install, which went unnoticed
/// while the override was set; once installed releases ignored the override
/// (#912) the game launched without the hook. In a dev build
/// (`npm run tauri dev`) neither exists -- Studio -> Configuration -> Paths'
/// override points at `target/i686-pc-windows-msvc/release/` there. Either
/// way, `build_hlae_process` treats a missing file as "not installed" and
/// doesn't inject it, same as any other optional path.
pub fn default_goldsrc_hooks_dll_path() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    Some(bundled_hooks_dll_in(&dir))
}

/// The bundled DLL under an install folder: the first spot that has it, else
/// the current layout's (so a message names where it should be).
fn bundled_hooks_dll_in(install_dir: &std::path::Path) -> std::path::PathBuf {
    let current = install_dir
        .join("goldsrc-hooks")
        .join("dodstudio_goldsrc_hooks.dll");
    let older = install_dir
        .join("resources")
        .join("goldsrc-hooks")
        .join("dodstudio_goldsrc_hooks.dll");
    if !current.exists() && older.exists() {
        older
    } else {
        current
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;

/// Deletes a file, retrying a few times with a short pause between attempts.
///
/// `taskkill /F` returning does not mean hl.exe's open file handles are
/// released yet, and even confirming the process itself has left the process
/// list (`sysinfo`) does not guarantee it either -- kernel object cleanup can
/// lag a beat past both. The demo files hl.exe was just playing
/// (`dodstudio_primer.dem`, `dodstudio_chain_NN.dem`) are what this exists for,
/// and both cleanup guards (`CaptureCleanupGuard` in `capture_engine.rs`,
/// `WorkspaceGuard` in `patch/builder.rs`) call it for them. See #198.
///
/// #218 asked whether the sibling auto-clear settings share that race. They do
/// not, and the answer is worth recording because it is not guessable from the
/// Rust side:
///
/// * `qconsole.log` is **not** held open across a session. `Con_DebugLog` in
///   `hw.dll` does `_open(O_WRONLY|O_CREAT|O_APPEND)` / `_write` / `_close`
///   once per console line, so outside the microseconds of a single line
///   there is no handle to wait on at all.
/// * The `.cfg` files are read whole and released; nothing keeps them open.
/// * `_preview.dem` files belong to a separate preview session, not to the
///   capture batch whose guard runs this.
///
/// Every scratch file goes through this anyway -- `remove_scratch_file` is a
/// thin reporting wrapper over it. When the file is not locked, which per the
/// above is the overwhelmingly common case, the first attempt succeeds and it
/// costs nothing, and it means one uniform path instead of a fast one and a
/// careful one that can drift apart.
///
/// Returns the last error seen, or `None` on success or if the file was
/// already gone -- callers decide how loudly to report a real failure; this
/// only decides how hard to try first.
pub fn remove_file_retrying(path: &Path) -> Option<std::io::Error> {
    const ATTEMPTS: u32 = 5;
    const DELAY: std::time::Duration = std::time::Duration::from_millis(200);
    let mut last_err = None;
    for attempt in 1..=ATTEMPTS {
        match std::fs::remove_file(path) {
            Ok(()) => return None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                last_err = Some(e);
                if attempt < ATTEMPTS {
                    std::thread::sleep(DELAY);
                }
            }
        }
    }
    last_err
}

#[cfg(test)]
#[path = "paths_auto_clear_previews_tests.rs"]
mod auto_clear_previews_tests;

#[cfg(test)]
#[path = "paths_remove_file_retrying_tests.rs"]
mod remove_file_retrying_tests;

#[cfg(test)]
#[path = "paths_appdata_migration_tests.rs"]
mod appdata_migration_tests;

#[cfg(test)]
#[path = "paths_bundled_hooks_dll_tests.rs"]
mod bundled_hooks_dll_tests;
