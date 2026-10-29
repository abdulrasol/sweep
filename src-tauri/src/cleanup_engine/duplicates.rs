//! Duplicate-file finder (PLAN 4.3b).
//!
//! Files of the same length are compared by a hash of their first 64 KB, and
//! only the ones that still match are hashed completely (BLAKE3). Like the
//! large-file finder it lists the user's own files as `Danger` items that can
//! only be moved to the Trash. The app refuses to remove the last remaining
//! copy of a set (see `cleanup_item`).

use super::large::{build_item, collect_files, Found};
use super::{ScanHooks, ScannedItem};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// The list is cut to this many sets, the ones wasting the most space first.
pub const MAX_GROUPS: usize = 100;
/// Bytes read for the quick first comparison.
const PREFIX_BYTES: usize = 64 * 1024;
const READ_CHUNK: usize = 1024 * 1024;

/// A set of identical files, oldest first.
pub struct ScannedGroup {
    /// Space one copy takes on disk.
    pub size_each: u64,
    /// Space freed by removing every copy but one.
    pub wasted: u64,
    pub items: Vec<ScannedItem>,
}

pub(super) fn find_duplicates(
    base: &Path,
    min_bytes: u64,
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    hooks: &ScanHooks<'_>,
) -> Option<Vec<ScannedGroup>> {
    find_duplicates_with_limit(base, min_bytes, MAX_GROUPS, is_ignored, hooks)
}

fn find_duplicates_with_limit(
    base: &Path,
    min_bytes: u64,
    limit: usize,
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    hooks: &ScanHooks<'_>,
) -> Option<Vec<ScannedGroup>> {
    let files = collect_files(base, min_bytes, is_ignored, hooks)?;

    // Only files of the same length can be identical.
    let mut by_len: HashMap<u64, Vec<Found>> = HashMap::new();
    for file in files {
        by_len.entry(file.len).or_default().push(file);
    }
    let candidates: Vec<Vec<Found>> = by_len
        .into_values()
        .filter(|group| group.len() > 1)
        .collect();

    let cancelled = hooks.cancelled;
    let mut sets: Vec<Vec<Found>> = candidates
        .into_par_iter()
        .flat_map_iter(|group| identical_sets(group, cancelled))
        .collect();
    if (hooks.cancelled)() {
        return None;
    }

    for set in &mut sets {
        // Oldest first: that is the copy most people want to keep.
        set.sort_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| a.path.cmp(&b.path))
        });
    }
    sets.sort_by(|a, b| {
        wasted(b)
            .cmp(&wasted(a))
            .then_with(|| a[0].path.cmp(&b[0].path))
    });
    sets.truncate(limit);

    let groups = sets
        .into_iter()
        .filter_map(|set| {
            let size_each = set[0].size;
            let copies = set.len();
            let note = format!(
                "One of {copies} identical copies. Keep at least one: the others are moved to the Trash, never deleted for good."
            );
            let items: Vec<ScannedItem> = set
                .into_iter()
                .filter_map(|f| build_item(f, "duplicates", "Duplicate", "Duplicates", &note))
                .collect();
            if items.len() < 2 {
                return None;
            }
            Some(ScannedGroup {
                size_each,
                wasted: size_each.saturating_mul(items.len() as u64 - 1),
                items,
            })
        })
        .collect();
    Some(groups)
}

/// Space freed by removing all but one file of the set.
fn wasted(set: &[Found]) -> u64 {
    set[0].size.saturating_mul(set.len() as u64 - 1)
}

/// Split files of one length into sets of identical files (two or more each).
fn identical_sets(group: Vec<Found>, cancelled: &(dyn Fn() -> bool + Sync)) -> Vec<Vec<Found>> {
    let by_prefix = split_by(group, |f| hash_file(&f.path, Some(PREFIX_BYTES), cancelled));
    by_prefix
        .into_iter()
        .flat_map(|set| split_by(set, |f| hash_file(&f.path, None, cancelled)))
        .collect()
}

