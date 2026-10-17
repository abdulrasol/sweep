//! Official cleanup commands Sweep runs instead of deleting files itself (PLAN 3.3).
//!
//! Every command is a variant of [`NativeCommand`] with fixed arguments, run with
//! `std::process::Command` and never through a shell. The program path and the
//! folder it acts on are checked again right before it runs, and the folder is
//! pinned through the tool's own setting so it cleans exactly what the scan showed.

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
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
    /// `conda clean --all --yes` with `CONDA_PKGS_DIRS=<root>/pkgs`.
    CondaClean { conda: PathBuf, root: PathBuf },
    /// `go clean -modcache` with `GOMODCACHE=<modcache>` (the `~/go/pkg/mod` folder).
    GoCleanModCache { go: PathBuf, modcache: PathBuf },
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
        NativeCommand::CondaClean { conda, root } => {
            if !root.is_absolute()
                || !root
                    .file_name()
                    .is_some_and(|n| CONDA_ROOTS.iter().any(|&r| n == r))
            {
                return Err("Sweep refused to clean an unexpected Conda root.".to_string());
            }
            if !conda.starts_with(root)
                || conda
                    .components()
                    .any(|c| matches!(c, Component::ParentDir))
            {
                return Err("Sweep refused to run a Conda binary outside its root.".to_string());
            }
            let pkgs = root.join("pkgs");
            check(conda, "conda", &pkgs, "pkgs")?;
            if !super::is_real_dir(&pkgs) {
                return Err(
                    "Sweep refused to clean a missing or symlinked pkgs directory.".to_string(),
                );
            }
            let before = super::disk_usage(&pkgs);
            output(
                "conda clean",
                Command::new(conda)
                    .args(["clean", "--all", "--yes"])
                    .env("CONDA_PKGS_DIRS", &pkgs)
                    .env("PATH", child_path(conda)),
            )?;
            Ok(Some(before.saturating_sub(super::disk_usage(&pkgs))))
        }
        NativeCommand::GoCleanModCache { go, modcache } => {
            if !modcache.is_absolute()
                || modcache
                    .components()
                    .any(|c| matches!(c, Component::ParentDir))
            {
                return Err(
                    "Sweep refused to clean an unexpected Go module cache path.".to_string()
                );
            }
            if modcache.file_name() != Some(OsStr::new("mod"))
                || modcache.parent().and_then(|p| p.file_name()) != Some(OsStr::new("pkg"))
            {
                return Err("Sweep refused to clean a folder not named pkg/mod.".to_string());
            }
            check(go, "go", modcache, "mod")?;
            if !super::is_real_dir(modcache) {
                return Err(
                    "Sweep refused to clean a missing or symlinked mod cache directory."
                        .to_string(),
                );
            }
            let before = super::disk_usage(modcache);
            output(
                "go clean -modcache",
                Command::new(go)
                    .args(["clean", "-modcache"])
                    .env("GOMODCACHE", modcache)
                    .env("PATH", child_path(go)),
            )?;
            Ok(Some(before.saturating_sub(super::disk_usage(modcache))))
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
    dirs.push(PathBuf::from("/usr/local/go/bin"));
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

pub(super) const CONDA_ROOTS: [&str; 4] = ["anaconda3", "miniconda3", "miniforge3", "mambaforge"];

pub(super) fn conda_in(root: &Path) -> Option<PathBuf> {
    let path = root.join("bin/conda");
    if is_tool(&path, "conda") {
        Some(path)
    } else {
        None
    }
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

    fn write_fake_conda(bin_dir: &Path) -> PathBuf {
        fs::create_dir_all(bin_dir).unwrap();
        let path = bin_dir.join("conda");
        fs::write(
            &path,
            "#!/bin/sh\necho \"$* CONDA_PKGS_DIRS=$CONDA_PKGS_DIRS\" > \"$(dirname \"$0\")/called.txt\"\n[ -n \"$CONDA_PKGS_DIRS\" ] && : > \"$CONDA_PKGS_DIRS/big.tar.bz2\"\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn conda_clean_pins_pkgs_and_reports_freed_bytes() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("miniconda3");
        let conda = write_fake_conda(&root.join("bin"));

        let pkgs = root.join("pkgs");
        fs::create_dir_all(&pkgs).unwrap();
        fs::write(pkgs.join("big.tar.bz2"), vec![0u8; 100_000]).unwrap();

        let env_file = root.join("envs/work/x");
        fs::create_dir_all(env_file.parent().unwrap()).unwrap();
        fs::write(&env_file, vec![0u8; 5_000]).unwrap();

        let freed = run(&NativeCommand::CondaClean {
            conda,
            root: root.clone(),
        })
        .unwrap();

        assert!(matches!(freed, Some(n) if n > 0));
        assert_eq!(
            called(&root.join("bin")),
            format!("clean --all --yes CONDA_PKGS_DIRS={}", pkgs.display())
        );
        assert!(env_file.exists());
        assert_eq!(fs::metadata(&env_file).unwrap().len(), 5_000);
    }

    #[test]
    fn conda_clean_refuses_unexpected_input() {
        let d = TempDir::new().unwrap();

        // (a) program named rm inside miniconda3/bin
        let root_a = d.path().join("miniconda3");
        fs::create_dir_all(root_a.join("pkgs")).unwrap();
        let rm = fake_tool(&root_a.join("bin"), "rm");
        assert!(run(&NativeCommand::CondaClean {
            conda: rm,
            root: root_a.clone(),
        })
        .is_err());

        // (b) real fake conda at <tmp>/other/bin/conda with root <tmp>/miniconda3 (conda outside root)
        let other_conda = write_fake_conda(&d.path().join("other/bin"));
        assert!(run(&NativeCommand::CondaClean {
            conda: other_conda,
            root: root_a.clone(),
        })
        .is_err());

        // (c) root named Documents (conda inside it)
        let root_c = d.path().join("Documents");
        fs::create_dir_all(root_c.join("pkgs")).unwrap();
        let conda_c = write_fake_conda(&root_c.join("bin"));
        assert!(run(&NativeCommand::CondaClean {
            conda: conda_c,
            root: root_c,
        })
        .is_err());

        // (d) root miniconda3 but pkgs folder missing
        let root_d = d.path().join("mambaforge");
        let conda_d = write_fake_conda(&root_d.join("bin"));
        assert!(run(&NativeCommand::CondaClean {
            conda: conda_d,
            root: root_d,
        })
        .is_err());

        // (e) relative root path
        assert!(run(&NativeCommand::CondaClean {
            conda: PathBuf::from("miniconda3/bin/conda"),
            root: PathBuf::from("miniconda3"),
        })
        .is_err());

        // (f) conda path "<tmp>/miniconda3/../other/bin/conda" (contains ..)
        let conda_f = root_a.join("../other/bin/conda");
        assert!(run(&NativeCommand::CondaClean {
            conda: conda_f,
            root: root_a.clone(),
        })
        .is_err());

        // (g) pkgs is a symlink to another real folder
        let root_g = d.path().join("miniforge3");
        let conda_g = write_fake_conda(&root_g.join("bin"));
        let real_pkgs = d.path().join("real_pkgs");
        fs::create_dir_all(&real_pkgs).unwrap();
        std::os::unix::fs::symlink(&real_pkgs, root_g.join("pkgs")).unwrap();
        assert!(run(&NativeCommand::CondaClean {
            conda: conda_g,
            root: root_g,
        })
        .is_err());

        fn check_no_called_txt(p: &Path) {
            if p.is_dir() {
                for entry in fs::read_dir(p).unwrap() {
                    let child = entry.unwrap().path();
                    if child.is_dir() {
                        check_no_called_txt(&child);
                    } else {
                        assert_ne!(
                            child.file_name().and_then(|s| s.to_str()),
                            Some("called.txt")
                        );
                    }
                }
            }
        }
        check_no_called_txt(d.path());
    }

    fn write_fake_go(bin_dir: &Path) -> PathBuf {
        fs::create_dir_all(bin_dir).unwrap();
        let path = bin_dir.join("go");
        fs::write(
            &path,
            "#!/bin/sh\necho \"$* GOMODCACHE=$GOMODCACHE\" > \"$(dirname \"$0\")/called.txt\"\n[ -n \"$GOMODCACHE\" ] && : > \"$GOMODCACHE/big.zip\"\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn go_clean_modcache_pins_folder_and_reports_freed_bytes() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("go");
        let go = write_fake_go(&root.join("bin"));

        let modcache = root.join("pkg/mod");
        fs::create_dir_all(&modcache).unwrap();
        fs::write(modcache.join("big.zip"), vec![0u8; 100_000]).unwrap();
        let download_file = modcache.join("cache/download/x");
        fs::create_dir_all(download_file.parent().unwrap()).unwrap();
        fs::write(&download_file, vec![0u8; 5_000]).unwrap();

        let tool = root.join("bin/tool");
        fs::write(&tool, vec![0u8; 5_000]).unwrap();

        let main_go = root.join("src/app/main.go");
        fs::create_dir_all(main_go.parent().unwrap()).unwrap();
        fs::write(&main_go, vec![0u8; 5_000]).unwrap();

        let freed = run(&NativeCommand::GoCleanModCache {
            go,
            modcache: modcache.clone(),
        })
        .unwrap();

        assert!(matches!(freed, Some(n) if n > 0));
        assert_eq!(
            called(&root.join("bin")),
            format!("clean -modcache GOMODCACHE={}", modcache.display())
        );
        assert!(tool.exists());
        assert_eq!(fs::metadata(&tool).unwrap().len(), 5_000);
        assert!(main_go.exists());
        assert_eq!(fs::metadata(&main_go).unwrap().len(), 5_000);
    }

    #[test]
    fn go_clean_modcache_refuses_unexpected_input() {
        let d = TempDir::new().unwrap();
        let root = d.path().join("go");
        let go = write_fake_go(&root.join("bin"));

        // (a) program named rm
        let rm = fake_tool(&root.join("bin"), "rm");
        let valid_modcache = root.join("pkg/mod");
        fs::create_dir_all(&valid_modcache).unwrap();
        assert!(run(&NativeCommand::GoCleanModCache {
            go: rm,
            modcache: valid_modcache.clone(),
        })
        .is_err());

        // (b) modcache <tmp>/go/pkg/other (not named mod)
        let other_cache = root.join("pkg/other");
        fs::create_dir_all(&other_cache).unwrap();
        assert!(run(&NativeCommand::GoCleanModCache {
            go: go.clone(),
            modcache: other_cache,
        })
        .is_err());

        // (c) modcache <tmp>/go/mod (parent not named pkg)
        let root_mod = root.join("mod");
        fs::create_dir_all(&root_mod).unwrap();
        assert!(run(&NativeCommand::GoCleanModCache {
            go: go.clone(),
            modcache: root_mod,
        })
        .is_err());

        // (d) modcache "<tmp>/go/pkg/mod/../mod" (contains ..)
        let parent_dir_cache = root.join("pkg/mod/../mod");
        assert!(run(&NativeCommand::GoCleanModCache {
            go: go.clone(),
            modcache: parent_dir_cache,
        })
        .is_err());

        // (e) relative modcache path "go/pkg/mod"
        assert!(run(&NativeCommand::GoCleanModCache {
            go: go.clone(),
            modcache: PathBuf::from("go/pkg/mod"),
        })
        .is_err());

        // (f) pkg/mod folder missing
        let missing_mod = d.path().join("other_go/pkg/mod");
        assert!(run(&NativeCommand::GoCleanModCache {
            go: go.clone(),
            modcache: missing_mod,
        })
        .is_err());

        // (g) <tmp>/go/pkg/mod is a symlink to another real folder
        let sym_root = d.path().join("sym_go");
        let sym_go = write_fake_go(&sym_root.join("bin"));
        let real_mod = d.path().join("real_mod");
        fs::create_dir_all(&real_mod).unwrap();
        let sym_pkg = sym_root.join("pkg");
        fs::create_dir_all(&sym_pkg).unwrap();
        std::os::unix::fs::symlink(&real_mod, sym_pkg.join("mod")).unwrap();
        assert!(run(&NativeCommand::GoCleanModCache {
            go: sym_go,
            modcache: sym_pkg.join("mod"),
        })
        .is_err());

        fn check_no_called_txt(p: &Path) {
            if p.is_dir() {
                for entry in fs::read_dir(p).unwrap() {
                    let child = entry.unwrap().path();
                    if child.is_dir() {
                        check_no_called_txt(&child);
                    } else {
                        assert_ne!(
                            child.file_name().and_then(|s| s.to_str()),
                            Some("called.txt")
                        );
                    }
                }
            }
        }
        check_no_called_txt(d.path());
    }
}
