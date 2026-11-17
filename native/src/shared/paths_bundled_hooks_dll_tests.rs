use super::bundled_hooks_dll_in;

fn install(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dodstudio_bundled_dll_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("install dir");
    dir
}

fn put(dir: &std::path::Path, rel: &[&str]) -> std::path::PathBuf {
    let path = rel.iter().fold(dir.to_path_buf(), |p, part| p.join(part));
    std::fs::create_dir_all(path.parent().unwrap()).expect("folders");
    std::fs::write(&path, b"").expect("dll");
    path
}

#[test]
fn a_current_install_finds_the_dll_beside_the_exe() {
    // Tauri v2's installer: resources beside the exe, no `resources` folder.
    let dir = install("current");
    let dll = put(&dir, &["goldsrc-hooks", "dodstudio_goldsrc_hooks.dll"]);
    assert_eq!(bundled_hooks_dll_in(&dir), dll);
}

#[test]
fn an_older_install_finds_it_under_resources() {
    let dir = install("older");
    let dll = put(
        &dir,
        &["resources", "goldsrc-hooks", "dodstudio_goldsrc_hooks.dll"],
    );
    assert_eq!(bundled_hooks_dll_in(&dir), dll);
}

#[test]
fn with_neither_it_names_the_current_spot() {
    let dir = install("neither");
    assert_eq!(
        bundled_hooks_dll_in(&dir),
        dir.join("goldsrc-hooks")
            .join("dodstudio_goldsrc_hooks.dll")
    );
}
