use super::*;
use hl_demo_auditor::find_duplicates_cached;

#[test]
fn the_audit_log_lines_say_what_ran_how_long_and_what_was_read() {
    assert_eq!(
        audit_start_log(&[], &["C:/".into(), "D:/".into()], false),
        "[audit] started: all drives (C:/ D:/)"
    );
    assert_eq!(
        audit_start_log(&["E:/demos".into()], &[], false),
        "[audit] started: 1 folder(s)"
    );
    assert_eq!(
        audit_start_log(&["E:/demos".into()], &[], true),
        "[audit] started: 1 folder(s), looking inside archives (.zip, .7z, .rar)"
    );
    let mut times = AuditTimes {
        cancelled: false,
        walk_secs: 5.04,
        key_secs: 1.55,
        demos: 3217,
        groups: 1007,
        copies: 1909,
        wasted_bytes: 129 * 1024 * 1024 * 1024,
        reads: hl_demo_auditor::Reads {
            heads: 2930,
            tails: 2919,
            fulls: 0,
            zips: 3,
            zip_demos: 2,
        },
        zips: None,
    };
    assert_eq!(
        audit_end_log(&times),
        "[audit] finished after 6.6 s (walk 5.0 s, keys 1.6 s): 3217 demo(s) checked, \
             1007 group(s) with copies, 1909 copies, 129.00 GB; opened 2930 start(s), 2919 end(s), \
             0 whole file(s), the rest remembered"
    );
    times.zips = Some((40, 120, 7));
    assert!(
        audit_end_log(&times).ends_with(
            "the rest remembered; looked inside 40 archive(s) holding 120 demo(s), opened 3 \
                 archive(s) and read 2 demo(s) in them in full, the rest remembered; 7 loose \
                 demo(s) backed up in an archive"
        ),
        "{}",
        audit_end_log(&times)
    );
    times.cancelled = true;
    assert_eq!(audit_end_log(&times), "[audit] cancelled after 6.6 s");
}

#[test]
fn skipped_zips_are_named_up_to_a_limit() {
    let few = skipped_log(&["Z:\\a.zip: invalid Zip archive".to_string()]);
    assert_eq!(few, ["[audit] skipped Z:\\a.zip: invalid Zip archive"]);
    let many: Vec<String> = (0..25).map(|i| format!("Z:\\{i}.zip: bad")).collect();
    let lines = skipped_log(&many);
    assert_eq!(lines.len(), SKIPPED_LINES + 1);
    assert_eq!(
        lines.last().unwrap(),
        "[audit] skipped 5 more archive(s) or archived demo(s) that couldn't be read"
    );
}

