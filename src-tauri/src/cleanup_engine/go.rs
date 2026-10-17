//! Go module cache, cleaned with `go clean -modcache` (PLAN 2.10).
//!
//! Downloaded module sources are cleaned using Go's official command.
//! The command handles read-only module permissions and pins GOMODCACHE.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::path::PathBuf;

pub(super) fn candidate(modcache: PathBuf, go: PathBuf) -> Candidate {
    Candidate {
        path: modcache.clone(),
        file_type: "Go Modules".to_string(),
        category: "Package Cache".to_string(),
        safety: Safety::Review,
        description: "Downloaded Go module sources (go clean -modcache). Go downloads them again on the next build, so a build that is offline, or that needs a private module you cannot reach right now, fails until they come back.".to_string(),
        blocked_if_running: vec!["go"],
        project_root: None,
        action: Action::Command(NativeCommand::GoCleanModCache { go, modcache }),
        known_size: None,
        min_bytes: 1,
        group: "Dev Tools",
        catch_all: false,
    }
}

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("go") {
        return;
    }
    // GOPATH and GOMODCACHE environment variables are intentionally ignored here
    // because the native command pins the exact directory it shows to the user.
    let modcache = env.home.join("go/pkg/mod");
    if !super::is_real_dir(&modcache) {
        return;
    }
    let Some(go) = native_commands::find_tool(&env.home, "go") else {
        return;
    };
    out.push(candidate(modcache, go));
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn candidate_is_review_and_names_the_command() {
        let d = TempDir::new().unwrap();
        let modcache = d.path().join("go/pkg/mod");
        let go = d.path().join("go/bin/go");
        let c = candidate(modcache.clone(), go.clone());
        assert_eq!(c.safety, Safety::Review);
        assert_eq!(c.blocked_if_running, vec!["go"]);
        assert!(c.path.ends_with("go/pkg/mod"));
        assert_eq!(
            c.action,
            Action::Command(NativeCommand::GoCleanModCache { go, modcache })
        );
    }

    #[test]
    fn collect_lists_nothing_without_a_real_module_folder() {
        let d = TempDir::new().unwrap();
        let home = d.path();

        // 1. home with go/bin/tool and go/src/app/main.go only (no pkg/mod) with enabled {"go"} -> empty
        let tool = home.join("go/bin/tool");
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(&tool, vec![0u8; 5000]).unwrap();
        let main_go = home.join("go/src/app/main.go");
        fs::create_dir_all(main_go.parent().unwrap()).unwrap();
        fs::write(&main_go, vec![0u8; 5000]).unwrap();

        let env = Env {
            home: home.to_path_buf(),
        };
        let enabled_go: HashSet<&str> = ["go"].into();
        let mut candidates = Vec::new();
        collect(&env, &enabled_go, &mut candidates);
        assert!(candidates.is_empty());

        // 2. module off: enabled {"node"} with a valid go/pkg/mod folder -> empty
        let real_mod = home.join("go/pkg/mod");
        fs::create_dir_all(&real_mod).unwrap();
        fs::write(real_mod.join("a"), vec![0u8; 5000]).unwrap();
        let enabled_node: HashSet<&str> = ["node"].into();
        let mut candidates = Vec::new();
        collect(&env, &enabled_node, &mut candidates);
        assert!(candidates.is_empty());

        // 3. pkg/mod is a symlink to another real folder with enabled {"go"} -> empty
        let d2 = TempDir::new().unwrap();
        let home2 = d2.path();
        let sym_target = home2.join("real_mod");
        fs::create_dir_all(&sym_target).unwrap();
        fs::write(sym_target.join("a"), vec![0u8; 5000]).unwrap();
        let pkg_dir = home2.join("go/pkg");
        fs::create_dir_all(&pkg_dir).unwrap();
        std::os::unix::fs::symlink(&sym_target, pkg_dir.join("mod")).unwrap();

        let env2 = Env {
            home: home2.to_path_buf(),
        };
        let mut candidates = Vec::new();
        collect(&env2, &enabled_go, &mut candidates);
        assert!(candidates.is_empty());
    }
}
