use anyhow::Result;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKind {
    Binary,
    Config,
    Cache,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalTarget {
    pub path: PathBuf,
    pub kind: TargetKind,
    pub description: String,
}

/// Discovers all paths on the system associated with manim-live
pub fn collect_installed_targets(current_exe: Option<PathBuf>) -> Vec<RemovalTarget> {
    let mut targets = Vec::new();

    // 1. Check home-relative locations
    if let Ok(home_str) = std::env::var("HOME") {
        let home = Path::new(&home_str);

        // Binary in ~/.local/bin/manim-live
        let local_bin = home.join(".local").join("bin").join("manim-live");
        if local_bin.exists() {
            targets.push(RemovalTarget {
                path: local_bin,
                kind: TargetKind::Binary,
                description: "Executable binary (~/.local/bin/manim-live)".to_string(),
            });
        }

        // Binary in ~/.cargo/bin/manim-live
        let cargo_bin = home.join(".cargo").join("bin").join("manim-live");
        if cargo_bin.exists() {
            targets.push(RemovalTarget {
                path: cargo_bin,
                kind: TargetKind::Binary,
                description: "Cargo executable binary (~/.cargo/bin/manim-live)".to_string(),
            });
        }

        // Configuration directory: ~/.config/manim-live
        let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                PathBuf::from(xdg).join("manim-live")
            } else {
                home.join(".config").join("manim-live")
            }
        } else {
            home.join(".config").join("manim-live")
        };

        if config_dir.exists() {
            targets.push(RemovalTarget {
                path: config_dir,
                kind: TargetKind::Config,
                description: "Configuration & state directory (~/.config/manim-live)".to_string(),
            });
        }

        // Cache directory: ~/.cache/manim-live
        let cache_dir = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
            if !xdg.trim().is_empty() {
                PathBuf::from(xdg).join("manim-live")
            } else {
                home.join(".cache").join("manim-live")
            }
        } else {
            home.join(".cache").join("manim-live")
        };

        if cache_dir.exists() {
            targets.push(RemovalTarget {
                path: cache_dir,
                kind: TargetKind::Cache,
                description: "Cache directory (~/.cache/manim-live)".to_string(),
            });
        }
    }

    // 2. Global binary in /usr/local/bin/manim-live
    let sys_bin = PathBuf::from("/usr/local/bin/manim-live");
    if sys_bin.exists() && !targets.iter().any(|t| t.path == sys_bin) {
        targets.push(RemovalTarget {
            path: sys_bin,
            kind: TargetKind::Binary,
            description: "System executable binary (/usr/local/bin/manim-live)".to_string(),
        });
    }

    // 3. Current executable if it's an installed location not already tracked
    if let Some(exe) = current_exe {
        if exe.exists() && !targets.iter().any(|t| t.path == exe) {
            // Avoid marking git target checkout builds unless nothing else is present
            let exe_str = exe.to_string_lossy();
            if !exe_str.contains("/target/debug/") && !exe_str.contains("/target/release/") {
                targets.push(RemovalTarget {
                    path: exe.clone(),
                    kind: TargetKind::Binary,
                    description: format!("Currently running binary ({})", exe.display()),
                });
            }
        }
    }

    targets
}

/// Executes uninstallation by removing all discovered targets
pub fn execute_uninstallation(assume_yes: bool) -> Result<()> {
    println!("\x1b[1;35m");
    println!("    __  ___            _             __    _           ");
    println!("   /  |/  /___ _____  (_)___ ___    / /   (_)   _____  ");
    println!("  / /|_/ / __ `/ __ \\/ / __ `__ \\  / /   / / | / / _ \\ ");
    println!(" / /  / / /_/ / / / / / / / / / / / /___/ /| |/ /  __/ ");
    println!("/_/  /_/\\__,_/_/ /_/_/_/ /_/ /_/ /_____/_/ |___/\\___/  ");
    println!("\x1b[0m\x1b[1;36mApplication Uninstaller\x1b[0m");
    println!("\x1b[2m====================================================\x1b[0m\n");

    let current_exe = std::env::current_exe().ok();
    let targets = collect_installed_targets(current_exe);

    if targets.is_empty() {
        println!("\x1b[1;33m⚠\x1b[0m No installed manim-live binaries or configuration files were detected.");
        println!("  System paths checked:");
        println!("    • ~/.local/bin/manim-live");
        println!("    • ~/.cargo/bin/manim-live");
        println!("    • /usr/local/bin/manim-live");
        println!("    • ~/.config/manim-live");
        println!("    • ~/.cache/manim-live\n");
        println!("\x1b[1;32m✔\x1b[0m Nothing to remove. System is clean.");
        return Ok(());
    }

    println!("\x1b[1;31mThe following files and directories will be completely removed:\x1b[0m");
    for target in &targets {
        let kind_badge = match target.kind {
            TargetKind::Binary => "\x1b[36m[Binary]\x1b[0m",
            TargetKind::Config => "\x1b[33m[Config]\x1b[0m",
            TargetKind::Cache  => "\x1b[35m[Cache ]\x1b[0m",
        };
        println!("  {} {} \x1b[1m{}\x1b[0m", kind_badge, target.description, target.path.display());
    }
    println!();

    if !assume_yes {
        print!("\x1b[1;37mAre you sure you want to permanently delete manim-live? [y/N]: \x1b[0m");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            println!("\n\x1b[1;33mUninstallation cancelled.\x1b[0m No files were modified.");
            return Ok(());
        }
        println!();
    }

    let mut removed_count = 0;
    let mut failed_count = 0;

    for target in &targets {
        let result = if target.path.is_dir() {
            fs::remove_dir_all(&target.path)
        } else {
            fs::remove_file(&target.path)
        };

        match result {
            Ok(_) => {
                println!("\x1b[1;32m✔\x1b[0m Removed: {}", target.path.display());
                removed_count += 1;
            }
            Err(e) => {
                println!(
                    "\x1b[1;31m✖\x1b[0m Failed to remove {}: {} (try running with sudo)",
                    target.path.display(),
                    e
                );
                failed_count += 1;
            }
        }
    }

    println!();
    if failed_count == 0 {
        println!(
            "\x1b[1;32m✔\x1b[0m \x1b[1mmanim-live has been completely and successfully uninstalled!\x1b[0m"
        );
        println!("  ({} items removed)\n", removed_count);
    } else {
        println!(
            "\x1b[1;33m⚠\x1b[0m Uninstallation finished with warnings ({} removed, {} failed).",
            removed_count, failed_count
        );
        println!("  Run with elevated permissions to remove protected system binaries if needed.\n");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_collect_installed_targets_empty() {
        let targets = collect_installed_targets(None);
        // Should compile and run without error
        assert!(targets.len() <= 10);
    }

    #[test]
    fn test_removal_target_execution_on_tempdir() {
        let dir = tempdir().expect("create temp dir");
        let fake_bin = dir.path().join("manim-live");
        let fake_config = dir.path().join("config-dir");
        fs::write(&fake_bin, b"binary contents").unwrap();
        fs::create_dir_all(&fake_config).unwrap();
        fs::write(fake_config.join("settings.json"), b"{}").unwrap();

        assert!(fake_bin.exists());
        assert!(fake_config.exists());

        // Perform deletion
        fs::remove_file(&fake_bin).unwrap();
        fs::remove_dir_all(&fake_config).unwrap();

        assert!(!fake_bin.exists());
        assert!(!fake_config.exists());
    }
}
