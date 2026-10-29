//! Folder sizes for the disk map (PLAN 4.4): the direct children of one folder,
//! each measured with the same code the scan uses, so numbers match Review.

use crate::cleanup_engine::disk_usage_cancellable;
use rayon::prelude::*;
use std::fs;
use std::path::Path;

/// Squares drawn for one folder. The rest are merged into one "other" square,
/// so a folder with thousands of files stays readable.
const MAX_ENTRIES: usize = 60;

#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(serde::Serialize, Debug)]
pub struct DiskMap {
    pub path: String,
    pub total: u64,
    /// Largest first, at most `MAX_ENTRIES`.
    pub entries: Vec<MapEntry>,
    /// Bytes and count of the entries that did not fit, including empty ones.
    pub other: u64,
    pub other_count: usize,
}

/// Sizes the direct children of `path`. Symlinks are skipped, never followed.
/// Returns `None` when `cancelled` turns true.
pub fn disk_map(path: &Path, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<DiskMap> {
    let children: Vec<(String, std::path::PathBuf, bool)> = fs::read_dir(path)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let kind = e.file_type().ok()?;
            if kind.is_symlink() {
                return None;
            }
            Some((e.file_name().to_string_lossy().into_owned(), e.path(), kind.is_dir()))
        })
        .collect();

    let mut sized: Vec<MapEntry> = children
        .par_iter()
        .map(|(name, p, is_dir)| MapEntry {
            name: name.clone(),
            path: p.to_string_lossy().into_owned(),
            size: disk_usage_cancellable(p, cancelled),
            is_dir: *is_dir,
        })
        .collect();

    if cancelled() {
        return None;
    }

    sized.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    let total = sized.iter().map(|e| e.size).sum();

    // Empty entries cannot be drawn, so they join the rest.
    let visible = sized.iter().take(MAX_ENTRIES).take_while(|e| e.size > 0).count();
    let rest = sized.split_off(visible);
    Some(DiskMap {
        path: path.to_string_lossy().into_owned(),
        total,
        entries: sized,
        other: rest.iter().map(|e| e.size).sum(),
        other_count: rest.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn write(path: &Path, kb: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; kb * 1024]).unwrap();
    }

    #[test]
    fn lists_children_largest_first_with_folder_totals() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("small.bin"), 8);
        write(&d.path().join("big/a.bin"), 64);
        write(&d.path().join("big/deep/b.bin"), 64);

        let map = disk_map(d.path(), &|| false).unwrap();
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].name, "big");
        assert!(map.entries[0].is_dir);
        assert_eq!(map.entries[1].name, "small.bin");
        assert!(!map.entries[1].is_dir);
        assert!(map.entries[0].size > map.entries[1].size);
        assert_eq!(map.total, map.entries.iter().map(|e| e.size).sum::<u64>());
        assert_eq!(map.other_count, 0);
    }

    #[test]
    fn merges_the_tail_and_empty_entries_into_other() {
        let d = TempDir::new().unwrap();
        for i in 0..(MAX_ENTRIES + 5) {
            write(&d.path().join(format!("f{i:03}.bin")), 4);
        }
        fs::create_dir(d.path().join("empty")).unwrap();

        let map = disk_map(d.path(), &|| false).unwrap();
        assert_eq!(map.entries.len(), MAX_ENTRIES);
        assert_eq!(map.other_count, 6);
        assert!(map.other > 0);
        assert_eq!(
            map.total,
            map.entries.iter().map(|e| e.size).sum::<u64>() + map.other
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("real/a.bin"), 16);
        std::os::unix::fs::symlink(d.path().join("real"), d.path().join("link")).unwrap();

        let map = disk_map(d.path(), &|| false).unwrap();
        assert_eq!(map.entries.len(), 1);
        assert_eq!(map.entries[0].name, "real");
    }

    #[test]
    fn unreadable_or_missing_folder_is_none() {
        let d = TempDir::new().unwrap();
        assert!(disk_map(&d.path().join("nope"), &|| false).is_none());
    }

    #[test]
    fn cancel_returns_none() {
        let d = TempDir::new().unwrap();
        for i in 0..20 {
            write(&d.path().join(format!("d{i}/a.bin")), 4);
        }
        let calls = AtomicUsize::new(0);
        let map = disk_map(d.path(), &|| calls.fetch_add(1, Ordering::Relaxed) >= 3);
        assert!(map.is_none());
    }
}
