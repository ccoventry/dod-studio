use super::*;
use crate::test_support::Scratch;

/// Per-process, so two `cargo test` runs on one machine don't clear each
/// other's folders mid-test.
fn scratch(name: &str) -> Scratch {
    Scratch::new(format!("hlae_ffmpeg_{name}"))
}

/// An HLAE install: the exe, and the `ffmpeg` folder its readme lives in.
/// Hold the `Scratch` for the whole test; dropping it removes the install.
fn install(name: &str) -> (Scratch, PathBuf) {
    let root = scratch(name);
    std::fs::create_dir_all(root.join(FFMPEG_DIR)).expect("ffmpeg dir");
    let exe = root.join("HLAE.exe");
    std::fs::write(&exe, b"").expect("exe");
    (root, exe)
}

fn a_real_ffmpeg(dir: &Path) -> PathBuf {
    let exe = dir.join("ffmpeg.exe");
    std::fs::write(&exe, b"").expect("ffmpeg");
    exe
}

#[test]
fn the_working_install_shape_is_reported_as_missing() {
    // What is actually on this machine: an ffmpeg folder holding nothing
    // but the readme. It looks set up and is not.
    let (_dir, hlae) = install("missing");
    std::fs::write(
        hlae.parent()
            .unwrap()
            .join(FFMPEG_DIR)
            .join("readme.advancedfx.txt"),
        b"install ffmpeg here",
    )
    .expect("readme");

    let state = detect(&hlae);
    assert!(matches!(state, HlaeFfmpeg::Missing { .. }), "{:?}", state);
    assert!(!state.is_usable());
    assert!(
        state.can_link(),
        "this is exactly the case worth offering to fix"
    );
}

#[test]
fn an_ini_written_before_the_rename_is_still_ours() {
    // #357: `link` wrote "Written by dod-tools" before #260; such a file
    // must still be offered for re-linking.
    assert!(authored_by_us(
        "; Written by dod-tools: points HLAE at FFmpeg\n[Ffmpeg]\nPath=C:\\x\\ffmpeg.exe\n"
    ));
    assert!(authored_by_us("; Written by dod-studio\n"));
    assert!(!authored_by_us("[Ffmpeg]\nPath=C:\\x\\ffmpeg.exe\n"));
}

#[test]
fn a_bundled_binary_needs_nothing_and_is_never_offered_an_ini() {
    let (_dir, hlae) = install("bundled");
    let bin = hlae.parent().unwrap().join(FFMPEG_DIR).join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::write(bin.join("ffmpeg.exe"), b"").expect("exe");

    let state = detect(&hlae);
    assert!(matches!(state, HlaeFfmpeg::Bundled { .. }), "{:?}", state);
    assert!(state.is_usable());
    assert!(!state.can_link());
}

#[test]
fn a_bundled_binary_wins_over_an_ini() {
    // HLAE finds the binary without consulting the ini, so reporting the
    // ini would describe something that has no effect on anything.
    let (_dir, hlae) = install("both");
    let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
    let bin = folder.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::write(bin.join("ffmpeg.exe"), b"").expect("exe");
    std::fs::write(
        folder.join(INI_NAME),
        "[Ffmpeg]\nPath=C:\\elsewhere\\ffmpeg.exe\n",
    )
    .expect("ini");

    assert!(matches!(detect(&hlae), HlaeFfmpeg::Bundled { .. }));
}

#[test]
fn a_stale_ini_reads_as_linked_but_not_usable() {
    // The case a "configured / not configured" boolean would hide: HLAE has
    // an answer, and the answer is wrong.
    let (_dir, hlae) = install("stale");
    let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
    std::fs::write(
        folder.join(INI_NAME),
        "[Ffmpeg]\nPath=C:\\gone\\ffmpeg.exe\n",
    )
    .expect("ini");

    match detect(&hlae) {
        HlaeFfmpeg::Linked { target_exists, .. } => assert!(!target_exists),
        other => panic!("{:?}", other),
    }
    assert!(!detect(&hlae).is_usable());
    assert!(
        !detect(&hlae).can_link(),
        "an existing ini is reported, never replaced"
    );
}

