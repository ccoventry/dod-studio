//! One-time changes to a saved settings file, and the console-name move that
//! runs at every load. `AppSettings::load_or_default` runs both.

use crate::settings_manager::AppSettings;

/// How many one-time changes to saved values exist; `migrate` runs the ones
/// past a file's `settings_migrations`. The frontend saves settings without
/// this field, so `save_settings` keeps the loaded value.
pub const MIGRATIONS: u32 = 3;

impl AppSettings {
    /// Moves every hook console name from before #555 (`dodstudio_*`) in
    /// Initial Commands, Scheduled Commands and the command profiles to the
    /// name that replaced it (`native::patch::console_prefix::migrated`, from
    /// the hook's own old -> new table). The game has no
    /// aliases for the old names, so a list left as it was would do nothing.
    /// True when anything changed.
    pub fn migrate_console_names(&mut self) -> bool {
        fn fix(command: &mut String, changed: &mut bool) {
            if let Some(new) = native::patch::console_prefix::migrated(command) {
                *command = new;
                *changed = true;
            }
        }
        let mut changed = false;
        let lists = std::iter::once((&mut self.init_commands, &mut self.custom_commands)).chain(
            self.command_profiles
                .iter_mut()
                .map(|p| (&mut p.init_commands, &mut p.custom_commands)),
        );
        for (init, scheduled) in lists {
            for command in init.iter_mut() {
                fix(command, &mut changed);
            }
            for payload in scheduled.iter_mut() {
                fix(&mut payload.command, &mut changed);
            }
        }
        changed
    }

    /// Applies the one-time changes this file hasn't had.
    pub fn migrate(&mut self) {
        if self.settings_migrations < 1 {
            // Seek Between Clips was off by default while experimental; a
            // file saved then has the old default and gets the new one once
            // (#434). Turning it off again after this sticks.
            self.hook_runs_batch = true;
        }
        // Migration 2 added Clean clips as an editable profile (#898). It is
        // built in now (#911), so 2 has nothing left to do, and the copies it
        // made are sorted out below with every other profile that has a
        // built-in's name. That runs at every load rather than once, so a
        // built-in added later takes its name the same way.
        let kept = crate::builtin_profiles::separate_builtin_copies(
            &mut self.command_profiles,
            &mut self.command_profile_active,
        );
        for name in kept {
            if !self.builtin_profile_notice.contains(&name) {
                self.builtin_profile_notice.push(name);
            }
        }
        if self.settings_migrations < 3 {
            // "Their own spray" left the POV fallback (#1322).
            self.pov_own_spray_notice = self.players.retire_own_fallback();
        }
        self.settings_migrations = MIGRATIONS;
    }
}
