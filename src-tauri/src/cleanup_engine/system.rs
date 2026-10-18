//! System logs, iPhone/iPad backups and the Trash (PLAN 2.15a).
//!
//! Everything here works under the home folder only and never follows a symlink.
//! Children of `~/Library/Logs` that another module already owns are skipped, so
//! the same folder is never listed twice.

use super::{is_real_dir, Action, Candidate, Env, Safety};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use Safety::{Danger, Review, Safe};

const MODULE: &str = "system";
const GROUP: &str = "System";

const LOGS_DESCRIPTION: &str = "Log files. Apps write new ones when needed.";
const TRASH_DESCRIPTION: &str = "Already in the Trash. Removing it frees the space for good.";
const UNKNOWN_DATE: &str = "an unknown date";

/// Extensions of installer and archive files offered from `~/Downloads`.
const DOWNLOAD_EXTENSIONS: &[&str] = &["dmg", "pkg", "xip", "ipa", "apk", "zip"];
/// A download is offered only once it has not changed for longer than this.
const OLD_DOWNLOAD_AGE_SECS: u64 = 30 * 86_400;

/// Direct children of `~/Library/Logs` covered by another module; skipped here.
const LOGS_COVERED: &[&str] = &[
    "JetBrains",
    "Claude",
    "Codex",
    "Google",
    "CoreSimulator",
    "DiagnosticReports",
];

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains(MODULE) {
        return;
    }
    collect_logs(env, out);
    collect_backups(env, out);
    collect_trash(env, out);
    collect_downloads(env, out);
}

/// One item per direct child of `~/Library/Logs`, plus the shared DiagnosticReports folder.
fn collect_logs(env: &Env, out: &mut Vec<Candidate>) {
    let logs = env.home.join("Library/Logs");
    if let Ok(entries) = fs::read_dir(&logs) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if LOGS_COVERED.contains(&name.as_str()) {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            out.push(log_item(path, name));
        }
    }

    let diagnostics = logs.join("DiagnosticReports");
    if is_real_dir(&diagnostics) {
        out.push(log_item(diagnostics, "DiagnosticReports".to_string()));
    }
}

fn log_item(path: PathBuf, name: String) -> Candidate {
    Candidate {
        path,
        file_type: name,
        category: "System Log".to_string(),
        group: GROUP,
        safety: Safe,
        description: LOGS_DESCRIPTION.to_string(),
        blocked_if_running: Vec::new(),
        project_root: None,
        action: Action::DeletePath,
        known_size: None,
        min_bytes: 1,
        catch_all: false,
    }
}

/// One item per device backup folder, marked Danger with the folder's date.
/// A missing or unreadable Backup folder yields no items and never fails the scan.
fn collect_backups(env: &Env, out: &mut Vec<Candidate>) {
    let root = env
        .home
        .join("Library/Application Support/MobileSync/Backup");
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_real_dir(&path) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let date = item_date(&path);
        out.push(Candidate {
            path,
            file_type: name,
            category: "Device Backup".to_string(),
            group: GROUP,
            safety: Danger,
            description: format!(
                "iPhone or iPad backup from {date}. Deleting it is permanent and cannot be undone."
            ),
            blocked_if_running: Vec::new(),
            project_root: None,
            action: Action::DeletePath,
            known_size: None,
            min_bytes: 1,
            catch_all: false,
        });
    }
}

/// One item per direct child of `~/.Trash`, skipping symlinks and `.DS_Store`.
fn collect_trash(env: &Env, out: &mut Vec<Candidate>) {
    let trash = env.home.join(".Trash");
    let Ok(entries) = fs::read_dir(&trash) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".DS_Store" {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        out.push(Candidate {
            path,
            file_type: name,
            category: "Trash".to_string(),
            group: GROUP,
            safety: Review,
            description: TRASH_DESCRIPTION.to_string(),
            blocked_if_running: Vec::new(),
            project_root: None,
            action: Action::DeletePath,
            known_size: None,
            min_bytes: 1,
            catch_all: false,
        });
    }
}

fn collect_downloads(env: &Env, out: &mut Vec<Candidate>) {
    collect_downloads_at(env, SystemTime::now(), out);
}

