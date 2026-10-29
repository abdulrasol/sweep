//! Large-file finder (PLAN 4.3a).
//!
//! Lists the biggest files under a folder. These are the user's own files, not
//! caches, so every result is `Danger` and is only ever moved to the Trash
//! (`DeleteMode::TrashOnly`). Folders that other modules already cover (build
//! output, package caches, `Library`), hidden folders and package bundles such
//! as `.app` or `.photoslibrary` are not searched: a file inside a bundle is
//! not safe to remove on its own.

use super::{stable_id, Action, CleanupItem, Safety, ScanHooks, ScannedItem, SKIP_DIRS};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

/// The list is cut to this many files, largest first.
pub const MAX_RESULTS: usize = 200;
/// The walk never goes deeper than this below the chosen folder.
const MAX_DEPTH: usize = 24;

/// Folder extensions that are really single documents or libraries.
const PACKAGE_EXTENSIONS: &[&str] = &[
    "app",
    "photoslibrary",
    "musiclibrary",
    "tvlibrary",
    "imovielibrary",
    "fcpbundle",
    "logicx",
    "band",
    "sparsebundle",
    "vmwarevm",
    "pvm",
    "utm",
    "framework",
    "bundle",
    "plugin",
    "kext",
    "xcodeproj",
    "xcworkspace",
    "lrlibrary",
    "lrdata",
    "aplibrary",
    "rtfd",
    "docset",
];

/// Tool folders whose big files are not the user's own (compared in lower case).
const TOOL_DIRS: &[&str] = &["cache", "caches", "system-images"];

struct Found {
    path: PathBuf,
    size: u64,
    modified: Option<i64>,
}

/// Files of at least `min_bytes` under `base`, largest first, as `Danger`
/// items. `None` when the search was cancelled.
pub(super) fn find_large_files(
    base: &Path,
    min_bytes: u64,
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    hooks: &ScanHooks<'_>,
) -> Option<Vec<ScannedItem>> {
    find_large_files_with_limit(base, min_bytes, MAX_RESULTS, is_ignored, hooks)
}

fn find_large_files_with_limit(
    base: &Path,
    min_bytes: u64,
    limit: usize,
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    hooks: &ScanHooks<'_>,
) -> Option<Vec<ScannedItem>> {
    let found = Mutex::new(Vec::new());
    walk(base, 0, min_bytes, is_ignored, hooks, &found);
    if (hooks.cancelled)() {
        return None;
    }

    let mut found = found.into_inner().unwrap_or_else(|e| e.into_inner());
    found.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    found.truncate(limit);
    Some(found.into_iter().filter_map(to_item).collect())
}

/// One folder of the parallel walk: files are measured here, subfolders are
/// searched in parallel.
fn walk(
    dir: &Path,
    depth: usize,
    min_bytes: u64,
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    hooks: &ScanHooks<'_>,
    found: &Mutex<Vec<Found>>,
) {
    if (hooks.cancelled)() {
        return;
    }
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };

    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut local: Vec<Found> = Vec::new();
    for entry in read_dir.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if depth < MAX_DEPTH && !skip_dir(&name) && !is_ignored(&path) {
                subdirs.push(path);
            }
        } else if file_type.is_file() && !is_ignored(&path) {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let Some(size) = counted_size(&meta) else {
                continue;
            };
            if size >= min_bytes {
                local.push(Found {
                    path,
                    size,
                    modified: modified_secs(&meta),
                });
            }
        }
    }
    if !local.is_empty() {
        found
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(local);
    }

    subdirs
        .par_iter()
        .for_each(|sub| walk(sub, depth + 1, min_bytes, is_ignored, hooks, found));
}

/// Folders that are never searched.
fn skip_dir(name: &str) -> bool {
    if name.starts_with('.') || SKIP_DIRS.contains(&name) {
        return true;
    }
    if TOOL_DIRS.contains(&name.to_lowercase().as_str()) {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PACKAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// Space the file really takes, or `None` for a file whose removal would free
/// nothing because another hard link keeps the data alive.
fn counted_size(meta: &fs::Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return None;
        }
        Some(meta.blocks() * 512)
    }
    #[cfg(not(unix))]
    {
        Some(meta.len())
    }
}

