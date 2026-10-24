//! Settings → "Show DoD Studio in Open With for .dem files" (#528).
//!
//! The user's demo handler is theirs: this never touches `.dem`'s default
//! value or Explorer's `UserChoice`. It adds DoD Studio to the Open With list
//! only, per user, with no installer involved, so it survives updates and
//! works the same for every install type:
//!
//! - `HKCU\Software\Classes\DoDStudio.Demo` (a ProgID whose open command runs
//!   this exe with the demo's path), and
//! - the value `DoDStudio.Demo` under `HKCU\Software\Classes\.dem\OpenWithProgids`.
//!
//! Turning it off deletes both, then `OpenWithProgids` and `.dem` only when
//! nothing else is left in them. The NSIS uninstaller removes the same two
//! entries when they point at the exe it is removing
//! (`windows/installer_hooks.nsh`).

use serde::Serialize;
use std::path::Path;

/// The ProgID registered for demos. The uninstall hook names it too.
pub const PROG_ID: &str = "DoDStudio.Demo";
const DEM_KEY: &str = ".dem";
const OPEN_WITH_KEY: &str = r".dem\OpenWithProgids";

/// What the setting shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemOpenWith {
    /// DoD Studio is in `.dem`'s Open With list.
    pub registered: bool,
    /// It opens this copy of DoD Studio (not, say, a dev build elsewhere).
    pub this_copy: bool,
    /// The exe it opens when that is another copy.
    pub other_exe: Option<String>,
}

/// The open command for demos: this exe, given the demo's path.
pub fn open_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// The exe an open command runs: its first, quoted, word.
fn exe_of_command(command: &str) -> String {
    match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(rest).to_string(),
        None => command.split(' ').next().unwrap_or(command).to_string(),
    }
}

#[cfg(windows)]
mod reg {
    use super::*;
    use std::io;
    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::{KEY_QUERY_VALUE, KEY_SET_VALUE, RegType};

    fn not_found(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::NotFound
    }

    /// Where it stands under `classes` (`HKCU\Software\Classes`, or a test
    /// key laid out the same way).
    pub fn status(classes: &RegKey, exe: &Path) -> DemOpenWith {
        let listed = classes
            .open_subkey(OPEN_WITH_KEY)
            .and_then(|k| k.get_raw_value(PROG_ID))
            .is_ok();
        let command: Option<String> = classes
            .open_subkey(format!(r"{PROG_ID}\shell\open\command"))
            .and_then(|k| k.get_value(""))
            .ok();
        let registered = listed && command.is_some();
        let this_copy = registered
            && command
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(&open_command(exe)));
        let other_exe = match (registered && !this_copy, command) {
            (true, Some(c)) => Some(exe_of_command(&c)),
            _ => None,
        };
        DemOpenWith {
            registered,
            this_copy,
            other_exe,
        }
    }

    /// Lists `exe` in `.dem`'s Open With, replacing an entry another copy made.
    pub fn register(classes: &RegKey, exe: &Path) -> io::Result<()> {
        let (prog, _) = classes.create_subkey(PROG_ID)?;
        prog.set_value("", &"Day of Defeat demo")?;
        let (icon, _) = prog.create_subkey("DefaultIcon")?;
        icon.set_value("", &format!("\"{}\",0", exe.display()))?;
        let (open, _) = prog.create_subkey(r"shell\open")?;
        open.set_value("FriendlyAppName", &"DoD Studio")?;
        let (command, _) = open.create_subkey("command")?;
        command.set_value("", &open_command(exe))?;
        // OpenWithProgids values are names only; REG_NONE, no data.
        let (list, _) = classes.create_subkey(OPEN_WITH_KEY)?;
        list.set_raw_value(
            PROG_ID,
            &RegValue {
                bytes: Vec::new(),
                vtype: RegType::REG_NONE,
            },
        )
    }

    /// Removes what [`register`] added, and `OpenWithProgids` and `.dem`
    /// when nothing else is in them.
    pub fn unregister(classes: &RegKey) -> io::Result<()> {
        match classes.delete_subkey_all(PROG_ID) {
            Err(e) if !not_found(&e) => return Err(e),
            _ => {}
        }
        match classes.open_subkey_with_flags(OPEN_WITH_KEY, KEY_SET_VALUE) {
            Ok(list) => match list.delete_value(PROG_ID) {
                Err(e) if !not_found(&e) => return Err(e),
                _ => {}
            },
            Err(e) if !not_found(&e) => return Err(e),
            Err(_) => {}
        }
        remove_if_empty(classes, OPEN_WITH_KEY)?;
        remove_if_empty(classes, DEM_KEY)
    }

    fn remove_if_empty(classes: &RegKey, path: &str) -> io::Result<()> {
        let key = match classes.open_subkey_with_flags(path, KEY_QUERY_VALUE) {
            Ok(key) => key,
            Err(e) if not_found(&e) => return Ok(()),
            Err(e) => return Err(e),
        };
        let info = key.query_info()?;
        drop(key);
        if info.sub_keys == 0 && info.values == 0 {
            classes.delete_subkey(path)?;
        }
        Ok(())
    }

    pub fn user_classes() -> io::Result<RegKey> {
        let hkcu = RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        hkcu.create_subkey(r"Software\Classes").map(|(k, _)| k)
    }

    /// Tells Explorer the Open With lists changed.
    pub fn notify_shell() {
        const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
        const SHCNF_IDLIST: u32 = 0;
        // Safety: no items are passed with SHCNE_ASSOCCHANGED.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHChangeNotify(
            event: i32,
            flags: u32,
            item1: *const std::ffi::c_void,
            item2: *const std::ffi::c_void,
        );
    }
}

