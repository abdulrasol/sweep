//! Global Node caches and old Node versions (PLAN 2.6).
//!
//! - npm cache: `npm cache clean --force` when npm is found, else the `_cacache`
//!   folder is deleted (that is all the command does).
//! - pnpm store: `pnpm store prune`, which removes only packages no project uses.
//!   Without pnpm the whole store is offered for review instead.
//! - Old Node versions from nvm, fnm and Volta. The newest version and the one each
//!   manager uses by default are always kept. If nvm's default alias cannot be
//!   resolved, no nvm version is offered.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
const PNPM_STORE: &str = "Library/pnpm/store";
#[cfg(target_os = "windows")]
const PNPM_STORE: &str = "AppData/Local/pnpm/store";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const PNPM_STORE: &str = ".local/share/pnpm/store";

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("node") {
        return;
    }
    let home = &env.home;

    let npm_cache = home.join(".npm");
    if super::is_real_dir(&npm_cache.join("_cacache")) {
        out.push(npm_candidate(
            npm_cache,
            native_commands::find_tool(home, "npm"),
        ));
    }

    let store = home.join(PNPM_STORE);
    if super::is_real_dir(&store) {
        out.push(pnpm_candidate(
            store,
            native_commands::find_tool(home, "pnpm"),
        ));
    }

    for (path, version, manager) in old_node_versions(home) {
        out.push(candidate(
            path,
            &format!("Node {version}"),
            "Node Version",
            Safety::Review,
            format!(
                "Node {version} installed with {manager}. A newer version is installed. Global npm packages installed under this version are removed with it, and projects pinned to it (.nvmrc) need it installed again."
            ),
            Action::DeletePath,
        ));
    }
}

fn candidate(
    path: PathBuf,
    label: &str,
    category: &str,
    safety: Safety,
    description: String,
    action: Action,
) -> Candidate {
    Candidate {
        path,
        file_type: label.to_string(),
        category: category.to_string(),
        safety,
        description,
        blocked_if_running: Vec::new(),
        project_root: None,
        action,
        known_size: None,
        min_bytes: 1,
        group: "Dev Tools",
        module: "node",
        catch_all: false,
    }
}

/// The item is `~/.npm/_cacache`; the command gets `~/.npm` as its cache setting.
pub(super) fn npm_candidate(npm_dir: PathBuf, npm: Option<PathBuf>) -> Candidate {
    let action = match npm {
        Some(npm) => Action::Command(NativeCommand::NpmCacheClean {
            npm,
            cache: npm_dir.clone(),
        }),
        None => Action::DeletePath,
    };
    candidate(
        npm_dir.join("_cacache"),
        "npm Cache",
        "Package Cache",
        Safety::Safe,
        "Packages npm downloaded before. npm downloads them again when a project needs them."
            .to_string(),
        action,
    )
}

pub(super) fn pnpm_candidate(store: PathBuf, pnpm: Option<PathBuf>) -> Candidate {
    match pnpm {
        Some(pnpm) => candidate(
            store.clone(),
            "pnpm Store",
            "Global Package Store",
            Safety::Safe,
            "Runs pnpm store prune, which removes only packages no project uses. The size shown is the whole store; the amount actually freed is usually smaller and is reported after cleaning.".to_string(),
            Action::Command(NativeCommand::PnpmStorePrune { pnpm, store }),
        ),
        None => candidate(
            store,
            "pnpm Store",
            "Global Package Store",
            Safety::Review,
            "The whole pnpm store. pnpm was not found, so its prune command cannot run. Installed projects keep working, but the next install downloads every package again.".to_string(),
            Action::DeletePath,
        ),
    }
}

/// "v20.11.0" -> [20, 11, 0]
fn node_version(name: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = name
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse().ok())
        .collect();
    parts.filter(|p| p.len() == 3)
}

fn version_dirs(dir: &Path) -> Vec<(Vec<u64>, String, PathBuf)> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            node_version(&name).map(|ver| (ver, name, e.path()))
        })
        .collect();
    v.sort();
    v
}

pub(super) fn newest_nvm_bin(home: &Path) -> Option<PathBuf> {
    version_dirs(&home.join(".nvm/versions/node"))
        .pop()
        .map(|(_, _, p)| p.join("bin"))
}