fn modified_secs(meta: &fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

/// True when the file lives in a folder that a cloud service keeps in sync, so
/// removing it here may remove it online too.
fn cloud_synced(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_lowercase();
        matches!(
            name.as_str(),
            "mega" | "megasync" | "googledrive" | "pcloud" | "pcloud drive" | "box"
        ) || name.starts_with("dropbox")
            || name.starts_with("onedrive")
            || name.starts_with("google drive")
    })
}

/// A short label for the kind of file, from its extension.
pub(super) fn kind_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mov" | "mkv" | "avi" | "m4v" | "webm" | "wmv" | "mpg" | "mpeg" => "Video",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" => "Archive",
        "dmg" | "iso" | "img" | "pkg" => "Disk image",
        "vmdk" | "vdi" | "qcow2" | "vhd" | "vhdx" => "Virtual disk",
        "wav" | "aiff" | "aif" | "flac" | "mp3" | "m4a" => "Audio",
        "raw" | "dng" | "cr2" | "cr3" | "nef" | "arw" => "Photo",
        _ => "File",
    }
}

fn to_item(found: Found) -> Option<ScannedItem> {
    let canonical = fs::canonicalize(&found.path).ok()?;
    let path = found.path.to_string_lossy().into_owned();
    let kind = kind_for(&found.path);
    let mut description = format!(
        "{kind} file. This is one of your own files, not a cache. It is moved to the Trash, never deleted for good."
    );
    if cloud_synced(&found.path) {
        description.push_str(
            " It is in a folder that syncs to the cloud: removing it here may remove it online too.",
        );
    }
    Some(ScannedItem {
        item: CleanupItem {
            id: stable_id(&path),
            path,
            file_type: kind.to_string(),
            category: "Large File".to_string(),
            group: "Large files".to_string(),
            module: "large_files".to_string(),
            size_bytes: found.size,
            status: Safety::Danger,
            description,
            blocked_by: None,
            last_active: found.modified,
        },
        canonical,
        blocked_if_running: Vec::new(),
        action: Action::DeletePath,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup_engine::ScanProgress;
    use tempfile::TempDir;

    const MIN: u64 = 100_000;

    fn write(path: &Path, bytes: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; bytes]).unwrap();
    }

    fn run(base: &Path, limit: usize, cancelled: bool) -> Option<Vec<ScannedItem>> {
        let cancel = move || cancelled;
        let progress = |_: ScanProgress| {};
        let hooks = ScanHooks {
            cancelled: &cancel,
            progress: &progress,
        };
        find_large_files_with_limit(base, MIN, limit, &|_: &Path| false, &hooks)
    }

    fn names(items: &[ScannedItem]) -> Vec<String> {
        items
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
    fn finds_big_files_largest_first_and_ignores_small_ones() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("movies/big.mov"), 400_000);
        write(&d.path().join("deep/er/bigger.zip"), 800_000);
        write(&d.path().join("small.bin"), 10_000);
        let items = run(d.path(), MAX_RESULTS, false).unwrap();
        assert_eq!(names(&items), vec!["bigger.zip", "big.mov"]);
        assert!(items[0].item.size_bytes > items[1].item.size_bytes);
    }

    #[test]
    fn items_are_danger_and_describe_a_user_file() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("clip.mp4"), 300_000);
        let items = run(d.path(), MAX_RESULTS, false).unwrap();
        let item = &items[0];
        assert_eq!(item.item.status, Safety::Danger);
        assert_eq!(item.item.module, "large_files");
        assert_eq!(item.item.category, "Large File");
        assert_eq!(item.item.group, "Large files");
        assert_eq!(item.item.file_type, "Video");
        assert!(item.item.last_active.is_some());
        assert!(item.blocked_if_running.is_empty());
        assert_eq!(item.action, Action::DeletePath);
        assert_eq!(
            item.canonical,
            fs::canonicalize(d.path().join("clip.mp4")).unwrap()
        );
    }

    #[test]
    fn skips_hidden_dependency_and_package_folders() {
        let d = TempDir::new().unwrap();
        for dir in [
            ".cache",
            ".hidden",
            "node_modules",
            "target",
            "Library",
            "Photos.photoslibrary",
            "Thing.app/Contents",
            "Frameworks/Some.framework",
            "flutter/bin/cache",
            "sdk/system-images/android-37",
            "Caches",
        ] {
            write(&d.path().join(dir).join("skipped.bin"), 300_000);
        }
        write(&d.path().join("keep/kept.bin"), 300_000);
        let items = run(d.path(), MAX_RESULTS, false).unwrap();
        assert_eq!(names(&items), vec!["kept.bin"]);
    }

    #[cfg(unix)]
    #[test]
    fn skips_hard_links_and_symlinks() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("a/real.bin"), 300_000);
        fs::create_dir_all(d.path().join("b")).unwrap();
        fs::hard_link(d.path().join("a/real.bin"), d.path().join("b/linked.bin")).unwrap();
        write(&d.path().join("c/only.bin"), 250_000);
        std::os::unix::fs::symlink(d.path().join("c/only.bin"), d.path().join("c/sym.bin"))
            .unwrap();
        let items = run(d.path(), MAX_RESULTS, false).unwrap();
        assert_eq!(names(&items), vec!["only.bin"]);
    }

    #[test]
    fn keeps_only_the_largest_when_there_are_too_many() {
        let d = TempDir::new().unwrap();
        for i in 0..5 {
            write(&d.path().join(format!("f{i}.bin")), 200_000 + i * 50_000);
        }
        let items = run(d.path(), 3, false).unwrap();
        assert_eq!(names(&items), vec!["f4.bin", "f3.bin", "f2.bin"]);
    }

    #[test]
    fn a_cancelled_search_returns_nothing() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("big.bin"), 300_000);
        assert!(run(d.path(), MAX_RESULTS, true).is_none());
    }

    #[test]
    fn ignored_paths_are_left_out() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("hide/secret.bin"), 300_000);
        write(&d.path().join("show/shown.bin"), 300_000);
        write(&d.path().join("one.bin"), 300_000);
        let hidden = d.path().join("hide");
        let one = d.path().join("one.bin");
        let ignore = move |p: &Path| p.starts_with(&hidden) || p == one;
        let cancel = || false;
        let progress = |_: ScanProgress| {};
        let hooks = ScanHooks {
            cancelled: &cancel,
            progress: &progress,
        };
        let items = find_large_files_with_limit(d.path(), MIN, MAX_RESULTS, &ignore, &hooks)
            .unwrap();
        assert_eq!(names(&items), vec!["shown.bin"]);
    }

    #[test]
    fn files_in_cloud_synced_folders_carry_a_warning() {
        let d = TempDir::new().unwrap();
        write(&d.path().join("MEGA/clip.mov"), 300_000);
        write(&d.path().join("Dropbox (Personal)/other.mov"), 300_000);
        write(&d.path().join("local/plain.mov"), 300_000);
        let items = run(d.path(), MAX_RESULTS, false).unwrap();
        assert_eq!(items.len(), 3);
        for item in &items {
            let synced = item.item.path.contains("MEGA") || item.item.path.contains("Dropbox");
            assert_eq!(
                item.item.description.contains("syncs to the cloud"),
                synced,
                "{}",
                item.item.path
            );
        }
    }

    #[test]
    fn cloud_folder_names_are_recognised() {
        assert!(cloud_synced(Path::new("/Users/a/MEGA/x.mov")));
        assert!(cloud_synced(Path::new("/Users/a/OneDrive - Work/x.mov")));
        assert!(cloud_synced(Path::new("/Users/a/Google Drive/x.mov")));
        assert!(!cloud_synced(Path::new("/Users/a/Movies/x.mov")));
        assert!(!cloud_synced(Path::new("/Users/a/megabytes/x.mov")));
    }

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(kind_for(Path::new("a.MOV")), "Video");
        assert_eq!(kind_for(Path::new("a.tar")), "Archive");
        assert_eq!(kind_for(Path::new("a.dmg")), "Disk image");
        assert_eq!(kind_for(Path::new("a.vmdk")), "Virtual disk");
        assert_eq!(kind_for(Path::new("a.flac")), "Audio");
        assert_eq!(kind_for(Path::new("a.dng")), "Photo");
        assert_eq!(kind_for(Path::new("a.bin")), "File");
        assert_eq!(kind_for(Path::new("noext")), "File");
    }
}
