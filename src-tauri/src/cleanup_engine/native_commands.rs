//! Official cleanup commands Sweep runs instead of deleting files itself (PLAN 3.3).
//!
//! Every command is a variant of [`NativeCommand`] with fixed arguments, run with
//! `std::process::Command` and never through a shell. The program path and the
//! folder it acts on are checked again right before it runs, and the folder is
//! pinned through the tool's own setting so it cleans exactly what the scan showed.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeCommand {
    /// `xcrun <args>`, limited to the `simctl` calls in `simulators::is_allowed_command`.
    Xcrun(Vec<String>),
    /// `dart pub cache clean -f` with `PUB_CACHE=<cache>`.
    DartPubCacheClean { dart: PathBuf, cache: PathBuf },
    /// `npm cache clean --force` with `npm_config_cache=<cache>` (the `~/.npm` folder).
    NpmCacheClean { npm: PathBuf, cache: PathBuf },
    /// `pnpm store prune --store-dir <store>`: removes only packages no project uses.
    PnpmStorePrune { pnpm: PathBuf, store: PathBuf },
    /// `rustup toolchain uninstall <toolchain>` with `RUSTUP_HOME=<rustup_home>`.
    RustupToolchainUninstall {
        rustup: PathBuf,
        rustup_home: PathBuf,
        toolchain: String,
    },
}

/// Run the command. Returns the bytes freed when the command measures them itself.
pub(super) fn run(cmd: &NativeCommand) -> Result<Option<u64>, String> {
    match cmd {
        NativeCommand::Xcrun(args) => super::simulators::run_command(args).map(|()| None),
        NativeCommand::DartPubCacheClean { dart, cache } => {
            check(dart, "dart", cache, ".pub-cache")?;
            output(
                "dart pub cache clean",
                Command::new(dart)
                    .args(["pub", "cache", "clean", "-f"])
                    .env("PUB_CACHE", cache),
            )
            .map(|()| None)
        }
        NativeCommand::NpmCacheClean { npm, cache } => {
            check(npm, "npm", cache, ".npm")?;
            output(
                "npm cache clean",
                Command::new(npm)
                    .args(["cache", "clean", "--force"])
                    .env("npm_config_cache", cache)
                    .env("PATH", child_path(npm)),
            )
            .map(|()| None)
        }
        NativeCommand::PnpmStorePrune { pnpm, store } => {
            check(pnpm, "pnpm", store, "store")?;
            let before = super::disk_usage(store);
            output(
                "pnpm store prune",
                Command::new(pnpm)
                    .args(["store", "prune", "--store-dir"])
                    .arg(store)
                    .env("PATH", child_path(pnpm)),
            )?;
            Ok(Some(before.saturating_sub(super::disk_usage(store))))
        }
        NativeCommand::RustupToolchainUninstall {
            rustup,
            rustup_home,
            toolchain,
        } => {
            check(rustup, "rustup", rustup_home, ".rustup")?;
            // Check again at run time: never the default, an override or an odd name.
            if !super::rust::removable_toolchains(rustup_home).contains(toolchain) {
                return Err(format!(
                    "Sweep refused to uninstall {toolchain}: it is in use or not installed."
                ));
            }
            output(
                "rustup toolchain uninstall",
                Command::new(rustup)
                    .args(["toolchain", "uninstall"])
                    .arg(toolchain)
                    .env("RUSTUP_HOME", rustup_home),
            )
            .map(|()| None)
        }
    }
}

fn check(program: &Path, name: &str, folder: &Path, folder_name: &str) -> Result<(), String> {
    if !is_tool(program, name) {
        return Err(format!(
            "Sweep refused to run an unexpected program as {name}."
        ));
    }
    if !folder.is_absolute() || folder.file_name() != Some(OsStr::new(folder_name)) {
        return Err(format!(
            "Sweep refused to let {name} clean an unexpected folder."
        ));
    }
    Ok(())
}