/// Writes a zip at `path` with `entries` (name, bytes, deflated?).
fn write_zip(path: &Path, entries: &[(&str, &[u8], bool)]) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes, deflate) in entries {
        let method = if *deflate {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        };
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// An archive is a backup (#929): a loose demo it holds is marked with
/// every archive entry holding it, in its group's row and in the list of
/// backed-up demos; a demo with no loose copy is in that list only, and
/// no archive entry is ever a row of a group.
#[test]
fn backed_up_demos_are_marked_and_never_offered_because_of_an_archive() {
    let s = Scratch::new("backups");
    let shared = demo_bytes(4);
    let alone = demo_bytes(5);
    let a = s.file("a.dem", &shared);
    let b = s.file("b.dem", &shared);
    let solo = s.file("solo.dem", &alone);
    let one = s.0.join("one.zip");
    let two = s.0.join("two.zip");
    write_zip(
        &one,
        &[("m.dem", &shared, true), ("solo.dem", &alone, true)],
    );
    write_zip(&two, &[("x/m.dem", &shared, false)]);
    let never = AtomicBool::new(false);
    let audit = find_duplicates_in(
        vec![a, b, solo],
        vec![one, two],
        &never,
        &None,
        &mut KeyCache::default(),
    );
    let (groups, backed_up) = audit_rows(audit.groups, &audit.backups, &KeepRules::default());
    let name = |p: &str| {
        Path::new(p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    let entries = |f: &AuditFile| -> Vec<String> {
        f.backups
            .iter()
            .map(|z| format!("{} {}", name(&z.zip), z.entry))
            .collect()
    };
    // a and b are a group of their own, as with no archive.
    assert_eq!(groups.len(), 1);
    let rows: Vec<(String, Vec<String>)> = groups[0]
        .files
        .iter()
        .map(|f| (name(&f.path), entries(f)))
        .collect();
    let both = vec!["one.zip m.dem".to_string(), "two.zip x/m.dem".into()];
    assert_eq!(
        rows,
        [
            ("a.dem".to_string(), both.clone()),
            ("b.dem".into(), both.clone())
        ]
    );
    let listed: Vec<(String, Vec<String>)> = backed_up
        .iter()
        .map(|f| (name(&f.path), entries(f)))
        .collect();
    assert_eq!(
        listed,
        [
            ("a.dem".to_string(), both.clone()),
            ("b.dem".into(), both),
            ("solo.dem".into(), vec!["one.zip solo.dem".into()]),
        ]
    );
    // Serialized for the frontend: `backups` only where there are some.
    let json = serde_json::to_value(&backed_up).unwrap();
    assert_eq!(json[2]["backups"][0]["entry"], "solo.dem");
    let plain = order_for_keeping(paths(&[r"Z:\x.dem"]), &KeepRules::default());
    assert!(
        serde_json::to_value(&plain).unwrap()[0]
            .get("backups")
            .is_none()
    );
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

/// A fresh folder for one test, removed when dropped.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("audit_compare_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Same size and first 64 KiB, so the audit groups them, but not the
/// same demo past that.
fn demo_bytes(tail: u8) -> Vec<u8> {
    let mut bytes = vec![7u8; COMPARE_CHUNK + 70_000];
    *bytes.last_mut().unwrap() = tail;
    bytes
}

#[test]
fn a_byte_for_byte_copy_can_be_deleted() {
    let s = Scratch::new("same");
    let keep = s.file("keep.dem", &demo_bytes(1));
    let copy = s.file("copy.dem", &demo_bytes(1));
    assert_eq!(check_same_contents(&copy, &keep), Ok(()));
}

#[test]
fn a_copy_that_differs_past_the_key_is_not_deleted() {
    let s = Scratch::new("differs");
    let keep = s.file("keep.dem", &demo_bytes(1));
    let copy = s.file("copy.dem", &demo_bytes(2));
    let Err(Refusal::NotACopy(err)) = check_same_contents(&copy, &keep) else {
        panic!("a different file must be refused as not a copy");
    };
    let last = (COMPARE_CHUNK + 70_000 - 1).to_string();
    assert!(err.contains(&format!("differ from byte {last}")), "{err}");
    let failure = Refusal::NotACopy(err).into_failure("copy.dem".into());
    assert!(failure.not_a_copy);
}

#[test]
fn a_copy_of_another_size_is_not_deleted() {
    let s = Scratch::new("size");
    let keep = s.file("keep.dem", b"abcdef");
    let copy = s.file("copy.dem", b"abc");
    let Err(Refusal::NotACopy(err)) = check_same_contents(&copy, &keep) else {
        panic!("another size must be refused as not a copy");
    };
    assert!(err.contains("differ from byte 3"), "{err}");
}

/// A copy found different is remembered, and the next scan lists it
/// apart from the copy it was grouped with (#888).
#[test]
fn a_copy_found_different_is_not_grouped_again() {
    let s = Scratch::new("remember");
    let keep = s.file("keep.dem", &demo_bytes(1));
    let copy = s.file("copy.dem", &demo_bytes(1));
    // Same first and last bytes as keep: only the middle differs.
    let mut middle = demo_bytes(1);
    middle[COMPARE_CHUNK / 2] = 9;
    let odd = s.file("odd.dem", &middle);
    let files = vec![keep.clone(), copy.clone(), odd.clone()];
    let never = AtomicBool::new(false);
    let mut cache = KeyCache::default();
    let before = find_duplicates_cached(files.clone(), &never, &None, &mut cache);
    assert_eq!(before.groups.len(), 1);
    assert_eq!(before.groups[0].files.len(), 3, "the ends match");

    assert!(matches!(
        check_same_contents(&odd, &keep),
        Err(Refusal::NotACopy(_))
    ));
    remember_differing(&mut cache, &odd, &keep);
    let after = find_duplicates_cached(files, &never, &None, &mut cache);
    assert_eq!(after.groups.len(), 1);
    let names: Vec<_> = after.groups[0]
        .files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["copy.dem", "keep.dem"]);

    // Files of two sizes are never grouped: nothing to remember.
    let short = s.file("short.dem", b"abc");
    let mut untouched = KeyCache::default();
    remember_differing(&mut untouched, &short, &keep);
    assert!(!untouched.has_changes());
}

#[test]
fn the_kept_copy_itself_or_a_missing_one_is_never_a_reason_to_delete() {
    let s = Scratch::new("self");
    let keep = s.file("keep.dem", b"abc");
    let Err(Refusal::Other(err)) = check_same_contents(&keep, &keep) else {
        panic!("the kept file itself must be refused");
    };
    assert!(err.contains("is the copy being kept"), "{err}");
    let gone = s.0.join("gone.dem");
    let Err(Refusal::Other(err)) = check_same_contents(&keep, &gone) else {
        panic!("a missing kept copy must be refused");
    };
    assert!(err.contains("no longer there"), "{err}");
    assert!(!Refusal::Other(err).into_failure("x".into()).not_a_copy);
}

fn kept_order(files: &[&str], rules: &KeepRules) -> Vec<String> {
    order_for_keeping(paths(files), rules)
        .into_iter()
        .map(|f| f.path)
        .collect()
}

#[test]
fn the_shortest_path_is_kept_when_nothing_uses_a_copy() {
    let rules = KeepRules::default();
    assert_eq!(
        kept_order(
            &[
                r"F:\downloads\old\match.dem",
                r"E:\match.dem",
                r"D:\match.dem"
            ],
            &rules
        ),
        [
            r"D:\match.dem",
            r"E:\match.dem",
            r"F:\downloads\old\match.dem"
        ]
    );
}

#[test]
fn a_copy_in_the_game_folder_is_kept_over_a_shorter_one() {
    let rules = KeepRules::new(&paths(&[r"Z:\no-such-install\Half-Life\dod"]), &[]);
    let files = order_for_keeping(
        paths(&[
            r"F:\m.dem",
            r"\\?\Z:\No-Such-Install\Half-Life\DOD\demos\m.dem",
        ]),
        &rules,
    );
    assert_eq!(
        files[0],
        AuditFile {
            path: r"\\?\Z:\No-Such-Install\Half-Life\DOD\demos\m.dem".into(),
            in_use: Some(InUse::GameFolder),
            backups: Vec::new(),
        }
    );
    assert_eq!(files[1].in_use, None);
}

#[test]
fn every_install_the_copies_sit_in_is_a_game_folder() {
    let root =
        std::env::temp_dir().join(format!("dodstudio-audit-installs-{}", std::process::id()));
    let post = root.join("POST");
    let loose = root.join("loose");
    std::fs::create_dir_all(post.join("dod").join("sub")).unwrap();
    std::fs::create_dir_all(loose.join("dod")).unwrap();
    std::fs::write(post.join("hl.exe"), b"").unwrap();
    let files = [
        post.join("dod").join("sub").join("a.dem"),
        post.join("dod").join("b.dem"),
        // A folder named dod with no hl.exe beside it is not an install.
        loose.join("dod").join("c.dem"),
    ];
    let found = install_game_folders(files.iter());
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(found, vec![post.join("dod")]);
}

#[test]
fn a_folder_that_only_starts_the_same_is_not_the_game_folder() {
    let rules = KeepRules::new(&paths(&[r"Z:\hl\dod"]), &[]);
    let files = order_for_keeping(paths(&[r"Z:\hl\dod_downloads\m.dem"]), &rules);
    assert_eq!(files[0].in_use, None);
}

#[test]
fn a_queued_copy_is_kept_and_matches_its_canonical_form() {
    let rules = KeepRules::new(&[], &[r"Z:\projects\wsod\m.dem".to_string()]);
    let files = order_for_keeping(
        paths(&[r"\\?\A:\m.dem", r"\\?\Z:\Projects\WSOD\m.dem"]),
        &rules,
    );
    assert_eq!(files[0].path, r"\\?\Z:\Projects\WSOD\m.dem");
    assert_eq!(files[0].in_use, Some(InUse::Queue));
}

#[test]
fn copies_both_in_use_fall_back_to_the_shortest_path() {
    let rules = KeepRules::new(&paths(&[r"Z:\hl\dod"]), &[r"Y:\q\m.dem".to_string()]);
    let files = order_for_keeping(paths(&[r"Z:\hl\dod\m.dem", r"Y:\q\m.dem"]), &rules);
    assert_eq!(files[0].path, r"Y:\q\m.dem");
    assert_eq!(files[1].in_use, Some(InUse::GameFolder));
}

#[test]
fn roots_drop_blanks_and_repeats_and_add_the_drives() {
    let folders = [
        r"C:\Demos".to_string(),
        " ".to_string(),
        r"c:\demos\".to_string(),
    ];
    let drives = [r"C:\".to_string(), r"F:\".to_string()];
    assert_eq!(
        audit_roots(&folders, &drives),
        paths(&[r"C:\Demos", r"C:\", r"F:\"])
    );
}

#[test]
fn comparable_strips_the_verbatim_prefix_and_case() {
    assert_eq!(comparable(Path::new(r"\\?\C:\A\b.dem")), r"c:\a\b.dem");
    assert_eq!(
        comparable(Path::new(r"\\?\UNC\nas\d\x.dem")),
        r"\\nas\d\x.dem"
    );
    assert_eq!(comparable(Path::new("C:/a/")), r"c:\a");
}

#[test]
fn only_demo_files_are_deletable() {
    let dir = crate::test_support::Scratch::new("audit_deletable");
    let demo = dir.path().join("a.dem");
    let other = dir.path().join("a.txt");
    std::fs::write(&demo, b"x").unwrap();
    std::fs::write(&other, b"x").unwrap();
    assert!(check_deletable(&demo).is_ok());
    assert!(check_deletable(&other).is_err());
    assert!(check_deletable(dir.path()).is_err());
    let err = check_deletable(&dir.path().join("gone.dem")).unwrap_err();
    assert!(
        err.contains(&dir.path().join("gone.dem").display().to_string()),
        "{err}"
    );
}

#[test]
fn every_fixed_drive_root_is_a_folder() {
    for root in fixed_drive_roots() {
        assert!(Path::new(&root).is_dir(), "{root}");
    }
}