/// Does version "v20.11.0" satisfy an alias like "20", "20.11" or "v20.11.0"?
fn matches(version: &str, alias: &str) -> bool {
    let v = version.trim_start_matches('v');
    let a = alias.trim_start_matches('v');
    v == a || v.starts_with(&format!("{a}."))
}

/// Follow nvm aliases ("default" -> "lts/*" -> "lts/iron" -> "v20.11.0").
/// `Some(None)` means "the newest version"; `None` means it could not be resolved.
fn nvm_default(home: &Path) -> Option<Option<String>> {
    let alias_dir = home.join(".nvm/alias");
    let mut name = "default".to_string();
    for _ in 0..8 {
        let Ok(text) = fs::read_to_string(alias_dir.join(&name)) else {
            // No default alias at all: nvm uses whatever is newest.
            return (name == "default").then_some(None);
        };
        let value = text.trim().to_string();
        if value.starts_with('v') || value.starts_with(|c: char| c.is_ascii_digit()) {
            return Some(Some(value));
        }
        if value == "node" || value == "stable" {
            return Some(None);
        }
        name = value;
    }
    None
}

/// Older Node versions as (folder, version, manager).
fn old_node_versions(home: &Path) -> Vec<(PathBuf, String, &'static str)> {
    let mut out = Vec::new();

    // nvm
    let nvm = version_dirs(&home.join(".nvm/versions/node"));
    if let Some(default) = nvm_default(home) {
        let keep_default = default.and_then(|alias| {
            nvm.iter()
                .rev()
                .find(|(_, name, _)| matches(name, &alias))
                .map(|(_, name, _)| name.clone())
        });
        let newest = nvm.last().map(|(_, n, _)| n.clone());
        for (_, name, path) in &nvm {
            if Some(name) != newest.as_ref() && Some(name) != keep_default.as_ref() {
                out.push((path.clone(), name.clone(), "nvm"));
            }
        }
    }

    // fnm: `aliases/default` is a symlink into `node-versions/<v>/installation`.
    for root in [
        "Library/Application Support/fnm",
        ".local/share/fnm",
        ".fnm",
    ] {
        let root = home.join(root);
        let versions = version_dirs(&root.join("node-versions"));
        let default = fs::canonicalize(root.join("aliases/default")).ok();
        let newest = versions.last().map(|(_, n, _)| n.clone());
        for (_, name, path) in &versions {
            let is_default = default
                .as_ref()
                .is_some_and(|d| fs::canonicalize(path).is_ok_and(|p| d.starts_with(p)));
            if Some(name) != newest.as_ref() && !is_default {
                out.push((path.clone(), name.clone(), "fnm"));
            }
        }
    }

    // Volta: the default is in tools/user/platform.json.
    let volta = version_dirs(&home.join(".volta/tools/image/node"));
    let volta_default = fs::read_to_string(home.join(".volta/tools/user/platform.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.pointer("/node/runtime")?.as_str().map(str::to_string));
    let newest = volta.last().map(|(_, n, _)| n.clone());
    for (_, name, path) in &volta {
        let is_default = volta_default.as_ref().is_some_and(|d| matches(name, d));
        if Some(name) != newest.as_ref() && !is_default {
            out.push((path.clone(), name.clone(), "Volta"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mkdirs(base: &Path, names: &[&str]) {
        for n in names {
            fs::create_dir_all(base.join(n).join("bin")).unwrap();
        }
    }

    fn listed(home: &Path) -> Vec<String> {
        let mut v: Vec<String> = old_node_versions(home)
            .into_iter()
            .map(|(_, name, manager)| format!("{manager} {name}"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn nvm_keeps_newest_and_default() {
        let d = TempDir::new().unwrap();
        mkdirs(
            &d.path().join(".nvm/versions/node"),
            &["v18.19.0", "v20.10.0", "v20.11.1", "v22.3.0"],
        );
        fs::create_dir_all(d.path().join(".nvm/alias")).unwrap();
        fs::write(d.path().join(".nvm/alias/default"), "20\n").unwrap();
        assert_eq!(listed(d.path()), vec!["nvm v18.19.0", "nvm v20.10.0"]);
    }

    #[test]
    fn nvm_follows_lts_aliases() {
        let d = TempDir::new().unwrap();
        mkdirs(
            &d.path().join(".nvm/versions/node"),
            &["v18.19.0", "v20.11.1", "v22.3.0"],
        );
        fs::create_dir_all(d.path().join(".nvm/alias/lts")).unwrap();
        fs::write(d.path().join(".nvm/alias/default"), "lts/*").unwrap();
        fs::write(d.path().join(".nvm/alias/lts/*"), "lts/iron").unwrap();
        fs::write(d.path().join(".nvm/alias/lts/iron"), "v20.11.1").unwrap();
        assert_eq!(listed(d.path()), vec!["nvm v18.19.0"]);
    }

    #[test]
    fn unresolvable_nvm_default_offers_nothing() {
        let d = TempDir::new().unwrap();
        mkdirs(
            &d.path().join(".nvm/versions/node"),
            &["v18.19.0", "v22.3.0"],
        );
        fs::create_dir_all(d.path().join(".nvm/alias")).unwrap();
        fs::write(d.path().join(".nvm/alias/default"), "lts/missing").unwrap();
        assert!(listed(d.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn fnm_and_volta_keep_their_defaults() {
        let d = TempDir::new().unwrap();
        let fnm = d.path().join(".local/share/fnm");
        for v in ["v18.19.0", "v20.11.1", "v22.3.0"] {
            fs::create_dir_all(fnm.join("node-versions").join(v).join("installation/bin")).unwrap();
        }
        fs::create_dir_all(fnm.join("aliases")).unwrap();
        std::os::unix::fs::symlink(
            fnm.join("node-versions/v18.19.0/installation"),
            fnm.join("aliases/default"),
        )
        .unwrap();

        let volta = d.path().join(".volta");
        mkdirs(
            &volta.join("tools/image/node"),
            &["16.20.2", "20.11.1", "22.3.0"],
        );
        fs::create_dir_all(volta.join("tools/user")).unwrap();
        fs::write(
            volta.join("tools/user/platform.json"),
            r#"{"node":{"runtime":"20.11.1","npm":null}}"#,
        )
        .unwrap();

        assert_eq!(listed(d.path()), vec!["Volta 16.20.2", "fnm v20.11.1"]);
    }

    #[test]
    fn single_version_is_never_offered() {
        let d = TempDir::new().unwrap();
        mkdirs(&d.path().join(".nvm/versions/node"), &["v22.3.0"]);
        assert!(listed(d.path()).is_empty());
        assert_eq!(
            newest_nvm_bin(d.path()),
            Some(d.path().join(".nvm/versions/node/v22.3.0/bin"))
        );
    }

    #[test]
    fn npm_cache_uses_command_when_npm_is_found() {
        let d = TempDir::new().unwrap();
        let npm_dir = d.path().join(".npm");
        let c = npm_candidate(
            npm_dir.clone(),
            Some(PathBuf::from("/opt/homebrew/bin/npm")),
        );
        assert_eq!(c.path, npm_dir.join("_cacache"));
        assert_eq!(c.safety, Safety::Safe);
        assert!(matches!(
            c.action,
            Action::Command(NativeCommand::NpmCacheClean { .. })
        ));
        let c = npm_candidate(npm_dir, None);
        assert_eq!(c.action, Action::DeletePath);
    }

    #[test]
    fn pnpm_store_is_pruned_or_reviewed() {
        let store = PathBuf::from("/Users/x/Library/pnpm/store");
        let c = pnpm_candidate(
            store.clone(),
            Some(PathBuf::from("/Users/x/Library/pnpm/pnpm")),
        );
        assert_eq!(c.safety, Safety::Safe);
        assert!(matches!(
            c.action,
            Action::Command(NativeCommand::PnpmStorePrune { .. })
        ));
        let c = pnpm_candidate(store, None);
        assert_eq!(c.safety, Safety::Review);
        assert_eq!(c.action, Action::DeletePath);
    }

    #[test]
    fn module_off_lists_nothing() {
        let d = TempDir::new().unwrap();
        fs::create_dir_all(d.path().join(".npm/_cacache")).unwrap();
        let env = Env {
            home: d.path().to_path_buf(),
        };
        let mut out = Vec::new();
        collect(&env, &HashSet::from(["flutter"]), &mut out);
        assert!(out.is_empty());
    }
}
