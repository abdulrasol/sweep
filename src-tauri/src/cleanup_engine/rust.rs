//! Rust toolchains installed with rustup (PLAN 2.7).
//!
//! Toolchains are read from `~/.rustup/toolchains` and `settings.toml`, so the scan
//! runs no command. Removal goes through `rustup toolchain uninstall`. The default
//! toolchain and any toolchain pinned to a folder with `rustup override` are never
//! offered, and linked toolchains (`rustup toolchain link`) are left alone.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("rust") {
        return;
    }
    let rustup_home = env.home.join(".rustup");
    let toolchains = removable_toolchains(&rustup_home);
    if toolchains.is_empty() {
        return;
    }
    let Some(rustup) = native_commands::find_tool(&env.home, "rustup") else {
        return;
    };
    let default = settings(&rustup_home)
        .map(|s| s.default)
        .unwrap_or_default();
    for name in toolchains {
        out.push(Candidate {
            path: rustup_home.join("toolchains").join(&name),
            file_type: format!("Toolchain {name}"),
            category: "Rust Toolchain".to_string(),
            safety: Safety::Review,
            description: format!(
                "A Rust toolchain that is not your default ({default}, kept). Projects that pin it in rust-toolchain.toml install it again on their next build."
            ),
            blocked_if_running: vec!["cargo", "rustc"],
            project_root: None,
            action: Action::Command(NativeCommand::RustupToolchainUninstall {
                rustup: rustup.clone(),
                rustup_home: rustup_home.clone(),
                toolchain: name,
            }),
            known_size: None,
            min_bytes: 1,
            group: "Dev Tools",
            module: "rust",
            catch_all: false,
        });
    }
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Settings {
    pub default: String,
    pub overrides: Vec<String>,
}

/// The parts of `settings.toml` that decide what is in use.
pub(super) fn settings(rustup_home: &Path) -> Option<Settings> {
    let text = fs::read_to_string(rustup_home.join("settings.toml")).ok()?;
    let mut s = Settings::default();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        if section.is_empty() && key.trim() == "default_toolchain" {
            s.default = value;
        } else if section == "overrides" {
            s.overrides.push(value);
        }
    }
    Some(s)
}

/// "stable" matches "stable-aarch64-apple-darwin"; a full name matches itself.
fn same_toolchain(dir: &str, name: &str) -> bool {
    !name.is_empty() && (dir == name || dir.starts_with(&format!("{name}-")))
}

/// Installed toolchains that are neither the default nor an override. Nothing is
/// offered when settings.toml cannot be read or names no default.
pub(super) fn removable_toolchains(rustup_home: &Path) -> Vec<String> {
    let Some(settings) = settings(rustup_home) else {
        return Vec::new();
    };
    if settings.default.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = fs::read_dir(rustup_home.join("toolchains"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| super::is_real_dir(&e.path())) // skips linked toolchains
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_toolchain_name(n))
        .filter(|n| {
            !same_toolchain(n, &settings.default)
                && !settings.overrides.iter().any(|o| same_toolchain(n, o))
        })
        .collect();
    names.sort();
    names
}

/// Names rustup itself creates: letters, digits, dots, dashes and underscores.
pub(super) fn is_toolchain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rustup_home(settings: &str, toolchains: &[&str]) -> TempDir {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("settings.toml"), settings).unwrap();
        for t in toolchains {
            fs::create_dir_all(d.path().join("toolchains").join(t).join("bin")).unwrap();
        }
        d
    }

    const SETTINGS: &str = r#"version = "12"
default_toolchain = "stable-aarch64-apple-darwin"
profile = "default"

[overrides]
"/Users/x/old-project" = "1.75.0-aarch64-apple-darwin"
"#;

    #[test]
    fn settings_are_parsed() {
        let d = rustup_home(SETTINGS, &[]);
        assert_eq!(
            settings(d.path()),
            Some(Settings {
                default: "stable-aarch64-apple-darwin".into(),
                overrides: vec!["1.75.0-aarch64-apple-darwin".into()],
            })
        );
    }

    #[test]
    fn default_and_overrides_are_kept() {
        let d = rustup_home(
            SETTINGS,
            &[
                "stable-aarch64-apple-darwin",
                "nightly-aarch64-apple-darwin",
                "1.75.0-aarch64-apple-darwin",
                "1.70.0-aarch64-apple-darwin",
            ],
        );
        assert_eq!(
            removable_toolchains(d.path()),
            vec![
                "1.70.0-aarch64-apple-darwin",
                "nightly-aarch64-apple-darwin"
            ]
        );
    }

    #[test]
    fn short_default_name_matches_full_toolchain() {
        let d = rustup_home(
            "default_toolchain = \"stable\"\n",
            &["stable-aarch64-apple-darwin", "beta-aarch64-apple-darwin"],
        );
        assert_eq!(
            removable_toolchains(d.path()),
            vec!["beta-aarch64-apple-darwin"]
        );
    }

    #[test]
    fn no_settings_or_no_default_offers_nothing() {
        let d = TempDir::new().unwrap();
        fs::create_dir_all(d.path().join("toolchains/nightly-x/bin")).unwrap();
        assert!(removable_toolchains(d.path()).is_empty());
        let d = rustup_home("profile = \"minimal\"\n", &["nightly-x", "stable-x"]);
        assert!(removable_toolchains(d.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn linked_toolchains_are_left_alone() {
        let d = rustup_home(SETTINGS, &["stable-aarch64-apple-darwin"]);
        let custom = TempDir::new().unwrap();
        std::os::unix::fs::symlink(custom.path(), d.path().join("toolchains/my-build")).unwrap();
        assert!(removable_toolchains(d.path()).is_empty());
    }

    #[test]
    fn toolchain_names_are_checked() {
        assert!(is_toolchain_name("nightly-2024-01-01-aarch64-apple-darwin"));
        assert!(is_toolchain_name("1.75.0-x86_64-unknown-linux-gnu"));
        assert!(!is_toolchain_name("--help"));
        assert!(!is_toolchain_name("a/b"));
        assert!(!is_toolchain_name("x y"));
        assert!(!is_toolchain_name(""));
    }
}
