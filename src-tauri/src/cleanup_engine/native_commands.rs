//! Official cleanup commands Sweep runs instead of deleting files itself (PLAN 3.3).
//!
//! Every command is a variant of [`NativeCommand`] with fixed arguments, run with
//! `std::process::Command` and never through a shell. The program path is checked
//! again right before it runs.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeCommand {
    /// `xcrun <args>`, limited to the `simctl` calls in `simulators::is_allowed_command`.
    Xcrun(Vec<String>),
    /// `<dart> pub cache clean -f`, with `PUB_CACHE` pinned to the folder the scan showed.
    DartPubCacheClean { dart: PathBuf, cache: PathBuf },
}

pub(super) fn run(cmd: &NativeCommand) -> Result<(), String> {
    match cmd {
        NativeCommand::Xcrun(args) => super::simulators::run_command(args),
        NativeCommand::DartPubCacheClean { dart, cache } => run_pub_cache_clean(dart, cache),
    }
}

fn run_pub_cache_clean(dart: &Path, cache: &Path) -> Result<(), String> {
    if !is_tool(dart, "dart") {
        return Err("Sweep refused to run an unexpected program as dart.".to_string());
    }
    if !cache.is_absolute() || cache.file_name() != Some(OsStr::new(".pub-cache")) {
        return Err("Sweep refused to clean an unexpected pub cache folder.".to_string());
    }
    let out = Command::new(dart)
        .args(["pub", "cache", "clean", "-f"])
        .env("PUB_CACHE", cache)
        .output()
        .map_err(|e| format!("Could not run dart: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "dart pub cache clean failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
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

/// Find the `dart` executable. Apps opened from Finder get a short `PATH`, so the
/// usual Flutter SDK and Homebrew locations are checked as well.
pub(super) fn find_dart(home: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for rel in [
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
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.into_iter()
        .map(|d| d.join("dart"))
        .find(|p| is_tool(p, "dart"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// A fake `dart` that records its arguments and PUB_CACHE instead of cleaning anything.
    fn fake_tool(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(
            &path,
            "#!/bin/sh\necho \"$* PUB_CACHE=$PUB_CACHE\" > \"$(dirname \"$0\")/called.txt\"\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn pub_cache_clean_runs_dart_with_fixed_args() {
        let d = TempDir::new().unwrap();
        let dart = fake_tool(d.path(), "dart");
        let cache = d.path().join("home/.pub-cache");
        run(&NativeCommand::DartPubCacheClean {
            dart,
            cache: cache.clone(),
        })
        .unwrap();
        let called = fs::read_to_string(d.path().join("called.txt")).unwrap();
        assert_eq!(
            called.trim(),
            format!("pub cache clean -f PUB_CACHE={}", cache.display())
        );
    }

    #[test]
    fn pub_cache_clean_refuses_unexpected_programs_and_folders() {
        let d = TempDir::new().unwrap();
        let rm = fake_tool(d.path(), "rm");
        let dart = fake_tool(d.path(), "dart");
        let cache = d.path().join(".pub-cache");
        let cases = [
            (rm, cache.clone()),
            (PathBuf::from("dart"), cache.clone()),
            (d.path().join("missing/dart"), cache.clone()),
            (dart.clone(), d.path().join("Documents")),
            (dart, PathBuf::from(".pub-cache")),
        ];
        for (dart, cache) in cases {
            let cmd = NativeCommand::DartPubCacheClean { dart, cache };
            assert!(run(&cmd).is_err(), "{cmd:?} should be refused");
        }
        assert!(!d.path().join("called.txt").exists());
    }

    #[test]
    fn find_dart_checks_flutter_sdk_in_home() {
        let d = TempDir::new().unwrap();
        let bin = d.path().join("development/flutter/bin");
        fs::create_dir_all(&bin).unwrap();
        let dart = fake_tool(&bin, "dart");
        // PATH may hold a real dart on the developer's machine; accept either, but a
        // match must always be an executable called dart.
        let found = find_dart(d.path()).unwrap();
        assert!(found == dart || is_tool(&found, "dart"));
    }
}
