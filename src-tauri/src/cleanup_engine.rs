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
    collect_projects(base_path, &enabled, &is_ignored, &mut candidates);

    candidates.retain(|c| !is_ignored(&c.path));
    let candidates = drop_git_tracked(candidates);
    let candidates = drop_nested(candidates);

    let mut items: Vec<ScannedItem> = candidates
        .into_par_iter()
        .filter_map(|mut c| {
            let size = disk_usage(&c.path);
            if size == 0 {
                return None;
            }
            if c.project_root.is_some() && c.safety == Review && has_cachedir_tag(&c.path) {
                c.safety = Safe;
            }
            let min = if c.category == "System Cache" {
                MIN_SPLIT_CHILD_BYTES
            } else {
                1
            };
            if size < min {
                return None;
            }
            let canonical = fs::canonicalize(&c.path).ok()?;
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
                    size_bytes: size,
                    status: c.safety,
                    description: c.description,
                    blocked_by,
                },
                canonical,
                blocked_if_running: c.blocked_if_running,
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

fn collect_global(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    for rule in GLOBAL_RULES.iter().filter(|r| enabled.contains(r.module)) {
        for path in expand(&env.home, rule.home_rel) {
            if !is_real_dir(&path) {
                continue;
            }
            if rule.split_children {
                let Ok(rd) = fs::read_dir(&path) else {
                    continue;
                };
                for entry in rd.flatten() {
                    let child = entry.path();
                    if !is_real_dir(&child) {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    out.push(Candidate {
                        path: child,
                        file_type: name.clone(),
                        category: rule.category.to_string(),
                        safety: rule.safety,
                        description: format!(
                            "Cache folder for {name}. Usually rebuilt by the app, but close the app first and review before removing."
                        ),
                        blocked_if_running: rule.blocked_if_running.to_vec(),
                        project_root: None,
                    });
                }
            } else {
                out.push(Candidate {
                    path,
                    file_type: "Global".to_string(),
                    category: rule.category.to_string(),
                    safety: rule.safety,
                    description: describe_global(rule),
                    blocked_if_running: rule.blocked_if_running.to_vec(),
                    project_root: None,
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
                size_bytes: 1,
                status: Safe,
                description: String::new(),
                blocked_by: None,
            },
            canonical: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
            blocked_if_running: vec![],
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
}