/// Group by `key` and keep only the groups of two or more. A file whose key
/// cannot be computed (unreadable, cancelled) is left out.
fn split_by(group: Vec<Found>, key: impl Fn(&Found) -> Option<[u8; 32]>) -> Vec<Vec<Found>> {
    if group.len() < 2 {
        return Vec::new();
    }
    let mut map: HashMap<[u8; 32], Vec<Found>> = HashMap::new();
    for file in group {
        if let Some(k) = key(&file) {
            map.entry(k).or_default().push(file);
        }
    }
    map.into_values().filter(|set| set.len() > 1).collect()
}

/// BLAKE3 of the first `limit` bytes of a file, or of all of it.
fn hash_file(
    path: &Path,
    limit: Option<usize>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; limit.map_or(READ_CHUNK, |l| l.min(READ_CHUNK))];
    let mut remaining = limit;
    loop {
        if cancelled() {
            return None;
        }
        let want = remaining.map_or(buf.len(), |r| r.min(buf.len()));
        if want == 0 {
            break;
        }
        let n = file.read(&mut buf[..want]).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(r) = remaining.as_mut() {
            *r -= n;
        }
    }
    Some(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup_engine::{Safety, ScanProgress};
    use std::fs;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const MIN: u64 = 100_000;

    fn write_bytes(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// 200,000 bytes of 1s, with byte `at` changed when given.
    fn content(len: usize, change_at: Option<usize>) -> Vec<u8> {
        let mut data = vec![1u8; len];
        if let Some(at) = change_at {
            data[at] = 2;
        }
        data
    }

    fn set_modified(path: &Path, secs_after_epoch: u64) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs_after_epoch))
            .unwrap();
    }

    fn run(base: &Path, limit: usize, cancelled: bool) -> Option<Vec<ScannedGroup>> {
        let cancel = move || cancelled;
        let progress = |_: ScanProgress| {};
        let hooks = ScanHooks {
            cancelled: &cancel,
            progress: &progress,
        };
        find_duplicates_with_limit(base, MIN, limit, &|_: &Path| false, &hooks)
    }

    fn names(group: &ScannedGroup) -> Vec<String> {
        group
            .items
            .iter()
            .map(|i| {
                Path::new(&i.item.path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn groups_only_files_with_identical_contents() {
        let d = TempDir::new().unwrap();
        write_bytes(&d.path().join("a/x.bin"), &content(200_000, None));
        write_bytes(&d.path().join("b/y.bin"), &content(200_000, None));
        // Same length, different byte after the quick-hash prefix.
        write_bytes(&d.path().join("c/z.bin"), &content(200_000, Some(150_000)));
        // Same length, different byte inside the prefix.
        write_bytes(&d.path().join("d/w.bin"), &content(200_000, Some(10)));
        // Different length.
        write_bytes(&d.path().join("e/u.bin"), &content(210_000, None));

        let groups = run(d.path(), MAX_GROUPS, false).unwrap();
        assert_eq!(groups.len(), 1);
        let mut found = names(&groups[0]);
        found.sort();
        assert_eq!(found, vec!["x.bin", "y.bin"]);
    }

    #[test]
    fn items_are_danger_user_files_with_a_keep_one_note() {
        let d = TempDir::new().unwrap();
        write_bytes(&d.path().join("a/x.mov"), &content(200_000, None));
        write_bytes(&d.path().join("b/y.mov"), &content(200_000, None));
        write_bytes(&d.path().join("c/z.mov"), &content(200_000, None));
        let groups = run(d.path(), MAX_GROUPS, false).unwrap();
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert_eq!(group.items.len(), 3);
        assert_eq!(group.wasted, group.size_each * 2);
        for item in &group.items {
            assert_eq!(item.item.status, Safety::Danger);
            assert_eq!(item.item.module, "duplicates");
            assert_eq!(item.item.category, "Duplicate");
            assert_eq!(item.item.group, "Duplicates");
            assert_eq!(item.item.size_bytes, group.size_each);
            assert!(item.item.description.contains("One of 3 identical copies"));
            assert!(item.blocked_if_running.is_empty());
        }
    }

    #[test]
    fn sets_are_ordered_by_wasted_space() {
        let d = TempDir::new().unwrap();
        // Three copies of 200 KB waste 400 KB.
        for n in ["a", "b", "c"] {
            write_bytes(&d.path().join(n).join("small.bin"), &content(200_000, None));
        }
        // Two copies of 600 KB waste 600 KB.
        for n in ["d", "e"] {
            write_bytes(&d.path().join(n).join("big.bin"), &content(600_000, None));
        }
        let groups = run(d.path(), MAX_GROUPS, false).unwrap();
        assert_eq!(groups.len(), 2);
        assert!(groups[0].wasted > groups[1].wasted);
        assert_eq!(names(&groups[0])[0], "big.bin");
    }

    #[test]
    fn the_oldest_copy_comes_first() {
        let d = TempDir::new().unwrap();
        let old = d.path().join("z/old.bin");
        let mid = d.path().join("m/mid.bin");
        let new = d.path().join("a/new.bin");
        for p in [&old, &mid, &new] {
            write_bytes(p, &content(200_000, None));
        }
        set_modified(&old, 1_000_000_000);
        set_modified(&mid, 1_500_000_000);
        set_modified(&new, 1_700_000_000);
        let groups = run(d.path(), MAX_GROUPS, false).unwrap();
        assert_eq!(names(&groups[0]), vec!["old.bin", "mid.bin", "new.bin"]);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_not_duplicates() {
        let d = TempDir::new().unwrap();
        write_bytes(&d.path().join("a/x.bin"), &content(200_000, None));
        fs::create_dir_all(d.path().join("b")).unwrap();
        fs::hard_link(d.path().join("a/x.bin"), d.path().join("b/linked.bin")).unwrap();
        assert!(run(d.path(), MAX_GROUPS, false).unwrap().is_empty());
    }

    #[test]
    fn small_files_and_skipped_folders_are_left_out() {
        let d = TempDir::new().unwrap();
        write_bytes(&d.path().join("a/s1.bin"), &content(10_000, None));
        write_bytes(&d.path().join("b/s2.bin"), &content(10_000, None));
        write_bytes(&d.path().join("node_modules/x.bin"), &content(200_000, None));
        write_bytes(&d.path().join("keep/y.bin"), &content(200_000, None));
        write_bytes(&d.path().join("Library/z.bin"), &content(200_000, None));
        assert!(run(d.path(), MAX_GROUPS, false).unwrap().is_empty());
    }

    #[test]
    fn only_the_biggest_sets_are_kept() {
        let d = TempDir::new().unwrap();
        for (i, len) in [200_000usize, 300_000, 400_000].iter().enumerate() {
            for side in ["l", "r"] {
                write_bytes(
                    &d.path().join(side).join(format!("f{i}.bin")),
                    &content(*len, None),
                );
            }
        }
        let groups = run(d.path(), 2, false).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(names(&groups[0])[0], "f2.bin");
        assert_eq!(names(&groups[1])[0], "f1.bin");
    }

    #[test]
    fn a_cancelled_search_returns_nothing() {
        let d = TempDir::new().unwrap();
        write_bytes(&d.path().join("a/x.bin"), &content(200_000, None));
        write_bytes(&d.path().join("b/y.bin"), &content(200_000, None));
        assert!(run(d.path(), MAX_GROUPS, true).is_none());
    }

    #[test]
    fn hashing_a_prefix_differs_from_hashing_everything() {
        let d = TempDir::new().unwrap();
        let a = d.path().join("a.bin");
        let b = d.path().join("b.bin");
        write_bytes(&a, &content(200_000, None));
        write_bytes(&b, &content(200_000, Some(150_000)));
        let never = || false;
        assert_eq!(
            hash_file(&a, Some(PREFIX_BYTES), &never),
            hash_file(&b, Some(PREFIX_BYTES), &never)
        );
        assert_ne!(hash_file(&a, None, &never), hash_file(&b, None, &never));
        assert!(hash_file(&d.path().join("missing"), None, &never).is_none());
        assert!(hash_file(&a, None, &|| true).is_none());
    }
}
