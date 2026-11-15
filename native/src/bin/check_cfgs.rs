//! What the game's own config files set behind the pipeline's back.
//!
//! ```text
//! cargo run --release -p native --bin check_cfgs -- <dod-folder> ["init command"...]
//! ```
//!
//! Any init commands passed after the folder are checked for conflicting with a
//! value a config, or another of them, sets.
//!
//! Read-only. This never writes, edits or removes a config file.

use native::messages;
use native::patch::cfg_scan;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect(messages::CHECK_CFGS_USAGE));
    let init_commands: Vec<String> = args.collect();

    let scan = cfg_scan::scan(&dir);
    println!(
        "{}",
        messages::cfgs_folder_and_count(dir.display(), scan.files_read)
    );

    let effective = scan.effective_settings();
    if effective.is_empty() {
        println!("{}", messages::CFGS_NOTHING_SET);
    } else {
        println!("{}", messages::CFGS_SET_BY_CONFIGS);
        for s in &effective {
            println!(
                "{}",
                messages::cfgs_setting_row(&s.cvar, &s.value, s.file_name(), s.line)
            );
        }
    }

    // Only the cvars in play. A full config.cfg is ~200 assignments, and
    // printing every one buries the two that matter.
    let shadowed: Vec<_> = scan
        .settings
        .iter()
        .filter(|s| {
            s.auto_executed
                && cfg_scan::WATCHED_CVARS
                    .iter()
                    .any(|w| s.cvar.eq_ignore_ascii_case(w))
                && !effective.iter().any(|e| std::ptr::eq(*e, *s))
        })
        .collect();
    if !shadowed.is_empty() {
        println!("{}", messages::CFGS_OVERRIDDEN_LATER);
        for s in shadowed {
            println!(
                "{}",
                messages::cfgs_setting_row(&s.cvar, &s.value, s.file_name(), s.line)
            );
        }
    }

    println!("{}", messages::cfgs_assignments_seen(scan.settings.len()));

    if !init_commands.is_empty() {
        // The same rule the app's Initial Commands warning uses (#216).
        let warnings = cfg_scan::value_warnings(&scan, &init_commands, init_commands.len(), &[]);
        println!();
        if warnings.conflicts.is_empty() {
            println!("{}", messages::CFGS_NO_INIT_CONFLICT);
        } else {
            println!("{}", messages::CFGS_INIT_CONFLICTS);
            for c in warnings.conflicts {
                let values: Vec<String> = c
                    .values
                    .iter()
                    .map(|v| match &v.source {
                        cfg_scan::ValueSource::Config { file, line } => {
                            messages::cfgs_value_from_config(
                                &v.value,
                                file.file_name().unwrap_or_default().to_string_lossy(),
                                line,
                            )
                        }
                        _ => messages::cfgs_value_from_init(&v.value),
                    })
                    .collect();
                println!(
                    "{}",
                    messages::cfgs_conflict_row(&c.cvar, values.join(", "))
                );
            }
        }
    }

    let fatal = cfg_scan::fatal_cvar_hazards(&scan);
    if !fatal.is_empty() {
        println!("{}", messages::CFGS_WILL_QUIT_GAME);
        for f in &fatal {
            println!(
                "{}",
                messages::cfgs_fatal_row(&f.cvar, &f.value, &f.required, f.file_name(), f.line)
            );
        }
    }

    if !scan.unreferenced.is_empty() {
        println!("{}", messages::CFGS_UNREFERENCED);
        for p in &scan.unreferenced {
            println!(
                "{}",
                messages::cfgs_unreferenced_row(
                    p.file_name().unwrap_or_default().to_string_lossy()
                )
            );
        }
    }
}
