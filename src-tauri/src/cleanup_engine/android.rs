//! Gradle, Android SDK and Android Studio leftovers (PLAN 2.5).
//!
//! - Gradle wrapper distributions: every version except the newest.
//! - SDK system images: only images no AVD uses. If any AVD cannot be read, no
//!   image is offered.
//! - SDK build-tools: every version except the newest.
//! - Android Studio: caches, logs and settings of versions older than the newest
//!   installed one, each version listed on its own.
//!
//! SDK packages are deleted as folders. Each package keeps its `package.xml` inside
//! its own folder, so the SDK Manager sees it as uninstalled, the same as removing it
//! there. `sdkmanager` was not used because it needs a Java runtime that apps opened
//! from Finder usually cannot find.

use super::{Action, Candidate, Env, Safety};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const SDK: &str = "Library/Android/sdk";

pub(super) fn collect(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    if !enabled.contains("android") {
        return;
    }
    let home = &env.home;

    for path in old_gradle_dists(&home.join(".gradle/wrapper/dists")) {
        out.push(candidate(
            path,
            "Gradle Distribution",
            "Gradle",
            Safety::Review,
            "An older Gradle version. A project that still uses it downloads it again on its next build (about 150 MB).",
        ));
    }

    let sdk = home.join(SDK);
    if let Some(used) = images_used_by_avds(&home.join(".android/avd")) {
        for path in unused_system_images(&sdk, &used) {
            out.push(candidate(
                path,
                "System Image",
                "Android SDK",
                Safety::Review,
                "An emulator system image no virtual device uses. The SDK Manager can download it again (often over 1 GB).",
            ));
        }
    }
    for path in old_build_tools(&sdk.join("build-tools")) {
        out.push(candidate(
            path,
            "Build Tools",
            "Android SDK",
            Safety::Review,
            "An older build-tools version. A newer one is installed, and Gradle downloads this one again if a project asks for it.",
        ));
    }

    for (root, safety, what) in [
        ("Library/Caches/Google", Safety::Safe, "Caches"),
        ("Library/Logs/Google", Safety::Safe, "Logs"),
        (
            "Library/Application Support/Google",
            Safety::Review,
            "Settings and plugins",
        ),
    ] {
        for (path, name) in old_studio_dirs(home, root) {
            out.push(candidate(
                path,
                &format!("{name} {}", what.to_lowercase()),
                "Android Studio",
                safety,
                &format!("{what} from {name}, an older Android Studio. A newer version is installed and already imported what it needs."),
            ));
        }
    }
}

fn candidate(
    path: PathBuf,
    label: &str,
    category: &str,
    safety: Safety,
    description: &str,
) -> Candidate {
    Candidate {
        path,
        file_type: label.to_string(),
        category: category.to_string(),
        safety,
        description: description.to_string(),
        blocked_if_running: Vec::new(),
        project_root: None,
        action: Action::DeletePath,
        known_size: None,
        min_bytes: 1,
        group: "Dev Tools",
        catch_all: false,
    }
}

/// Dotted number after a prefix: "gradle-8.10.2-bin" -> [8, 10, 2], "34.0.0-rc1" -> [34, 0, 0].
fn version_in(s: &str) -> Option<Vec<u64>> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let tok: String = s[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let parts: Option<Vec<u64>> = tok
        .trim_end_matches('.')
        .split('.')
        .map(|p| p.parse().ok())
        .collect();
    parts.filter(|p| !p.is_empty())
}

fn subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect()
}

/// Folders whose version is below the highest one. Folders without a version are kept.
fn older_than_newest(dirs: Vec<(Vec<u64>, PathBuf)>) -> Vec<PathBuf> {
    let Some(newest) = dirs.iter().map(|(v, _)| v.clone()).max() else {
        return Vec::new();
    };
    let mut old: Vec<PathBuf> = dirs
        .into_iter()
        .filter(|(v, _)| *v < newest)
        .map(|(_, p)| p)
        .collect();
    old.sort();
    old
}

/// `gradle-8.7-bin` and `gradle-8.7-all` of the newest version are both kept.
fn old_gradle_dists(dir: &Path) -> Vec<PathBuf> {
    older_than_newest(
        subdirs(dir)
            .into_iter()
            .filter(|(n, _)| n.starts_with("gradle-"))
            .filter_map(|(n, p)| version_in(&n).map(|v| (v, p)))
            .collect(),
    )
}

fn old_build_tools(dir: &Path) -> Vec<PathBuf> {
    older_than_newest(
        subdirs(dir)
            .into_iter()
            .filter_map(|(n, p)| version_in(&n).map(|v| (v, p)))
            .collect(),
    )
}