fn output(what: &str, cmd: &mut Command) -> Result<(), String> {
    let out = cmd
        .output()
        .map_err(|e| format!("Could not run {what}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// npm and pnpm are often node scripts (`#!/usr/bin/env node`). Apps opened from
/// Finder get a short PATH, so put the tool's own folder and the usual node
/// locations first.
fn child_path(program: &Path) -> OsString {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = program
        .parent()
        .map(Path::to_path_buf)
        .into_iter()
        .collect();
    dirs.extend(search_dirs(&home));
    std::env::join_paths(dirs).unwrap_or_default()
}

/// An absolute path to an executable file called `name`.
fn is_tool(path: &Path, name: &str) -> bool {
    path.is_absolute() && path.file_name().is_some_and(|n| n == name) && is_executable(path)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

/// Folders searched for developer tools: `PATH`, then the places installers use.
fn search_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for rel in [
        // Rust, Node version managers and standalone installers
        ".cargo/bin",
        ".volta/bin",
        "Library/pnpm",
        ".local/share/pnpm",
        ".bun/bin",
        "Library/Application Support/fnm/aliases/default/bin",
        ".local/share/fnm/aliases/default/bin",
        ".fnm/aliases/default/bin",
        // Flutter SDK
        "fvm/default/bin",
        "flutter/bin",
        "development/flutter/bin",
        "Developer/flutter/bin",
        "sdk/flutter/bin",
        "src/flutter/bin",
        ".puro/envs/stable/flutter/bin",
    ] {
        dirs.push(home.join(rel));
    }
    if let Some(nvm) = super::node::newest_nvm_bin(home) {
        dirs.push(nvm);
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.push(PathBuf::from("/bin"));
    dirs
}

/// Find an executable called `name` in the usual places.
pub(super) fn find_tool(home: &Path, name: &str) -> Option<PathBuf> {
    search_dirs(home)
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| is_tool(p, name))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// A fake tool that records its arguments and settings instead of cleaning anything.
    fn fake_tool(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(
            &path,
            "#!/bin/sh\necho \"$* PUB_CACHE=$PUB_CACHE npm_config_cache=$npm_config_cache\" > \"$(dirname \"$0\")/called.txt\"\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn called(dir: &Path) -> String {
        fs::read_to_string(dir.join("called.txt"))
            .unwrap()
            .trim()
            .to_string()
    }

    #[test]
    fn pub_cache_clean_runs_dart_with_fixed_args() {
        let d = TempDir::new().unwrap();
        let dart = fake_tool(d.path(), "dart");
        let cache = d.path().join("home/.pub-cache");
        let freed = run(&NativeCommand::DartPubCacheClean {
            dart,
            cache: cache.clone(),
        })
        .unwrap();
        assert_eq!(freed, None);
        assert!(called(d.path()).starts_with(&format!(
            "pub cache clean -f PUB_CACHE={} ",
            cache.display()
        )));
    }

    #[test]
    fn npm_cache_clean_pins_the_cache_folder() {
        let d = TempDir::new().unwrap();
        let npm = fake_tool(d.path(), "npm");
        let cache = d.path().join("home/.npm");
        run(&NativeCommand::NpmCacheClean {
            npm,
            cache: cache.clone(),
        })
        .unwrap();
        assert!(called(d.path()).starts_with("cache clean --force "));
        assert!(called(d.path()).ends_with(&format!("npm_config_cache={}", cache.display())));
    }

    #[test]
    fn pnpm_prune_reports_measured_bytes() {
        let d = TempDir::new().unwrap();
        let pnpm = fake_tool(d.path(), "pnpm");
        let store = d.path().join("Library/pnpm/store");
        fs::create_dir_all(store.join("v10")).unwrap();
        let freed = run(&NativeCommand::PnpmStorePrune {
            pnpm,
            store: store.clone(),
        })
        .unwrap();
        // The fake removes nothing, so nothing was freed.
        assert_eq!(freed, Some(0));
        assert!(
            called(d.path()).starts_with(&format!("store prune --store-dir {} ", store.display()))
        );
    }

    #[test]
    fn rustup_uninstalls_only_removable_toolchains() {
        let d = TempDir::new().unwrap();
        let rustup = fake_tool(d.path(), "rustup");
        let home = d.path().join(".rustup");
        fs::create_dir_all(home.join("toolchains/stable-aarch64-apple-darwin/bin")).unwrap();
        fs::create_dir_all(home.join("toolchains/nightly-aarch64-apple-darwin/bin")).unwrap();
        fs::write(
            home.join("settings.toml"),
            "default_toolchain = \"stable-aarch64-apple-darwin\"\n",
        )
        .unwrap();
        let cmd = |t: &str| NativeCommand::RustupToolchainUninstall {
            rustup: rustup.clone(),
            rustup_home: home.clone(),
            toolchain: t.to_string(),
        };
        // Negative: the default, a missing toolchain and an odd name are refused.
        for t in ["stable-aarch64-apple-darwin", "1.70.0-x", "--help"] {
            assert!(run(&cmd(t)).is_err(), "{t} should be refused");
        }
        assert!(!d.path().join("called.txt").exists());
        run(&cmd("nightly-aarch64-apple-darwin")).unwrap();
        assert!(called(d.path()).starts_with("toolchain uninstall nightly-aarch64-apple-darwin "));
    }

    #[test]
    fn commands_refuse_unexpected_programs_and_folders() {
        let d = TempDir::new().unwrap();
        let rm = fake_tool(d.path(), "rm");
        let dart = fake_tool(d.path(), "dart");
        let npm = fake_tool(d.path(), "npm");
        let pnpm = fake_tool(d.path(), "pnpm");
        let cmds = [
            NativeCommand::DartPubCacheClean {
                dart: rm.clone(),
                cache: d.path().join(".pub-cache"),
            },
            NativeCommand::DartPubCacheClean {
                dart: PathBuf::from("dart"),
                cache: d.path().join(".pub-cache"),
            },
            NativeCommand::DartPubCacheClean {
                dart: d.path().join("missing/dart"),
                cache: d.path().join(".pub-cache"),
            },
            NativeCommand::DartPubCacheClean {
                dart: dart.clone(),
                cache: d.path().join("Documents"),
            },
            NativeCommand::DartPubCacheClean {
                dart,
                cache: PathBuf::from(".pub-cache"),
            },
            NativeCommand::NpmCacheClean {
                npm: rm.clone(),
                cache: d.path().join(".npm"),
            },
            NativeCommand::NpmCacheClean {
                npm,
                cache: d.path().join("Documents"),
            },
            NativeCommand::PnpmStorePrune {
                pnpm: rm,
                store: d.path().join("store"),
            },
            NativeCommand::PnpmStorePrune {
                pnpm,
                store: d.path().join("Projects"),
            },
        ];
        for cmd in cmds {
            assert!(run(&cmd).is_err(), "{cmd:?} should be refused");
        }
        assert!(!d.path().join("called.txt").exists());
    }

    #[test]
    fn find_tool_checks_installer_locations_in_home() {
        let d = TempDir::new().unwrap();
        let dart = fake_tool(&d.path().join("development/flutter/bin"), "dart");
        let pnpm = fake_tool(&d.path().join("Library/pnpm"), "pnpm");
        // PATH may hold a real tool on the developer's machine; accept either, but a
        // match must always be an executable with that name.
        let found = find_tool(d.path(), "dart").unwrap();
        assert!(found == dart || is_tool(&found, "dart"));
        let found = find_tool(d.path(), "pnpm").unwrap();
        assert!(found == pnpm || is_tool(&found, "pnpm"));
        assert!(find_tool(d.path(), "definitely-not-a-tool").is_none());
    }
}