/// One item per old installer or archive directly in `~/Downloads`.
/// Only direct children are considered: the folder is never entered, and a
/// missing or unreadable Downloads folder yields no items.
fn collect_downloads_at(env: &Env, now: SystemTime, out: &mut Vec<Candidate>) {
    let downloads = env.home.join("Downloads");
    let Ok(entries) = fs::read_dir(&downloads) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Some(extension) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
            continue;
        };
        if !DOWNLOAD_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        // symlink_metadata never follows a link; only a regular file counts.
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age.as_secs() <= OLD_DOWNLOAD_AGE_SECS {
            continue;
        }
        let days = age.as_secs() / 86_400;
        let date = modified
            .duration_since(UNIX_EPOCH)
            .map(|d| civil_date(d.as_secs()))
            .unwrap_or_else(|_| UNKNOWN_DATE.to_string());
        out.push(Candidate {
            path,
            file_type: name,
            category: "Old Download".to_string(),
            group: GROUP,
            safety: Review,
            description: format!(
                "Installer or archive in Downloads, last changed on {date} ({days} days ago). It is your own file, so check it before removing."
            ),
            blocked_if_running: Vec::new(),
            project_root: None,
            action: Action::DeletePath,
            known_size: None,
            min_bytes: 1,
            catch_all: false,
        });
    }
}

/// Modification time of `path` as `YYYY-MM-DD` (UTC), or a placeholder text.
/// A time before 1970 or a missing mtime never fails the scan.
fn item_date(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return UNKNOWN_DATE.to_string();
    };
    let Ok(modified) = metadata.modified() else {
        return UNKNOWN_DATE.to_string();
    };
    let Ok(since_epoch) = modified.duration_since(UNIX_EPOCH) else {
        return UNKNOWN_DATE.to_string();
    };
    civil_date(since_epoch.as_secs())
}

