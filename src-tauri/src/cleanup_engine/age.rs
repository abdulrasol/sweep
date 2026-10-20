//! Project activity for the abandoned-projects filter (PLAN 3.6).
//!
//! `last_active` is the newest activity signal for a project: the HEAD commit
//! time or the newest source-file mtime, whichever is later. Generated folders
//! (build output, package caches, `.git`) are ignored so a build or a checkout
//! does not look like activity.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;
use walkdir::{DirEntry, WalkDir};

/// Stop walking a project after this many entries.
const MAX_ENTRIES: usize = 5000;
/// Only files this close to the project root count as source activity.
const WALK_DEPTH: usize = 4;
/// Seconds in one day, used by `age_days` in tests.
#[cfg(test)]
const DAY_SECS: i64 = 86_400;

/// Newest activity time (unix seconds) under `root`, or `None` when nothing can
/// be determined. A cancelled scan returns the best value found so far, which
/// is `None` when cancellation is seen before any work starts.
pub(super) fn project_last_active(root: &Path, cancelled: &dyn Fn() -> bool) -> Option<i64> {
    if cancelled() {
        return None;
    }

    let mut newest = head_commit_time(root);
    let skip = skip_dirs();
    let walker = WalkDir::new(root)
        .max_depth(WALK_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| keep_entry(entry, &skip));

    let mut seen = 0usize;
    for entry in walker.flatten() {
        if cancelled() {
            break;
        }
        seen += 1;
        if seen > MAX_ENTRIES {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let Ok(since) = modified.duration_since(UNIX_EPOCH) else {
            continue;
        };
        let secs = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
        newest = Some(newest.map_or(secs, |n| n.max(secs)));
    }
    newest
}

/// Whole days since `last_active`, clamped at zero for future timestamps.
#[cfg(test)]
pub(super) fn age_days(last_active: i64, now: i64) -> i64 {
    ((now - last_active) / DAY_SECS).max(0)
}

/// Committer time of HEAD, or `None` for a missing or unborn HEAD.
fn head_commit_time(root: &Path) -> Option<i64> {
    let repo = open_project_repo(root)?;
    let commit = repo.head().ok()?.peel_to_commit().ok()?;
    Some(commit.time().seconds())
}

/// `Repository::open` first. `discover` only counts when its workdir is `root`,
/// so a project inside another repository does not borrow the parent's commit.
fn open_project_repo(root: &Path) -> Option<git2::Repository> {
    if let Ok(repo) = git2::Repository::open(root) {
        return Some(repo);
    }
    let repo = git2::Repository::discover(root).ok()?;
    let workdir = repo.workdir()?;
    same_path(workdir, root).then_some(repo)
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Folder names that never count as activity: the scan skips them, and every
/// project cleanup target contributes its concrete directory name (globs
/// stripped). Only the target folder itself is skipped, never a parent such as
/// `src` or `ios`, so source edits still count.
fn skip_dirs() -> HashSet<&'static str> {
    let mut skip: HashSet<&'static str> = super::SKIP_DIRS.iter().copied().collect();
    for rule in super::PROJECT_RULES {
        for target in rule.targets {
            if let Some(name) = target_dir_name(target.rel) {
                skip.insert(name);
            }
        }
    }
    skip
}

fn target_dir_name(rel: &'static str) -> Option<&'static str> {
    rel.split('/')
        .rfind(|part| !part.is_empty() && !part.contains('*') && !part.contains('?'))
}

fn keep_entry(entry: &DirEntry, skip: &HashSet<&'static str>) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    if name == ".git" {
        return false;
    }
    if entry.file_type().is_dir() {
        return !name.starts_with('.') && !skip.contains(name.as_ref());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    /// 2020-01-01 00:00:00 UTC, used as a fixed commit time.
    const COMMIT_TIME: i64 = 1_577_836_800;
    /// 2100-01-01 00:00:00 UTC, safely newer than any test file.
    const FUTURE: i64 = 4_102_444_800;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; bytes]).unwrap();
    }

    fn set_mtime(path: &Path, secs: i64) {
        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64);
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(when).unwrap();
    }

    fn init_repo(root: &Path) -> git2::Repository {
        fs::create_dir_all(root).unwrap();
        let repo = git2::Repository::init(root).unwrap();
        let sig = git2::Signature::new("t", "t@t", &git2::Time::new(COMMIT_TIME, 0)).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }
        repo
    }

    fn project(name: &str) -> (TempDir, PathBuf) {
        let d = TempDir::new().unwrap();
        let root = d.path().join(name);
        (d, root)
    }

    #[test]
    fn commit_time_is_used_when_files_are_older() {
        let (_d, root) = project("repo");
        init_repo(&root);
        let source = root.join("src/main.rs");
        write(&source, 10);
        set_mtime(&source, COMMIT_TIME - 1000);

        assert_eq!(project_last_active(&root, &|| false), Some(COMMIT_TIME));
    }

    #[test]
    fn newer_source_file_wins_over_the_commit() {
        let (_d, root) = project("repo");
        init_repo(&root);
        let source = root.join("src/main.rs");
        write(&source, 10);
        set_mtime(&source, COMMIT_TIME + 5000);

        assert_eq!(
            project_last_active(&root, &|| false),
            Some(COMMIT_TIME + 5000)
        );
    }

    #[test]
    fn generated_folders_are_ignored() {
        let (_d, root) = project("repo");
        init_repo(&root);
        let source = root.join("src/main.rs");
        write(&source, 10);
        set_mtime(&source, COMMIT_TIME - 1000);

        for rel in [
            "node_modules/pkg/a.js",
            "target/debug/out.bin",
            ".git/leftover",
            "build/out.bin",
        ] {
            let path = root.join(rel);
            write(&path, 10);
            set_mtime(&path, FUTURE);
        }

        assert_eq!(project_last_active(&root, &|| false), Some(COMMIT_TIME));
    }

    #[test]
    fn non_git_project_uses_the_newest_source_mtime() {
        let (_d, root) = project("plain");
        let older = root.join("src/main.rs");
        let newer = root.join("README.md");
        write(&older, 10);
        write(&newer, 10);
        set_mtime(&older, 1_600_000_000);
        set_mtime(&newer, 1_600_000_500);

        assert_eq!(project_last_active(&root, &|| false), Some(1_600_000_500));
    }

    #[test]
    fn empty_project_has_no_activity() {
        let (_d, root) = project("empty");
        fs::create_dir_all(&root).unwrap();

        assert_eq!(project_last_active(&root, &|| false), None);
    }

    #[test]
    fn cancelled_scan_returns_none_without_panicking() {
        let (_d, root) = project("repo");
        init_repo(&root);
        write(&root.join("src/main.rs"), 10);

        assert_eq!(project_last_active(&root, &|| true), None);
    }

    #[test]
    fn a_parent_repository_commit_is_not_borrowed() {
        let (_d, parent) = project("parent");
        init_repo(&parent);
        let nested = parent.join("nested");
        let source = nested.join("main.rs");
        write(&source, 10);
        set_mtime(&source, COMMIT_TIME - 5000);

        // The nested folder is not its own repository, so only its files count.
        assert_eq!(
            project_last_active(&nested, &|| false),
            Some(COMMIT_TIME - 5000)
        );
    }

    #[test]
    fn age_days_clamps_and_counts_boundaries() {
        assert_eq!(age_days(100, 50), 0);
        assert_eq!(age_days(0, -86_400 * 5), 0);
        assert_eq!(age_days(0, 2_592_000), 30);
        assert_eq!(age_days(0, 2_592_000 - 1), 29);
        assert_eq!(age_days(0, 31_536_000), 365);
    }
}