#[test]
fn linking_writes_an_ini_hlae_can_read_back() {
    let (_dir, hlae) = install("link");
    let ffmpeg = a_real_ffmpeg(hlae.parent().unwrap());

    let ini = write_link(&hlae, &ffmpeg).expect("link");
    assert!(ini.is_file());

    match detect(&hlae) {
        HlaeFfmpeg::Linked {
            target,
            target_exists,
            ..
        } => {
            assert!(target_exists);
            assert_eq!(target, ffmpeg);
        }
        other => panic!("{:?}", other),
    }
    assert!(detect(&hlae).is_usable());
}

#[test]
fn an_existing_ini_is_refused_not_overwritten() {
    // HLAE is shared with Source work. Repointing it silently would break
    // somebody else's workflow to fix ours.
    let (_dir, hlae) = install("refuse");
    let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
    let original = "[Ffmpeg]\nPath=D:\\someone-elses\\ffmpeg.exe\n";
    std::fs::write(folder.join(INI_NAME), original).expect("ini");
    let ffmpeg = a_real_ffmpeg(hlae.parent().unwrap());

    let err = write_link(&hlae, &ffmpeg).expect_err("must refuse");
    assert!(matches!(err, LinkError::AlreadyLinked { .. }), "{:?}", err);
    assert_eq!(
        std::fs::read_to_string(folder.join(INI_NAME)).unwrap(),
        original,
        "the existing file was modified"
    );
}

#[test]
fn linking_refuses_an_ffmpeg_that_is_not_there() {
    let (_dir, hlae) = install("no_ffmpeg");
    let err = link(&hlae, Path::new("C:/nowhere/ffmpeg.exe")).expect_err("must refuse");
    assert!(matches!(err, LinkError::NoSuchFfmpeg { .. }), "{:?}", err);
}

#[test]
fn a_path_whose_folder_exists_but_whose_exe_does_not_is_no_install() {
    // `hlae_path` names HLAE.exe, not the folder — the same convention
    // `game_path` follows, and the same one-level-too-high trap. Checking
    // only the parent would accept a typo'd exe name, or the folder typed
    // in place of the exe, and then offer to write an ini into somewhere
    // that is not an HLAE install.
    let root = scratch("folder_given");
    assert_eq!(detect(&root.join("HLAE.exe")), HlaeFfmpeg::NoInstall);
    assert_eq!(detect(&root), HlaeFfmpeg::NoInstall, "the folder itself");
    assert!(!detect(&root).can_link());
}

#[test]
fn an_ini_we_cannot_make_sense_of_reads_as_no_answer() {
    for body in [
        "",
        "; just a comment\n",
        "[Ffmpeg]\n",
        "nonsense\n",
        "[Ffmpeg]\nPath=\n",
    ] {
        assert_eq!(parse_ini_path(body), None, "parsed {:?}", body);
    }
}

#[test]
fn the_ini_format_is_read_the_way_the_readme_writes_it() {
    assert_eq!(
        parse_ini_path("[Ffmpeg]\nPath=C:\\Users\\x\\ffmpeg\\bin\\ffmpeg.exe\n"),
        Some(PathBuf::from("C:\\Users\\x\\ffmpeg\\bin\\ffmpeg.exe"))
    );
    // Tolerating what a human might reasonably have typed instead.
    assert_eq!(
        parse_ini_path("[Ffmpeg]\n  path = \"C:\\a\\ffmpeg.exe\"  \n"),
        Some(PathBuf::from("C:\\a\\ffmpeg.exe"))
    );
}

#[test]
fn a_configured_path_that_exists_resolves_to_itself() {
    let dir = scratch("resolve");
    let ffmpeg = a_real_ffmpeg(&dir);
    let got = resolve_absolute(&ffmpeg.to_string_lossy()).expect("resolve");
    assert!(got.is_file());
}

#[test]
fn a_configured_path_that_is_wrong_is_not_quietly_swapped_for_another() {
    // Falling back to a PATH search here would hand HLAE a different binary
    // than the one Render Studio was told to use.
    assert_eq!(resolve_absolute("C:/nowhere/at/all/ffmpeg.exe"), None);
    assert_eq!(resolve_absolute("   "), None);
}