/// `system-images/...` paths used by AVDs, or `None` if any AVD could not be read.
fn images_used_by_avds(avd_dir: &Path) -> Option<HashSet<String>> {
    let mut avds: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(avd_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".avd") && path.is_dir() {
            avds.push(path);
        } else if name.ends_with(".ini") {
            // `<name>.ini` points at the AVD folder, which may live elsewhere.
            let text = fs::read_to_string(&path).ok()?;
            let target = ini_value(&text, "path")?;
            avds.push(PathBuf::from(target));
        }
    }
    let mut used = HashSet::new();
    for avd in avds {
        let config = fs::read_to_string(avd.join("config.ini")).ok()?;
        let sysdir = ini_value(&config, "image.sysdir.1")?;
        used.insert(normalize_image(&sysdir));
    }
    Some(used)
}

fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// "system-images/android-34/google_apis/arm64-v8a/" -> "android-34/google_apis/arm64-v8a"
fn normalize_image(s: &str) -> String {
    let s = s.replace('\\', "/");
    let s = s.trim_matches('/');
    s.strip_prefix("system-images/").unwrap_or(s).to_string()
}

/// `system-images/<api>/<tag>/<abi>` folders not in `used`.
fn unused_system_images(sdk: &Path, used: &HashSet<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (api, api_path) in subdirs(&sdk.join("system-images")) {
        for (tag, tag_path) in subdirs(&api_path) {
            for (abi, abi_path) in subdirs(&tag_path) {
                if !used.contains(&format!("{api}/{tag}/{abi}")) {
                    out.push(abi_path);
                }
            }
        }
    }
    out.sort();
    out
}

/// "AndroidStudioPreview2024.2" -> ("AndroidStudioPreview", [2024, 2])
fn studio_version(name: &str) -> Option<(&str, Vec<u64>)> {
    if !name.starts_with("AndroidStudio") {
        return None;
    }
    let split = name.find(|c: char| c.is_ascii_digit())?;
    Some((&name[..split], version_in(name)?))
}

