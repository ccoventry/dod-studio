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

use native::patch::cfg_scan;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(
        args.next()
            .expect("usage: check_cfgs <dod-folder> [\"init command\"...]"),
    );
    let init_commands: Vec<String> = args.collect();

    let scan = cfg_scan::scan(&dir);
    println!(
        "{}\n{} config file(s) executed\n",
        dir.display(),
        scan.files_read
    );

    let effective = scan.effective_settings();
    if effective.is_empty() {
        println!("Nothing the pipeline depends on is set by an executed config.");
    } else {
        println!("Set by configs the engine runs on its own:");
        for s in &effective {
            println!(
                "  {:<12} {:<8} {}:{}",
                s.cvar,
                s.value,
                s.file_name(),
                s.line
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
        println!("\nAlso set, but overridden later in the chain:");
        for s in shadowed {
            println!(
                "  {:<12} {:<8} {}:{}",
                s.cvar,
                s.value,
                s.file_name(),
                s.line
            );
        }
    }

    println!(
        "\n{} assignment(s) seen across those configs.",
        scan.settings.len()
    );

    if !init_commands.is_empty() {
        // The same rule the app's Initial Commands warning uses (#216).
        let warnings = cfg_scan::value_warnings(&scan, &init_commands, init_commands.len(), &[]);
        println!();
        if warnings.conflicts.is_empty() {
            println!("No value those init commands set conflicts with another.");
        } else {
            println!("Values that conflict (the last one is in effect):");
            for c in warnings.conflicts {
                let values: Vec<String> = c
                    .values
                    .iter()
                    .map(|v| match &v.source {
                        cfg_scan::ValueSource::Config { file, line } => format!(
                            "{} ({}:{})",
                            v.value,
                            file.file_name().unwrap_or_default().to_string_lossy(),
                            line
                        ),
                        _ => format!("{} (init)", v.value),
                    })
                    .collect();
                println!("  {:<20} {}", c.cvar, values.join(", "));
            }
        }
    }

    let fatal = cfg_scan::fatal_cvar_hazards(&scan);
    if !fatal.is_empty() {
        println!("\nWill quit the game the moment the HUD renders:");
        for f in &fatal {
            println!(
                "  {:<12} {:<8} DoD requires {} ({}:{})",
                f.cvar,
                f.value,
                f.required,
                f.file_name(),
                f.line
            );
        }
    }

    if !scan.unreferenced.is_empty() {
        println!("\nConfigs present that nothing execs (they set nothing today):");
        for p in &scan.unreferenced {
            println!("  {}", p.file_name().unwrap_or_default().to_string_lossy());
        }
    }
}