#[test]
fn a_path_is_quoted_so_powershell_expands_nothing_in_it() {
    // These end up inside a script this code generates, and they are
    // user-supplied paths. Single quotes stop PowerShell expanding `$`,
    // backticks or anything else, and doubling is the only escape needed.
    assert_eq!(
        ps_literal(Path::new(r"C:\Program Files (x86)\HLAE")),
        r"'C:\Program Files (x86)\HLAE'"
    );
    assert_eq!(
        ps_literal(Path::new(r"C:\it's\$env:PATH")),
        r"'C:\it''s\$env:PATH'"
    );
}

#[test]
fn elevation_still_refuses_everything_a_plain_link_refuses() {
    // Elevation buys permission to write, not permission to clobber. If it
    // skipped these checks, the never-overwrite rule would have a hole in
    // it that only opens on the machines where the button is most useful.
    let (_dir, hlae) = install("elevated_refuse");
    let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
    let original = "[Ffmpeg]\nPath=D:\\someone-elses\\ffmpeg.exe\n";
    std::fs::write(folder.join(INI_NAME), original).expect("ini");

    // A real FFmpeg, so the refusal under test is the ini rule and not the
    // is-it-FFmpeg check that now runs before it. Skipped where none is
    // installed rather than asserting about the environment.
    let Some(ffmpeg) = resolve_absolute("ffmpeg") else {
        eprintln!("no ffmpeg on PATH; skipping");
        return;
    };

    let err = link_elevated(&hlae, &ffmpeg).expect_err("must refuse");
    assert!(matches!(err, LinkError::AlreadyLinked { .. }), "{:?}", err);
    assert_eq!(
        std::fs::read_to_string(folder.join(INI_NAME)).unwrap(),
        original,
        "no UAC prompt should have been raised, and nothing written"
    );
}

#[test]
fn a_wrong_executable_is_refused_before_any_prompt_or_write() {
    // The ordering matters: raising a UAC prompt to install a program that
    // cannot record would be worse than not offering at all, and the ini
    // must be untouched either way.
    let (_dir, hlae) = install("elevated_not_ffmpeg");
    let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
    let not_ffmpeg = a_real_ffmpeg(hlae.parent().unwrap()); // a zero-byte stand-in

    for err in [
        link(&hlae, &not_ffmpeg).expect_err("plain must refuse"),
        link_elevated(&hlae, &not_ffmpeg).expect_err("elevated must refuse"),
    ] {
        assert!(matches!(err, LinkError::NotFfmpeg { .. }), "{:?}", err);
    }
    assert!(
        !folder.join(INI_NAME).exists(),
        "nothing should have been written"
    );
}

#[test]
fn elevation_refuses_an_ffmpeg_that_is_not_there_before_prompting() {
    // No point raising a UAC prompt to copy a file that does not exist.
    let (_dir, hlae) = install("elevated_no_ffmpeg");
    let err = link_elevated(&hlae, Path::new("C:/nowhere/ffmpeg.exe")).expect_err("refuse");
    assert!(matches!(err, LinkError::NoSuchFfmpeg { .. }), "{:?}", err);
}

#[test]
fn a_canonical_path_loses_the_extended_length_prefix() {
    // `canonicalize` always adds `\\?\`. It is a correct path and not a
    // universally accepted one — it turns off the normalisation a lot of
    // software assumes — and this value gets handed to HLAE to spawn a
    // process with, so keeping it risks a silent launch failure.
    assert_eq!(
        strip_extended_prefix(Path::new(r"\\?\C:\Program Files (x86)\FFmpeg\ffmpeg.exe")),
        PathBuf::from(r"C:\Program Files (x86)\FFmpeg\ffmpeg.exe")
    );
    // The network form maps back to a plain UNC path.
    assert_eq!(
        strip_extended_prefix(Path::new(r"\\?\UNC\server\share\ffmpeg.exe")),
        PathBuf::from(r"\\server\share\ffmpeg.exe")
    );
    // Anything else is left exactly as it is.
    assert_eq!(
        strip_extended_prefix(Path::new(r"C:\plain\ffmpeg.exe")),
        PathBuf::from(r"C:\plain\ffmpeg.exe")
    );
}

