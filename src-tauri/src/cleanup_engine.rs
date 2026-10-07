//! Sweep cleanup engine.
//!
//! Safety model (see CLAUDE.md, section 3):
//! - Project targets are only reported when they contain no git-tracked files.
//! - Directories carrying a valid `CACHEDIR.TAG` are treated as safe caches.
//! - Deletion goes through `delete_path`, which re-validates the canonical path
//!   against a fixed deny-list and the roots the scan was allowed to touch.
//! - Symlinks are never followed: a symlink target is removed as a link.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

mod ai_rules;
mod simulators;
mod versions;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum Safety {
    Safe,
    Review,
    Danger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupItem {
    pub id: String,
    pub path: String,
    /// Framework / tool name, e.g. "Flutter".
    pub file_type: String,
    /// Human label for the kind of artifact, e.g. "Build Artifacts".
    #[serde(rename = "type")]
    pub category: String,
    /// Broad bucket for filtering: Projects, Dev Tools, AI Tools, Editors, System.
    pub group: String,
    pub size_bytes: u64,
    pub status: Safety,
    pub description: String,
    /// Name of a running app that must be closed before this item can be removed.
    pub blocked_by: Option<String>,
}

/// Paths the engine needs from the environment. Injected so tests can use a fake home.
#[derive(Debug, Clone)]
pub struct Env {
    pub home: PathBuf,
}

impl Env {
    pub fn from_system() -> Self {
        Env {
            home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/nonexistent-home")),
        }
    }
}

/// Everything the app keeps about an item after a scan, so deletion never trusts the frontend.
#[derive(Debug, Clone)]
pub struct ScannedItem {
    pub item: CleanupItem,
    pub canonical: PathBuf,
    pub blocked_if_running: Vec<&'static str>,
    pub action: Action,
}

/// How an item is removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Delete `item.path` after the safety checks in `delete_path`.
    DeletePath,
    /// Run an official tool instead (PLAN 3.3). Only commands accepted by
    /// `simulators::is_allowed_command` ever run.
    Command(Vec<String>),
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub items: Vec<ScannedItem>,
    /// Roots that deletions are allowed under (the scan root and the user's home).
    pub allowed_roots: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

struct Target {
    /// Path relative to the project root. May contain `*` (glob).
    rel: &'static str,
    safety: Safety,
}

struct ProjectRule {
    module: &'static str,
    name: &'static str,
    /// Any of these markers identifies a project root. `*.ext` matches by suffix,
    /// a value containing `/` is checked as a relative path.
    markers: &'static [&'static str],
    label: &'static str,
    targets: &'static [Target],
    blocked_if_running: &'static [&'static str],
}

const fn t(rel: &'static str, safety: Safety) -> Target {
    Target { rel, safety }
}

use Safety::{Danger, Review, Safe};

