//! Conda package cache, cleaned with `conda clean --all` (PLAN 2.9c).
//!
//! Downloaded packages and tarballs that are no longer used by any environment
//! are cleaned using Conda's official command. Environments are left untouched.

use super::native_commands::{self, NativeCommand};
use super::{Action, Candidate, Env, Safety};
use std::collections::HashSet;

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("python") {
        return;
    }
    for name in native_commands::CONDA_ROOTS {
        let root = env.home.join(name);
        let pkgs = root.join("pkgs");
        if !super::is_real_dir(&pkgs) {
            continue;
        }
        let Some(conda) = native_commands::conda_in(&root) else {
            continue;
        };
        out.push(Candidate {
            path: pkgs,
            file_type: "Conda Packages".to_string(),
            category: "Package Cache".to_string(),
            safety: Safety::Review,
            description: "Downloaded Conda packages and tarballs that no environment uses (conda clean --all). Your environments are not touched. Creating a new environment downloads packages again.".to_string(),
            blocked_if_running: vec!["conda"],
            project_root: None,
            action: Action::Command(NativeCommand::CondaClean { conda, root }),
            known_size: None,
            min_bytes: 1,
            group: "Dev Tools",
            module: "python",
            catch_all: false,
        });
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_executable(path: &PathBuf, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn pkgs_candidate_is_review_and_envs_untouched() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let root = home.join("miniconda3");
        let pkgs = root.join("pkgs/a");
        fs::create_dir_all(pkgs.parent().unwrap()).unwrap();
        fs::write(&pkgs, vec![0u8; 5000]).unwrap();

        let env_file = root.join("envs/work/x");
        fs::create_dir_all(env_file.parent().unwrap()).unwrap();
        fs::write(&env_file, vec![0u8; 5000]).unwrap();

        let conda = root.join("bin/conda");
        make_executable(&conda, 0o755);

        let env = Env {
            home: home.to_path_buf(),
        };
        let enabled: HashSet<&str> = ["python"].into();
        let mut candidates = Vec::new();
        collect(&env, &enabled, &mut candidates);

        assert_eq!(candidates.len(), 1);
        let c = &candidates[0];
        assert!(c.path.ends_with("miniconda3/pkgs"));
        assert_eq!(c.safety, Safety::Review);
        assert_eq!(
            c.action,
            Action::Command(NativeCommand::CondaClean {
                conda,
                root: root.clone()
            })
        );
        assert!(env_file.exists());
    }

    #[test]
    fn no_conda_binary_lists_nothing() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let root = home.join("miniconda3");
        let pkgs = root.join("pkgs/a");
        fs::create_dir_all(pkgs.parent().unwrap()).unwrap();
        fs::write(&pkgs, vec![0u8; 5000]).unwrap();

        let env = Env {
            home: home.to_path_buf(),
        };
        let enabled: HashSet<&str> = ["python"].into();

        // 1. No bin/conda -> empty
        let mut candidates = Vec::new();
        collect(&env, &enabled, &mut candidates);
        assert!(candidates.is_empty());

        // 2. bin/conda present but mode 0o644 -> empty
        let conda = root.join("bin/conda");
        make_executable(&conda, 0o644);
        let mut candidates = Vec::new();
        collect(&env, &enabled, &mut candidates);
        assert!(candidates.is_empty());
    }

    #[test]
    fn module_off_and_unlisted_roots_list_nothing() {
        let d = TempDir::new().unwrap();
        let home = d.path();

        // Valid miniconda3
        let root = home.join("miniconda3");
        let pkgs = root.join("pkgs/a");
        fs::create_dir_all(pkgs.parent().unwrap()).unwrap();
        fs::write(&pkgs, vec![0u8; 5000]).unwrap();
        make_executable(&root.join("bin/conda"), 0o755);

        // Valid-looking layout under myconda
        let myconda = home.join("myconda");
        let mypkgs = myconda.join("pkgs/a");
        fs::create_dir_all(mypkgs.parent().unwrap()).unwrap();
        fs::write(&mypkgs, vec![0u8; 5000]).unwrap();
        make_executable(&myconda.join("bin/conda"), 0o755);

        let env = Env {
            home: home.to_path_buf(),
        };

        // enabled {"node"} -> empty even with valid miniconda3
        let enabled_node: HashSet<&str> = ["node"].into();
        let mut candidates = Vec::new();
        collect(&env, &enabled_node, &mut candidates);
        assert!(candidates.is_empty());

        // enabled {"python"} -> only miniconda3 matches, myconda is ignored
        let enabled_python: HashSet<&str> = ["python"].into();
        let mut candidates = Vec::new();
        collect(&env, &enabled_python, &mut candidates);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].path.ends_with("miniconda3/pkgs"));
    }

    #[test]
    fn symlinked_pkgs_is_never_listed() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let root = home.join("miniconda3");
        fs::create_dir_all(&root).unwrap();
        make_executable(&root.join("bin/conda"), 0o755);

        let real_pkgs = d.path().join("real_pkgs");
        fs::create_dir_all(&real_pkgs).unwrap();
        fs::write(real_pkgs.join("a"), vec![0u8; 5000]).unwrap();

        std::os::unix::fs::symlink(&real_pkgs, root.join("pkgs")).unwrap();

        let env = Env {
            home: home.to_path_buf(),
        };
        let enabled: HashSet<&str> = ["python"].into();
        let mut candidates = Vec::new();
        collect(&env, &enabled, &mut candidates);
        assert!(candidates.is_empty());
    }
}
