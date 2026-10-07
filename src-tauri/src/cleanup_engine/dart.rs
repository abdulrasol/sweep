//! Dart's global package cache, cleaned with `dart pub cache clean` (PLAN 2.4).
//!
//! Flutter project folders (build, .dart_tool, Pods...) stay in the project rules and
//! are deleted directly, behind the git checks. `flutter clean` was left out on purpose:
//! it also deletes generated files the scan never showed.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("flutter") {
        return;
    }
    // PUB_CACHE is ignored on purpose: the command pins it to this folder, so the
    // cache Sweep shows is the one dart cleans.
    let cache = env.home.join(".pub-cache");
    if !super::is_real_dir(&cache) {
        return;
    }
    if let Some(dart) = native_commands::find_dart(&env.home) {
        out.push(pub_cache_candidate(cache, dart));
    }
}

pub(super) fn pub_cache_candidate(cache: PathBuf, dart: PathBuf) -> Candidate {
    let globals = global_packages(&cache);
    let mut description = "Packages downloaded by dart and flutter pub get. They download again the next time a project needs them.".to_string();
    if !globals.is_empty() {
        description.push_str(&format!(
            " This also removes packages you installed with dart pub global activate ({}). Activate them again afterwards.",
            globals.join(", ")
        ));
    }
    Candidate {
        path: cache.clone(),
        file_type: "Pub Cache".to_string(),
        category: "Package Cache".to_string(),
        safety: Safety::Review,
        description,
        blocked_if_running: Vec::new(),
        project_root: None,
        action: Action::Command(NativeCommand::DartPubCacheClean { dart, cache }),
        known_size: None,
        min_bytes: 1,
        group: "Dev Tools",
        catch_all: false,
    }
}

/// Names of packages under `global_packages`, sorted.
fn global_packages(cache: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(cache.join("global_packages"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pub_cache_is_review_and_names_global_packages() {
        let d = TempDir::new().unwrap();
        let cache = d.path().join(".pub-cache");
        fs::create_dir_all(cache.join("hosted/pub.dev/http-1.2.0")).unwrap();
        fs::create_dir_all(cache.join("global_packages/fvm")).unwrap();
        fs::create_dir_all(cache.join("global_packages/flutterfire_cli")).unwrap();
        let c = pub_cache_candidate(cache.clone(), PathBuf::from("/usr/local/bin/dart"));
        assert_eq!(c.safety, Safety::Review);
        assert!(c.description.contains("(flutterfire_cli, fvm)"));
        assert_eq!(
            c.action,
            Action::Command(NativeCommand::DartPubCacheClean {
                dart: PathBuf::from("/usr/local/bin/dart"),
                cache,
            })
        );
    }

    #[test]
    fn pub_cache_without_global_packages_has_no_warning() {
        let d = TempDir::new().unwrap();
        let cache = d.path().join(".pub-cache");
        fs::create_dir_all(cache.join("hosted")).unwrap();
        let c = pub_cache_candidate(cache, PathBuf::from("/usr/local/bin/dart"));
        assert!(!c.description.contains("global activate"));
    }

    #[test]
    fn no_pub_cache_or_module_off_lists_nothing() {
        let d = TempDir::new().unwrap();
        let env = Env {
            home: d.path().to_path_buf(),
        };
        let mut out = Vec::new();
        collect(&env, &HashSet::from(["flutter"]), &mut out);
        assert!(out.is_empty(), "no ~/.pub-cache folder");

        fs::create_dir_all(d.path().join(".pub-cache/hosted")).unwrap();
        collect(&env, &HashSet::from(["node"]), &mut out);
        assert!(out.is_empty(), "flutter module is off");
    }
}