#[test]
fn resolving_never_hands_back_an_extended_length_path() {
    // The end-to-end version of the above: whatever `canonicalize` does,
    // what reaches the ini has to be a path HLAE will accept.
    let dir = scratch("resolve_prefix");
    let ffmpeg = a_real_ffmpeg(&dir);
    let got = resolve_absolute(&ffmpeg.to_string_lossy()).expect("resolve");
    assert!(
        !got.to_string_lossy().starts_with(r"\?\"),
        "{} still carries the prefix",
        got.display()
    );
}

#[test]
fn our_own_ini_is_rewritten_rather_than_treated_as_untouchable() {
    // Without this the first link is permanent: change the app's FFmpeg and
    // HLAE stays pointed at the old one, with no way back through the UI and
    // a folder that usually needs administrator rights to edit by hand.
    let (_dir, hlae) = install("relink");
    let first = a_real_ffmpeg(hlae.parent().unwrap());
    write_link(&hlae, &first).expect("first link");
    assert!(
        detect(&hlae).can_link(),
        "our own file must stay correctable"
    );

    let second_dir = hlae.parent().unwrap().join("other");
    std::fs::create_dir_all(&second_dir).expect("dir");
    let second = a_real_ffmpeg(&second_dir);
    write_link(&hlae, &second).expect("relink");

    match detect(&hlae) {
        HlaeFfmpeg::Linked { target, ours, .. } => {
            assert_eq!(target, second);
            assert!(ours);
        }
        other => panic!("{:?}", other),
    }
}

#[test]
fn an_empty_or_contentless_ini_is_replaced_rather_than_protected() {
    // The rule protects a *configuration*, not a filename. A file with no
    // `Path=` in it expresses nothing, so refusing to write would strand
    // somebody behind a file that does nothing — and there is no data in it
    // to lose. A zero-length file is the case that actually turns up, from
    // an interrupted write or a hand-created placeholder.
    for body in ["", "\n\n", "; notes to self\n", "[Ffmpeg]\n"] {
        let (_dir, hlae) = install("contentless");
        let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
        std::fs::write(folder.join(INI_NAME), body).expect("ini");

        assert!(
            detect(&hlae).can_link(),
            "an ini containing {:?} should not be treated as somebody's setup",
            body
        );
        let ffmpeg = a_real_ffmpeg(hlae.parent().unwrap());
        write_link(&hlae, &ffmpeg).expect("should write over a file with nothing in it");
        assert_eq!(read_target(&hlae), Some(ffmpeg));
    }
}

fn read_target(hlae: &Path) -> Option<PathBuf> {
    match detect(hlae) {
        HlaeFfmpeg::Linked { target, .. } => Some(target),
        _ => None,
    }
}

#[test]
fn a_file_without_our_header_is_still_untouchable() {
    // The distinction the rewrite rule rests on. Somebody else's file is
    // left alone however much we would like to correct it.
    let (_dir, hlae) = install("not_ours");
    let folder = hlae.parent().unwrap().join(FFMPEG_DIR);
    let theirs = "[Ffmpeg]\nPath=D:\\theirs\\ffmpeg.exe\n";
    std::fs::write(folder.join(INI_NAME), theirs).expect("ini");

    match detect(&hlae) {
        HlaeFfmpeg::Linked { ours, .. } => assert!(!ours),
        other => panic!("{:?}", other),
    }
    assert!(!detect(&hlae).can_link());

    let ffmpeg = a_real_ffmpeg(hlae.parent().unwrap());
    assert!(matches!(
        write_link(&hlae, &ffmpeg).expect_err("must refuse"),
        LinkError::AlreadyLinked { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(folder.join(INI_NAME)).unwrap(),
        theirs
    );
}

#[test]
fn a_file_that_is_not_a_program_is_not_ffmpeg() {
    let dir = scratch("verify_junk");
    let fake = dir.join("ffmpeg.exe");
    std::fs::write(&fake, b"not actually a program").expect("write");
    // Named exactly right, and still not FFmpeg — which is the whole reason
    // the name is not the test.
    assert!(verify_is_ffmpeg(&fake).is_err());
    assert!(verify_is_ffmpeg(&dir.join("absent.exe")).is_err());
}

#[test]
fn a_program_not_named_ffmpeg_is_refused_without_running_it() {
    let dir = scratch("verify_not_named");
    // A real program would start if it were run: this one is a stand-in,
    // so the test is that the refusal names it and never gets to running.
    for name in ["HLAE.exe", "ffplay.exe", "setup.exe"] {
        let exe = dir.join(name);
        std::fs::write(&exe, b"MZ").expect("write");
        let why = verify_is_ffmpeg(&exe).expect_err(name);
        assert!(why.contains(name), "{why}");
        assert!(why.contains("wasn't started"), "{why}");
    }
    assert!(named_like_ffmpeg(Path::new("C:/tools/ffmpeg.exe")));
    assert!(named_like_ffmpeg(Path::new("C:/tools/FFmpeg-7.1.exe")));
    assert!(!named_like_ffmpeg(Path::new("C:/tools/ffprobe.exe")));
}

#[test]
fn a_real_ffmpeg_identifies_itself() {
    // Skipped rather than failed where FFmpeg is not installed: this asserts
    // about the environment, not the code, and CI need not have one.
    let Some(ffmpeg) = resolve_absolute("ffmpeg") else {
        eprintln!("no ffmpeg on PATH; skipping");
        return;
    };
    let banner = verify_is_ffmpeg(&ffmpeg).expect("the real thing must pass");
    assert!(
        banner.to_lowercase().starts_with("ffmpeg version"),
        "{}",
        banner
    );
}

#[test]
fn a_sibling_ffmpeg_tool_is_rejected_by_name_of_what_it_actually_is() {
    // The mistake that prompted this: ffplay.exe sits in the same folder as
    // ffmpeg.exe and is one click away in a picker. Both exist; only one can
    // record. Skipped where the tools are not installed.
    let Some(ffmpeg) = resolve_absolute("ffmpeg") else {
        eprintln!("no ffmpeg on PATH; skipping");
        return;
    };
    let ffplay = ffmpeg.with_file_name("ffplay.exe");
    if !ffplay.is_file() {
        eprintln!("no ffplay beside ffmpeg; skipping");
        return;
    }
    let why = verify_is_ffmpeg(&ffplay).expect_err("ffplay cannot record");
    assert!(
        why.to_lowercase().contains("ffplay"),
        "the message should name what it actually found: {}",
        why
    );
}

#[test]
fn an_install_carrying_the_hook_dll_has_nothing_to_say() {
    let (_dir, hlae) = install("hook_present");
    std::fs::write(hlae.parent().unwrap().join(HOOK_DLL), b"").expect("dll");
    assert_eq!(missing_hook_dll(&hlae), None);
}

#[test]
fn an_executable_with_no_hook_dll_beside_it_is_reported() {
    // The wrong exe picked in a file dialog, or a folder that is not an
    // HLAE install at all.
    let (_dir, hlae) = install("hook_absent");
    let missing = missing_hook_dll(&hlae).expect("nothing beside it");
    assert!(missing.ends_with(HOOK_DLL), "{}", missing.display());
}

#[test]
fn a_real_hlae_missing_its_dll_is_still_reported() {
    // What metadata could not catch: the executable is genuine and the
    // install is broken anyway, because antivirus quarantined the DLL or
    // somebody deleted it. The capture would fail either way, so the check
    // that matters is whether the file the pipeline passes is there.
    let (_dir, hlae) = install("hook_quarantined");
    let dll = hlae.parent().unwrap().join(HOOK_DLL);
    std::fs::write(&dll, b"").expect("dll");
    assert_eq!(missing_hook_dll(&hlae), None);

    std::fs::remove_file(&dll).expect("quarantine it");
    assert_eq!(missing_hook_dll(&hlae), Some(dll));
}