/// Older Android Studio folders under `root`, judged against the newest version found
/// in any of the three Google folders, so a version that only left caches behind
/// still counts as old.
fn old_studio_dirs(home: &Path, root: &str) -> Vec<(PathBuf, String)> {
    let mut newest: HashMap<String, Vec<u64>> = HashMap::new();
    for r in [
        "Library/Caches/Google",
        "Library/Logs/Google",
        "Library/Application Support/Google",
    ] {
        for (name, _) in subdirs(&home.join(r)) {
            if let Some((product, v)) = studio_version(&name) {
                let best = newest.entry(product.to_string()).or_default();
                if v > *best {
                    *best = v;
                }
            }
        }
    }
    let mut out: Vec<(PathBuf, String)> = subdirs(&home.join(root))
        .into_iter()
        .filter(|(name, _)| {
            studio_version(name)
                .is_some_and(|(product, v)| newest.get(product).is_some_and(|n| v < *n))
        })
        .map(|(name, path)| (path, name))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mkdir(p: &Path) {
        fs::create_dir_all(p).unwrap();
        fs::write(p.join("f"), vec![1u8; 2000]).unwrap();
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn versions_parse() {
        assert_eq!(version_in("gradle-8.10.2-bin"), Some(vec![8, 10, 2]));
        assert_eq!(version_in("34.0.0-rc1"), Some(vec![34, 0, 0]));
        assert_eq!(version_in("AndroidStudio2024.1"), Some(vec![2024, 1]));
        assert_eq!(version_in("latest"), None);
    }

    #[test]
    fn newest_gradle_version_is_kept_in_both_flavors() {
        let d = TempDir::new().unwrap();
        for n in [
            "gradle-7.6-bin",
            "gradle-8.9-all",
            "gradle-8.10.2-bin",
            "gradle-8.10.2-all",
            "notes",
        ] {
            mkdir(&d.path().join(n));
        }
        assert_eq!(
            names(&old_gradle_dists(d.path())),
            vec!["gradle-7.6-bin", "gradle-8.9-all"]
        );
    }

    #[test]
    fn newest_build_tools_are_kept() {
        let d = TempDir::new().unwrap();
        for n in ["33.0.1", "34.0.0", "35.0.0"] {
            mkdir(&d.path().join(n));
        }
        assert_eq!(names(&old_build_tools(d.path())), vec!["33.0.1", "34.0.0"]);
        let single = TempDir::new().unwrap();
        mkdir(&single.path().join("35.0.0"));
        assert!(old_build_tools(single.path()).is_empty());
    }

    fn avd(avd_dir: &Path, name: &str, sysdir: &str) {
        let dir = avd_dir.join(format!("{name}.avd"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("config.ini"),
            format!("hw.ramSize=2048\nimage.sysdir.1={sysdir}\n"),
        )
        .unwrap();
        fs::write(
            avd_dir.join(format!("{name}.ini")),
            format!("avd.ini.encoding=UTF-8\npath={}\n", dir.display()),
        )
        .unwrap();
    }

    #[test]
    fn system_images_used_by_an_avd_are_kept() {
        let d = TempDir::new().unwrap();
        let sdk = d.path().join("sdk");
        let avds = d.path().join("avd");
        for img in [
            "android-34/google_apis/arm64-v8a",
            "android-33/google_apis_playstore/arm64-v8a",
            "android-30/default/x86_64",
        ] {
            mkdir(&sdk.join("system-images").join(img));
        }
        avd(
            &avds,
            "Pixel_8",
            "system-images/android-34/google_apis/arm64-v8a/",
        );
        avd(&avds, "Old", "system-images\\android-30\\default\\x86_64\\");
        let used = images_used_by_avds(&avds).unwrap();
        let unused = unused_system_images(&sdk, &used);
        assert_eq!(
            unused,
            vec![sdk.join("system-images/android-33/google_apis_playstore/arm64-v8a")]
        );
    }

    #[test]
    fn unreadable_avd_offers_no_images() {
        let d = TempDir::new().unwrap();
        let avds = d.path().join(".android/avd");
        fs::create_dir_all(avds.join("Broken.avd")).unwrap(); // no config.ini
        assert!(images_used_by_avds(&avds).is_none());

        mkdir(
            &d.path()
                .join(SDK)
                .join("system-images/android-34/default/arm64-v8a"),
        );
        let env = Env {
            home: d.path().to_path_buf(),
        };
        let mut out = Vec::new();
        collect(&env, &HashSet::from(["android"]), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn no_avds_means_every_image_is_unused() {
        let d = TempDir::new().unwrap();
        assert_eq!(
            images_used_by_avds(&d.path().join("missing")),
            Some(HashSet::new())
        );
    }

    #[test]
    fn older_android_studio_versions_are_listed_separately() {
        let d = TempDir::new().unwrap();
        let h = d.path();
        for v in [
            "AndroidStudio2023.1",
            "AndroidStudio2023.3",
            "AndroidStudio2024.2",
            "AndroidStudioPreview2024.3",
            "Chrome",
        ] {
            mkdir(&h.join("Library/Caches/Google").join(v));
        }
        mkdir(&h.join("Library/Application Support/Google/AndroidStudio2023.3"));
        mkdir(&h.join("Library/Application Support/Google/AndroidStudio2024.2"));
        // A newer version known only from its logs still makes 2024.2 old.
        mkdir(&h.join("Library/Logs/Google/AndroidStudio2025.1"));

        let caches = old_studio_dirs(h, "Library/Caches/Google");
        let names: Vec<&str> = caches.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "AndroidStudio2023.1",
                "AndroidStudio2023.3",
                "AndroidStudio2024.2"
            ]
        );

        let env = Env {
            home: h.to_path_buf(),
        };
        let mut out = Vec::new();
        collect(&env, &HashSet::from(["android"]), &mut out);
        let support: Vec<_> = out
            .iter()
            .filter(|c| c.path.to_string_lossy().contains("Application Support"))
            .collect();
        assert_eq!(support.len(), 2);
        assert!(support.iter().all(|c| c.safety == Safety::Review));
        // Negative: the newest version, the preview channel and other Google apps stay.
        assert!(out.iter().all(|c| !c.path.ends_with("AndroidStudio2025.1")));
        assert!(out
            .iter()
            .all(|c| !c.path.ends_with("AndroidStudioPreview2024.3")));
        assert!(out.iter().all(|c| !c.path.ends_with("Chrome")));
    }

    #[test]
    fn module_off_lists_nothing() {
        let d = TempDir::new().unwrap();
        mkdir(&d.path().join(".gradle/wrapper/dists/gradle-7.6-bin"));
        mkdir(&d.path().join(".gradle/wrapper/dists/gradle-8.10-bin"));
        let env = Env {
            home: d.path().to_path_buf(),
        };
        let mut out = Vec::new();
        collect(&env, &HashSet::from(["flutter"]), &mut out);
        assert!(out.is_empty());
    }
}