#[tauri::command(async)]
pub fn dem_open_with_status() -> Result<DemOpenWith, String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Could not find DoD Studio's own exe: {e}"))?;
        let classes = reg::user_classes()
            .map_err(|e| format!("Could not open HKCU\\Software\\Classes: {e}"))?;
        Ok(reg::status(&classes, &exe))
    }
    #[cfg(not(windows))]
    Err("Open With for .dem files is Windows-only".to_string())
}

#[tauri::command(async)]
pub fn set_dem_open_with(enabled: bool) -> Result<DemOpenWith, String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Could not find DoD Studio's own exe: {e}"))?;
        let classes = reg::user_classes()
            .map_err(|e| format!("Could not open HKCU\\Software\\Classes: {e}"))?;
        let result = if enabled {
            reg::register(&classes, &exe)
                .map_err(|e| format!("Could not add DoD Studio to Open With for .dem files: {e}"))
        } else {
            reg::unregister(&classes).map_err(|e| {
                format!("Could not remove DoD Studio from Open With for .dem files: {e}")
            })
        };
        reg::notify_shell();
        result?;
        native::log_markdown(&format!(
            "📂 **Open With** — DoD Studio {} for .dem files ({})",
            if enabled { "listed" } else { "removed" },
            exe.display()
        ));
        Ok(reg::status(&classes, &exe))
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("Open With for .dem files is Windows-only".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_exe_and_the_demo() {
        let exe = Path::new(r"C:\Users\x\AppData\Local\DoD Studio\dod-studio.exe");
        assert_eq!(
            open_command(exe),
            r#""C:\Users\x\AppData\Local\DoD Studio\dod-studio.exe" "%1""#
        );
        assert_eq!(
            exe_of_command(&open_command(exe)),
            exe.display().to_string()
        );
        assert_eq!(exe_of_command(r"C:\a.exe %1"), r"C:\a.exe");
    }

    /// Every registry test runs under its own throwaway key in
    /// `HKCU\Software\DoDStudioTest`, deleted on drop, never under
    /// `Software\Classes`.
    #[cfg(windows)]
    mod registry {
        use super::super::reg;
        use super::*;
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        const TEST_PARENT: &str = r"Software\DoDStudioTest";

        struct TestRoot {
            name: String,
            key: RegKey,
        }

        impl TestRoot {
            fn new(tag: &str) -> Self {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let name = format!(r"{TEST_PARENT}\{tag}-{}-{nanos}", std::process::id());
                let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
                    .create_subkey(&name)
                    .expect("create the test key");
                Self { name, key }
            }
        }

        impl Drop for TestRoot {
            fn drop(&mut self) {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                let _ = hkcu.delete_subkey_all(&self.name);
                // Only when no other test still has a key in it.
                let _ = hkcu.delete_subkey(TEST_PARENT);
            }
        }

        fn exe() -> &'static Path {
            Path::new(r"C:\Apps\DoD Studio\dod-studio.exe")
        }

        #[test]
        fn register_lists_studio_and_unregister_leaves_nothing() {
            let root = TestRoot::new("clean");
            assert!(!reg::status(&root.key, exe()).registered);

            reg::register(&root.key, exe()).unwrap();
            let status = reg::status(&root.key, exe());
            assert_eq!(
                status,
                DemOpenWith {
                    registered: true,
                    this_copy: true,
                    other_exe: None
                }
            );
            let command: String = root
                .key
                .open_subkey(r"DoDStudio.Demo\shell\open\command")
                .unwrap()
                .get_value("")
                .unwrap();
            assert_eq!(command, open_command(exe()));
            // The default handler is untouched: `.dem` has no default value.
            let dem = root.key.open_subkey(".dem").unwrap();
            assert!(dem.get_value::<String, _>("").is_err());

            reg::unregister(&root.key).unwrap();
            assert!(!reg::status(&root.key, exe()).registered);
            assert!(root.key.open_subkey(".dem").is_err());
            assert!(root.key.open_subkey(PROG_ID).is_err());
        }

        #[test]
        fn the_users_own_demo_handler_is_left_alone() {
            let root = TestRoot::new("existing");
            let (dem, _) = root.key.create_subkey(".dem").unwrap();
            dem.set_value("", &"HLAE.Demo").unwrap();
            let (list, _) = root.key.create_subkey(r".dem\OpenWithProgids").unwrap();
            list.set_value("Other.Viewer", &"").unwrap();

            reg::register(&root.key, exe()).unwrap();
            let dem = root.key.open_subkey(".dem").unwrap();
            assert_eq!(dem.get_value::<String, _>("").unwrap(), "HLAE.Demo");

            reg::unregister(&root.key).unwrap();
            let dem = root.key.open_subkey(".dem").unwrap();
            assert_eq!(dem.get_value::<String, _>("").unwrap(), "HLAE.Demo");
            let list = root.key.open_subkey(r".dem\OpenWithProgids").unwrap();
            assert!(list.get_raw_value("Other.Viewer").is_ok());
            assert!(list.get_raw_value(PROG_ID).is_err());
        }

        #[test]
        fn another_copys_entry_is_reported_and_replaced() {
            let root = TestRoot::new("other");
            let dev = Path::new(r"D:\src\target\debug\dod-studio.exe");
            reg::register(&root.key, dev).unwrap();
            let status = reg::status(&root.key, exe());
            assert!(status.registered);
            assert!(!status.this_copy);
            assert_eq!(
                status.other_exe.as_deref(),
                Some(r"D:\src\target\debug\dod-studio.exe")
            );

            reg::register(&root.key, exe()).unwrap();
            assert!(reg::status(&root.key, exe()).this_copy);
        }

        #[test]
        fn turning_it_off_twice_is_fine() {
            let root = TestRoot::new("twice");
            reg::unregister(&root.key).unwrap();
            reg::register(&root.key, exe()).unwrap();
            reg::unregister(&root.key).unwrap();
            reg::unregister(&root.key).unwrap();
        }
    }
}