/// Format seconds since the Unix epoch as `YYYY-MM-DD` in UTC, using std only
/// (civil-from-days, no date crate).
fn civil_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn collect_from(home: &Path) -> Vec<Candidate> {
        let env = Env {
            home: home.to_path_buf(),
        };
        let enabled: HashSet<&str> = [MODULE].into_iter().collect();
        let mut out = Vec::new();
        collect(&env, &enabled, &mut out);
        out
    }

    fn downloads_from(home: &Path, now: SystemTime) -> Vec<Candidate> {
        let env = Env {
            home: home.to_path_buf(),
        };
        let mut out = Vec::new();
        collect_downloads_at(&env, now, &mut out);
        out
    }

    fn now_plus_days(days: u64) -> SystemTime {
        SystemTime::now() + Duration::from_secs(days * 86_400)
    }

    fn names(items: &[Candidate]) -> Vec<String> {
        let mut v: Vec<String> = items
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn write(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn has_ymd(s: &str) -> bool {
        let b = s.as_bytes();
        b.windows(10).any(|w| {
            w[4] == b'-'
                && w[7] == b'-'
                && w[..4].iter().all(u8::is_ascii_digit)
                && w[5..7].iter().all(u8::is_ascii_digit)
                && w[8..10].iter().all(u8::is_ascii_digit)
        })
    }

    #[test]
    fn civil_date_matches_known_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(86_399), "1970-01-01");
        assert_eq!(civil_date(86_400), "1970-01-02");
        assert_eq!(civil_date(951_782_400), "2000-02-29");
        assert_eq!(civil_date(1_700_000_000), "2023-11-14");
    }

    #[test]
    fn logs_skip_covered_and_include_diagnostics() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let logs = home.join("Library/Logs");
        fs::create_dir_all(logs.join("JetBrains")).unwrap();
        fs::create_dir_all(logs.join("MyApp")).unwrap();
        fs::create_dir_all(logs.join("DiagnosticReports")).unwrap();
        write(&logs.join("install.log"));

        let items = collect_from(home);
        let found = names(&items);
        assert!(found.contains(&"MyApp".to_string()), "{found:?}");
        assert!(
            found.contains(&"DiagnosticReports".to_string()),
            "{found:?}"
        );
        assert!(found.contains(&"install.log".to_string()), "{found:?}");
        // Negative: the JetBrains folder is owned by the editors module.
        assert!(!found.contains(&"JetBrains".to_string()), "{found:?}");
        assert_eq!(items.len(), 3, "{found:?}");
        assert!(items.iter().all(|c| c.safety == Safe));
        assert!(items.iter().all(|c| c.category == "System Log"));
    }

    #[test]
    fn backups_are_danger_with_a_date() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let root = home.join("Library/Application Support/MobileSync/Backup");
        fs::create_dir_all(root.join("00008110-001A2C3D4E5F601E")).unwrap();
        fs::create_dir_all(root.join("00008020-000B1C2D3E4F502D")).unwrap();

        let items = collect_from(home);
        assert_eq!(items.len(), 2, "{}", names(&items).join(","));
        assert!(items.iter().all(|c| c.safety == Danger));
        assert!(items.iter().all(|c| c.category == "Device Backup"));
        for item in &items {
            assert!(
                item.description.starts_with("iPhone or iPad backup from "),
                "{}",
                item.description
            );
            assert!(has_ymd(&item.description), "{}", item.description);
        }
    }

    #[test]
    fn missing_backup_folder_is_silent() {
        let d = TempDir::new().unwrap();
        let items = collect_from(d.path());
        assert!(items.is_empty(), "{}", names(&items).join(","));
    }

    #[test]
    fn trash_skips_symlinks_and_ds_store() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let trash = home.join(".Trash");
        fs::create_dir_all(trash.join("Old Folder")).unwrap();
        write(&trash.join("movie.mp4"));
        write(&trash.join(".DS_Store"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(trash.join("movie.mp4"), trash.join("link")).unwrap();

        let items = collect_from(home);
        let found = names(&items);
        assert!(found.contains(&"movie.mp4".to_string()), "{found:?}");
        assert!(found.contains(&"Old Folder".to_string()), "{found:?}");
        assert!(!found.contains(&".DS_Store".to_string()), "{found:?}");
        // Negative: a symlink is never followed and never listed.
        assert!(!found.contains(&"link".to_string()), "{found:?}");
        assert!(items.iter().all(|c| c.safety == Review));
        assert!(items.iter().all(|c| c.category == "Trash"));
    }

    #[test]
    fn every_path_stays_inside_the_fake_home() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        fs::create_dir_all(home.join("Library/Logs/MyApp")).unwrap();
        fs::create_dir_all(home.join("Library/Logs/DiagnosticReports")).unwrap();
        fs::create_dir_all(home.join("Library/Application Support/MobileSync/Backup/device"))
            .unwrap();
        fs::create_dir_all(home.join(".Trash")).unwrap();
        write(&home.join(".Trash/leftover.bin"));

        let items = collect_from(home);
        assert!(!items.is_empty());
        for item in &items {
            assert!(item.path.starts_with(home), "{}", item.path.display());
        }
    }

    #[test]
    fn downloads_list_old_installers() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let downloads = home.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        for name in [
            "app.dmg",
            "tool.PKG",
            "backup.zip",
            "x.xip",
            "a.ipa",
            "b.apk",
        ] {
            write(&downloads.join(name));
        }

        let items = downloads_from(home, now_plus_days(40));
        assert_eq!(items.len(), 6, "{}", names(&items).join(","));
        assert!(items.iter().all(|c| c.safety == Review));
        assert!(items.iter().all(|c| c.category == "Old Download"));
    }

    #[test]
    fn downloads_ignore_other_files() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let downloads = home.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        // Negative: unknown extensions and a bare "zip" name are never listed.
        for name in ["notes.txt", "photo.png", "report.pdf", "doc.docx", "zip"] {
            write(&downloads.join(name));
        }

        let items = downloads_from(home, now_plus_days(40));
        assert!(items.is_empty(), "{}", names(&items).join(","));
    }

    #[test]
    fn downloads_ignore_fresh_files() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let downloads = home.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        write(&downloads.join("app.dmg"));

        // Negative: a just-created file is not older than 30 days.
        let items = downloads_from(home, SystemTime::now());
        assert!(items.is_empty(), "{}", names(&items).join(","));
    }

    #[test]
    fn downloads_ignore_dirs_symlinks_hidden_and_subfolders() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let downloads = home.join("Downloads");
        fs::create_dir_all(downloads.join("inner")).unwrap();
        write(&downloads.join("inner/deep.zip"));
        // A directory that happens to end in .zip must not be listed.
        fs::create_dir_all(downloads.join("thing.zip")).unwrap();
        write(&downloads.join(".hidden.zip"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(downloads.join("inner/deep.zip"), downloads.join("link.zip"))
            .unwrap();

        let items = downloads_from(home, now_plus_days(40));
        assert!(items.is_empty(), "{}", names(&items).join(","));
    }

    #[test]
    fn missing_downloads_folder_is_silent() {
        let d = TempDir::new().unwrap();
        let items = downloads_from(d.path(), now_plus_days(40));
        assert!(items.is_empty(), "{}", names(&items).join(","));
    }

    #[test]
    fn download_paths_stay_inside_the_fake_home() {
        let d = TempDir::new().unwrap();
        let home = d.path();
        let downloads = home.join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        write(&downloads.join("app.dmg"));

        let items = downloads_from(home, now_plus_days(40));
        assert!(!items.is_empty());
        for item in &items {
            assert!(item.path.starts_with(home), "{}", item.path.display());
        }
    }
}