static PROJECT_RULES: &[ProjectRule] = &[
    ProjectRule {
        module: "flutter",
        name: "Flutter",
        markers: &["pubspec.yaml"],
        label: "Flutter Artifacts",
        targets: &[
            t("build", Safe),
            t(".dart_tool", Safe),
            t("ios/Pods", Safe),
            t("ios/.symlinks", Safe),
            t("macos/Pods", Safe),
            t("android/.gradle", Safe),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "node",
        name: "Node / PNPM",
        markers: &["package.json"],
        label: "Modules & Cache",
        targets: &[
            t("node_modules", Safe),
            t(".next", Safe),
            t(".cache", Safe),
            t(".pnpm-store", Safe),
            t("dist", Review),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "rust",
        name: "Rust",
        markers: &["Cargo.toml"],
        label: "Rust Build",
        // Upgraded to SAFE when Cargo's CACHEDIR.TAG is present.
        targets: &[t("target", Review)],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "php",
        name: "PHP / Laravel",
        markers: &["composer.json"],
        label: "Vendor & Cache",
        targets: &[
            t("vendor", Review),
            t("storage/framework/cache/data", Safe),
            t("storage/framework/views", Safe),
            t("bootstrap/cache/*.php", Safe),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "android",
        name: "Android / Gradle",
        markers: &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
        label: "Gradle Build",
        targets: &[t("build", Safe), t(".gradle", Safe), t("app/build", Safe)],
        blocked_if_running: &["studio"],
    },
    ProjectRule {
        module: "dotnet",
        name: ".NET / C#",
        markers: &["*.csproj", "*.fsproj", "*.sln"],
        label: "Build Binaries",
        targets: &[t("bin", Safe), t("obj", Safe)],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "ruby",
        name: "Ruby on Rails",
        markers: &["Gemfile"],
        label: "Bundle & Tmp",
        targets: &[
            t("vendor/bundle", Review),
            t("tmp/cache", Safe),
            t("log/*.log", Safe),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "cpp",
        name: "C++ / CMake",
        markers: &["CMakeLists.txt"],
        label: "CMake Build",
        targets: &[
            t("build", Safe),
            t("cmake-build-debug", Safe),
            t("cmake-build-release", Safe),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "unreal",
        name: "Unreal Engine",
        markers: &["*.uproject"],
        label: "Project Artifacts",
        targets: &[
            t("Intermediate", Danger),
            t("Saved", Danger),
            t("Binaries", Danger),
            t("DerivedDataCache", Danger),
        ],
        blocked_if_running: &["UnrealEditor", "UE4Editor"],
    },
    ProjectRule {
        module: "unity",
        name: "Unity",
        markers: &["ProjectSettings/ProjectVersion.txt"],
        label: "Unity Library",
        targets: &[
            t("Library", Danger),
            t("Temp", Danger),
            t("Obj", Danger),
            t("Logs", Danger),
        ],
        blocked_if_running: &["Unity"],
    },
    ProjectRule {
        module: "ai_assistants",
        name: "Aider",
        markers: &[
            ".aider.chat.history.md",
            ".aider.input.history",
            ".aider.tags.cache.v3",
            ".aider.tags.cache.v4",
        ],
        label: "Aider Files",
        targets: &[
            t(".aider.tags.cache.v*", Safe),
            t(".aider.input.history", Review),
            t(".aider.chat.history.md", Danger),
        ],
        blocked_if_running: &["aider"],
    },
    // Go builds nothing inside the project tree; its caches are global (PLAN 2.10).
    // Python and AI project rules were removed: `envs/`, `pkgs/`, `outputs/`, `runs/` are
    // common names for real source/data directories (PLAN 0.2).
];

struct GlobalRule {
    module: &'static str,
    label: &'static str,
    /// Path relative to the home directory. May contain `*` (glob).
    home_rel: &'static str,
    category: &'static str,
    safety: Safety,
    blocked_if_running: &'static [&'static str],
    /// Report each child directory as its own item instead of the folder itself.
    split_children: bool,
    /// Item description; a default based on `safety` is used when empty.
    note: &'static str,
    /// Items smaller than this are not listed.
    min_bytes: u64,
}

const fn g(
    module: &'static str,
    label: &'static str,
    home_rel: &'static str,
    category: &'static str,
    safety: Safety,
    blocked_if_running: &'static [&'static str],
) -> GlobalRule {
    GlobalRule {
        module,
        label,
        home_rel,
        category,
        safety,
        blocked_if_running,
        split_children: false,
        note: "",
        min_bytes: 1,
    }
}

#[cfg(target_os = "macos")]
static GLOBAL_RULES: &[GlobalRule] = &[
    g(
        "xcode",
        "Xcode DerivedData",
        "Library/Developer/Xcode/DerivedData",
        "Build Cache",
        Safe,
        &["Xcode"],
    ),
    g(
        "xcode",
        "Xcode Previews",
        "Library/Developer/Xcode/UserData/Previews",
        "Build Cache",
        Safe,
        &["Xcode"],
    ),
    g(
        "xcode",
        "Xcode Documentation Cache",
        "Library/Developer/Xcode/DocumentationCache",
        "Build Cache",
        Safe,
        &["Xcode"],
    ),
    g(
        "xcode",
        "Xcode Cache",
        "Library/Caches/com.apple.dt.Xcode",
        "Build Cache",
        Safe,
        &["Xcode"],
    ),
    g(
        "xcode",
        "Simulator Caches",
        "Library/Developer/CoreSimulator/Caches",
        "Simulator Cache",
        Safe,
        &["Simulator"],
    ),
    g(
        "xcode",
        "Simulator Logs",
        "Library/Logs/CoreSimulator",
        "Simulator Cache",
        Safe,
        &["Simulator"],
    ),
    GlobalRule {
        module: "xcode",
        label: "Xcode Archive",
        home_rel: "Library/Developer/Xcode/Archives",
        category: "Archives",
        safety: Review,
        blocked_if_running: &["Xcode"],
        split_children: true,
        note: "Xcode archives from one day. They hold the dSYM files you need to read crash reports for builds you shipped. Keep the ones for versions still in users' hands.",
        min_bytes: 1,
    },
    g(
        "cocoapods",
        "CocoaPods Cache",
        "Library/Caches/CocoaPods",
        "Package Cache",
        Safe,
        &[],
    ),
    GlobalRule {
        module: "cocoapods",
        label: "CocoaPods Spec Repos",
        home_rel: ".cocoapods/repos",
        category: "Package Cache",
        safety: Review,
        blocked_if_running: &[],
        split_children: false,
        note: "CocoaPods spec repositories. The public trunk is downloaded again on the next `pod install`, which can take a while. Private spec repos must be added again with `pod repo add`.",
        min_bytes: 1,
    },
    g(
        "android",
        "Android AVD Images",
        ".android/avd",
        "Emulator Data",
        Review,
        &["qemu-system-aarch64", "qemu-system-x86_64"],
    ),
    g(
        "os",
        "Vagrant Boxes",
        ".vagrant.d/boxes",
        "Virtualization",
        Danger,
        &[],
    ),
    g(
        "ai",
        "Hugging Face Models",
        ".cache/huggingface",
        "AI Model Cache",
        Review,
        &[],
    ),
    g(
        "ai",
        "PyTorch Cache",
        ".cache/torch",
        "AI Model Cache",
        Review,
        &[],
    ),
    g(
        "social",
        "Telegram Desktop Media",
        "Library/Application Support/Telegram Desktop/tdata/user_data",
        "Media Cache",
        Review,
        &["Telegram"],
    ),
    g(
        "social",
        "Telegram Media",
        "Library/Group Containers/*.ru.keepcoder.Telegram/*/account-*/postbox/media",
        "Media Cache",
        Review,
        &["Telegram"],
    ),
    g(
        "social",
        "Discord Cache",
        "Library/Application Support/discord/Cache",
        "App Cache",
        Safe,
        &["Discord"],
    ),
    g(
        "social",
        "Spotify Cache",
        "Library/Caches/com.spotify.client",
        "Media Cache",
        Safe,
        &["Spotify"],
    ),
    g(
        "node",
        "PNPM Store",
        "Library/pnpm/store",
        "Global Package Store",
        Safe,
        &[],
    ),
    g(
        "python",
        "Conda Environments",
        "anaconda3/envs",
        "Conda Envs",
        Danger,
        &[],
    ),
    g(
        "python",
        "Conda Environments",
        "miniconda3/envs",
        "Conda Envs",
        Danger,
        &[],
    ),
    g(
        "homebrew",
        "Homebrew Cache",
        "Library/Caches/Homebrew",
        "Download Cache",
        Safe,
        &[],
    ),
    g(
        "adobe",
        "Adobe Media Cache",
        "Library/Caches/Adobe/Common",
        "Media Cache",
        Review,
        &[],
    ),
    GlobalRule {
        module: "os_system",
        label: "App Cache",
        home_rel: "Library/Caches",
        category: "System Cache",
        safety: Review,
        blocked_if_running: &[],
        split_children: true,
        note: "",
        min_bytes: MIN_SPLIT_CHILD_BYTES,
    },
];

#[cfg(target_os = "windows")]
static GLOBAL_RULES: &[GlobalRule] = &[
    g(
        "android",
        "Android AVD Images",
        ".android/avd",
        "Emulator Data",
        Review,
        &[],
    ),
    g(
        "ai",
        "Hugging Face Models",
        ".cache/huggingface",
        "AI Model Cache",
        Review,
        &[],
    ),
    g(
        "node",
        "PNPM Store",
        "AppData/Local/pnpm/store",
        "Global Package Store",
        Safe,
        &[],
    ),
    g(
        "dotnet",
        "NuGet Cache",
        ".nuget/packages",
        "Package Cache",
        Safe,
        &[],
    ),
    g(
        "adobe",
        "Adobe Media Cache",
        "AppData/Roaming/Adobe/Common",
        "Media Cache",
        Review,
        &[],
    ),
    g(
        "os_system",
        "Windows Temp",
        "AppData/Local/Temp",
        "System Cache",
        Review,
        &[],
    ),
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
static GLOBAL_RULES: &[GlobalRule] = &[
    g(
        "android",
        "Android AVD Images",
        ".android/avd",
        "Emulator Data",
        Review,
        &[],
    ),
    g(
        "os",
        "Vagrant Boxes",
        ".vagrant.d/boxes",
        "Virtualization",
        Danger,
        &[],
    ),
    g(
        "ai",
        "Hugging Face Models",
        ".cache/huggingface",
        "AI Model Cache",
        Review,
        &[],
    ),
    g(
        "ai",
        "PyTorch Cache",
        ".cache/torch",
        "AI Model Cache",
        Review,
        &[],
    ),
    g(
        "node",
        "PNPM Store",
        ".local/share/pnpm/store",
        "Global Package Store",
        Safe,
        &[],
    ),
    g(
        "python",
        "Conda Environments",
        "anaconda3/envs",
        "Conda Envs",
        Danger,
        &[],
    ),
];

/// Directories the walker never descends into while looking for project roots.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "build",
    "dist",
    ".next",
    ".cache",
    "Pods",
    ".gradle",
    "vendor",
    ".venv",
    "venv",
    ".dart_tool",
    "DerivedData",
    "Intermediate",
    "__pycache__",
    ".pnpm-store",
    "Library",
    ".Trash",
];

const MAX_DEPTH: usize = 8;
/// Children of split global folders smaller than this are not worth listing.
const MIN_SPLIT_CHILD_BYTES: u64 = 10 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

struct Candidate {
    path: PathBuf,
    file_type: String,
    category: String,
    safety: Safety,
    description: String,
    blocked_if_running: Vec<&'static str>,
    project_root: Option<PathBuf>,
    min_bytes: u64,
    group: &'static str,
    action: Action,
    /// Size reported by the tool that owns the item; skips walking `path`.
    known_size: Option<u64>,
}

pub fn scan_directory(
    base_path: &Path,
    enabled_modules: &[String],
    ignored_paths: &[String],
    env: &Env,
    running: &HashSet<String>,
) -> ScanResult {
    let enabled: HashSet<&str> = enabled_modules.iter().map(String::as_str).collect();
    let ignored: Vec<PathBuf> = ignored_paths.iter().map(PathBuf::from).collect();
    let is_ignored = |p: &Path| ignored.iter().any(|i| p.starts_with(i));

    let mut candidates = Vec::new();
    collect_global(env, &enabled, &mut candidates);
    versions::collect(env, &enabled, &mut candidates);
    simulators::collect(env, &enabled, &mut candidates);
    collect_projects(base_path, &enabled, &is_ignored, &mut candidates);

    candidates.retain(|c| !is_ignored(&c.path));
    let candidates = drop_git_tracked(candidates);
    let candidates = drop_nested(candidates);

    let mut items: Vec<ScannedItem> = candidates
        .into_par_iter()
        .filter_map(|mut c| {
            let size = c.known_size.unwrap_or_else(|| disk_usage(&c.path));
            if size == 0 {
                return None;
            }
            if c.project_root.is_some() && c.safety == Review && has_cachedir_tag(&c.path) {
                c.safety = Safe;
            }
            if size < c.min_bytes {
                return None;
            }
            let canonical = match c.action {
                Action::DeletePath => fs::canonicalize(&c.path).ok()?,
                Action::Command(_) => c.path.clone(),
            };
            let path_str = c.path.to_string_lossy().into_owned();
            let blocked_by = c
                .blocked_if_running
                .iter()
                .find(|name| running.contains(&name.to_lowercase()))
                .map(|s| s.to_string());
            Some(ScannedItem {
                item: CleanupItem {
                    id: stable_id(&path_str),
                    path: path_str,
                    file_type: c.file_type,
                    category: c.category,
                    group: c.group.to_string(),
                    size_bytes: size,
                    status: c.safety,
                    description: c.description,
                    blocked_by,
                },
                canonical,
                blocked_if_running: c.blocked_if_running,
                action: c.action,
            })
        })
        .collect();

    items.sort_by_key(|i| std::cmp::Reverse(i.item.size_bytes));

    let mut allowed_roots = vec![env.home.clone()];
    if let Ok(root) = fs::canonicalize(base_path) {
        allowed_roots.push(root);
    }
    ScanResult {
        items,
        allowed_roots,
    }
}

/// Filter bucket for a global rule's module.
fn module_group(module: &str) -> &'static str {
    match module {
        "ai_assistants" | "ai" => "AI Tools",
        "editors" => "Editors",
        "os_system" | "social" | "adobe" => "System & Apps",
        _ => "Dev Tools",
    }
}

fn collect_global(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    let rules = GLOBAL_RULES.iter().chain(ai_rules::ai_rules());
    for rule in rules.filter(|r| enabled.contains(r.module)) {
        for path in expand(&env.home, rule.home_rel) {
            if rule.split_children {
                if !is_real_dir(&path) {
                    continue;
                }
                let Ok(rd) = fs::read_dir(&path) else {
                    continue;
                };
                for entry in rd.flatten() {
                    let child = entry.path();
                    if !is_real_dir(&child) {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let description = if rule.note.is_empty() {
                        format!(
                            "Cache folder for {name}. Usually rebuilt by the app, but close the app first and review before removing."
                        )
                    } else {
                        rule.note.to_string()
                    };
                    out.push(Candidate {
                        path: child,
                        file_type: if rule.note.is_empty() {
                            name
                        } else {
                            rule.label.to_string()
                        },
                        category: rule.category.to_string(),
                        safety: rule.safety,
                        description,
                        blocked_if_running: rule.blocked_if_running.to_vec(),
                        project_root: None,
                        action: Action::DeletePath,
                        known_size: None,
                        min_bytes: rule.min_bytes,
                        group: module_group(rule.module),
                    });
                }
            } else {
                // Folders or single files; never a symlink.
                let is_plain = fs::symlink_metadata(&path)
                    .map(|m| !m.file_type().is_symlink())
                    .unwrap_or(false);
                if !is_plain {
                    continue;
                }
                out.push(Candidate {
                    path,
                    file_type: rule.label.to_string(),
                    category: rule.category.to_string(),
                    safety: rule.safety,
                    description: if rule.note.is_empty() {
                        describe_global(rule)
                    } else {
                        rule.note.to_string()
                    },
                    blocked_if_running: rule.blocked_if_running.to_vec(),
                    project_root: None,
                    action: Action::DeletePath,
                    known_size: None,
                    min_bytes: rule.min_bytes,
                    group: module_group(rule.module),
                });
            }
        }
    }
}

fn describe_global(rule: &GlobalRule) -> String {
    match rule.safety {
        Danger => format!("{}: contains data that cannot be regenerated automatically. Remove only if you are sure.", rule.label),
        Review => format!("{}: can be regenerated, but may need a long re-download or contain media you want to keep.", rule.label),
        Safe => format!("{}: regenerated automatically when needed.", rule.label),
    }
}

fn collect_projects(
    base: &Path,
    enabled: &HashSet<&str>,
    is_ignored: &dyn Fn(&Path) -> bool,
    out: &mut Vec<Candidate>,
) {
    let rules: Vec<&ProjectRule> = PROJECT_RULES
        .iter()
        .filter(|r| enabled.contains(r.module))
        .collect();
    if rules.is_empty() {
        return;
    }

    let walker = WalkDir::new(base)
        .max_depth(MAX_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return false;
            }
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref()) && !is_ignored(e.path())
        });

    for entry in walker.flatten() {
        let dir = entry.path();
        let Ok(rd) = fs::read_dir(dir) else { continue };
        let names: HashSet<OsString> = rd.flatten().map(|e| e.file_name()).collect();

        for rule in &rules {
            if !rule.markers.iter().any(|m| marker_matches(dir, &names, m)) {
                continue;
            }
            for target in rule.targets {
                for path in expand(dir, target.rel) {
                    if fs::symlink_metadata(&path).is_err() {
                        continue;
                    }
                    let project = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    out.push(Candidate {
                        path,
                        file_type: rule.name.to_string(),
                        category: rule.label.to_string(),
                        safety: target.safety,
                        description: format!(
                            "{} in project \"{}\" ({}). Regenerated by the next build or install.",
                            target.rel, project, rule.name
                        ),
                        blocked_if_running: rule.blocked_if_running.to_vec(),
                        project_root: Some(dir.to_path_buf()),
                        action: Action::DeletePath,
                        known_size: None,
                        min_bytes: 1,
                        group: "Projects",
                    });
                }
            }
        }
    }
}

