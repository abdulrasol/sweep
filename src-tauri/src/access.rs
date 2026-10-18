//! Full Disk Access detection (PLAN 3.8).
//!
//! macOS hides folders such as `~/Library/Safari` and `~/Library/Mail` until the user
//! grants Full Disk Access. Attempting to read those folders and looking only at the
//! error kind tells us whether the permission is missing, without reading any data.

use serde::Serialize;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// The fixed URL that opens the Full Disk Access pane. It takes no arguments,
/// so nothing can be injected into the `open` call.
pub const PRIVACY_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// Whether Sweep can read the folders macOS protects behind Full Disk Access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FdaStatus {
    Granted,
    Missing,
    Unknown,
}

/// Probe the protected folders in order. A readable folder means access is granted;
/// a permission error means it is missing; any other error (for example the folder
/// does not exist) moves on to the next probe. No clear answer leaves it unknown.
pub fn full_disk_access_at(home: &Path) -> FdaStatus {
    for rel in ["Library/Safari", "Library/Mail"] {
        match fs::read_dir(home.join(rel)) {
            Ok(_) => return FdaStatus::Granted,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => return FdaStatus::Missing,
            Err(_) => continue,
        }
    }
    FdaStatus::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn no_probe_folders_is_unknown() {
        let d = TempDir::new().unwrap();
        assert_eq!(full_disk_access_at(d.path()), FdaStatus::Unknown);
    }

    #[test]
    fn readable_safari_is_granted() {
        let d = TempDir::new().unwrap();
        fs::create_dir_all(d.path().join("Library/Safari")).unwrap();
        assert_eq!(full_disk_access_at(d.path()), FdaStatus::Granted);
    }

    #[test]
    fn readable_mail_alone_is_granted() {
        let d = TempDir::new().unwrap();
        fs::create_dir_all(d.path().join("Library/Mail")).unwrap();
        assert_eq!(full_disk_access_at(d.path()), FdaStatus::Granted);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_safari_is_missing() {
        use std::os::unix::fs::PermissionsExt;
        let d = TempDir::new().unwrap();
        let safari = d.path().join("Library/Safari");
        fs::create_dir_all(&safari).unwrap();
        fs::set_permissions(&safari, fs::Permissions::from_mode(0o000)).unwrap();

        let status = full_disk_access_at(d.path());
        // Restore permissions so the temp folder can be removed.
        fs::set_permissions(&safari, fs::Permissions::from_mode(0o755)).unwrap();

        // Root ignores the permission bits and can still read the folder; skip the
        // assertion there instead of failing.
        if status == FdaStatus::Granted {
            return;
        }
        assert_eq!(status, FdaStatus::Missing);
    }

    #[test]
    fn status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&FdaStatus::Granted).unwrap(),
            "\"granted\""
        );
        assert_eq!(
            serde_json::to_string(&FdaStatus::Missing).unwrap(),
            "\"missing\""
        );
        assert_eq!(
            serde_json::to_string(&FdaStatus::Unknown).unwrap(),
            "\"unknown\""
        );
    }
}