fn marker_matches(dir: &Path, names: &HashSet<OsString>, marker: &str) -> bool {
    if let Some(ext) = marker.strip_prefix('*') {
        names.iter().any(|n| n.to_string_lossy().ends_with(ext))
    } else if marker.contains('/') {
        dir.join(marker).is_file()
    } else {
        names.contains(&OsString::from(marker))
    }
}

/// Expand a relative pattern under `base`. Patterns without `*` resolve to one path.
fn expand(base: &Path, rel: &str) -> Vec<PathBuf> {
    if !rel.contains('*') {
        return vec![base.join(rel)];
    }
    let pattern = format!("{}/{}", glob::Pattern::escape(&base.to_string_lossy()), rel);
    match glob::glob(&pattern) {
        Ok(paths) => paths.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

fn is_real_dir(p: &Path) -> bool {
    fs::symlink_metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

/// Drop project candidates that contain git-tracked files (PLAN 0.3).
fn drop_git_tracked(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut repos: HashMap<PathBuf, Option<(PathBuf, Vec<String>)>> = HashMap::new();
    candidates
        .into_iter()
        .filter(|c| {
            let Some(root) = &c.project_root else {
                return true;
            };
            let entry = repos
                .entry(root.clone())
                .or_insert_with(|| load_repo_index(root));
            match entry {
                None => true,
                Some((workdir, tracked)) => !contains_tracked(workdir, tracked, &c.path),
            }
        })
        .collect()
}

fn load_repo_index(project_root: &Path) -> Option<(PathBuf, Vec<String>)> {
    let repo = git2::Repository::discover(project_root).ok()?;
    let workdir = fs::canonicalize(repo.workdir()?).ok()?;
    let index = repo.index().ok()?;
    let mut paths: Vec<String> = index
        .iter()
        .map(|e| String::from_utf8_lossy(&e.path).into_owned())
        .collect();
    paths.sort();
    Some((workdir, paths))
}

fn contains_tracked(workdir: &Path, tracked: &[String], path: &Path) -> bool {
    let Ok(canonical) = fs::canonicalize(path) else {
        return false;
    };
    let Ok(rel) = canonical.strip_prefix(workdir) else {
        return false;
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    if rel.is_empty() {
        return true; // the whole repository
    }
    if tracked.binary_search(&rel).is_ok() {
        return true;
    }
    let prefix = format!("{rel}/");
    let i = tracked.partition_point(|p| p.as_str() < prefix.as_str());
    tracked.get(i).is_some_and(|p| p.starts_with(&prefix))
}

/// Keep only the outermost of nested candidates and remove duplicates.
fn drop_nested(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    let mut kept: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for c in candidates {
        if kept.last().is_some_and(|k| c.path.starts_with(&k.path)) {
            continue;
        }
        kept.push(c);
    }
    kept
}

const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

pub fn has_cachedir_tag(dir: &Path) -> bool {
    let mut buf = [0u8; 43];
    fs::File::open(dir.join("CACHEDIR.TAG"))
        .and_then(|mut f| f.read_exact(&mut buf))
        .map(|_| buf == CACHEDIR_SIGNATURE)
        .unwrap_or(false)
}

/// FNV-1a: stable across runs, unlike `DefaultHasher`.
fn stable_id(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

// ---------------------------------------------------------------------------
// Size
// ---------------------------------------------------------------------------

/// Bytes that would actually be freed by deleting `path` (PLAN 1.1).
/// Counts allocated blocks, never follows symlinks, and counts a hard-linked file
/// only when every link to it lives inside `path`.
pub fn disk_usage(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut total = 0u64;
        // (dev, ino) -> (nlink, links seen, bytes)
        let mut linked: HashMap<(u64, u64), (u64, u64, u64)> = HashMap::new();
        for entry in WalkDir::new(path).follow_links(false).into_iter().flatten() {
            let Ok(m) = entry.metadata() else { continue };
            if m.file_type().is_symlink() {
                continue;
            }
            let bytes = m.blocks() * 512;
            if m.is_dir() || m.nlink() <= 1 {
                total += bytes;
            } else {
                let e = linked
                    .entry((m.dev(), m.ino()))
                    .or_insert((m.nlink(), 0, bytes));
                e.1 += 1;
            }
        }
        total
            + linked
                .values()
                .filter(|(nlink, seen, _)| seen >= nlink)
                .map(|(_, _, b)| b)
                .sum::<u64>()
    }
    #[cfg(not(unix))]
    {
        WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Deletion
// ---------------------------------------------------------------------------

/// Paths that must never be deleted, nor any of their ancestors (PLAN 0.5).
pub fn protected_paths(home: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        "/",
        "/System",
        "/Applications",
        "/Library",
        "/usr",
        "/bin",
        "/sbin",
        "/etc",
        "/var",
        "/private",
        "/opt",
        "/opt/homebrew",
        "/Volumes",
        "/Users",
        "/home",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    v.push(home.to_path_buf());
    for rel in [
        "Documents",
        "Desktop",
        "Downloads",
        "Pictures",
        "Movies",
        "Music",
        "Library",
        "Library/Caches",
        "Library/Application Support",
        "Library/Developer",
        "Library/Group Containers",
        "Library/Containers",
        ".ssh",
        ".gnupg",
        ".config",
        ".cache",
    ] {
        v.push(home.join(rel));
    }
    v.extend(ai_rules::PROTECTED.iter().map(|rel| home.join(rel)));
    // Compare against canonical forms too (/var -> /private/var on macOS).
    let canon: Vec<PathBuf> = v.iter().filter_map(|p| fs::canonicalize(p).ok()).collect();
    v.extend(canon);
    v
}

/// Validate and delete one scanned item.
pub fn delete_path(
    item: &ScannedItem,
    allowed_roots: &[PathBuf],
    protected: &[PathBuf],
) -> Result<(), String> {
    let path = Path::new(&item.item.path);
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };

    // Re-resolve: the path must still point where it pointed during the scan.
    let canonical = if meta.file_type().is_symlink() {
        let parent = path.parent().ok_or("Invalid path")?;
        fs::canonicalize(parent)
            .map_err(|e| e.to_string())?
            .join(path.file_name().ok_or("Invalid path")?)
    } else {
        fs::canonicalize(path).map_err(|e| e.to_string())?
    };
    if !meta.file_type().is_symlink() && canonical != item.canonical {
        return Err(format!(
            "{} changed since the scan. Scan again.",
            path.display()
        ));
    }

    if protected.iter().any(|p| p.starts_with(&canonical)) {
        return Err(format!("{} is a protected location.", canonical.display()));
    }
    let inside_allowed = allowed_roots
        .iter()
        .any(|root| canonical.starts_with(root) && canonical != *root);
    if !inside_allowed {
        return Err(format!(
            "{} is outside the scanned folder and your home folder.",
            canonical.display()
        ));
    }

    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        // Plain files and symlinks (the link itself, never its target).
        fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to delete {}: {e}", path.display())),
    }
}

/// Remove one scanned item: delete its path or run its official command.
pub fn execute(
    item: &ScannedItem,
    allowed_roots: &[PathBuf],
    protected: &[PathBuf],
) -> Result<(), String> {
    match &item.action {
        Action::DeletePath => delete_path(item, allowed_roots, protected),
        Action::Command(args) => simulators::run_command(args),
    }
}

/// Lower-cased names of running processes.
pub fn running_process_names() -> HashSet<String> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let sys =
        System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    sys.processes()
        .values()
        .map(|p| p.name().to_lowercase())
        .collect()
}

pub fn blocking_app(item: &ScannedItem, running: &HashSet<String>) -> Option<&'static str> {
    item.blocked_if_running
        .iter()
        .copied()
        .find(|name| running.contains(&name.to_lowercase()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn write(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; bytes]).unwrap();
    }

    fn scan(base: &Path, modules: &[&str]) -> ScanResult {
        let env = Env {
            home: base.join("__home"),
        };
        fs::create_dir_all(&env.home).unwrap();
        let modules: Vec<String> = modules.iter().map(|s| s.to_string()).collect();
        scan_directory(base, &modules, &[], &env, &HashSet::new())
    }

    fn paths(r: &ScanResult) -> Vec<String> {
        let mut v: Vec<String> = r.items.iter().map(|i| i.item.path.clone()).collect();
        v.sort();
        v
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn flutter_targets_found() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join("build/out.bin"), 5000);
        write(&p.join(".dart_tool/x"), 5000);
        write(&p.join("lib/main.dart"), 100);
        let r = scan(d.path(), &["flutter"]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 2);
        assert!(ps
            .iter()
            .all(|p| p.ends_with("build") || p.ends_with(".dart_tool")));
        assert!(r.items.iter().all(|i| i.item.status == Safe));
    }

    #[test]
    fn go_source_dirs_are_never_targets() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("svc");
        write(&p.join("go.mod"), 10);
        write(&p.join("pkg/api/handler.go"), 5000);
        write(&p.join("bin/tool"), 5000);
        let r = scan(d.path(), &["go", "ai", "python"]);
        assert!(r.items.is_empty());
    }

    #[test]
    fn ai_outputs_in_git_repo_are_not_targets() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("ml");
        write(&p.join(".gitattributes"), 10);
        write(&p.join("outputs/results.csv"), 5000);
        write(&p.join("runs/exp1/log"), 5000);
        let r = scan(d.path(), &["ai"]);
        assert!(r.items.is_empty());
    }

    #[test]
    fn git_tracked_build_dir_is_skipped() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("web");
        write(&p.join("package.json"), 10);
        write(&p.join("dist/app.js"), 5000); // tracked: must stay
        write(&p.join("node_modules/a/index.js"), 5000); // ignored: may go
        fs::write(p.join(".gitignore"), "node_modules\n").unwrap();
        git(&p, &["init", "-q"]);
        git(&p, &["add", "-A"]);
        git(&p, &["commit", "-qm", "init"]);
        let ps = paths(&scan(d.path(), &["node"]));
        assert_eq!(ps.len(), 1);
        assert!(ps[0].ends_with("node_modules"));
    }

    #[test]
    fn untracked_build_dir_in_repo_is_reported() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        git(&p, &["init", "-q"]);
        git(&p, &["add", "-A"]);
        git(&p, &["commit", "-qm", "init"]);
        write(&p.join("build/x"), 5000);
        assert_eq!(scan(d.path(), &["flutter"]).items.len(), 1);
    }

    #[test]
    fn cachedir_tag_upgrades_rust_target() {
        let d = TempDir::new().unwrap();
        let tagged = d.path().join("a");
        write(&tagged.join("Cargo.toml"), 10);
        write(&tagged.join("target/debug/x"), 5000);
        fs::write(
            tagged.join("target/CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n# cargo",
        )
        .unwrap();
        let plain = d.path().join("b");
        write(&plain.join("Cargo.toml"), 10);
        write(&plain.join("target/notes.txt"), 5000);
        let r = scan(d.path(), &["rust"]);
        let status = |name: &str| {
            r.items
                .iter()
                .find(|i| i.item.path.contains(&format!("/{name}/")))
                .unwrap()
                .item
                .status
        };
        assert_eq!(status("a"), Safe);
        assert_eq!(status("b"), Review);
    }

    #[test]
    fn glob_targets_expand() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("api");
        write(&p.join("composer.json"), 10);
        write(&p.join("bootstrap/cache/services.php"), 5000);
        write(&p.join("bootstrap/cache/.gitignore"), 10);
        let ps = paths(&scan(d.path(), &["php"]));
        assert_eq!(ps.len(), 1);
        assert!(ps[0].ends_with("services.php"));
    }

    #[test]
    fn same_project_name_gets_distinct_ids() {
        let d = TempDir::new().unwrap();
        for parent in ["one", "two"] {
            let p = d.path().join(parent).join("app");
            write(&p.join("package.json"), 10);
            write(&p.join("node_modules/x"), 5000);
        }
        let r = scan(d.path(), &["node"]);
        assert_eq!(r.items.len(), 2);
        assert_ne!(r.items[0].item.id, r.items[1].item.id);
    }

    #[test]
    fn nested_candidates_collapse() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("package.json"), 10);
        write(&p.join("node_modules/dep/package.json"), 10);
        write(&p.join("node_modules/dep/dist/x.js"), 5000);
        let ps = paths(&scan(d.path(), &["node"]));
        assert_eq!(ps.len(), 1);
        assert!(ps[0].ends_with("app/node_modules"));
    }

    #[test]
    fn ignored_paths_are_skipped() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("package.json"), 10);
        write(&p.join("node_modules/x"), 5000);
        let env = Env {
            home: d.path().join("__home"),
        };
        let ignored = vec![p.join("node_modules").to_string_lossy().into_owned()];
        let r = scan_directory(d.path(), &["node".into()], &ignored, &env, &HashSet::new());
        assert!(r.items.is_empty());
    }

    #[test]
    fn running_app_blocks_item() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("game");
        write(&p.join("ProjectSettings/ProjectVersion.txt"), 10);
        write(&p.join("Library/cache.bin"), 5000);
        let env = Env {
            home: d.path().join("__home"),
        };
        let running: HashSet<String> = ["unity".to_string()].into();
        let r = scan_directory(d.path(), &["unity".into()], &[], &env, &running);
        assert_eq!(r.items[0].item.blocked_by.as_deref(), Some("Unity"));
        assert_eq!(r.items[0].item.status, Danger);
    }

    #[cfg(unix)]
    #[test]
    fn hardlinks_shared_outside_are_not_counted() {
        let d = TempDir::new().unwrap();
        let store = d.path().join("store/blob");
        write(&store, 64 * 1024);
        let inside = d.path().join("nm");
        fs::create_dir_all(&inside).unwrap();
        fs::hard_link(&store, inside.join("blob")).unwrap();
        let only_dir = disk_usage(&inside);
        assert!(
            only_dir < 64 * 1024,
            "shared hardlink was counted: {only_dir}"
        );

        let both = d.path().join("both");
        write(&both.join("a"), 64 * 1024);
        fs::hard_link(both.join("a"), both.join("b")).unwrap();
        let u = disk_usage(&both);
        assert!(
            (64 * 1024..2 * 64 * 1024).contains(&u),
            "counted twice or not at all: {u}"
        );
    }

    fn scanned(path: &Path) -> ScannedItem {
        ScannedItem {
            item: CleanupItem {
                id: "x".into(),
                path: path.to_string_lossy().into_owned(),
                file_type: String::new(),
                category: String::new(),
                group: String::new(),
                size_bytes: 1,
                status: Safe,
                description: String::new(),
                blocked_by: None,
            },
            canonical: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
            blocked_if_running: vec![],
            action: Action::DeletePath,
        }
    }

    #[test]
    fn delete_refuses_protected_and_outside_paths() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        let protected = protected_paths(&home);
        let roots = vec![home.clone(), root.join("code")];

        assert!(delete_path(&scanned(&home), &roots, &protected).is_err());
        assert!(delete_path(&scanned(&home.join("Documents")), &roots, &protected).is_err());
        assert!(delete_path(&scanned(&root), &roots, &protected).is_err());

        let outside = root.join("elsewhere/build");
        fs::create_dir_all(&outside).unwrap();
        assert!(delete_path(&scanned(&outside), &roots, &protected).is_err());
        assert!(outside.exists());

        let ok = root.join("code/app/build");
        write(&ok.join("x"), 10);
        assert!(delete_path(&scanned(&ok), &roots, &protected).is_ok());
        assert!(!ok.exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_removes_symlink_not_target() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let precious = root.join("precious");
        write(&precious.join("data.txt"), 10);
        let link = root.join("code/app/build");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&precious, &link).unwrap();

        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));
        delete_path(&scanned(&link), &roots, &protected).unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(precious.join("data.txt").exists());
    }

    #[test]
    fn delete_refuses_path_swapped_after_scan() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let p = root.join("code/app/build");
        write(&p.join("x"), 10);
        let mut item = scanned(&p);
        item.canonical = root.join("code/other");
        let err = delete_path(
            &item,
            &[root.join("code")],
            &protected_paths(&root.join("home")),
        );
        assert!(err.is_err());
        assert!(p.exists());
    }

    #[test]
    fn stable_id_is_deterministic() {
        assert_eq!(stable_id("/a/b"), stable_id("/a/b"));
        assert_ne!(stable_id("/a/b"), stable_id("/a/c"));
    }

    // ----- AI assistants (PLAN 2.16) -----------------------------------------

    fn ai_scan(home_files: &[(&str, usize)]) -> (TempDir, ScanResult) {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        for (rel, size) in home_files {
            write(&home.join(rel), *size);
        }
        let r = scan(d.path(), &["ai_assistants"]);
        (d, r)
    }

    fn find<'a>(r: &'a ScanResult, suffix: &str) -> Option<&'a ScannedItem> {
        r.items.iter().find(|i| i.item.path.ends_with(suffix))
    }

    const BIG: usize = 2 * 1024 * 1024;

    #[test]
    fn ai_history_is_danger_and_caches_are_safe() {
        let (_d, r) = ai_scan(&[
            (".codex/sessions/2026/10/05/rollout-a.jsonl", BIG),
            (".codex/log/codex-tui.log", 5000),
            (".claude/projects/-Users-me-app/s1.jsonl", BIG),
            (".claude/shell-snapshots/snap.sh", 5000),
            (".gemini/antigravity/brain/uuid-1/task.md", BIG),
            ("Library/Application Support/Cursor/Cache/data_0", 5000),
            (".deepseek/sessions/s1.json", 5000),
            (".deepseek/audit.log", 500),
        ]);
        assert_eq!(find(&r, ".deepseek/sessions").unwrap().item.status, Danger);
        assert_eq!(find(&r, ".deepseek/audit.log").unwrap().item.status, Safe);
        assert_eq!(
            find(&r, ".codex/sessions/2026").unwrap().item.status,
            Danger
        );
        assert_eq!(
            find(&r, ".claude/projects/-Users-me-app")
                .unwrap()
                .item
                .status,
            Danger
        );
        assert_eq!(
            find(&r, "antigravity/brain/uuid-1").unwrap().item.status,
            Danger
        );
        assert_eq!(find(&r, ".codex/log").unwrap().item.status, Safe);
        assert_eq!(
            find(&r, ".claude/shell-snapshots").unwrap().item.status,
            Safe
        );
        assert_eq!(find(&r, "Cursor/Cache").unwrap().item.status, Safe);
    }

    #[test]
    fn ai_settings_and_logins_are_never_listed() {
        let secrets = [
            ".codex/auth.json",
            ".codex/config.toml",
            ".claude/settings.json",
            ".claude.json",
            ".gemini/settings.json",
            ".gemini/oauth_creds.json",
            ".copilot/config.json",
            ".continue/config.yaml",
            ".local/share/amp/secrets.json",
            ".deepseek/config.toml",
            ".deepseek/instructions.md",
            ".deepseek/skills/s.md",
            ".deepseek/tasks/t.json",
            ".kimi-work/bin/kimi",
            "Library/Application Support/Cursor/User/globalStorage/state.vscdb",
            "Library/Application Support/Claude/claude_desktop_config.json",
            "Library/Application Support/Claude/vm_bundles/claudevm.bundle/sessiondata.img",
        ];
        let files: Vec<(&str, usize)> = secrets.iter().map(|s| (*s, BIG)).collect();
        let (_d, r) = ai_scan(&files);
        for item in &r.items {
            for secret in secrets {
                assert!(
                    !Path::new(secret).starts_with(Path::new(&item.item.path))
                        && !item.item.path.ends_with(secret),
                    "{} would remove {secret}",
                    item.item.path
                );
            }
        }
        assert!(r.items.is_empty(), "unexpected items: {:?}", paths(&r));
    }

    #[test]
    fn claude_vm_image_is_review_and_session_data_kept() {
        let (_d, r) = ai_scan(&[
            (
                "Library/Application Support/Claude/vm_bundles/claudevm.bundle/rootfs.img",
                BIG,
            ),
            (
                "Library/Application Support/Claude/vm_bundles/claudevm.bundle/sessiondata.img",
                BIG,
            ),
        ]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 1);
        assert!(ps[0].ends_with("rootfs.img"));
        assert_eq!(r.items[0].item.status, Review);
    }

    #[test]
    fn cline_checkpoints_found_in_any_editor() {
        let (_d, r) = ai_scan(&[
            ("Library/Application Support/Cursor/User/globalStorage/saoudrizwan.claude-dev/checkpoints/h/x", BIG),
            ("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/tasks/t1/ui_messages.json", BIG),
            ("Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json", BIG),
        ]);
        assert_eq!(
            find(
                &r,
                "Cursor/User/globalStorage/saoudrizwan.claude-dev/checkpoints"
            )
            .unwrap()
            .item
            .status,
            Review
        );
        assert_eq!(
            find(&r, "Code/User/globalStorage/saoudrizwan.claude-dev/tasks")
                .unwrap()
                .item
                .status,
            Danger
        );
        assert!(find(&r, "settings").is_none());
    }

    #[test]
    fn chatgpt_mirror_is_review() {
        let (_d, r) = ai_scan(&[
            (
                "Library/Application Support/com.openai.chat/conversations-v3-abc/c1.data",
                BIG,
            ),
            (
                "Library/Application Support/com.openai.chat/drafts-v2-abc/d1.data",
                BIG,
            ),
            ("Library/Caches/com.openai.chat/img", 5000),
        ]);
        assert_eq!(
            find(&r, "conversations-v3-abc").unwrap().item.status,
            Review
        );
        assert!(find(&r, "drafts-v2-abc").is_none());
        assert_eq!(
            find(&r, "Caches/com.openai.chat").unwrap().item.status,
            Safe
        );
    }

    #[test]
    fn small_split_history_is_hidden() {
        let (_d, r) = ai_scan(&[(".claude/projects/-tiny/s.jsonl", 2000)]);
        assert!(r.items.is_empty());
    }

    #[test]
    fn aider_project_files() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join(".aider.chat.history.md"), 5000);
        write(&p.join(".aider.tags.cache.v4/cache.db"), 5000);
        write(&p.join("main.py"), 5000);
        let r = scan(d.path(), &["ai_assistants"]);
        assert_eq!(find(&r, ".aider.tags.cache.v4").unwrap().item.status, Safe);
        assert_eq!(
            find(&r, ".aider.chat.history.md").unwrap().item.status,
            Danger
        );
        assert_eq!(r.items.len(), 2);
    }

    #[test]
    fn delete_refuses_ai_config_roots() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        for rel in [
            ".codex/auth.json",
            ".claude/settings.json",
            ".gemini/oauth_creds.json",
        ] {
            write(&home.join(rel), 10);
        }
        let protected = protected_paths(&home);
        let roots = vec![home.clone()];
        for rel in [
            ".codex",
            ".codex/auth.json",
            ".claude",
            ".gemini",
            ".gemini/oauth_creds.json",
        ] {
            assert!(
                delete_path(&scanned(&home.join(rel)), &roots, &protected).is_err(),
                "{rel} was not protected"
            );
        }
        assert!(home.join(".codex/auth.json").exists());
    }

    // ----- Old extensions and versions -----------------------------------------

    #[test]
    fn old_extensions_use_editor_signals_only() {
        let d = TempDir::new().unwrap();
        let ext = d.path().join("__home/.antigravity-ide/extensions");
        for name in [
            "anthropic.claude-code-2.1.287-darwin-arm64",
            "anthropic.claude-code-2.1.289-darwin-arm64",
            "qwenlm.qwen-code-0.24.7",
            "qwenlm.qwen-code-0.25.0",
            "dart-code.flutter-3.134.0",
            "lonely.unlisted-1.0.0",
        ] {
            write(&ext.join(name).join("package.json"), 5000);
        }
        fs::write(ext.join(".obsolete"), r#"{"qwenlm.qwen-code-0.24.7":true}"#).unwrap();
        fs::write(
            ext.join("extensions.json"),
            r#"[{"relativeLocation":"anthropic.claude-code-2.1.289-darwin-arm64"},
                {"relativeLocation":"qwenlm.qwen-code-0.25.0"},
                {"relativeLocation":"dart-code.flutter-3.134.0"}]"#,
        )
        .unwrap();
        let mut ps: Vec<String> = scan(d.path(), &["editors"])
            .items
            .iter()
            .map(|i| i.item.path.rsplit('/').next().unwrap().to_string())
            .collect();
        ps.sort();
        assert_eq!(
            ps,
            vec![
                "anthropic.claude-code-2.1.287-darwin-arm64",
                "qwenlm.qwen-code-0.24.7"
            ]
        );
    }

    #[test]
    fn obsolete_but_referenced_extension_is_kept() {
        let d = TempDir::new().unwrap();
        let ext = d.path().join("__home/.trae/extensions");
        write(&ext.join("a.b-1.0.0/x"), 5000);
        fs::write(ext.join(".obsolete"), r#"{"a.b-1.0.0":true}"#).unwrap();
        fs::write(
            ext.join("extensions.json"),
            r#"[{"relativeLocation":"a.b-1.0.0"}]"#,
        )
        .unwrap();
        assert!(scan(d.path(), &["editors"]).items.is_empty());
    }

    #[test]
    fn newest_bundled_version_is_kept() {
        let (_d, r) = ai_scan(&[
            (
                "Library/Application Support/Claude/claude-code/2.1.9/claude",
                5000,
            ),
            (
                "Library/Application Support/Claude/claude-code/2.1.286/claude",
                5000,
            ),
            (
                "Library/Application Support/Claude/claude-code-vm/2.1.286/claude",
                5000,
            ),
        ]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 1, "{ps:?}");
        assert!(ps[0].ends_with("claude-code/2.1.9"));
    }

    #[test]
    fn antigravity_ide_history_and_browser_cache() {
        let (_d, r) = ai_scan(&[
            (".gemini/antigravity-ide/conversations/c1.pb", BIG),
            (
                ".gemini/antigravity-browser-profile/Default/Cache/Cache_Data/f",
                5000,
            ),
            (".gemini/antigravity-browser-profile/Default/Cookies", BIG),
            (
                ".gemini/antigravity-browser-profile/Default/Login Data",
                BIG,
            ),
            (
                "Library/Application Support/Claude/local-agent-mode-sessions/s-1/log",
                BIG,
            ),
        ]);
        assert_eq!(
            find(&r, "antigravity-ide/conversations")
                .unwrap()
                .item
                .status,
            Danger
        );
        assert_eq!(find(&r, "Default/Cache").unwrap().item.status, Safe);
        assert_eq!(
            find(&r, "local-agent-mode-sessions/s-1")
                .unwrap()
                .item
                .status,
            Danger
        );
        assert!(find(&r, "Cookies").is_none());
        assert!(find(&r, "Login Data").is_none());
        assert_eq!(r.items.len(), 3);
    }

    // ----- Xcode (PLAN 2.1) ------------------------------------------------------

    #[test]
    fn xcode_device_support_keeps_two_newest() {
        let d = TempDir::new().unwrap();
        let ds = d
            .path()
            .join("__home/Library/Developer/Xcode/iOS DeviceSupport");
        for v in [
            "iPhone15,2 17.5 (21F79)",
            "iPhone15,2 18.1 (22B83)",
            "16.4.1 (20E252)",
            "iPhone16,1 18.6 (22G86)",
        ] {
            write(&ds.join(v).join("Symbols/x"), 5000);
        }
        let r = scan(d.path(), &["xcode"]);
        let mut names: Vec<String> = r
            .items
            .iter()
            .map(|i| i.item.path.rsplit('/').next().unwrap().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["16.4.1 (20E252)", "iPhone15,2 17.5 (21F79)"]);
        assert!(r.items.iter().all(|i| i.item.status == Safe));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn xcode_archives_and_simulator_caches() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Developer/Xcode/Archives/2026-10-01/App.xcarchive/dSYMs/a"),
            5000,
        );
        write(
            &home.join("Library/Developer/CoreSimulator/Caches/dyld/x"),
            5000,
        );
        let r = scan(d.path(), &["xcode"]);
        assert_eq!(find(&r, "Archives/2026-10-01").unwrap().item.status, Review);
        assert_eq!(find(&r, "CoreSimulator/Caches").unwrap().item.status, Safe);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cocoapods_cache_and_spec_repos() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        // Over the 10 MB split threshold, so `os_system` would list it too.
        write(
            &home.join("Library/Caches/CocoaPods/Pods/Release/Alamofire/a"),
            11 * 1024 * 1024,
        );
        write(&home.join(".cocoapods/repos/trunk/Specs/x.json"), 5000);
        // Negative: CocoaPods config next to the repos must stay.
        write(&home.join(".cocoapods/config.yaml"), 100);
        let r = scan(d.path(), &["cocoapods"]);
        assert_eq!(
            find(&r, "Library/Caches/CocoaPods").unwrap().item.status,
            Safe
        );
        assert_eq!(find(&r, ".cocoapods/repos").unwrap().item.status, Review);
        assert!(find(&r, ".cocoapods").is_none());
        assert!(find(&r, "config.yaml").is_none());
        assert_eq!(r.items.len(), 2);

        // The specific rule wins over the per-app split of ~/Library/Caches.
        let r = scan(d.path(), &["cocoapods", "os_system"]);
        let pods: Vec<_> = r
            .items
            .iter()
            .filter(|i| i.item.path.ends_with("Library/Caches/CocoaPods"))
            .collect();
        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].item.file_type, "CocoaPods Cache");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cocoapods_disabled_lists_nothing() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/CocoaPods/Pods/a"), 5000);
        write(&home.join(".cocoapods/repos/trunk/a"), 5000);
        let r = scan(d.path(), &["homebrew"]);
        assert!(r.items.is_empty());
    }
}
