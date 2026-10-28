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
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

mod age;
mod ai_models;
mod ai_rules;
mod android;
mod brew;
mod conda;
mod dart;
mod docker;
mod editors;
mod go;
mod native_commands;
mod node;
mod rust;
mod simulators;
mod system;
mod versions;

use native_commands::NativeCommand;

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
    /// Scan-screen card id that produced the item, e.g. "xcode" or "node".
    pub module: String,
    pub size_bytes: u64,
    pub status: Safety,
    pub description: String,
    /// Name of a running app that must be closed before this item can be removed.
    pub blocked_by: Option<String>,
    /// Newest known project activity (unix seconds); `None` for global items and
    /// projects whose activity cannot be determined.
    pub last_active: Option<i64>,
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
    /// Run an official tool instead (PLAN 3.3). See `native_commands`.
    Command(NativeCommand),
}

/// How a scanned item is removed from disk (PLAN 3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    /// Remove the path completely.
    Permanent,
    /// Move the path to the system Trash when that is safe, else delete it.
    Trash,
}

impl DeleteMode {
    /// Parse a mode sent by the UI. Only the exact lower-case names are accepted.
    pub fn parse(s: &str) -> Option<DeleteMode> {
        match s {
            "permanent" => Some(DeleteMode::Permanent),
            "trash" => Some(DeleteMode::Trash),
            _ => None,
        }
    }
}

/// Above this size an item is deleted permanently instead of filling the Trash.
const TRASH_MAX_BYTES: u64 = 5_000_000_000;

/// What one removal did: the bytes freed, and whether it went to the Trash.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Removed {
    pub bytes: u64,
    pub trashed: bool,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub items: Vec<ScannedItem>,
    /// Roots that deletions are allowed under (the scan root and the user's home).
    pub allowed_roots: Vec<PathBuf>,
}

/// Progress reported while a scan runs (PLAN 3.5).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanProgress {
    /// A new stage started.
    Phase { label: String },
    /// The project walker is looking at this directory.
    Path { path: String },
    /// Sizes are being measured; `done` of `total` items, `bytes` so far.
    Measuring { done: usize, total: usize, bytes: u64 },
}

/// Cancellation and progress callbacks for `scan_directory_with`.
pub struct ScanHooks<'a> {
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    pub progress: &'a (dyn Fn(ScanProgress) + Sync),
}

/// Optional scan settings. `Default` keeps the previous behaviour.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOptions {
    /// How many folder levels below the projects folder the walk visits.
    /// `None` uses [`MAX_DEPTH`]; values are clamped to `2..=12`.
    pub max_depth: Option<usize>,
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

struct Target {
    /// Path relative to the project root. May contain `*` (glob).
    rel: &'static str,
    safety: Safety,
    /// A file that must exist inside the target folder for it to be listed (empty = no check).
    needs: &'static str,
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
    Target {
        rel,
        safety,
        needs: "",
    }
}

const fn tn(rel: &'static str, safety: Safety, needs: &'static str) -> Target {
    Target { rel, safety, needs }
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
            t(".flutter-plugins", Safe),
            t(".flutter-plugins-dependencies", Safe),
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
    // For projects you are still working on: only the incremental cache, so the next
    // build keeps its compiled dependencies (PLAN 2.7).
    ProjectRule {
        module: "rust_incremental",
        name: "Rust",
        markers: &["Cargo.toml"],
        label: "Rust Incremental Cache",
        targets: &[
            t("target/debug/incremental", Safe),
            t("target/release/incremental", Safe),
        ],
        blocked_if_running: &["cargo", "rust-analyzer"],
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
            t("storage/logs/*.log", Safe),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "python",
        name: "Python",
        markers: &[
            "pyproject.toml",
            "requirements.txt",
            "setup.py",
            "setup.cfg",
            "Pipfile",
            "tox.ini",
        ],
        label: "Python Caches",
        targets: &[
            t("__pycache__", Safe),
            t("*/__pycache__", Safe),
            t("src/*/__pycache__", Safe),
            t(".pytest_cache", Safe),
            t(".mypy_cache", Safe),
            t(".ruff_cache", Safe),
            t(".tox", Safe),
        ],
        blocked_if_running: &[],
    },
    ProjectRule {
        module: "python_venv",
        name: "Python",
        markers: &[
            "pyproject.toml",
            "requirements.txt",
            "setup.py",
            "setup.cfg",
            "Pipfile",
            "tox.ini",
        ],
        label: "Virtual Environments",
        targets: &[
            tn(".venv", Review, "pyvenv.cfg"),
            tn("venv", Review, "pyvenv.cfg"),
        ],
        blocked_if_running: &["python", "python3"],
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
    g(
        "xcode",
        "Swift Package Cache",
        "Library/Caches/org.swift.swiftpm",
        "Build Cache",
        Safe,
        &["Xcode", "swift-build", "swift"],
    ),
    g(
        "xcode",
        "Swift Package Manager Data",
        "Library/org.swift.swiftpm",
        "Package Cache",
        Review,
        &["Xcode", "swift-build", "swift"],
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
        "rust",
        "Cargo Registry Downloads",
        ".cargo/registry/cache",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "rust",
        "Cargo Registry Sources",
        ".cargo/registry/src",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "rust",
        "Cargo Git Checkouts",
        ".cargo/git/checkouts",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "php",
        "Composer Cache",
        "Library/Caches/composer",
        "Package Cache",
        Safe,
        &["composer"],
    ),
    g(
        "php",
        "Composer Cache (legacy)",
        ".composer/cache",
        "Package Cache",
        Safe,
        &["composer"],
    ),
    g(
        "python",
        "Pip Cache",
        "Library/Caches/pip",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "poetry"],
    ),
    g(
        "python",
        "uv Cache",
        ".cache/uv",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "uvx", "poetry"],
    ),
    g(
        "python",
        "Poetry Cache",
        "Library/Caches/pypoetry",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "poetry"],
    ),
    g(
        "go",
        "Go Build Cache",
        "Library/Caches/go-build",
        "Build Cache",
        Safe,
        &["go"],
    ),
    GlobalRule {
        module: "java",
        label: "Maven Repository",
        home_rel: ".m2/repository",
        category: "Package Cache",
        safety: Review,
        blocked_if_running: &["mvn", "java"],
        split_children: false,
        note: "Maven local repository. Dependencies are downloaded again on the next build.",
        min_bytes: 1,
    },
    g(
        "devops",
        "Terraform Plugin Cache",
        ".terraform.d/plugin-cache",
        "Package Cache",
        Safe,
        &["terraform"],
    ),
    g(
        "android",
        "Android AVD Images",
        ".android/avd",
        "Emulator Data",
        Review,
        &["qemu-system-aarch64", "qemu-system-x86_64"],
    ),
    GlobalRule {
        module: "android",
        label: "Gradle Caches",
        home_rel: ".gradle/caches",
        category: "Gradle",
        safety: Review,
        blocked_if_running: &["studio"],
        split_children: false,
        note: "Dependencies and build caches shared by every Gradle project. Each project downloads what it needs again on its next build. Run ./gradlew --stop first if a build is running.",
        min_bytes: 1,
    },
    g(
        "android",
        "Gradle Daemon Logs",
        ".gradle/daemon",
        "Gradle",
        Safe,
        &[],
    ),
    g(
        "android",
        "Android SDK Cache",
        ".android/cache",
        "Android SDK",
        Safe,
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
    // Work apps (PLAN 2.18): caches only, never cookies, storage or sign-in data.
    g(
        "work_apps",
        "Slack Cache",
        "Library/Application Support/Slack/Cache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack Code Cache",
        "Library/Application Support/Slack/Code Cache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack GPU Cache",
        "Library/Application Support/Slack/GPUCache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack Service Worker Cache",
        "Library/Application Support/Slack/Service Worker/CacheStorage",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack App Cache",
        "Library/Caches/com.tinyspeck.slackmacgap",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Zoom Cache",
        "Library/Caches/us.zoom.xos",
        "App Cache",
        Safe,
        &["zoom.us"],
    ),
    g(
        "work_apps",
        "Teams Cache",
        "Library/Application Support/Microsoft/Teams/Cache",
        "App Cache",
        Safe,
        &["Microsoft Teams", "MSTeams"],
    ),
    g(
        "work_apps",
        "Teams Code Cache",
        "Library/Application Support/Microsoft/Teams/Code Cache",
        "App Cache",
        Safe,
        &["Microsoft Teams", "MSTeams"],
    ),
    g(
        "work_apps",
        "Teams GPU Cache",
        "Library/Application Support/Microsoft/Teams/GPUCache",
        "App Cache",
        Safe,
        &["Microsoft Teams", "MSTeams"],
    ),
    g(
        "work_apps",
        "Teams App Cache",
        "Library/Caches/com.microsoft.teams2",
        "App Cache",
        Safe,
        &["Microsoft Teams", "MSTeams"],
    ),
    g(
        "work_apps",
        "Notion Cache",
        "Library/Application Support/Notion/Cache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion Code Cache",
        "Library/Application Support/Notion/Code Cache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion GPU Cache",
        "Library/Application Support/Notion/GPUCache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion App Cache",
        "Library/Caches/notion.id",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Postman Cache",
        "Library/Application Support/Postman/Cache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman Code Cache",
        "Library/Application Support/Postman/Code Cache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman GPU Cache",
        "Library/Application Support/Postman/GPUCache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman App Cache",
        "Library/Caches/com.postmanlabs.mac",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Figma Cache",
        "Library/Application Support/Figma/Cache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma Code Cache",
        "Library/Application Support/Figma/Code Cache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma GPU Cache",
        "Library/Application Support/Figma/GPUCache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma App Cache",
        "Library/Caches/com.figma.Desktop",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "node",
        "Yarn Cache",
        "Library/Caches/Yarn",
        "Package Cache",
        Safe,
        &[],
    ),
    g(
        "node",
        "Yarn Berry Cache",
        ".yarn/berry/cache",
        "Package Cache",
        Safe,
        &[],
    ),
    g(
        "node",
        "Bun Cache",
        ".bun/install/cache",
        "Package Cache",
        Safe,
        &[],
    ),
    g("node", "npx Cache", ".npm/_npx", "Package Cache", Safe, &[]),
    g(
        "node",
        "Electron Downloads",
        "Library/Caches/electron",
        "Download Cache",
        Safe,
        &[],
    ),
    g(
        "node",
        "Deno Cache",
        "Library/Caches/deno",
        "Package Cache",
        Safe,
        &["deno"],
    ),
    GlobalRule {
        module: "node",
        label: "Playwright Browsers",
        home_rel: "Library/Caches/ms-playwright",
        category: "Test Browsers",
        safety: Review,
        blocked_if_running: &[],
        split_children: false,
        note: "Browsers downloaded by Playwright for tests. Run npx playwright install again to get them back (several hundred MB each).",
        min_bytes: 1,
    },
    GlobalRule {
        module: "node",
        label: "Cypress Binaries",
        home_rel: "Library/Caches/Cypress",
        category: "Test Browsers",
        safety: Review,
        blocked_if_running: &["Cypress"],
        split_children: false,
        note: "Cypress app versions. The next npm install of a project that uses Cypress downloads its version again.",
        min_bytes: 1,
    },
    GlobalRule {
        module: "node",
        label: "Puppeteer Browsers",
        home_rel: ".cache/puppeteer",
        category: "Test Browsers",
        safety: Review,
        blocked_if_running: &[],
        split_children: false,
        note: "Browsers downloaded by Puppeteer. The next npm install of a project that uses Puppeteer downloads them again.",
        min_bytes: 1,
    },
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
        "rust",
        "Cargo Registry Downloads",
        ".cargo/registry/cache",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "rust",
        "Cargo Registry Sources",
        ".cargo/registry/src",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "rust",
        "Cargo Git Checkouts",
        ".cargo/git/checkouts",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "php",
        "Composer Cache",
        "AppData/Local/Composer",
        "Package Cache",
        Safe,
        &["composer"],
    ),
    g(
        "python",
        "Pip Cache",
        "AppData/Local/pip/Cache",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "poetry"],
    ),
    g(
        "python",
        "uv Cache",
        "AppData/Local/uv/cache",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "uvx", "poetry"],
    ),
    g(
        "python",
        "Poetry Cache",
        "AppData/Local/pypoetry/Cache",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "poetry"],
    ),
    g(
        "go",
        "Go Build Cache",
        "AppData/Local/go-build",
        "Build Cache",
        Safe,
        &["go"],
    ),
    GlobalRule {
        module: "java",
        label: "Maven Repository",
        home_rel: ".m2/repository",
        category: "Package Cache",
        safety: Review,
        blocked_if_running: &["mvn", "java"],
        split_children: false,
        note: "Maven local repository. Dependencies are downloaded again on the next build.",
        min_bytes: 1,
    },
    g(
        "devops",
        "Terraform Plugin Cache",
        ".terraform.d/plugin-cache",
        "Package Cache",
        Safe,
        &["terraform"],
    ),
    g(
        "android",
        "Android AVD Images",
        ".android/avd",
        "Emulator Data",
        Review,
        &[],
    ),
    GlobalRule {
        module: "android",
        label: "Gradle Caches",
        home_rel: ".gradle/caches",
        category: "Gradle",
        safety: Review,
        blocked_if_running: &["studio"],
        split_children: false,
        note: "Dependencies and build caches shared by every Gradle project. Each project downloads what it needs again on its next build. Run ./gradlew --stop first if a build is running.",
        min_bytes: 1,
    },
    g(
        "android",
        "Gradle Daemon Logs",
        ".gradle/daemon",
        "Gradle",
        Safe,
        &[],
    ),
    g(
        "android",
        "Android SDK Cache",
        ".android/cache",
        "Android SDK",
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
        "node",
        "Deno Cache",
        "AppData/Local/deno",
        "Package Cache",
        Safe,
        &["deno"],
    ),
    g(
        "os_system",
        "Windows Temp",
        "AppData/Local/Temp",
        "System Cache",
        Review,
        &[],
    ),
    // Work apps (PLAN 2.18): Electron caches only. Zoom and Teams are macOS-only for now.
    g(
        "work_apps",
        "Slack Cache",
        "AppData/Roaming/Slack/Cache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack Code Cache",
        "AppData/Roaming/Slack/Code Cache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack GPU Cache",
        "AppData/Roaming/Slack/GPUCache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Notion Cache",
        "AppData/Roaming/Notion/Cache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion Code Cache",
        "AppData/Roaming/Notion/Code Cache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion GPU Cache",
        "AppData/Roaming/Notion/GPUCache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Postman Cache",
        "AppData/Roaming/Postman/Cache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman Code Cache",
        "AppData/Roaming/Postman/Code Cache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman GPU Cache",
        "AppData/Roaming/Postman/GPUCache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Figma Cache",
        "AppData/Roaming/Figma/Cache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma Code Cache",
        "AppData/Roaming/Figma/Code Cache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma GPU Cache",
        "AppData/Roaming/Figma/GPUCache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
static GLOBAL_RULES: &[GlobalRule] = &[
    g(
        "rust",
        "Cargo Registry Downloads",
        ".cargo/registry/cache",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "rust",
        "Cargo Registry Sources",
        ".cargo/registry/src",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "rust",
        "Cargo Git Checkouts",
        ".cargo/git/checkouts",
        "Package Cache",
        Safe,
        &["cargo"],
    ),
    g(
        "php",
        "Composer Cache",
        ".cache/composer",
        "Package Cache",
        Safe,
        &["composer"],
    ),
    g(
        "php",
        "Composer Cache (legacy)",
        ".composer/cache",
        "Package Cache",
        Safe,
        &["composer"],
    ),
    g(
        "python",
        "Pip Cache",
        ".cache/pip",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "poetry"],
    ),
    g(
        "python",
        "uv Cache",
        ".cache/uv",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "uvx", "poetry"],
    ),
    g(
        "python",
        "Poetry Cache",
        ".cache/pypoetry",
        "Package Cache",
        Safe,
        &["pip", "pip3", "uv", "poetry"],
    ),
    g(
        "go",
        "Go Build Cache",
        ".cache/go-build",
        "Build Cache",
        Safe,
        &["go"],
    ),
    GlobalRule {
        module: "java",
        label: "Maven Repository",
        home_rel: ".m2/repository",
        category: "Package Cache",
        safety: Review,
        blocked_if_running: &["mvn", "java"],
        split_children: false,
        note: "Maven local repository. Dependencies are downloaded again on the next build.",
        min_bytes: 1,
    },
    g(
        "devops",
        "Terraform Plugin Cache",
        ".terraform.d/plugin-cache",
        "Package Cache",
        Safe,
        &["terraform"],
    ),
    g(
        "android",
        "Android AVD Images",
        ".android/avd",
        "Emulator Data",
        Review,
        &[],
    ),
    GlobalRule {
        module: "android",
        label: "Gradle Caches",
        home_rel: ".gradle/caches",
        category: "Gradle",
        safety: Review,
        blocked_if_running: &["studio"],
        split_children: false,
        note: "Dependencies and build caches shared by every Gradle project. Each project downloads what it needs again on its next build. Run ./gradlew --stop first if a build is running.",
        min_bytes: 1,
    },
    g(
        "android",
        "Gradle Daemon Logs",
        ".gradle/daemon",
        "Gradle",
        Safe,
        &[],
    ),
    g(
        "android",
        "Android SDK Cache",
        ".android/cache",
        "Android SDK",
        Safe,
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
        "PyTorch Cache",
        ".cache/torch",
        "AI Model Cache",
        Review,
        &[],
    ),
    g(
        "node",
        "Yarn Cache",
        ".cache/yarn",
        "Package Cache",
        Safe,
        &[],
    ),
    g(
        "node",
        "Yarn Berry Cache",
        ".yarn/berry/cache",
        "Package Cache",
        Safe,
        &[],
    ),
    g(
        "node",
        "Bun Cache",
        ".bun/install/cache",
        "Package Cache",
        Safe,
        &[],
    ),
    g("node", "npx Cache", ".npm/_npx", "Package Cache", Safe, &[]),
    g(
        "node",
        "Electron Downloads",
        ".cache/electron",
        "Download Cache",
        Safe,
        &[],
    ),
    g(
        "node",
        "Deno Cache",
        ".cache/deno",
        "Package Cache",
        Safe,
        &["deno"],
    ),
    GlobalRule {
        module: "node",
        label: "Playwright Browsers",
        home_rel: ".cache/ms-playwright",
        category: "Test Browsers",
        safety: Review,
        blocked_if_running: &[],
        split_children: false,
        note: "Browsers downloaded by Playwright for tests. Run npx playwright install again to get them back (several hundred MB each).",
        min_bytes: 1,
    },
    GlobalRule {
        module: "node",
        label: "Cypress Binaries",
        home_rel: ".cache/Cypress",
        category: "Test Browsers",
        safety: Review,
        blocked_if_running: &["Cypress"],
        split_children: false,
        note: "Cypress app versions. The next npm install of a project that uses Cypress downloads its version again.",
        min_bytes: 1,
    },
    GlobalRule {
        module: "node",
        label: "Puppeteer Browsers",
        home_rel: ".cache/puppeteer",
        category: "Test Browsers",
        safety: Review,
        blocked_if_running: &[],
        split_children: false,
        note: "Browsers downloaded by Puppeteer. The next npm install of a project that uses Puppeteer downloads them again.",
        min_bytes: 1,
    },
    g(
        "python",
        "Conda Environments",
        "anaconda3/envs",
        "Conda Envs",
        Danger,
        &[],
    ),
    // Work apps (PLAN 2.18): Electron caches only. Zoom and Teams are macOS-only for now.
    g(
        "work_apps",
        "Slack Cache",
        ".config/Slack/Cache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack Code Cache",
        ".config/Slack/Code Cache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Slack GPU Cache",
        ".config/Slack/GPUCache",
        "App Cache",
        Safe,
        &["Slack"],
    ),
    g(
        "work_apps",
        "Notion Cache",
        ".config/Notion/Cache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion Code Cache",
        ".config/Notion/Code Cache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Notion GPU Cache",
        ".config/Notion/GPUCache",
        "App Cache",
        Safe,
        &["Notion"],
    ),
    g(
        "work_apps",
        "Postman Cache",
        ".config/Postman/Cache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman Code Cache",
        ".config/Postman/Code Cache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Postman GPU Cache",
        ".config/Postman/GPUCache",
        "App Cache",
        Safe,
        &["Postman"],
    ),
    g(
        "work_apps",
        "Figma Cache",
        ".config/Figma/Cache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma Code Cache",
        ".config/Figma/Code Cache",
        "App Cache",
        Safe,
        &["Figma"],
    ),
    g(
        "work_apps",
        "Figma GPU Cache",
        ".config/Figma/GPUCache",
        "App Cache",
        Safe,
        &["Figma"],
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
    // Nuxt's generated build output; like `.next`, never holds a project of its own.
    ".nuxt",
    "Library",
    ".Trash",
];

/// Default depth of the project walk (PLAN 3.4). User choices are clamped to
/// `MIN_DEPTH..=MAX_DEPTH`.
const MAX_DEPTH: usize = 8;
const MIN_DEPTH: usize = 2;
const MAX_DEPTH_LIMIT: usize = 12;
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
    /// Scan-screen card id that produced the item.
    module: &'static str,
    action: Action,
    /// Size reported by the tool that owns the item; skips walking `path`.
    known_size: Option<u64>,
    /// One folder of a catch-all listing (each folder in ~/Library/Caches). It gives
    /// way to any specific rule that lists the same folder or something inside it.
    catch_all: bool,
}

/// Scan without progress reporting or cancellation.
///
/// The Tauri command uses `scan_directory_with`; this stays as the plain API
/// (and is what the engine tests compare against).
#[cfg_attr(not(test), allow(dead_code))]
pub fn scan_directory(
    base_path: &Path,
    enabled_modules: &[String],
    ignored_paths: &[String],
    env: &Env,
    running: &HashSet<String>,
) -> ScanResult {
    let cancelled = || false;
    let progress = |_: ScanProgress| {};
    let hooks = ScanHooks {
        cancelled: &cancelled,
        progress: &progress,
    };
    scan_directory_with(
        base_path,
        enabled_modules,
        ignored_paths,
        env,
        running,
        &hooks,
    )
    .unwrap_or_default()
}

/// Scan with progress reporting and cancellation. Returns `None` when cancelled.
pub fn scan_directory_with(
    base_path: &Path,
    enabled_modules: &[String],
    ignored_paths: &[String],
    env: &Env,
    running: &HashSet<String>,
    hooks: &ScanHooks<'_>,
) -> Option<ScanResult> {
    scan_directory_with_options(
        base_path,
        enabled_modules,
        ignored_paths,
        env,
        running,
        hooks,
        ScanOptions::default(),
    )
}

/// Like [`scan_directory_with`], with caller-provided scan options.
pub fn scan_directory_with_options(
    base_path: &Path,
    enabled_modules: &[String],
    ignored_paths: &[String],
    env: &Env,
    running: &HashSet<String>,
    hooks: &ScanHooks<'_>,
    options: ScanOptions,
) -> Option<ScanResult> {
    let max_depth = options
        .max_depth
        .unwrap_or(MAX_DEPTH)
        .clamp(MIN_DEPTH, MAX_DEPTH_LIMIT);
    let enabled: HashSet<&str> = enabled_modules.iter().map(String::as_str).collect();
    let ignored: Vec<PathBuf> = ignored_paths.iter().map(PathBuf::from).collect();
    let is_ignored = |p: &Path| ignored.iter().any(|i| p.starts_with(i));

    let mut candidates = Vec::new();
    (hooks.progress)(ScanProgress::Phase {
        label: "Checking installed tools".to_string(),
    });
    if (hooks.cancelled)() {
        return None;
    }
    collect_global(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    versions::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    editors::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    simulators::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    dart::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    android::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    node::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    rust::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    conda::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    go::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    docker::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    brew::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    ai_models::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }
    system::collect(env, &enabled, &mut candidates);
    if (hooks.cancelled)() {
        return None;
    }

    (hooks.progress)(ScanProgress::Phase {
        label: "Searching your projects".to_string(),
    });
    collect_projects(
        base_path,
        &enabled,
        &is_ignored,
        &mut candidates,
        hooks,
        max_depth,
    );
    if (hooks.cancelled)() {
        return None;
    }

    candidates.retain(|c| !is_ignored(&c.path));
    let candidates = drop_git_tracked(candidates);
    let candidates = drop_shadowed_catch_alls(candidates);
    let candidates = drop_nested(candidates);
    if (hooks.cancelled)() {
        return None;
    }

    // Project activity is identical for every item of a project, so it is
    // computed once per distinct root before the parallel sizing stage.
    let roots: HashSet<&PathBuf> = candidates
        .iter()
        .filter_map(|candidate| candidate.project_root.as_ref())
        .collect();
    let activity: HashMap<PathBuf, Option<i64>> = roots
        .into_par_iter()
        .map(|root| {
            (
                root.clone(),
                age::project_last_active(root, hooks.cancelled),
            )
        })
        .collect();

    (hooks.progress)(ScanProgress::Phase {
        label: "Measuring sizes".to_string(),
    });
    let total = candidates.len();
    let done = AtomicUsize::new(0);
    let bytes = AtomicU64::new(0);
    let last_report = Mutex::new(Instant::now());
    let cancelled = hooks.cancelled;
    let mut items: Vec<ScannedItem> = candidates
        .into_par_iter()
        .filter_map(|mut c| {
            if cancelled() {
                return None;
            }
            let done_now = done.fetch_add(1, Ordering::Relaxed) + 1;
            let size = c
                .known_size
                .unwrap_or_else(|| disk_usage_cancellable(&c.path, cancelled));
            let bytes_now = bytes.fetch_add(size, Ordering::Relaxed) + size;
            if let Ok(mut last) = last_report.lock() {
                if last.elapsed() >= Duration::from_millis(100) {
                    *last = Instant::now();
                    (hooks.progress)(ScanProgress::Measuring {
                        done: done_now,
                        total,
                        bytes: bytes_now,
                    });
                }
            }
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
            let last_active = c
                .project_root
                .as_ref()
                .and_then(|root| activity.get(root))
                .copied()
                .flatten();
            Some(ScannedItem {
                item: CleanupItem {
                    id: stable_id(&path_str),
                    path: path_str,
                    file_type: c.file_type,
                    category: c.category,
                    group: c.group.to_string(),
                    module: c.module.to_string(),
                    size_bytes: size,
                    status: c.safety,
                    description: c.description,
                    blocked_by,
                    last_active,
                },
                canonical,
                blocked_if_running: c.blocked_if_running,
                action: c.action,
            })
        })
        .collect();

    (hooks.progress)(ScanProgress::Measuring {
        done: total,
        total,
        bytes: bytes.load(Ordering::Relaxed),
    });
    if (hooks.cancelled)() {
        return None;
    }

    items.sort_by_key(|i| std::cmp::Reverse(i.item.size_bytes));

    let mut allowed_roots = vec![env.home.clone()];
    if let Ok(root) = fs::canonicalize(base_path) {
        allowed_roots.push(root);
    }
    Some(ScanResult {
        items,
        allowed_roots,
    })
}

/// Filter bucket for a global rule's module.
fn module_group(module: &str) -> &'static str {
    match module {
        "ai_assistants" | "ai" => "AI Tools",
        "editors" => "Editors",
        "system" => "System",
        "os_system" | "social" | "adobe" | "work_apps" => "System & Apps",
        "java" => "Java",
        "devops" => "DevOps",
        _ => "Dev Tools",
    }
}

fn collect_global(env: &Env, enabled: &HashSet<&str>, out: &mut Vec<Candidate>) {
    let rules = GLOBAL_RULES
        .iter()
        .chain(ai_rules::ai_rules())
        .chain(editors::editor_rules());
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
                        module: rule.module,
                        catch_all: rule.module == "os_system",
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
                    module: rule.module,
                    catch_all: false,
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
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    out: &mut Vec<Candidate>,
    hooks: &ScanHooks<'_>,
    max_depth: usize,
) {
    let rules: Vec<&ProjectRule> = PROJECT_RULES
        .iter()
        .filter(|r| enabled.contains(r.module))
        .collect();
    if rules.is_empty() {
        return;
    }

    let found = Mutex::new(Vec::new());
    let last_path = Mutex::new(None::<Instant>);
    walk_projects(
        base,
        0,
        max_depth,
        &rules,
        is_ignored,
        hooks,
        &found,
        &last_path,
    );

    // Tasks finish in any order; sort so the candidates (and therefore the
    // output for equal sizes) do not depend on the thread schedule.
    let mut found = found.into_inner().unwrap_or_else(|e| e.into_inner());
    found.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.module.cmp(b.module))
            .then_with(|| a.file_type.cmp(&b.file_type))
    });
    out.extend(found);
}

/// One directory of the parallel project walk. `depth` is the distance from
/// the scan root, which is depth 0, matching the old `WalkDir::max_depth`.
#[allow(clippy::too_many_arguments)]
fn walk_projects(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    rules: &[&ProjectRule],
    is_ignored: &(dyn Fn(&Path) -> bool + Sync),
    hooks: &ScanHooks<'_>,
    found: &Mutex<Vec<Candidate>>,
    last_path: &Mutex<Option<Instant>>,
) {
    if (hooks.cancelled)() {
        return;
    }

    let report = match last_path.lock() {
        Ok(mut last) => {
            if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(100)) {
                *last = Some(Instant::now());
                true
            } else {
                false
            }
        }
        Err(_) => true,
    };
    if report {
        (hooks.progress)(ScanProgress::Path {
            path: dir.to_string_lossy().into_owned(),
        });
    }

    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    let mut names: HashSet<OsString> = HashSet::new();
    let mut children: Vec<PathBuf> = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        if depth < max_depth
            && entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
            && !SKIP_DIRS.contains(&name.to_string_lossy().as_ref())
            && !is_ignored(&entry.path())
        {
            children.push(entry.path());
        }
        names.insert(name);
    }

    let mut local = Vec::new();
    for rule in rules {
        if !rule.markers.iter().any(|m| marker_matches(dir, &names, m)) {
            continue;
        }
        for target in rule.targets {
            for path in expand(dir, target.rel) {
                if fs::symlink_metadata(&path).is_err() {
                    continue;
                }
                if !target.needs.is_empty() {
                    let ok = is_real_dir(&path)
                        && fs::symlink_metadata(path.join(target.needs))
                            .map(|m| m.is_file())
                            .unwrap_or(false);
                    if !ok {
                        continue;
                    }
                }
                let project = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                local.push(Candidate {
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
                    module: rule.module,
                    catch_all: false,
                });
            }
        }
    }
    if !local.is_empty() {
        found.lock().unwrap_or_else(|e| e.into_inner()).extend(local);
    }

    children.par_iter().for_each(|child| {
        walk_projects(
            child,
            depth + 1,
            max_depth,
            rules,
            is_ignored,
            hooks,
            found,
            last_path,
        );
    });
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

pub(super) fn is_real_dir(p: &Path) -> bool {
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
/// Drop catch-all items that a specific rule lists, or lists something inside.
/// Without this, the whole `~/Library/Caches/Google` folder would hide the old
/// Android Studio versions listed inside it.
fn drop_shadowed_catch_alls(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let specific: Vec<PathBuf> = candidates
        .iter()
        .filter(|c| !c.catch_all)
        .map(|c| c.path.clone())
        .collect();
    candidates
        .into_iter()
        .filter(|c| !c.catch_all || !specific.iter().any(|p| p.starts_with(&c.path)))
        .collect()
}

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
    disk_usage_cancellable(path, &|| false)
}

/// `disk_usage` that stops at a partial sum when the flag turns true.
/// The flag is checked every 256 entries.
fn disk_usage_cancellable(path: &Path, cancelled: &(dyn Fn() -> bool + Sync)) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut total = 0u64;
        // (dev, ino) -> (nlink, links seen, bytes)
        let mut linked: HashMap<(u64, u64), (u64, u64, u64)> = HashMap::new();
        let mut entries = 0usize;
        for entry in WalkDir::new(path).follow_links(false).into_iter().flatten() {
            entries += 1;
            if entries.is_multiple_of(256) && cancelled() {
                break;
            }
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
        let mut total = 0u64;
        let mut entries = 0usize;
        for entry in WalkDir::new(path).follow_links(false).into_iter().flatten() {
            entries += 1;
            if entries.is_multiple_of(256) && cancelled() {
                break;
            }
            let Ok(m) = entry.metadata() else { continue };
            if m.is_file() {
                total += m.len();
            }
        }
        total
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
    v.extend(editors::PROTECTED.iter().map(|rel| home.join(rel)));
    v.extend(ai_models::PROTECTED.iter().map(|rel| home.join(rel)));
    // Compare against canonical forms too (/var -> /private/var on macOS).
    let canon: Vec<PathBuf> = v.iter().filter_map(|p| fs::canonicalize(p).ok()).collect();
    v.extend(canon);
    v
}

/// True when an error means "the directory still has entries". Something wrote
/// into the folder between listing it and removing it.
fn is_directory_not_empty(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::DirectoryNotEmpty || matches!(e.raw_os_error(), Some(66) | Some(39))
}

/// Remove a directory, retrying only while it is not empty. `attempts` is the
/// maximum number of `remover` calls; the last error is returned if they all
/// fail. Any other error (and NotFound) is returned immediately.
fn remove_dir_retrying(
    path: &Path,
    attempts: u32,
    delay: Duration,
    remover: &dyn Fn(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let mut last_err = None;
    for _ in 0..attempts {
        match remover(path) {
            Ok(()) => return Ok(()),
            Err(e) if is_directory_not_empty(&e) => {
                last_err = Some(e);
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("could not remove directory")))
}

/// Move a path to the system Trash with the `trash` crate.
fn os_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Validate and remove one scanned item. Returns true when it was moved to the
/// Trash. Every safety check runs before anything is removed.
pub fn delete_path_with(
    item: &ScannedItem,
    allowed_roots: &[PathBuf],
    protected: &[PathBuf],
    mode: DeleteMode,
    trasher: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<bool, String> {
    let path = Path::new(&item.item.path);
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
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

    // The Trash is used only for a small, real entry that is not the Trash itself.
    // A symlink is always removed as a link, never moved.
    let trash_component = std::ffi::OsStr::new(".Trash");
    let use_trash = mode == DeleteMode::Trash
        && !meta.file_type().is_symlink()
        && item.item.size_bytes <= TRASH_MAX_BYTES
        && item.item.category != "Trash"
        && !canonical
            .components()
            .any(|c| c.as_os_str() == trash_component);

    if use_trash {
        trasher(path)
            .map_err(|e| format!("Could not move {} to the Trash: {e}", path.display()))?;
        if fs::symlink_metadata(path).is_ok() {
            return Err(format!(
                "{} still exists after moving to the Trash.",
                path.display()
            ));
        }
        return Ok(true);
    }

    let result = if meta.is_dir() {
        remove_dir_retrying(path, 3, Duration::from_millis(150), &|p| {
            fs::remove_dir_all(p)
        })
    } else {
        // Plain files and symlinks (the link itself, never its target).
        fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) if is_directory_not_empty(&e) => Err(format!(
            "Could not remove {} because something is still writing into it. Close the apps that use it and try again.",
            path.display()
        )),
        Err(e) => Err(format!("Failed to delete {}: {e}", path.display())),
    }
}

/// Remove one scanned item: delete its path or run its official command.
/// `Removed::bytes` is the amount a command measured, when it reports one;
/// otherwise it is the item's scanned size.
pub fn execute(
    item: &ScannedItem,
    allowed_roots: &[PathBuf],
    protected: &[PathBuf],
    mode: DeleteMode,
) -> Result<Removed, String> {
    execute_with(item, allowed_roots, protected, mode, &os_trash)
}

fn execute_with(
    item: &ScannedItem,
    allowed_roots: &[PathBuf],
    protected: &[PathBuf],
    mode: DeleteMode,
    trasher: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<Removed, String> {
    let bytes = item.item.size_bytes;
    match &item.action {
        Action::DeletePath => {
            let trashed = delete_path_with(item, allowed_roots, protected, mode, trasher)?;
            Ok(Removed { bytes, trashed })
        }
        // Official commands decide on their own how to remove; the mode is ignored.
        Action::Command(cmd) => {
            let freed = native_commands::run(cmd)?;
            Ok(Removed {
                bytes: freed.unwrap_or(bytes),
                trashed: false,
            })
        }
    }
}

/// Bytes freed by a removal that failed partway (PLAN 3.7c).
///
/// A path that is gone counts as fully removed. A directory that still exists is
/// measured again and the difference is reported. A file or symlink that still
/// exists reports 0, and symlinks are never followed. Any error reports 0.
pub fn freed_after_failure(item: &ScannedItem) -> u64 {
    if item.action != Action::DeletePath {
        return 0;
    }
    let path = Path::new(&item.item.path);
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => item.item.size_bytes.saturating_sub(disk_usage(path)),
        Ok(_) => 0,
        Err(e) if e.kind() == io::ErrorKind::NotFound => item.item.size_bytes,
        Err(_) => 0,
    }
}

pub fn bundle_name(exe_path: &str) -> Option<String> {
    let p = Path::new(exe_path);
    let mut last_app = None;
    for comp in p.components() {
        let s = comp.as_os_str().to_string_lossy();
        if let Some(name) = s.strip_suffix(".app") {
            if !name.is_empty() {
                last_app = Some(name.to_lowercase());
            }
        }
    }
    last_app
}

/// Lower-cased names of running processes and macOS app bundles.
pub fn running_process_names() -> HashSet<String> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
    let sys = System::new_with_specifics(
        RefreshKind::new()
            .with_processes(ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet)),
    );
    let mut names = HashSet::new();
    for p in sys.processes().values() {
        names.insert(p.name().to_lowercase());
        if let Some(exe) = p.exe() {
            if let Some(app) = bundle_name(&exe.to_string_lossy()) {
                names.insert(app);
            }
        }
    }
    names
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
    use std::cell::{Cell, RefCell};
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
    fn flutter_plugin_files_found() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join(".flutter-plugins"), 300);
        write(&p.join(".flutter-plugins-dependencies"), 900);
        // Negative: the lock file and sources must stay.
        write(&p.join("pubspec.lock"), 900);
        write(&p.join("lib/main.dart"), 100);
        let r = scan(d.path(), &["flutter"]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 2, "{ps:?}");
        assert!(ps.iter().any(|p| p.ends_with("app/.flutter-plugins")));
        assert!(ps
            .iter()
            .any(|p| p.ends_with("app/.flutter-plugins-dependencies")));
    }

    #[test]
    fn tracked_flutter_plugin_file_is_kept() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join(".flutter-plugins-dependencies"), 900);
        git(&p, &["init", "-q"]);
        git(
            &p,
            &["add", "pubspec.yaml", ".flutter-plugins-dependencies"],
        );
        let r = scan(d.path(), &["flutter"]);
        assert!(r.items.is_empty());
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

    #[test]
    fn test_bundle_name() {
        assert_eq!(
            bundle_name("/Applications/Antigravity IDE.app/Contents/MacOS/Electron"),
            Some("antigravity ide".to_string())
        );
        assert_eq!(
            bundle_name("/Applications/Visual Studio Code.app/Contents/MacOS/Electron"),
            Some("visual studio code".to_string())
        );
        assert_eq!(bundle_name("/usr/bin/python3"), None);
    }

    #[test]
    fn test_blocking_app_bundle_matching() {
        let mut item = scanned(Path::new("/dummy"));
        item.blocked_if_running = vec!["Code", "Visual Studio Code"];

        let running_unrelated: HashSet<String> =
            ["electron".to_string(), "antigravity ide".to_string()].into();
        assert_eq!(blocking_app(&item, &running_unrelated), None);

        let running_vscode: HashSet<String> = ["visual studio code".to_string()].into();
        assert_eq!(
            blocking_app(&item, &running_vscode),
            Some("Visual Studio Code")
        );
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
                module: String::new(),
                size_bytes: 1,
                status: Safe,
                description: String::new(),
                blocked_by: None,
                last_active: None,
            },
            canonical: fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
            blocked_if_running: vec![],
            action: Action::DeletePath,
        }
    }

    fn item_with(path: &Path, category: &str, size_bytes: u64) -> ScannedItem {
        let mut item = scanned(path);
        item.item.category = category.to_string();
        item.item.size_bytes = size_bytes;
        item
    }

    /// Permanent-delete helper for tests: the trasher must never be called.
    fn delete_permanently(
        item: &ScannedItem,
        allowed_roots: &[PathBuf],
        protected: &[PathBuf],
    ) -> Result<(), String> {
        delete_path_with(
            item,
            allowed_roots,
            protected,
            DeleteMode::Permanent,
            &no_trasher,
        )
        .map(|_| ())
    }

    fn no_trasher(_: &Path) -> Result<(), String> {
        panic!("the trasher must not be called for a permanent delete")
    }

    #[test]
    fn delete_refuses_protected_and_outside_paths() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        let protected = protected_paths(&home);
        let roots = vec![home.clone(), root.join("code")];

        assert!(delete_permanently(&scanned(&home), &roots, &protected).is_err());
        assert!(delete_permanently(&scanned(&home.join("Documents")), &roots, &protected).is_err());
        assert!(delete_permanently(&scanned(&root), &roots, &protected).is_err());

        let outside = root.join("elsewhere/build");
        fs::create_dir_all(&outside).unwrap();
        assert!(delete_permanently(&scanned(&outside), &roots, &protected).is_err());
        assert!(outside.exists());

        let ok = root.join("code/app/build");
        write(&ok.join("x"), 10);
        assert!(delete_permanently(&scanned(&ok), &roots, &protected).is_ok());
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
        delete_permanently(&scanned(&link), &roots, &protected).unwrap();
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
        let err = delete_permanently(
            &item,
            &[root.join("code")],
            &protected_paths(&root.join("home")),
        );
        assert!(err.is_err());
        assert!(p.exists());
    }

    #[test]
    fn negative_7_missing_path_delete_is_ok() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let p = root.join("code/app/build");
        write(&p.join("x"), 10);
        let item = scanned(&p);
        fs::remove_dir_all(&p).unwrap();

        let result = delete_permanently(
            &item,
            &[root.join("code")],
            &protected_paths(&root.join("home")),
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(!p.exists());
    }

    // ----- Trash or permanent (PLAN 3.7) --------------------------------------

    #[test]
    fn permanent_mode_never_calls_the_trasher() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let file = root.join("code/app/build/out.bin");
        write(&file, 10);
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let calls = Cell::new(0);
        let trasher = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let trashed = delete_path_with(
            &scanned(&file),
            &roots,
            &protected,
            DeleteMode::Permanent,
            &trasher,
        )
        .unwrap();
        assert!(!trashed);
        assert_eq!(calls.get(), 0);
        assert!(!file.exists());
    }

    #[test]
    fn trash_mode_moves_a_small_item_into_the_fake_trash() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let fake = root.join("fake-trash");
        fs::create_dir_all(&fake).unwrap();
        let file = root.join("code/app/build/out.bin");
        write(&file, 10);
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let seen = RefCell::new(Vec::new());
        let dest = fake.join("out.bin");
        let trasher = |p: &Path| -> Result<(), String> {
            seen.borrow_mut().push(p.to_path_buf());
            fs::rename(p, &dest).map_err(|e| e.to_string())
        };
        let trashed = delete_path_with(
            &scanned(&file),
            &roots,
            &protected,
            DeleteMode::Trash,
            &trasher,
        )
        .unwrap();
        assert!(trashed);
        assert_eq!(*seen.borrow(), vec![file.clone()]);
        assert!(!file.exists());
        assert!(dest.exists());
    }

    #[test]
    fn trash_mode_deletes_items_above_the_size_limit() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let file = root.join("code/app/build/big.bin");
        write(&file, 10);
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let calls = Cell::new(0);
        let trasher = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let item = item_with(&file, "", TRASH_MAX_BYTES + 1);
        let trashed =
            delete_path_with(&item, &roots, &protected, DeleteMode::Trash, &trasher).unwrap();
        assert!(!trashed);
        assert_eq!(calls.get(), 0);
        assert!(!file.exists());
    }

    #[test]
    fn trash_mode_never_trashes_the_trash_category() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let file = root.join("code/app/build/out.bin");
        write(&file, 10);
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let calls = Cell::new(0);
        let trasher = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let item = item_with(&file, "Trash", 10);
        let trashed =
            delete_path_with(&item, &roots, &protected, DeleteMode::Trash, &trasher).unwrap();
        assert!(!trashed);
        assert_eq!(calls.get(), 0);
        assert!(!file.exists());
    }

    #[test]
    fn trash_mode_never_trashes_a_dot_trash_path() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let file = root.join(".Trash/old.bin");
        write(&file, 10);
        let roots = vec![root.clone()];
        let protected = protected_paths(&root.join("home"));

        let calls = Cell::new(0);
        let trasher = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let item = item_with(&file, "", 10);
        let trashed =
            delete_path_with(&item, &roots, &protected, DeleteMode::Trash, &trasher).unwrap();
        assert!(!trashed);
        assert_eq!(calls.get(), 0);
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn trash_mode_removes_symlinks_as_links() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let precious = root.join("precious");
        write(&precious.join("data.txt"), 10);
        let link = root.join("code/app/build");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&precious, &link).unwrap();
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let calls = Cell::new(0);
        let trasher = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let trashed = delete_path_with(
            &scanned(&link),
            &roots,
            &protected,
            DeleteMode::Trash,
            &trasher,
        )
        .unwrap();
        assert!(!trashed);
        assert_eq!(calls.get(), 0);
        assert!(fs::symlink_metadata(&link).is_err());
        assert!(precious.join("data.txt").exists());
    }

    #[test]
    fn a_failing_trasher_leaves_the_file_alone() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let file = root.join("code/app/build/out.bin");
        write(&file, 10);
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let trasher = |_: &Path| -> Result<(), String> { Err("boom".into()) };
        let err = delete_path_with(
            &scanned(&file),
            &roots,
            &protected,
            DeleteMode::Trash,
            &trasher,
        )
        .unwrap_err();
        assert!(err.contains("Could not move"), "unexpected error: {err}");
        assert!(file.exists());
    }

    #[test]
    fn a_trasher_that_moves_nothing_is_an_error() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let file = root.join("code/app/build/out.bin");
        write(&file, 10);
        let roots = vec![root.join("code")];
        let protected = protected_paths(&root.join("home"));

        let trasher = |_: &Path| -> Result<(), String> { Ok(()) };
        let err = delete_path_with(
            &scanned(&file),
            &roots,
            &protected,
            DeleteMode::Trash,
            &trasher,
        );
        assert!(err.is_err());
        assert!(file.exists());
    }

    #[test]
    fn trash_mode_still_refuses_protected_and_outside_paths() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let protected = protected_paths(&home);
        let roots = vec![home.clone(), root.join("code")];
        let outside = root.join("elsewhere/build");
        fs::create_dir_all(&outside).unwrap();

        let calls = Cell::new(0);
        let trasher = |_: &Path| -> Result<(), String> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        assert!(delete_path_with(
            &scanned(&home),
            &roots,
            &protected,
            DeleteMode::Trash,
            &trasher
        )
        .is_err());
        assert!(delete_path_with(
            &scanned(&outside),
            &roots,
            &protected,
            DeleteMode::Trash,
            &trasher
        )
        .is_err());
        assert_eq!(calls.get(), 0);
        assert!(home.exists());
        assert!(outside.exists());
    }

    #[test]
    fn delete_mode_parse_accepts_only_the_exact_names() {
        assert_eq!(DeleteMode::parse("trash"), Some(DeleteMode::Trash));
        assert_eq!(DeleteMode::parse("permanent"), Some(DeleteMode::Permanent));
        assert_eq!(DeleteMode::parse("TRASH"), None);
        assert_eq!(DeleteMode::parse(""), None);
        assert_eq!(DeleteMode::parse("x"), None);
    }

    // ----- Retrying a directory that is still being written to ---------------

    #[test]
    fn remove_dir_retrying_retries_until_it_succeeds() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("uv-cache");
        fs::create_dir_all(&dir).unwrap();

        let calls = Cell::new(0);
        let remover = |_: &Path| -> io::Result<()> {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(io::Error::from(io::ErrorKind::DirectoryNotEmpty))
            } else {
                Ok(())
            }
        };
        assert!(remove_dir_retrying(&dir, 3, Duration::ZERO, &remover).is_ok());
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn remove_dir_retrying_stops_after_the_last_attempt() {
        let calls = Cell::new(0);
        let remover = |_: &Path| -> io::Result<()> {
            calls.set(calls.get() + 1);
            Err(io::Error::from(io::ErrorKind::DirectoryNotEmpty))
        };
        let err =
            remove_dir_retrying(Path::new("/nowhere"), 3, Duration::ZERO, &remover).unwrap_err();
        assert!(is_directory_not_empty(&err));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn remove_dir_retrying_does_not_retry_other_errors() {
        let calls = Cell::new(0);
        let remover = |_: &Path| -> io::Result<()> {
            calls.set(calls.get() + 1);
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        };
        let err =
            remove_dir_retrying(Path::new("/nowhere"), 3, Duration::ZERO, &remover).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn remove_dir_retrying_returns_not_found_immediately() {
        let calls = Cell::new(0);
        let remover = |_: &Path| -> io::Result<()> {
            calls.set(calls.get() + 1);
            Err(io::Error::from(io::ErrorKind::NotFound))
        };
        let err =
            remove_dir_retrying(Path::new("/nowhere"), 3, Duration::ZERO, &remover).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(calls.get(), 1);
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
                delete_permanently(&scanned(&home.join(rel)), &roots, &protected).is_err(),
                "{rel} was not protected"
            );
        }
        assert!(home.join(".codex/auth.json").exists());
    }

    // ----- Trae updates and Jan models (PLAN 2.19) -----------------------------

    #[test]
    fn trae_update_caches_are_safe() {
        let (_d, r) = ai_scan(&[
            ("Library/Caches/Trae/pending/blob", 5000),
            ("Library/Caches/Trae/update.zip", 5000),
            ("Library/Caches/com.trae.app/blob", 5000),
            ("Library/Caches/com.trae.app.ShipIt/blob", 5000),
            ("Library/Caches/Trae/other-folder/blob", 5000),
            ("Library/Application Support/Trae/Cookies/data", 5000),
            (".trae/trae-jwt-token", 5000),
        ]);

        for (suffix, label) in [
            ("Library/Caches/Trae/pending", "Trae pending update"),
            ("Library/Caches/Trae/update.zip", "Trae update archive"),
            ("Library/Caches/com.trae.app", "Trae App Cache"),
            ("Library/Caches/com.trae.app.ShipIt", "Trae updater"),
        ] {
            let item = find(&r, suffix).unwrap_or_else(|| panic!("missing {suffix}"));
            assert_eq!(item.item.status, Safe, "{suffix}");
            assert_eq!(item.item.file_type, label, "{suffix}");
            assert_eq!(item.blocked_if_running, vec!["Trae"], "{suffix}");
            assert!(
                item.item.description.to_lowercase().contains("download"),
                "{suffix}: {}",
                item.item.description
            );
        }

        // Negative: the rest of the Trae cache, cookies and the token are never listed.
        assert!(find(&r, "Library/Caches/Trae/other-folder").is_none());
        assert!(find(&r, "Application Support/Trae/Cookies").is_none());
        assert!(find(&r, ".trae/trae-jwt-token").is_none());
        assert_eq!(r.items.len(), 4, "{:?}", paths(&r));
    }

    #[test]
    fn trae_caches_shadow_the_app_cache_catch_all() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Caches/Trae/pending/blob"),
            11 * 1024 * 1024,
        );
        write(&home.join("Library/Caches/Trae/update.zip"), 10);
        write(
            &home.join("Library/Caches/Unrelated/blob"),
            11 * 1024 * 1024,
        );

        let r = scan(d.path(), &["ai_assistants", "os_system"]);
        // The catch-all item for the whole Trae folder gives way to the specific rows.
        assert!(find(&r, "Library/Caches/Trae").is_none(), "{:?}", paths(&r));
        assert!(find(&r, "Library/Caches/Trae/pending").is_some());
        assert!(find(&r, "Library/Caches/Trae/update.zip").is_some());
        // The catch-all still lists other apps.
        let unrelated = find(&r, "Library/Caches/Unrelated").unwrap();
        assert_eq!(unrelated.item.file_type, "Unrelated");
        assert_eq!(unrelated.item.category, "System Cache");
    }

    #[test]
    fn jan_models_are_review_and_machinery_is_skipped() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let data = home.join("Library/Application Support/Jan/data");
        write(&data.join("llamacpp/models/llama3/model.yml"), 5000);
        write(&data.join("llamacpp/models/org/repo/model.gguf"), 5000);
        write(&data.join("llamacpp/models/local-only/weights.gguf"), 5000);
        write(&data.join("mlx/models/mlx-community/Qwen/model.yml"), 5000);
        for rel in [
            "llamacpp/backends/backend.bin",
            "llamacpp/settings.json",
            "llamacpp/router.preset.ini",
            "threads/t1.json",
            "assistants/a.json",
            "extensions/e.js",
            "files/f.bin",
            "logs/l.log",
            "mcp_config.json",
            "store.json",
            "db/db.sqlite",
            "agent-workspace/w.txt",
        ] {
            write(&data.join(rel), 5000);
        }
        // Negative: ~/.jan is never scanned.
        write(&home.join(".jan/models/secret/model.yml"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 4, "{:?}", paths(&r));
        for (suffix, name) in [
            ("Jan/data/llamacpp/models/llama3", "llama3"),
            ("Jan/data/llamacpp/models/org/repo", "org/repo"),
            ("Jan/data/llamacpp/models/local-only", "local-only"),
            ("Jan/data/mlx/models/mlx-community/Qwen", "mlx-community/Qwen"),
        ] {
            let item = find(&r, suffix).unwrap_or_else(|| panic!("missing {suffix}"));
            assert_eq!(item.item.file_type, "Jan Models", "{suffix}");
            assert_eq!(item.item.status, Review, "{suffix}");
            assert_eq!(item.blocked_if_running, vec!["Jan"], "{suffix}");
            assert!(
                item.item.description.starts_with(&format!("{name}: ")),
                "{suffix}: {}",
                item.item.description
            );
        }
        for suffix in [
            "Jan/data/llamacpp/backends",
            "Jan/data/llamacpp/models/org",
            "Jan/data/threads",
            "Jan/data/assistants",
            "Jan/data/store.json",
            "Jan/data/llamacpp/settings.json",
            "Jan/data/llamacpp/router.preset.ini",
            ".jan",
        ] {
            assert!(find(&r, suffix).is_none(), "{suffix} was listed");
        }
    }

    #[test]
    fn jan_missing_folder_gives_no_items() {
        let d = TempDir::new().unwrap();
        let r = scan(d.path(), &["ai"]);
        assert!(r.items.is_empty(), "{:?}", paths(&r));
    }

    #[test]
    fn jan_unfinished_download_is_review() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let data = home.join("Library/Application Support/Jan/data");
        write(&data.join("llamacpp/models/llama3/model.gguf.tmp"), 5000);
        write(&data.join("llamacpp/models/llama3/model.gguf.url"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        let item = find(&r, "Jan/data/llamacpp/models/llama3").unwrap();
        assert_eq!(item.item.file_type, "Jan Unfinished Downloads");
        assert_eq!(item.item.status, Review);
        assert_eq!(item.blocked_if_running, vec!["Jan"]);
        assert!(
            item.item.description.starts_with("llama3: "),
            "{}",
            item.item.description
        );
        assert!(item.item.description.contains("did not finish"));
    }

    #[test]
    fn jan_finished_model_with_partial_file_is_listed_once() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let data = home.join("Library/Application Support/Jan/data");
        write(&data.join("llamacpp/models/llama3/model.yml"), 5000);
        write(&data.join("llamacpp/models/llama3/model.gguf.tmp"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        let item = find(&r, "Jan/data/llamacpp/models/llama3").unwrap();
        assert_eq!(item.item.file_type, "Jan Models");
        assert!(item.item.description.starts_with("llama3: "));
    }

    #[test]
    fn jan_url_only_folder_is_not_listed() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let data = home.join("Library/Application Support/Jan/data");
        write(&data.join("llamacpp/models/llama3/model.gguf.url"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert!(r.items.is_empty(), "{:?}", paths(&r));
    }

    #[test]
    fn jan_unfinished_download_at_depth_two_uses_relative_name() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let data = home.join("Library/Application Support/Jan/data");
        write(&data.join("llamacpp/models/org/repo/model.gguf.tmp"), 5000);
        write(&data.join("llamacpp/models/org/repo/mmproj.gguf.tmp"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        let item = find(&r, "Jan/data/llamacpp/models/org/repo").unwrap();
        assert_eq!(item.item.file_type, "Jan Unfinished Downloads");
        assert_eq!(item.item.status, Review);
        assert!(
            item.item.description.starts_with("org/repo: "),
            "{}",
            item.item.description
        );
        assert!(find(&r, "Jan/data/llamacpp/models/org").is_none());
    }

    #[test]
    fn protected_jan_trae_and_unsloth_paths_are_refused() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        for rel in [
            ".trae/trae-jwt-token",
            ".unsloth/studio/studio.db",
            "Library/Application Support/Jan/data/threads/t1.json",
        ] {
            write(&home.join(rel), 10);
        }
        let protected = protected_paths(&home);
        let roots = vec![home.clone()];

        for rel in [
            ".trae/trae-jwt-token",
            ".unsloth/studio/studio.db",
            "Library/Application Support/Jan/data/threads",
        ] {
            assert!(
                delete_permanently(&scanned(&home.join(rel)), &roots, &protected).is_err(),
                "{rel} was not protected"
            );
        }
        assert!(home.join(".trae/trae-jwt-token").exists());
        assert!(home.join(".unsloth/studio/studio.db").exists());
        assert!(home
            .join("Library/Application Support/Jan/data/threads/t1.json")
            .exists());
    }

    // ----- Tool filter on the Review screen (PLAN 4.10) -----------------------

    #[test]
    fn every_item_carries_the_enabled_module() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let project = d.path().join("app");
        write(&project.join("pubspec.yaml"), 10);
        write(&project.join("build/out.bin"), 5000);
        write(
            &home.join("Library/Developer/Xcode/DerivedData/App-abc/out.o"),
            5000,
        );
        write(&home.join("Library/Caches/Trae/pending/blob"), 5000);
        write(
            &home.join("Library/Application Support/Jan/data/llamacpp/models/llama3/model.yml"),
            5000,
        );
        write(&home.join("Library/Logs/MyApp/app.log"), 5000);
        write(&home.join(".npm/_cacache/blob"), 5000);

        let modules = ["flutter", "node", "xcode", "ai_assistants", "ai", "system"];
        let r = scan(d.path(), &modules);
        assert!(!r.items.is_empty());
        for item in &r.items {
            assert!(!item.item.module.is_empty(), "{}", item.item.path);
            assert!(
                modules.contains(&item.item.module.as_str()),
                "{} -> {}",
                item.item.path,
                item.item.module
            );
        }
        let distinct: HashSet<&str> = r.items.iter().map(|i| i.item.module.as_str()).collect();
        assert!(distinct.len() >= 5, "{distinct:?}");
    }

    #[test]
    fn trae_and_jan_items_name_their_modules() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/Trae/pending/blob"), 5000);
        write(
            &home.join("Library/Application Support/Jan/data/llamacpp/models/llama3/model.yml"),
            5000,
        );

        let r = scan(d.path(), &["ai_assistants", "ai"]);
        assert_eq!(
            find(&r, "Library/Caches/Trae/pending").unwrap().item.module,
            "ai_assistants"
        );
        assert_eq!(
            find(&r, "Jan/data/llamacpp/models/llama3")
                .unwrap()
                .item
                .module,
            "ai"
        );
    }

    #[test]
    fn one_enabled_module_gives_only_its_module() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/Trae/pending/blob"), 5000);
        write(
            &home.join("Library/Application Support/Jan/data/llamacpp/models/llama3/model.yml"),
            5000,
        );
        write(&home.join("Library/Logs/MyApp/app.log"), 5000);

        let r = scan(d.path(), &["ai_assistants"]);
        assert!(!r.items.is_empty());
        assert!(r.items.iter().all(|i| i.item.module == "ai_assistants"));
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
    fn old_android_studio_wins_over_app_cache_folder() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let big = 11 * 1024 * 1024;
        write(
            &home.join("Library/Caches/Google/AndroidStudio2023.1/idx"),
            big,
        );
        write(
            &home.join("Library/Caches/Google/AndroidStudio2024.2/idx"),
            5000,
        );

        // App caches alone: the whole Google folder is one item.
        let r = scan(d.path(), &["os_system"]);
        assert!(find(&r, "Library/Caches/Google").is_some());

        // With Android on, the old version is listed and the Google folder gives way.
        let r = scan(d.path(), &["os_system", "android"]);
        assert!(find(&r, "Google/AndroidStudio2023.1").is_some());
        assert!(find(&r, "Library/Caches/Google").is_none());
        // Negative: the newest version is never listed.
        assert!(find(&r, "Google/AndroidStudio2024.2").is_none());
    }

    #[test]
    fn cargo_caches_are_safe_and_config_stays() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join(".cargo/registry/cache/index.crates.io-1/serde-1.0.crate"),
            5000,
        );
        write(
            &home.join(".cargo/registry/src/index.crates.io-1/serde-1.0/lib.rs"),
            5000,
        );
        write(
            &home.join(".cargo/git/checkouts/repo-abc/1234/lib.rs"),
            5000,
        );
        // Negative: installed binaries, settings and the registry index stay.
        write(&home.join(".cargo/bin/cargo"), 5000);
        write(&home.join(".cargo/config.toml"), 100);
        write(&home.join(".cargo/registry/index/x"), 5000);
        let r = scan(d.path(), &["rust"]);
        assert_eq!(r.items.len(), 3, "{:?}", paths(&r));
        assert!(r.items.iter().all(|i| i.item.status == Safe));
    }

    #[test]
    fn rust_incremental_keeps_compiled_dependencies() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("Cargo.toml"), 10);
        write(&p.join("target/debug/incremental/app-1/s"), 5000);
        write(&p.join("target/release/incremental/app-2/s"), 5000);
        write(&p.join("target/debug/deps/libserde.rlib"), 5000);
        let r = scan(d.path(), &["rust_incremental"]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 2, "{ps:?}");
        assert!(ps.iter().all(|p| p.ends_with("incremental")));
        assert!(find(&r, "deps").is_none());

        // With the full Rust module on, the whole target folder covers both.
        let r = scan(d.path(), &["rust", "rust_incremental"]);
        assert_eq!(paths(&r).len(), 1);
        assert!(paths(&r)[0].ends_with("app/target"));
    }

    #[test]
    fn gradle_caches_are_review_and_daemon_logs_safe() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".gradle/caches/modules-2/files-2.1/x.jar"), 5000);
        write(&home.join(".gradle/daemon/8.10/daemon-1.out.log"), 5000);
        // Negative: Gradle settings must stay.
        write(&home.join(".gradle/gradle.properties"), 100);
        let r = scan(d.path(), &["android"]);
        assert_eq!(find(&r, ".gradle/caches").unwrap().item.status, Review);
        assert_eq!(find(&r, ".gradle/daemon").unwrap().item.status, Safe);
        assert!(find(&r, "gradle.properties").is_none());
        assert!(find(&r, "__home/.gradle").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn node_global_caches_are_listed_separately() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let big = 11 * 1024 * 1024;
        write(&home.join(".npm/_cacache/index-v5/a"), 5000);
        write(&home.join(".npm/_npx/1234/package.json"), 5000);
        write(&home.join("Library/Caches/Yarn/v6/pkg"), big);
        write(&home.join(".bun/install/cache/pkg"), 5000);
        write(
            &home.join("Library/Caches/ms-playwright/chromium-1194/chrome"),
            5000,
        );
        // Negative: npm settings and logs stay.
        write(&home.join(".npmrc"), 100);
        write(&home.join(".npm/_logs/debug.log"), 100);

        let r = scan(d.path(), &["node", "os_system"]);
        for (suffix, level) in [
            (".npm/_cacache", Safe),
            (".npm/_npx", Safe),
            ("Library/Caches/Yarn", Safe),
            (".bun/install/cache", Safe),
            ("Library/Caches/ms-playwright", Review),
        ] {
            assert_eq!(find(&r, suffix).unwrap().item.status, level, "{suffix}");
        }
        assert_eq!(
            find(&r, "Library/Caches/Yarn").unwrap().item.file_type,
            "Yarn Cache"
        );
        assert!(find(&r, ".npmrc").is_none());
        assert!(find(&r, "_logs").is_none());
        assert!(find(&r, "__home/.npm").is_none());
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
        assert!(r
            .items
            .iter()
            .all(|i| !i.item.path.to_lowercase().contains("cocoapods")));
    }

    #[test]
    fn laravel_logs_are_found_but_uploads_stay() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("composer.json"), 10);
        write(&p.join("storage/logs/laravel.log"), 5000);
        write(&p.join("storage/logs/laravel-2026-10-01.log"), 5000);
        // Negatives: gitignore in logs, uploads in public, and project .env must stay.
        write(&p.join("storage/logs/.gitignore"), 10);
        write(&p.join("storage/app/public/photo.png"), 5000);
        write(&p.join(".env"), 100);

        let r = scan(d.path(), &["php"]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 2, "{ps:?}");
        assert!(find(&r, "storage/logs/laravel.log").is_some());
        assert!(find(&r, "storage/logs/laravel-2026-10-01.log").is_some());
        assert!(r.items.iter().all(|i| i.item.status == Safe));
        assert!(find(&r, ".gitignore").is_none());
        assert!(find(&r, "photo.png").is_none());
        assert!(find(&r, ".env").is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn composer_cache_is_safe_and_auth_stays() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".composer/cache/files/a"), 5000);
        // Negatives: auth tokens, config and global vendor binaries must stay.
        write(&home.join(".composer/auth.json"), 100);
        write(&home.join(".composer/config.json"), 100);
        write(&home.join(".composer/vendor/bin/tool"), 5000);

        let r = scan(d.path(), &["php"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        let item = &r.items[0].item;
        assert_eq!(item.status, Safe);
        assert!(item.path.ends_with(".composer/cache"));
        assert!(find(&r, "auth.json").is_none());
        assert!(find(&r, "config.json").is_none());
        assert!(find(&r, "tool").is_none());
        assert!(find(&r, "__home/.composer").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn composer_cache_os_folder() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/composer/files/a"), 5000);
        let r = scan(d.path(), &["php"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        assert_eq!(
            find(&r, "Library/Caches/composer").unwrap().item.status,
            Safe
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn composer_cache_os_folder() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".cache/composer/files/a"), 5000);
        let r = scan(d.path(), &["php"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        assert_eq!(find(&r, ".cache/composer").unwrap().item.status, Safe);
    }

    #[test]
    fn python_project_caches_are_safe_and_code_stays() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pyproject.toml"), 10);
        write(&p.join("main.py"), 100);
        write(&p.join("tests/test_main.py"), 100);
        write(&p.join("__pycache__/a.pyc"), 5000);
        write(&p.join("tests/__pycache__/b.pyc"), 5000);
        write(&p.join("src/pkg/__pycache__/c.pyc"), 5000);
        write(&p.join(".pytest_cache/v"), 5000);
        write(&p.join(".mypy_cache/m"), 5000);
        write(&p.join(".ruff_cache/r"), 5000);
        write(&p.join(".tox/py/x"), 5000);

        let r = scan(d.path(), &["python"]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 7, "{ps:?}");
        assert!(find(&r, "app/__pycache__").is_some());
        assert!(find(&r, "tests/__pycache__").is_some());
        assert!(find(&r, "src/pkg/__pycache__").is_some());
        assert!(find(&r, ".pytest_cache").is_some());
        assert!(find(&r, ".mypy_cache").is_some());
        assert!(find(&r, ".ruff_cache").is_some());
        assert!(find(&r, ".tox").is_some());
        assert!(r.items.iter().all(|i| i.item.status == Safe));
        assert!(find(&r, "main.py").is_none());
        assert!(find(&r, "test_main.py").is_none());
        assert!(find(&r, "pyproject.toml").is_none());
    }

    #[test]
    fn python_venv_is_not_touched_here() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("requirements.txt"), 10);
        write(&p.join(".venv/pyvenv.cfg"), 10);
        write(
            &p.join(".venv/lib/python3.12/site-packages/x/__pycache__/d.pyc"),
            5000,
        );
        write(&p.join("venv/pyvenv.cfg"), 10);

        let r = scan(d.path(), &["python"]);
        for item in &r.items {
            assert!(
                !item.item.path.contains(".venv") && !item.item.path.contains("/venv"),
                "found venv path: {}",
                item.item.path
            );
        }
    }

    #[test]
    fn python_nested_projects_have_no_duplicates() {
        let d = TempDir::new().unwrap();
        let mono = d.path().join("mono");
        write(&mono.join("pyproject.toml"), 10);
        let pkg = mono.join("pkg");
        write(&pkg.join("pyproject.toml"), 10);
        write(&pkg.join("__pycache__/a.pyc"), 5000);

        let r = scan(d.path(), &["python"]);
        let ps = paths(&r);
        let mut deduped = ps.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ps.len(), deduped.len(), "duplicates found: {ps:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn python_global_caches_safe_and_settings_stay() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/pip/http/a"), 5000);
        write(&home.join(".cache/uv/wheels/b"), 5000);
        write(&home.join("Library/Caches/pypoetry/c"), 5000);
        // Negatives: settings and credentials must stay.
        write(&home.join(".config/pypoetry/config.toml"), 100);
        write(&home.join(".pypirc"), 100);
        write(&home.join(".config/pip/pip.conf"), 100);

        let r = scan(d.path(), &["python"]);
        assert_eq!(r.items.len(), 3, "{:?}", paths(&r));
        assert!(find(&r, "Library/Caches/pip").is_some());
        assert!(find(&r, ".cache/uv").is_some());
        assert!(find(&r, "Library/Caches/pypoetry").is_some());
        assert!(r.items.iter().all(|i| i.item.status == Safe));
        assert!(find(&r, "config.toml").is_none());
        assert!(find(&r, ".pypirc").is_none());
        assert!(find(&r, "pip.conf").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn python_global_caches_safe_and_settings_stay() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".cache/pip/http/a"), 5000);
        write(&home.join(".cache/uv/wheels/b"), 5000);
        write(&home.join(".cache/pypoetry/c"), 5000);
        // Negatives: settings and credentials must stay.
        write(&home.join(".config/pypoetry/config.toml"), 100);
        write(&home.join(".pypirc"), 100);
        write(&home.join(".config/pip/pip.conf"), 100);

        let r = scan(d.path(), &["python"]);
        assert_eq!(r.items.len(), 3, "{:?}", paths(&r));
        assert!(find(&r, ".cache/pip").is_some());
        assert!(find(&r, ".cache/uv").is_some());
        assert!(find(&r, ".cache/pypoetry").is_some());
        assert!(r.items.iter().all(|i| i.item.status == Safe));
        assert!(find(&r, "config.toml").is_none());
        assert!(find(&r, ".pypirc").is_none());
        assert!(find(&r, "pip.conf").is_none());
    }

    #[test]
    fn python_venv_with_pyvenv_cfg_is_review() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("requirements.txt"), 10);
        write(&p.join(".venv/pyvenv.cfg"), 10);
        write(&p.join(".venv/bin/python"), 5000);
        write(&p.join("venv/pyvenv.cfg"), 10);
        write(&p.join("venv/lib/x"), 5000);

        let r = scan(d.path(), &["python_venv"]);
        let ps = paths(&r);
        assert_eq!(ps.len(), 2, "{ps:?}");
        assert!(find(&r, "app/.venv").is_some());
        assert!(find(&r, "app/venv").is_some());
        assert!(r.items.iter().all(|i| i.item.status == Review));
    }

    #[test]
    fn folders_named_venv_without_pyvenv_cfg_stay() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("requirements.txt"), 10);
        write(&p.join("venv/data.txt"), 5000);
        write(&p.join(".venv/notes.txt"), 5000);

        let r = scan(d.path(), &["python_venv"]);
        assert!(r.items.is_empty(), "unexpected items: {:?}", paths(&r));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_venv_is_never_listed() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("requirements.txt"), 10);
        let real_env = d.path().join("real_env");
        write(&real_env.join("pyvenv.cfg"), 10);
        write(&real_env.join("bin/python"), 5000);

        std::os::unix::fs::symlink(&real_env, p.join(".venv")).unwrap();

        let r = scan(d.path(), &["python_venv"]);
        assert!(r.items.is_empty(), "unexpected items: {:?}", paths(&r));
        assert!(real_env.exists());
    }

    #[test]
    fn python_modules_stay_separate() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pyproject.toml"), 10);
        write(&p.join("__pycache__/a.pyc"), 5000);
        write(&p.join(".venv/pyvenv.cfg"), 10);
        write(&p.join(".venv/bin/python"), 5000);

        let r_py = scan(d.path(), &["python"]);
        let ps_py = paths(&r_py);
        assert_eq!(ps_py.len(), 1, "{ps_py:?}");
        assert!(ps_py[0].ends_with("__pycache__"));

        let r_venv = scan(d.path(), &["python_venv"]);
        let ps_venv = paths(&r_venv);
        assert_eq!(ps_venv.len(), 1, "{ps_venv:?}");
        assert!(ps_venv[0].ends_with(".venv"));

        let r_both = scan(d.path(), &["python", "python_venv"]);
        assert_eq!(paths(&r_both).len(), 2);
    }

    #[test]
    fn folder_not_at_project_root() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("requirements.txt"), 10);
        write(&p.join("sub/venv/pyvenv.cfg"), 10);
        write(&p.join("sub/venv/bin/python"), 5000);

        let r = scan(d.path(), &["python_venv"]);
        assert!(r.items.is_empty(), "unexpected items: {:?}", paths(&r));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn go_build_cache_is_safe_and_projects_untouched() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/go-build/ab/x"), 5000);
        write(&home.join("go/bin/tool"), 5000);
        write(&home.join("go/src/app/main.go"), 5000);

        let svc = d.path().join("svc");
        write(&svc.join("go.mod"), 10);
        write(&svc.join("pkg/api/handler.go"), 5000);
        write(&svc.join("bin/tool"), 5000);

        let r = scan(d.path(), &["go"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        let item = &r.items[0].item;
        assert!(item.path.ends_with("Library/Caches/go-build"));
        assert_eq!(item.status, Safe);
        assert!(find(&r, "go/bin").is_none());
        assert!(find(&r, "go/src").is_none());
        assert!(find(&r, "svc").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn go_build_cache_is_safe_and_projects_untouched() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".cache/go-build/ab/x"), 5000);
        write(&home.join("go/bin/tool"), 5000);
        write(&home.join("go/src/app/main.go"), 5000);

        let svc = d.path().join("svc");
        write(&svc.join("go.mod"), 10);
        write(&svc.join("pkg/api/handler.go"), 5000);
        write(&svc.join("bin/tool"), 5000);

        let r = scan(d.path(), &["go"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        let item = &r.items[0].item;
        assert!(item.path.ends_with(".cache/go-build"));
        assert_eq!(item.status, Safe);
        assert!(find(&r, "go/bin").is_none());
        assert!(find(&r, "go/src").is_none());
        assert!(find(&r, "svc").is_none());
    }

    // ----- Editors (PLAN 2.11) -----------------------------------------------

    #[test]
    fn vscode_family_electron_caches_are_safe_under_editors() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Application Support/Code/Cache/data_0"),
            5000,
        );
        write(
            &home.join("Library/Application Support/Code - Insiders/Code Cache/js/x"),
            5000,
        );
        write(
            &home.join("Library/Application Support/VSCodium/GPUCache/data_1"),
            5000,
        );

        let r = scan(d.path(), &["editors"]);
        let code_cache = find(&r, "Code/Cache").unwrap();
        assert_eq!(code_cache.item.status, Safe);
        assert_eq!(code_cache.item.group, "Editors");

        let insiders_cache = find(&r, "Code - Insiders/Code Cache").unwrap();
        assert_eq!(insiders_cache.item.status, Safe);
        assert_eq!(insiders_cache.item.group, "Editors");

        let codium_cache = find(&r, "VSCodium/GPUCache").unwrap();
        assert_eq!(codium_cache.item.status, Safe);
        assert_eq!(codium_cache.item.group, "Editors");
    }

    #[test]
    fn active_workspace_storage_is_not_reported() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let project = d.path().join("active_project");
        write(&project.join("main.rs"), 100);

        let ws = home.join("Library/Application Support/Code/User/workspaceStorage/active_ws");
        fs::create_dir_all(&ws).unwrap();
        fs::write(
            ws.join("workspace.json"),
            format!(r#"{{"folder": "file://{}"}}"#, project.display()),
        )
        .unwrap();
        write(&ws.join("state.vscdb"), 5000);

        let r = scan(d.path(), &["editors"]);
        assert!(find(&r, "workspaceStorage/active_ws").is_none());
    }

    #[test]
    fn orphaned_workspace_storage_without_chats_is_review() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let parent = d.path().join("projects");
        fs::create_dir_all(&parent).unwrap();
        let missing = parent.join("deleted_project");

        let ws = home.join("Library/Application Support/Code/User/workspaceStorage/orphan_ws");
        fs::create_dir_all(&ws).unwrap();
        fs::write(
            ws.join("workspace.json"),
            format!(r#"{{"folder": "file://{}"}}"#, missing.display()),
        )
        .unwrap();
        write(&ws.join("state.vscdb"), 5000);

        let r = scan(d.path(), &["editors"]);
        let item = find(&r, "workspaceStorage/orphan_ws").unwrap();
        assert_eq!(item.item.status, Review);
        assert_eq!(item.item.group, "Editors");
    }

    #[test]
    fn orphaned_workspace_storage_with_chats_is_danger() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let parent = d.path().join("projects");
        fs::create_dir_all(&parent).unwrap();
        let missing = parent.join("deleted_with_chats");

        let ws = home.join("Library/Application Support/Code/User/workspaceStorage/orphan_chat_ws");
        fs::create_dir_all(&ws).unwrap();
        fs::write(
            ws.join("workspace.json"),
            format!(r#"{{"folder": "file://{}"}}"#, missing.display()),
        )
        .unwrap();
        write(&ws.join("chatSessions/session1.json"), 5000);

        let r = scan(d.path(), &["editors"]);
        let item = find(&r, "workspaceStorage/orphan_chat_ws").unwrap();
        assert_eq!(item.item.status, Danger);
        assert_eq!(item.item.group, "Editors");

        // When scanned with both editors and ai_assistants, drop_nested keeps the parent as Danger.
        let r_both = scan(d.path(), &["editors", "ai_assistants"]);
        let item_both = find(&r_both, "workspaceStorage/orphan_chat_ws").unwrap();
        assert_eq!(item_both.item.status, Danger);
        assert!(find(&r_both, "chatSessions").is_none());
    }

    #[test]
    fn jetbrains_old_cache_versions_are_review_and_newest_kept() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Caches/JetBrains/IntelliJIdea2023.1/caches/data"),
            5000,
        );
        write(
            &home.join("Library/Caches/JetBrains/IntelliJIdea2024.1/caches/data"),
            5000,
        );
        write(
            &home.join("Library/Caches/JetBrains/PyCharm2024.2/caches/data"),
            5000,
        );

        let r = scan(d.path(), &["editors"]);
        let old_idea = find(&r, "JetBrains/IntelliJIdea2023.1").unwrap();
        assert_eq!(old_idea.item.status, Review);
        assert_eq!(old_idea.item.group, "Editors");
        assert!(find(&r, "JetBrains/IntelliJIdea2024.1").is_none());
        assert!(find(&r, "JetBrains/PyCharm2024.2").is_none());
    }

    #[test]
    fn jetbrains_old_logs_are_safe_and_app_support_untouched() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Logs/JetBrains/IntelliJIdea2023.1/idea.log"),
            5000,
        );
        write(
            &home.join("Library/Logs/JetBrains/IntelliJIdea2024.1/idea.log"),
            5000,
        );
        write(
            &home.join("Library/Application Support/JetBrains/IntelliJIdea2023.1/options.xml"),
            5000,
        );
        write(
            &home.join("Library/Application Support/JetBrains/IntelliJIdea2024.1/options.xml"),
            5000,
        );

        let r = scan(d.path(), &["editors"]);
        let old_log = find(&r, "JetBrains/IntelliJIdea2023.1").unwrap();
        assert_eq!(old_log.item.status, Safe);
        assert_eq!(old_log.item.group, "Editors");
        assert!(find(&r, "Logs/JetBrains/IntelliJIdea2024.1").is_none());
        assert!(find(&r, "Application Support/JetBrains").is_none());

        let protected = protected_paths(&home);
        assert!(protected
            .iter()
            .any(|p| p.ends_with("Library/Application Support/JetBrains")));
        let app_support_item =
            scanned(&home.join("Library/Application Support/JetBrains/IntelliJIdea2023.1"));
        assert!(delete_permanently(&app_support_item, std::slice::from_ref(&home), &protected).is_err());
    }

    #[test]
    fn settings_and_user_data_never_offered_for_editors() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let user_files = [
            "Library/Application Support/Code/User/settings.json",
            "Library/Application Support/Code/User/keybindings.json",
            "Library/Application Support/Code/User/snippets/x.json",
            "Library/Application Support/Code/User/globalStorage/state.vscdb",
            "Library/Application Support/Code/User/profiles/x/settings.json",
        ];
        for f in &user_files {
            write(&home.join(f), 5000);
        }
        let r = scan(d.path(), &["editors"]);
        for item in &r.items {
            let item_path = Path::new(&item.item.path);
            for f in &user_files {
                let full = home.join(f);
                assert!(
                    !full.starts_with(item_path),
                    "Result {} matches or contains protected file {}",
                    item.item.path,
                    full.display()
                );
            }
        }
    }

    #[test]
    fn cursor_cache_appears_once_with_editors_and_ai_enabled() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Application Support/Cursor/Cache/data_0"),
            5000,
        );

        let r = scan(d.path(), &["editors", "ai_assistants"]);
        let count = r
            .items
            .iter()
            .filter(|i| i.item.path.ends_with("Cursor/Cache"))
            .count();
        assert_eq!(count, 1, "expected Cursor/Cache once, found {count}");
    }

    #[test]
    fn workspace_storage_edge_cases() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let ws = home.join("Library/Application Support/Code/User/workspaceStorage");

        // 1. Without workspace.json
        write(&ws.join("no_json/state.vscdb"), 5000);

        // 2. Remote workspace
        write(&ws.join("remote/state.vscdb"), 5000);
        fs::write(
            ws.join("remote/workspace.json"),
            r#"{"folder": "vscode-remote://ssh-remote+x/home/u/p"}"#,
        )
        .unwrap();

        // 3. Parent does not exist (e.g. unplugged drive)
        write(&ws.join("unplugged/state.vscdb"), 5000);
        fs::write(
            ws.join("unplugged/workspace.json"),
            r#"{"folder": "file:///Volumes/NoSuchDrive/proj"}"#,
        )
        .unwrap();

        // 4. Invalid JSON
        write(&ws.join("invalid/state.vscdb"), 5000);
        fs::write(
            ws.join("invalid/workspace.json"),
            "invalid json content {{{",
        )
        .unwrap();

        // 5. Missing project inside an EXISTING temp parent
        let existing_parent = d.path().join("existing_parent");
        fs::create_dir_all(&existing_parent).unwrap();
        let missing_proj = existing_parent.join("missing_proj");
        write(&ws.join("valid_orphan/state.vscdb"), 5000);
        fs::write(
            ws.join("valid_orphan/workspace.json"),
            format!(r#"{{"folder": "file://{}"}}"#, missing_proj.display()),
        )
        .unwrap();

        let r = scan(d.path(), &["editors"]);
        assert!(find(&r, "workspaceStorage/no_json").is_none());
        assert!(find(&r, "workspaceStorage/remote").is_none());
        assert!(find(&r, "workspaceStorage/unplugged").is_none());
        assert!(find(&r, "workspaceStorage/invalid").is_none());

        let orphan = find(&r, "workspaceStorage/valid_orphan").unwrap();
        assert_eq!(orphan.item.status, Review);
    }

    #[test]
    fn workspace_storage_percent_encoding() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let parent = d.path().join("projects");
        fs::create_dir_all(&parent).unwrap();
        let my_proj = parent.join("My Project");
        fs::create_dir_all(&my_proj).unwrap();
        write(&my_proj.join("main.rs"), 100);

        let ws = home.join("Library/Application Support/Code/User/workspaceStorage/encoded_ws");
        fs::create_dir_all(&ws).unwrap();
        write(&ws.join("state.vscdb"), 5000);
        let encoded_path = my_proj.display().to_string().replace(' ', "%20");
        fs::write(
            ws.join("workspace.json"),
            format!(r#"{{"folder": "file://{encoded_path}"}}"#),
        )
        .unwrap();

        let r1 = scan(d.path(), &["editors"]);
        assert!(find(&r1, "workspaceStorage/encoded_ws").is_none());

        // Remove the project directory while keeping its parent
        fs::remove_dir_all(&my_proj).unwrap();

        let r2 = scan(d.path(), &["editors"]);
        let item = find(&r2, "workspaceStorage/encoded_ws").unwrap();
        assert_eq!(item.item.status, Review);
    }

    #[test]
    fn jetbrains_version_order_is_numeric() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(
            &home.join("Library/Caches/JetBrains/IntelliJIdea2024.9/caches/data"),
            5000,
        );
        write(
            &home.join("Library/Caches/JetBrains/IntelliJIdea2024.10/caches/data"),
            5000,
        );

        let r = scan(d.path(), &["editors"]);
        assert!(find(&r, "JetBrains/IntelliJIdea2024.9").is_some());
        assert!(find(&r, "JetBrains/IntelliJIdea2024.10").is_none());
    }

    // ----- Docker (PLAN 2.12) ------------------------------------------------

    #[cfg(unix)]
    fn write_fake_docker(bin_dir: &Path, script_body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(bin_dir).unwrap();
        let path = bin_dir.join("docker");
        fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn docker_parse_size_cases() {
        use super::docker::parse_docker_size;
        assert_eq!(parse_docker_size("0B"), Some(0));
        assert_eq!(parse_docker_size("1.5kB"), Some(1500));
        assert_eq!(parse_docker_size("12.3MB"), Some(12_300_000));
        assert_eq!(parse_docker_size("4.2GB"), Some(4_200_000_000));
        assert_eq!(parse_docker_size("1TB"), Some(1_000_000_000_000));
        assert_eq!(parse_docker_size("garbage"), None);
        assert_eq!(parse_docker_size(""), None);
    }

    #[cfg(unix)]
    #[test]
    fn fake_docker_scan_reports_three_items_and_no_volumes() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let bin = home.join(".orbstack/bin");
        let json_output = r#"{"Type":"Images","TotalCount":"5","Active":"0","Size":"4.2GB","Reclaimable":"4.2GB (100%)"}
{"Type":"Containers","TotalCount":"2","Active":"0","Size":"12.3MB","Reclaimable":"12.3MB (100%)"}
{"Type":"Local Volumes","TotalCount":"3","Active":"0","Size":"50GB","Reclaimable":"50GB (100%)"}
{"Type":"Build Cache","TotalCount":"10","Active":"0","Size":"1.5kB","Reclaimable":"1.5kB (100%)"}"#;
        write_fake_docker(&bin, &format!("cat << 'EOF'\n{json_output}\nEOF"));

        let r = scan(d.path(), &["docker"]);
        assert_eq!(r.items.len(), 3, "expected 3 items: {:?}", paths(&r));

        let cache = find(&r, "build-cache").unwrap();
        assert_eq!(cache.item.size_bytes, 1500);
        assert_eq!(cache.item.status, Safe);
        assert_eq!(cache.item.file_type, "Docker build cache");
        assert_eq!(cache.item.group, "Dev Tools");

        let containers = find(&r, "containers").unwrap();
        assert_eq!(containers.item.size_bytes, 12_300_000);
        assert_eq!(containers.item.status, Review);
        assert_eq!(containers.item.file_type, "Docker stopped containers");

        let images = find(&r, "images").unwrap();
        assert_eq!(images.item.size_bytes, 4_200_000_000);
        assert_eq!(images.item.status, Review);
        assert_eq!(images.item.file_type, "Docker unused images");

        assert!(r.items.iter().all(|i| !i.item.path.contains("volume")
            && !i.item.file_type.to_lowercase().contains("volume")));
    }

    #[cfg(unix)]
    #[test]
    fn fake_docker_exit_code_1_gives_no_items() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let bin = home.join(".orbstack/bin");
        write_fake_docker(&bin, "exit 1");

        let r = scan(d.path(), &["docker"]);
        assert!(
            r.items.is_empty(),
            "expected 0 items when docker fails, got: {:?}",
            paths(&r)
        );
    }

    #[cfg(unix)]
    #[test]
    fn fake_docker_timeout_gives_no_items() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let bin = home.join(".orbstack/bin");
        write_fake_docker(&bin, "sleep 5");

        let env = Env { home };
        let mut candidates = Vec::new();
        let enabled: HashSet<&str> = ["docker"].into_iter().collect();
        super::docker::collect_with_timeout(
            &env,
            &enabled,
            std::time::Duration::from_millis(50),
            &mut candidates,
        );
        assert!(
            candidates.is_empty(),
            "expected timeout to return no candidates"
        );
    }

    #[cfg(unix)]
    #[test]
    fn docker_prune_runs_exact_commands_and_parses_reclaimed() {
        use super::native_commands::{DockerKind, NativeCommand};
        let d = TempDir::new().unwrap();
        let bin = d.path().join("bin");
        let log_file = d.path().join("docker_args.log");
        let log_str = log_file.display().to_string();
        let script = format!(
            r#"echo "$@" >> "{log_str}"
echo "Total reclaimed space: 1.5kB""#
        );
        let docker = write_fake_docker(&bin, &script);

        let res1 = native_commands::run(&NativeCommand::DockerPrune {
            docker: docker.clone(),
            kind: DockerKind::BuilderCache,
        })
        .unwrap();
        assert_eq!(res1, Some(1500));

        let res2 = native_commands::run(&NativeCommand::DockerPrune {
            docker: docker.clone(),
            kind: DockerKind::StoppedContainers,
        })
        .unwrap();
        assert_eq!(res2, Some(1500));

        let res3 = native_commands::run(&NativeCommand::DockerPrune {
            docker: docker.clone(),
            kind: DockerKind::UnusedImages,
        })
        .unwrap();
        assert_eq!(res3, Some(1500));

        let logged = fs::read_to_string(&log_file).unwrap();
        let lines: Vec<&str> = logged.lines().collect();
        assert_eq!(
            lines,
            vec![
                "builder prune -a -f",
                "container prune -f",
                "image prune -a -f",
            ]
        );
    }

    #[test]
    fn docker_prune_commands_never_contain_forbidden_keywords() {
        use super::native_commands::DockerKind;
        let forbidden_words = [
            "volume", "system", "rm", "rmi", "stop", "kill", "run", "exec",
        ];
        for kind in [
            DockerKind::BuilderCache,
            DockerKind::StoppedContainers,
            DockerKind::UnusedImages,
        ] {
            let args = kind.args();
            for arg in args {
                let lower = arg.to_lowercase();
                for word in &forbidden_words {
                    assert_ne!(
                        &lower, word,
                        "DockerKind::{:?} contains forbidden keyword {}",
                        kind, word
                    );
                    assert!(
                        !lower.contains("volume"),
                        "DockerKind::{:?} contains 'volume'",
                        kind
                    );
                }
                assert_ne!(*arg, "--volumes");
            }
        }
    }

    #[test]
    fn docker_run_time_check_refuses_directory_or_missing() {
        use super::native_commands::{DockerKind, NativeCommand};
        let d = TempDir::new().unwrap();
        let dir_as_docker = d.path().join("docker");
        fs::create_dir_all(&dir_as_docker).unwrap();

        let err_dir = native_commands::run(&NativeCommand::DockerPrune {
            docker: dir_as_docker,
            kind: DockerKind::BuilderCache,
        });
        assert!(
            err_dir.is_err(),
            "expected error for directory as docker binary"
        );

        let missing_docker = d.path().join("bin/docker");
        let err_missing = native_commands::run(&NativeCommand::DockerPrune {
            docker: missing_docker,
            kind: DockerKind::BuilderCache,
        });
        assert!(
            err_missing.is_err(),
            "expected error for missing docker binary"
        );
    }

    // ----- Homebrew (PLAN 2.13) ----------------------------------------------

    #[cfg(unix)]
    fn write_fake_brew(bin_dir: &Path, script_body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(bin_dir).unwrap();
        let path = bin_dir.join("brew");
        fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn brew_parse_size_cases() {
        use super::brew::parse_brew_size;
        assert_eq!(parse_brew_size("61.2MB"), Some(61_200_000));
        assert_eq!(parse_brew_size("4kB"), Some(4_000));
        assert_eq!(parse_brew_size("4KB"), Some(4_000));
        assert_eq!(parse_brew_size("0B"), Some(0));
        assert_eq!(parse_brew_size("2GB"), Some(2_000_000_000));
        assert_eq!(parse_brew_size("garbage"), None);
    }

    #[test]
    fn brew_parse_line_cases() {
        use super::brew::parse_cleanup_line;
        let line1 = "Would remove: /opt/homebrew/Cellar/node/22.1.0 (2,456 files, 61.2MB)";
        assert_eq!(
            parse_cleanup_line(line1),
            Some(("/opt/homebrew/Cellar/node/22.1.0", 61_200_000))
        );

        let line2 = "Would remove: /Users/x/Library/Caches/Homebrew/downloads/a.tar.gz (10MB)";
        assert_eq!(parse_cleanup_line(line2), None);

        let line3 = "Would remove: /opt/homebrew/Caskroom/google-chrome/120.0 (150MB)";
        assert_eq!(
            parse_cleanup_line(line3),
            Some(("/opt/homebrew/Caskroom/google-chrome/120.0", 150_000_000))
        );
    }

    #[cfg(unix)]
    #[test]
    fn fake_brew_scan_reports_old_versions_and_separate_cache() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let bin = home.join(".cargo/bin");
        let brew_output = r#"Would remove: /opt/homebrew/Cellar/node/22.1.0 (2,456 files, 61.2MB)
Would remove: /opt/homebrew/Cellar/git/2.44.0 (1,000 files, 30MB)
Would remove: /opt/homebrew/Caskroom/docker/4.28.0 (100MB)
Would remove: /Users/x/Library/Caches/Homebrew/downloads/pkg.tar.gz (500MB)
==> This operation would free approximately 691.2MB of disk space."#;
        write_fake_brew(&bin, &format!("cat << 'EOF'\n{brew_output}\nEOF"));

        write(
            &home.join("Library/Caches/Homebrew/downloads/cache.tar.gz"),
            5000,
        );

        let r = scan(d.path(), &["homebrew"]);

        let old_versions: Vec<_> = r
            .items
            .iter()
            .filter(|i| i.item.path == "brew://old-versions")
            .collect();
        assert_eq!(old_versions.len(), 1);
        let item = old_versions[0];
        assert_eq!(item.item.status, Review);
        assert_eq!(item.item.size_bytes, 191_200_000);
        assert!(item.item.description.contains("3 old versions"));
        assert_eq!(item.item.file_type, "Homebrew old versions");
        assert_eq!(item.item.group, "Dev Tools");

        let cache_item = find(&r, "Library/Caches/Homebrew").unwrap();
        assert_eq!(cache_item.item.status, Safe);
        assert!(cache_item.item.size_bytes > 0);
        assert_ne!(cache_item.item.size_bytes, 191_200_000);
    }

    #[cfg(unix)]
    #[test]
    fn fake_brew_cache_only_or_fail_or_timeout_gives_no_items() {
        let d1 = TempDir::new().unwrap();
        let home1 = d1.path().join("__home");
        let bin1 = home1.join(".cargo/bin");
        write_fake_brew(
            &bin1,
            "echo 'Would remove: /Users/x/Library/Caches/Homebrew/downloads/a.tar.gz (10MB)'",
        );
        let r1 = scan(d1.path(), &["homebrew"]);
        assert!(find(&r1, "old-versions").is_none());

        let d2 = TempDir::new().unwrap();
        let home2 = d2.path().join("__home");
        let bin2 = home2.join(".cargo/bin");
        write_fake_brew(&bin2, "exit 1");
        let r2 = scan(d2.path(), &["homebrew"]);
        assert!(find(&r2, "old-versions").is_none());

        let d4 = TempDir::new().unwrap();
        let home4 = d4.path().join("__home");
        let bin4 = home4.join(".cargo/bin");
        write_fake_brew(&bin4, "sleep 5");
        let env4 = Env { home: home4 };
        let mut candidates4 = Vec::new();
        let enabled4: HashSet<&str> = ["homebrew"].into_iter().collect();
        super::brew::collect_with_timeout(
            &env4,
            &enabled4,
            std::time::Duration::from_millis(50),
            &mut candidates4,
        );
        assert!(candidates4.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn brew_real_output_sample_is_parsed_safely() {
        let sample_output = r#"Would remove: /opt/homebrew/var/homebrew/tmp/.cellar/xorgproto (267 files, 4.1MB)
Would remove: /opt/homebrew/var/homebrew/tmp/.cellar/libpng (27 files, 1.4MB)
Would remove: /opt/homebrew/var/homebrew/tmp/.cellar/harfbuzz (95 files, 14.1MB)
Would remove: /opt/homebrew/var/homebrew/tmp/.cellar/pcre2 (244 files, 7.3MB)
Would remove (broken link): /opt/homebrew/share/man/man1/npm-adduser.1
Would remove (broken link): /opt/homebrew/share/man/man5/npm-shrinkwrap-json.5
Would remove (empty directory): /opt/homebrew/lib/gio/modules
Would remove (empty directory): /opt/homebrew/lib/gio
Would remove: /opt/homebrew/Library/Homebrew/vendor/portable-ruby/4.0.6 (1,707 files, 34.6MB)
==> This operation would free approximately 61.5MB of disk space."#;

        // Part one: gives NO item whose path is "brew://old-versions".
        let d1 = TempDir::new().unwrap();
        let home1 = d1.path().join("__home");
        let bin1 = home1.join(".cargo/bin");
        write_fake_brew(&bin1, &format!("cat << 'EOF'\n{sample_output}\nEOF"));

        let r1 = scan(d1.path(), &["homebrew"]);
        assert!(
            r1.items
                .iter()
                .all(|i| i.item.path != "brew://old-versions"),
            "expected no brew://old-versions item"
        );

        // Part two: second fake home with the same output plus one more line
        let d2 = TempDir::new().unwrap();
        let home2 = d2.path().join("__home");
        let bin2 = home2.join(".cargo/bin");
        let sample_output_with_node = format!(
            "{sample_output}\nWould remove: /opt/homebrew/Cellar/node/22.1.0 (2,456 files, 61.2MB)"
        );
        write_fake_brew(
            &bin2,
            &format!("cat << 'EOF'\n{sample_output_with_node}\nEOF"),
        );

        let r2 = scan(d2.path(), &["homebrew"]);
        let old_versions: Vec<_> = r2
            .items
            .iter()
            .filter(|i| i.item.path == "brew://old-versions")
            .collect();
        assert_eq!(old_versions.len(), 1);
        let item = old_versions[0];
        assert_eq!(item.item.size_bytes, 61_200_000);
        assert!(
            item.item.description.contains("1 old versions"),
            "expected description to contain '1 old versions', got: {}",
            item.item.description
        );
    }

    #[cfg(unix)]
    #[test]
    fn brew_cleanup_runs_exact_args_and_parses_freed_bytes() {
        let d = TempDir::new().unwrap();
        let bin = d.path().join("bin");
        let log_file = d.path().join("brew_args.log");
        let log_str = log_file.display().to_string();
        let script = format!(
            r#"echo "$@" >> "{log_str}"
echo "AUTO_UPDATE=$HOMEBREW_NO_AUTO_UPDATE" >> "{log_str}"
echo "==> This operation has freed approximately 1.5GB of disk space.""#
        );
        let brew = write_fake_brew(&bin, &script);

        let res = native_commands::run(&NativeCommand::BrewCleanup { brew }).unwrap();
        assert_eq!(res, Some(1_500_000_000));

        let logged = fs::read_to_string(&log_file).unwrap();
        let lines: Vec<&str> = logged.lines().collect();
        assert_eq!(lines, vec!["cleanup", "AUTO_UPDATE=1"]);
    }

    #[cfg(unix)]
    #[test]
    fn command_action_uses_the_bytes_the_tool_measured() {
        use super::native_commands::NativeCommand;
        let d = TempDir::new().unwrap();
        let brew = write_fake_brew(
            &d.path().join("bin"),
            "echo '==> This operation has freed approximately 7MB of disk space.'",
        );
        let mut item = scanned(&d.path().join("bin"));
        item.item.size_bytes = 100;
        item.action = Action::Command(NativeCommand::BrewCleanup { brew });

        let removed = execute_with(&item, &[], &[], DeleteMode::Trash, &no_trasher).unwrap();
        assert_eq!(removed.bytes, 7_000_000);
        assert!(!removed.trashed);
    }

    #[cfg(unix)]
    #[test]
    fn command_action_falls_back_to_the_scanned_size() {
        use super::native_commands::NativeCommand;
        let d = TempDir::new().unwrap();
        let brew = write_fake_brew(&d.path().join("bin"), "echo done");
        let mut item = scanned(&d.path().join("bin"));
        item.item.size_bytes = 100;
        item.action = Action::Command(NativeCommand::BrewCleanup { brew });

        let removed = execute_with(&item, &[], &[], DeleteMode::Trash, &no_trasher).unwrap();
        assert_eq!(removed.bytes, 100);
        assert!(!removed.trashed);
    }

    #[test]
    fn brew_calls_never_contain_forbidden_flags_or_commands() {
        let preview_args: &[&str] = &["cleanup", "-n"];
        let run_args: &[&str] = &["cleanup"];
        let forbidden = ["-s", "--prune", "uninstall", "remove", "autoremove", "zap"];

        for call in [preview_args, run_args] {
            for arg in call {
                let lower = arg.to_lowercase();
                for f in &forbidden {
                    assert_ne!(&lower, f, "brew call contains forbidden argument: {arg}");
                    assert!(
                        !lower.starts_with("--prune"),
                        "brew call starts with --prune: {arg}"
                    );
                }
            }
        }
    }

    #[test]
    fn brew_run_time_check_refuses_directory_or_wrong_name() {
        let d = TempDir::new().unwrap();
        let dir_as_brew = d.path().join("brew");
        fs::create_dir_all(&dir_as_brew).unwrap();

        let err_dir = native_commands::run(&NativeCommand::BrewCleanup { brew: dir_as_brew });
        assert!(
            err_dir.is_err(),
            "expected error for directory as brew binary"
        );

        let wrong_name = d.path().join("not_brew");
        fs::write(&wrong_name, "echo hi").unwrap();
        let err_name = native_commands::run(&NativeCommand::BrewCleanup { brew: wrong_name });
        assert!(err_name.is_err(), "expected error for file not named brew");

        let missing = d.path().join("missing/brew");
        let err_missing = native_commands::run(&NativeCommand::BrewCleanup { brew: missing });
        assert!(
            err_missing.is_err(),
            "expected error for missing brew binary"
        );
    }

    // ----- AI Models (PLAN 2.14a) --------------------------------------------

    #[test]
    fn huggingface_hub_models_datasets_and_spaces() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let hub = home.join(".cache/huggingface/hub");

        write(
            &hub.join("models--org--name/snapshots/main/model.safetensors"),
            5000,
        );
        write(&hub.join("datasets--x--y/data.parquet"), 5000);
        write(&hub.join("models--solo/weights.bin"), 5000);
        write(&hub.join(".locks/models/lock"), 10);
        write(&hub.join("version.txt"), 10);
        write(&home.join(".cache/huggingface/token"), 10);
        write(&home.join(".cache/huggingface/stored_tokens"), 10);

        let r = scan(d.path(), &["ai"]);

        assert_eq!(r.items.len(), 3, "got items: {:?}", paths(&r));

        let m1 = r
            .items
            .iter()
            .find(|i| i.item.description.contains("org/name"))
            .unwrap();
        assert_eq!(m1.item.file_type, "Hugging Face Models");
        assert_eq!(m1.item.category, "AI Model Cache");
        assert_eq!(m1.item.status, Review);
        assert_eq!(m1.item.group, "AI Tools");

        let m2 = r
            .items
            .iter()
            .find(|i| i.item.description.contains("x/y"))
            .unwrap();
        assert_eq!(m2.item.file_type, "Hugging Face Datasets");
        assert_eq!(m2.item.status, Review);

        let m3 = r
            .items
            .iter()
            .find(|i| i.item.description.contains("solo"))
            .unwrap();
        assert_eq!(m3.item.file_type, "Hugging Face Models");
        assert_eq!(m3.item.status, Review);

        assert!(find(&r, ".locks").is_none());
        assert!(find(&r, "version.txt").is_none());
        assert!(find(&r, "token").is_none());
        assert!(r
            .items
            .iter()
            .all(|i| i.item.path != home.join(".cache/huggingface").display().to_string()));
    }

    #[test]
    fn huggingface_extra_caches_datasets_and_xet() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".cache/huggingface/datasets/cache1.arrow"), 5000);
        write(&home.join(".cache/huggingface/xet/cache2.bin"), 5000);
        write(&home.join(".cache/huggingface/assets/x"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 2, "got items: {:?}", paths(&r));

        let ds = find(&r, ".cache/huggingface/datasets").unwrap();
        assert_eq!(ds.item.file_type, "Hugging Face Datasets Cache");
        assert_eq!(ds.item.status, Review);

        let xet = find(&r, ".cache/huggingface/xet").unwrap();
        assert_eq!(xet.item.file_type, "Hugging Face Xet Cache");
        assert_eq!(xet.item.status, Review);

        assert!(find(&r, "assets").is_none());
    }

    #[test]
    fn lm_studio_models_hierarchical_discovery() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");

        // Primary root: ~/.lmstudio/models
        write(&home.join(".lmstudio/models/pub/model-a/a.gguf"), 5000);
        write(&home.join(".lmstudio/models/pub/model-b/b.gguf"), 5000);
        write(&home.join(".lmstudio/models/.internal/x/y"), 5000);
        write(&home.join(".lmstudio/models/loose.gguf"), 5000);

        // Legacy root: ~/.cache/lm-studio/models
        write(
            &home.join(".cache/lm-studio/models/pub2/model-c/c.gguf"),
            5000,
        );

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 3, "got: {:?}", paths(&r));

        let m_a = find(&r, "pub/model-a").unwrap();
        assert_eq!(m_a.item.file_type, "LM Studio Models");
        assert_eq!(m_a.item.status, Review);
        assert_eq!(m_a.item.group, "AI Tools");
        assert_eq!(m_a.blocked_if_running, &["LM Studio"]);
        assert!(m_a.item.description.contains("pub/model-a"));

        let m_b = find(&r, "pub/model-b").unwrap();
        assert_eq!(m_b.item.file_type, "LM Studio Models");
        assert_eq!(m_b.blocked_if_running, &["LM Studio"]);

        let m_c = find(&r, "pub2/model-c").unwrap();
        assert_eq!(m_c.item.file_type, "LM Studio Models");
        assert_eq!(m_c.blocked_if_running, &["LM Studio"]);

        assert!(find(&r, ".internal").is_none());
        assert!(find(&r, "loose.gguf").is_none());
        assert!(
            find(&r, ".lmstudio/models").is_none()
                || r.items
                    .iter()
                    .all(|i| i.item.path != home.join(".lmstudio/models").display().to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn huggingface_hub_symlink_ignored() {
        use std::os::unix::fs::symlink;
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let hub = home.join(".cache/huggingface/hub");
        fs::create_dir_all(&hub).unwrap();

        let outside = d.path().join("outside_target");
        write(&outside.join("file.bin"), 5000);

        symlink(&outside, hub.join("models--evil")).unwrap();

        let r = scan(d.path(), &["ai"]);
        assert!(
            r.items.is_empty(),
            "symlink in hub must not be offered: {:?}",
            paths(&r)
        );
    }

    #[test]
    fn ai_models_protected_paths_and_delete_refused() {
        let d = TempDir::new().unwrap();
        let root = fs::canonicalize(d.path()).unwrap();
        let home = root.join("home");
        let token_rel = ".cache/huggingface/token";
        write(&home.join(token_rel), 10);

        let protected = protected_paths(&home);
        for expected in super::ai_models::PROTECTED {
            let p = home.join(expected);
            assert!(
                protected.iter().any(|prot| prot == &p),
                "expected protected path missing: {expected}"
            );
        }

        let roots = vec![home.clone()];
        assert!(
            delete_permanently(&scanned(&home.join(token_rel)), &roots, &protected).is_err(),
            "{token_rel} delete was not refused"
        );
    }

    #[test]
    fn pytorch_cache_still_works() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".cache/torch/kernels/kernel.bin"), 5000);

        let r = scan(d.path(), &["ai"]);
        assert_eq!(r.items.len(), 1);
        let item = &r.items[0];
        assert!(item.item.path.ends_with(".cache/torch"));
        assert_eq!(item.item.file_type, "PyTorch Cache");
        assert_eq!(item.item.status, Review);
    }

    // ----- Ollama Models (PLAN 2.14b) ----------------------------------------

    #[cfg(unix)]
    fn write_fake_ollama(bin_dir: &Path, script_body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(bin_dir).unwrap();
        let path = bin_dir.join("ollama");
        fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn ollama_manifests_discovery_and_filtering() {
        use super::native_commands::NativeCommand;
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write_fake_ollama(&home.join(".cargo/bin"), "exit 0");

        let manifests = home.join(".ollama/models/manifests");

        // 1. library/llama3/latest (config size 100, layers 1000 and 2000) -> 3100
        let m1 = manifests.join("registry.ollama.ai/library/llama3/latest");
        fs::create_dir_all(m1.parent().unwrap()).unwrap();
        fs::write(
            &m1,
            r#"{"config": {"size": 100}, "layers": [{"size": 1000}, {"size": 2000}]}"#,
        )
        .unwrap();

        // 2. someone/mymodel/7b
        let m2 = manifests.join("registry.ollama.ai/someone/mymodel/7b");
        fs::create_dir_all(m2.parent().unwrap()).unwrap();
        fs::write(
            &m2,
            r#"{"config": {"size": 50}, "layers": [{"size": 500}]}"#,
        )
        .unwrap();

        // 3. hf.co/org/repo/q4
        let m3 = manifests.join("hf.co/org/repo/q4");
        fs::create_dir_all(m3.parent().unwrap()).unwrap();
        fs::write(
            &m3,
            r#"{"config": {"size": 20}, "layers": [{"size": 200}]}"#,
        )
        .unwrap();

        // 4. invalid JSON -> skipped
        let m_bad = manifests.join("registry.ollama.ai/library/badjson/latest");
        fs::create_dir_all(m_bad.parent().unwrap()).unwrap();
        fs::write(&m_bad, "not valid json").unwrap();

        // 5. hidden directory -> skipped
        let m_hidden = manifests.join("registry.ollama.ai/library/.hidden/latest");
        fs::create_dir_all(m_hidden.parent().unwrap()).unwrap();
        fs::write(&m_hidden, r#"{"config": {"size": 10}}"#).unwrap();

        // 6. file at wrong depth -> skipped
        let m_shallow = manifests.join("registry.ollama.ai/library/wrong_depth.txt");
        fs::write(&m_shallow, r#"{"config": {"size": 10}}"#).unwrap();

        // 7. symlinked manifest -> skipped
        let m_target = home.join("real_manifest");
        fs::write(&m_target, r#"{"config": {"size": 10}}"#).unwrap();
        let m_symlink = manifests.join("registry.ollama.ai/library/symlinked/latest");
        fs::create_dir_all(m_symlink.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&m_target, &m_symlink).unwrap();

        let r = scan(d.path(), &["ai"]);

        let item1 = r
            .items
            .iter()
            .find(|i| i.item.description.contains("llama3:latest"))
            .unwrap();
        assert_eq!(item1.item.file_type, "Ollama Models");
        assert_eq!(item1.item.category, "AI Model Cache");
        assert_eq!(item1.item.status, Review);
        assert_eq!(item1.item.group, "AI Tools");
        assert_eq!(item1.item.size_bytes, 3100);
        assert!(matches!(
            item1.action,
            Action::Command(NativeCommand::OllamaRemove { ref model, .. }) if model == "llama3:latest"
        ));

        let item2 = r
            .items
            .iter()
            .find(|i| i.item.description.contains("someone/mymodel:7b"))
            .unwrap();
        assert_eq!(item2.item.status, Review);
        assert_eq!(item2.item.group, "AI Tools");
        assert_eq!(item2.item.size_bytes, 550);
        assert!(matches!(
            item2.action,
            Action::Command(NativeCommand::OllamaRemove { ref model, .. }) if model == "someone/mymodel:7b"
        ));

        let item3 = r
            .items
            .iter()
            .find(|i| i.item.description.contains("hf.co/org/repo:q4"))
            .unwrap();
        assert_eq!(item3.item.status, Review);
        assert_eq!(item3.item.group, "AI Tools");
        assert_eq!(item3.item.size_bytes, 220);
        assert!(matches!(
            item3.action,
            Action::Command(NativeCommand::OllamaRemove { ref model, .. }) if model == "hf.co/org/repo:q4"
        ));

        assert!(r.items.iter().all(|i| !i.item.path.contains("badjson")));
        assert!(r.items.iter().all(|i| !i.item.path.contains(".hidden")));
        assert!(r.items.iter().all(|i| !i.item.path.contains("wrong_depth")));
        assert!(r.items.iter().all(|i| !i.item.path.contains("symlinked")));
    }

    #[test]
    fn valid_ollama_model_cases() {
        use super::native_commands::valid_ollama_model;
        assert!(valid_ollama_model("llama3:latest"));
        assert!(valid_ollama_model("someone/mymodel:7b"));
        assert!(valid_ollama_model("registry.example.com/ns/m:1"));
        assert!(!valid_ollama_model(""));
        assert!(!valid_ollama_model("-rf"));
        assert!(!valid_ollama_model("a b"));
        assert!(!valid_ollama_model("../x"));
        assert!(!valid_ollama_model("a;b"));
        assert!(!valid_ollama_model("a//b"));
        assert!(!valid_ollama_model("a:b:c"));
        assert!(!valid_ollama_model("x\ny"));
    }

    #[test]
    fn manifest_path_cases() {
        use super::native_commands::manifest_path;
        let m = Path::new("/fake/models");
        assert_eq!(
            manifest_path(m, "llama3:latest"),
            Some(m.join("manifests/registry.ollama.ai/library/llama3/latest"))
        );
        assert_eq!(
            manifest_path(m, "someone/mymodel:7b"),
            Some(m.join("manifests/registry.ollama.ai/someone/mymodel/7b"))
        );
        assert_eq!(
            manifest_path(m, "hf.co/org/repo:q4"),
            Some(m.join("manifests/hf.co/org/repo/q4"))
        );
        assert_eq!(manifest_path(m, "a/b/c/d:tag"), None);
        assert_eq!(manifest_path(m, "llama3"), None);
    }

    #[cfg(unix)]
    #[test]
    fn ollama_remove_runs_and_measures_freed_blobs() {
        use super::native_commands::{manifest_path, NativeCommand};
        let d = TempDir::new().unwrap();
        let home = d.path();
        let models_dir = home.join(".ollama/models");
        let manifest = manifest_path(&models_dir, "llama3:latest").unwrap();
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, "{}").unwrap();

        let blob_dir = models_dir.join("blobs");
        fs::create_dir_all(&blob_dir).unwrap();
        let blob_file = blob_dir.join("sha256-blob1");
        fs::write(&blob_file, vec![0u8; 10000]).unwrap();

        let log_file = home.join("ollama_args.log");
        let log_str = log_file.display().to_string();
        let manifest_str = manifest.display().to_string();
        let blob_str = blob_file.display().to_string();

        let script = format!(
            r#"echo "$@" >> "{log_str}"
rm -f "{manifest_str}"
rm -f "{blob_str}""#
        );
        let ollama = write_fake_ollama(&home.join("bin"), &script);

        let res = native_commands::run(&NativeCommand::OllamaRemove {
            ollama,
            models_dir,
            model: "llama3:latest".to_string(),
        })
        .unwrap();

        assert!(res.is_some_and(|n| n > 0));
        let logged = fs::read_to_string(&log_file).unwrap();
        assert_eq!(logged.trim(), "rm llama3:latest");
    }

    #[cfg(unix)]
    #[test]
    fn ollama_remove_leaves_manifest_returns_err() {
        use super::native_commands::{manifest_path, NativeCommand};
        let d = TempDir::new().unwrap();
        let home = d.path();
        let models_dir = home.join(".ollama/models");
        let manifest = manifest_path(&models_dir, "llama3:latest").unwrap();
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, "{}").unwrap();

        let ollama = write_fake_ollama(&home.join("bin"), "exit 0");

        let err = native_commands::run(&NativeCommand::OllamaRemove {
            ollama,
            models_dir,
            model: "llama3:latest".to_string(),
        })
        .unwrap_err();

        assert!(err.contains("did not remove"), "unexpected error: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn ollama_remove_exit_1_returns_stderr() {
        use super::native_commands::{manifest_path, NativeCommand};
        let d = TempDir::new().unwrap();
        let home = d.path();
        let models_dir = home.join(".ollama/models");
        let manifest = manifest_path(&models_dir, "llama3:latest").unwrap();
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, "{}").unwrap();

        let ollama = write_fake_ollama(
            &home.join("bin"),
            "echo 'model not found on server' >&2\nexit 1",
        );

        let err = native_commands::run(&NativeCommand::OllamaRemove {
            ollama,
            models_dir,
            model: "llama3:latest".to_string(),
        })
        .unwrap_err();

        assert!(
            err.contains("model not found on server"),
            "unexpected error: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ollama_remove_run_time_checks_refuse_invalid() {
        use super::native_commands::NativeCommand;
        let d = TempDir::new().unwrap();
        let home = d.path();
        let models_dir = home.join(".ollama/models");

        let log_file = home.join("should_not_run.log");
        let log_str = log_file.display().to_string();
        let script = format!("echo ran >> \"{log_str}\"\nexit 0");
        let ollama = write_fake_ollama(&home.join("bin"), &script);

        // 1. Manifest file does not exist
        let err_no_manifest = native_commands::run(&NativeCommand::OllamaRemove {
            ollama: ollama.clone(),
            models_dir: models_dir.clone(),
            model: "llama3:latest".to_string(),
        });
        assert!(err_no_manifest.is_err());
        assert!(!log_file.exists());

        // 2. Model name is invalid
        let err_invalid_name = native_commands::run(&NativeCommand::OllamaRemove {
            ollama: ollama.clone(),
            models_dir: models_dir.clone(),
            model: "-rf".to_string(),
        });
        assert!(err_invalid_name.is_err());
        assert!(!log_file.exists());

        // Create manifest file so tool path checks can be tested
        let manifest = super::native_commands::manifest_path(&models_dir, "llama3:latest").unwrap();
        fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        fs::write(&manifest, "{}").unwrap();

        // 3. Ollama path is a directory
        let dir_as_ollama = home.join("bin");
        let err_dir = native_commands::run(&NativeCommand::OllamaRemove {
            ollama: dir_as_ollama,
            models_dir: models_dir.clone(),
            model: "llama3:latest".to_string(),
        });
        assert!(err_dir.is_err());
        assert!(!log_file.exists());

        // 4. Ollama path is not named ollama
        let wrong_name = home.join("bin").join("other_name");
        fs::copy(&ollama, &wrong_name).unwrap();
        let err_name = native_commands::run(&NativeCommand::OllamaRemove {
            ollama: wrong_name,
            models_dir: models_dir.clone(),
            model: "llama3:latest".to_string(),
        });
        assert!(err_name.is_err());
        assert!(!log_file.exists());
    }

    // ----- Global caches: Maven, Swift Package Manager, Deno, Terraform (PLAN 2.17) -----

    fn global_candidates(base: &Path, modules: &[&str]) -> Vec<Candidate> {
        let env = Env {
            home: base.join("__home"),
        };
        fs::create_dir_all(&env.home).unwrap();
        let enabled: HashSet<&str> = modules.iter().copied().collect();
        let mut out = Vec::new();
        collect_global(&env, &enabled, &mut out);
        out
    }

    fn global_candidate<'a>(cands: &'a [Candidate], suffix: &str) -> &'a Candidate {
        cands
            .iter()
            .find(|c| c.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("no candidate ending with {suffix}"))
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn maven_swift_deno_terraform_rules_found() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join(".m2/repository/org/apache/x/a.jar"), 5000);
        write(&home.join("Library/Caches/org.swift.swiftpm/objects/x"), 5000);
        write(&home.join("Library/org.swift.swiftpm/registries/x"), 5000);
        write(&home.join("Library/Caches/deno/deps/x"), 5000);
        write(&home.join(".terraform.d/plugin-cache/registry/x"), 5000);

        let r = scan(d.path(), &["java", "xcode", "node", "devops"]);
        assert_eq!(find(&r, ".m2/repository").unwrap().item.status, Review);
        assert_eq!(
            find(&r, "Library/Caches/org.swift.swiftpm")
                .unwrap()
                .item
                .status,
            Safe
        );
        assert_eq!(
            find(&r, "Library/org.swift.swiftpm").unwrap().item.status,
            Review
        );
        assert_eq!(
            find(&r, "Library/Caches/deno").unwrap().item.status,
            Safe
        );
        assert_eq!(
            find(&r, ".terraform.d/plugin-cache").unwrap().item.status,
            Safe
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn maven_deno_terraform_rules_found() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        #[cfg(target_os = "windows")]
        let deno = "AppData/Local/deno";
        #[cfg(not(target_os = "windows"))]
        let deno = ".cache/deno";

        write(&home.join(".m2/repository/org/apache/x/a.jar"), 5000);
        write(&home.join(deno).join("deps/x"), 5000);
        write(&home.join(".terraform.d/plugin-cache/registry/x"), 5000);

        let r = scan(d.path(), &["java", "node", "devops"]);
        assert_eq!(find(&r, ".m2/repository").unwrap().item.status, Review);
        assert_eq!(find(&r, deno).unwrap().item.status, Safe);
        assert_eq!(
            find(&r, ".terraform.d/plugin-cache").unwrap().item.status,
            Safe
        );
    }

    #[test]
    fn new_global_rules_are_absent_when_folders_missing() {
        let d = TempDir::new().unwrap();
        #[cfg(target_os = "macos")]
        let deno = "Library/Caches/deno";
        #[cfg(target_os = "windows")]
        let deno = "AppData/Local/deno";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let deno = ".cache/deno";

        let r = scan(d.path(), &["java", "node", "devops"]);
        assert!(find(&r, ".m2/repository").is_none());
        assert!(find(&r, deno).is_none());
        assert!(find(&r, ".terraform.d/plugin-cache").is_none());

        #[cfg(target_os = "macos")]
        {
            let r = scan(d.path(), &["xcode"]);
            assert!(find(&r, "Library/Caches/org.swift.swiftpm").is_none());
            assert!(find(&r, "Library/org.swift.swiftpm").is_none());
        }
    }

    #[test]
    fn new_global_rules_block_their_tools() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        #[cfg(target_os = "macos")]
        let deno = "Library/Caches/deno";
        #[cfg(target_os = "windows")]
        let deno = "AppData/Local/deno";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let deno = ".cache/deno";

        write(&home.join(".m2/repository/org/apache/x/a.jar"), 10);
        write(&home.join(deno).join("deps/x"), 10);
        write(&home.join(".terraform.d/plugin-cache/registry/x"), 10);

        let cands = global_candidates(d.path(), &["java", "node", "devops"]);
        assert_eq!(
            global_candidate(&cands, ".m2/repository").blocked_if_running,
            vec!["mvn", "java"]
        );
        assert_eq!(
            global_candidate(&cands, deno).blocked_if_running,
            vec!["deno"]
        );
        assert_eq!(
            global_candidate(&cands, ".terraform.d/plugin-cache").blocked_if_running,
            vec!["terraform"]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn swift_package_rules_block_xcode_and_swift() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        write(&home.join("Library/Caches/org.swift.swiftpm/objects/x"), 10);
        write(&home.join("Library/org.swift.swiftpm/registries/x"), 10);

        let cands = global_candidates(d.path(), &["xcode"]);
        for suffix in ["Library/Caches/org.swift.swiftpm", "Library/org.swift.swiftpm"] {
            assert_eq!(
                global_candidate(&cands, suffix).blocked_if_running,
                vec!["Xcode", "swift-build", "swift"]
            );
        }
    }

    // ----- Work apps: Slack, Zoom, Teams, Notion, Postman and Figma (PLAN 2.18) -----

    #[cfg(target_os = "macos")]
    const WORK_APPS_MACOS_CACHES: &[(&str, &str)] = &[
        ("Library/Application Support/Slack/Cache", "Slack Cache"),
        (
            "Library/Application Support/Slack/Code Cache",
            "Slack Code Cache",
        ),
        (
            "Library/Application Support/Slack/GPUCache",
            "Slack GPU Cache",
        ),
        (
            "Library/Application Support/Slack/Service Worker/CacheStorage",
            "Slack Service Worker Cache",
        ),
        (
            "Library/Caches/com.tinyspeck.slackmacgap",
            "Slack App Cache",
        ),
        ("Library/Caches/us.zoom.xos", "Zoom Cache"),
        (
            "Library/Application Support/Microsoft/Teams/Cache",
            "Teams Cache",
        ),
        (
            "Library/Application Support/Microsoft/Teams/Code Cache",
            "Teams Code Cache",
        ),
        (
            "Library/Application Support/Microsoft/Teams/GPUCache",
            "Teams GPU Cache",
        ),
        ("Library/Caches/com.microsoft.teams2", "Teams App Cache"),
        ("Library/Application Support/Notion/Cache", "Notion Cache"),
        (
            "Library/Application Support/Notion/Code Cache",
            "Notion Code Cache",
        ),
        (
            "Library/Application Support/Notion/GPUCache",
            "Notion GPU Cache",
        ),
        ("Library/Caches/notion.id", "Notion App Cache"),
        (
            "Library/Application Support/Postman/Cache",
            "Postman Cache",
        ),
        (
            "Library/Application Support/Postman/Code Cache",
            "Postman Code Cache",
        ),
        (
            "Library/Application Support/Postman/GPUCache",
            "Postman GPU Cache",
        ),
        ("Library/Caches/com.postmanlabs.mac", "Postman App Cache"),
        ("Library/Application Support/Figma/Cache", "Figma Cache"),
        (
            "Library/Application Support/Figma/Code Cache",
            "Figma Code Cache",
        ),
        (
            "Library/Application Support/Figma/GPUCache",
            "Figma GPU Cache",
        ),
        ("Library/Caches/com.figma.Desktop", "Figma App Cache"),
    ];

    #[cfg(target_os = "macos")]
    #[test]
    fn work_apps_macos_caches_are_safe() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        for &(rel, _) in WORK_APPS_MACOS_CACHES {
            write(&home.join(rel).join("blob"), 5000);
        }

        let r = scan(d.path(), &["work_apps"]);
        assert_eq!(r.items.len(), WORK_APPS_MACOS_CACHES.len());
        for &(rel, label) in WORK_APPS_MACOS_CACHES {
            let item = find(&r, rel).unwrap_or_else(|| panic!("missing {rel}"));
            assert_eq!(item.item.file_type, label, "{rel}");
            assert_eq!(item.item.status, Safe, "{rel}");
            assert_eq!(item.item.category, "App Cache", "{rel}");
            assert_eq!(item.item.group, "System & Apps", "{rel}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn work_apps_missing_folders_give_no_items() {
        let d = TempDir::new().unwrap();
        let r = scan(d.path(), &["work_apps"]);
        assert!(r.items.is_empty(), "{:?}", paths(&r));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn work_apps_blocked_lists_match_the_rules() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        for &(rel, _) in WORK_APPS_MACOS_CACHES {
            write(&home.join(rel).join("blob"), 10);
        }

        let cands = global_candidates(d.path(), &["work_apps"]);
        let expected: &[(&str, &[&str])] = &[
            ("Library/Application Support/Slack/Cache", &["Slack"]),
            (
                "Library/Application Support/Slack/Service Worker/CacheStorage",
                &["Slack"],
            ),
            ("Library/Caches/com.tinyspeck.slackmacgap", &["Slack"]),
            ("Library/Caches/us.zoom.xos", &["zoom.us"]),
            (
                "Library/Application Support/Microsoft/Teams/Cache",
                &["Microsoft Teams", "MSTeams"],
            ),
            (
                "Library/Caches/com.microsoft.teams2",
                &["Microsoft Teams", "MSTeams"],
            ),
            ("Library/Application Support/Notion/Cache", &["Notion"]),
            ("Library/Caches/notion.id", &["Notion"]),
            ("Library/Application Support/Postman/Cache", &["Postman"]),
            ("Library/Caches/com.postmanlabs.mac", &["Postman"]),
            ("Library/Application Support/Figma/Cache", &["Figma"]),
            ("Library/Caches/com.figma.Desktop", &["Figma"]),
        ];
        for &(rel, procs) in expected {
            assert_eq!(
                global_candidate(&cands, rel).blocked_if_running,
                procs.to_vec(),
                "{rel}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn work_apps_ignore_cookies_and_browser_storage() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let app = home.join("Library/Application Support/Slack");
        write(&app.join("Cache/blob"), 5000);
        for name in [
            "Cookies",
            "Local Storage",
            "IndexedDB",
            "Session Storage",
            "Partitions",
            "databases",
        ] {
            write(&app.join(name).join("data"), 5000);
        }

        let r = scan(d.path(), &["work_apps"]);
        assert_eq!(r.items.len(), 1, "{:?}", paths(&r));
        assert!(find(&r, "Library/Application Support/Slack/Cache").is_some());
        for name in [
            "Cookies",
            "Local Storage",
            "IndexedDB",
            "Session Storage",
            "Partitions",
            "databases",
        ] {
            assert!(
                !r.items.iter().any(|i| i.item.path.contains(name)),
                "{name} was reported: {:?}",
                paths(&r)
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn work_apps_slack_bundle_cache_shadows_the_app_caches_catch_all() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        let bundle = home.join("Library/Caches/com.tinyspeck.slackmacgap");
        write(&bundle.join("blob"), 11 * 1024 * 1024);

        // The catch-all alone lists the folder as a generic app cache...
        let r = scan(d.path(), &["os_system"]);
        let generic = find(&r, "Library/Caches/com.tinyspeck.slackmacgap").unwrap();
        assert_eq!(generic.item.file_type, "com.tinyspeck.slackmacgap");
        assert_eq!(generic.item.category, "System Cache");

        // ...and the specific rule takes its place when both modules are on.
        let r = scan(d.path(), &["os_system", "work_apps"]);
        let matches: Vec<&ScannedItem> = r
            .items
            .iter()
            .filter(|i| {
                i.item
                    .path
                    .ends_with("Library/Caches/com.tinyspeck.slackmacgap")
            })
            .collect();
        assert_eq!(matches.len(), 1, "{:?}", paths(&r));
        assert_eq!(matches[0].item.file_type, "Slack App Cache");
        assert_eq!(matches[0].item.category, "App Cache");
        assert_eq!(matches[0].item.status, Safe);
    }

    #[cfg(target_os = "windows")]
    const WORK_APPS_WINDOWS_CACHES: &[(&str, &str)] = &[
        ("AppData/Roaming/Slack/Cache", "Slack Cache"),
        ("AppData/Roaming/Slack/Code Cache", "Slack Code Cache"),
        ("AppData/Roaming/Slack/GPUCache", "Slack GPU Cache"),
        ("AppData/Roaming/Notion/Cache", "Notion Cache"),
        ("AppData/Roaming/Notion/Code Cache", "Notion Code Cache"),
        ("AppData/Roaming/Notion/GPUCache", "Notion GPU Cache"),
        ("AppData/Roaming/Postman/Cache", "Postman Cache"),
        ("AppData/Roaming/Postman/Code Cache", "Postman Code Cache"),
        ("AppData/Roaming/Postman/GPUCache", "Postman GPU Cache"),
        ("AppData/Roaming/Figma/Cache", "Figma Cache"),
        ("AppData/Roaming/Figma/Code Cache", "Figma Code Cache"),
        ("AppData/Roaming/Figma/GPUCache", "Figma GPU Cache"),
    ];

    #[cfg(target_os = "windows")]
    #[test]
    fn work_apps_windows_caches_are_safe() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        for &(rel, _) in WORK_APPS_WINDOWS_CACHES {
            write(&home.join(rel).join("blob"), 5000);
        }

        let r = scan(d.path(), &["work_apps"]);
        assert_eq!(r.items.len(), WORK_APPS_WINDOWS_CACHES.len());
        for &(rel, label) in WORK_APPS_WINDOWS_CACHES {
            let item = find(&r, rel).unwrap_or_else(|| panic!("missing {rel}"));
            assert_eq!(item.item.file_type, label, "{rel}");
            assert_eq!(item.item.status, Safe, "{rel}");
            assert_eq!(item.item.category, "App Cache", "{rel}");
        }
    }

    #[cfg(target_os = "linux")]
    const WORK_APPS_LINUX_CACHES: &[(&str, &str)] = &[
        (".config/Slack/Cache", "Slack Cache"),
        (".config/Slack/Code Cache", "Slack Code Cache"),
        (".config/Slack/GPUCache", "Slack GPU Cache"),
        (".config/Notion/Cache", "Notion Cache"),
        (".config/Notion/Code Cache", "Notion Code Cache"),
        (".config/Notion/GPUCache", "Notion GPU Cache"),
        (".config/Postman/Cache", "Postman Cache"),
        (".config/Postman/Code Cache", "Postman Code Cache"),
        (".config/Postman/GPUCache", "Postman GPU Cache"),
        (".config/Figma/Cache", "Figma Cache"),
        (".config/Figma/Code Cache", "Figma Code Cache"),
        (".config/Figma/GPUCache", "Figma GPU Cache"),
    ];

    #[cfg(target_os = "linux")]
    #[test]
    fn work_apps_linux_caches_are_safe() {
        let d = TempDir::new().unwrap();
        let home = d.path().join("__home");
        for &(rel, _) in WORK_APPS_LINUX_CACHES {
            write(&home.join(rel).join("blob"), 5000);
        }

        let r = scan(d.path(), &["work_apps"]);
        assert_eq!(r.items.len(), WORK_APPS_LINUX_CACHES.len());
        for &(rel, label) in WORK_APPS_LINUX_CACHES {
            let item = find(&r, rel).unwrap_or_else(|| panic!("missing {rel}"));
            assert_eq!(item.item.file_type, label, "{rel}");
            assert_eq!(item.item.status, Safe, "{rel}");
            assert_eq!(item.item.category, "App Cache", "{rel}");
        }
    }

    // ----- Partial removal reporting (PLAN 3.7c) -----------------------------

    #[test]
    fn freed_after_failure_reports_the_full_size_when_the_path_is_gone() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("gone");
        write(&dir.join("a.bin"), 5000);
        let size = disk_usage(&dir);
        let item = item_with(&dir, "", size);
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(freed_after_failure(&item), size);
    }

    #[test]
    fn freed_after_failure_reports_the_difference_when_part_was_removed() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("partial");
        write(&dir.join("a.bin"), 8192);
        write(&dir.join("b.bin"), 8192);
        write(&dir.join("c.bin"), 8192);
        let original = disk_usage(&dir);
        let item = item_with(&dir, "", original);
        fs::remove_file(dir.join("b.bin")).unwrap();
        let remaining = disk_usage(&dir);

        assert!(remaining > 0);
        assert_eq!(freed_after_failure(&item), original - remaining);
        assert!(freed_after_failure(&item) > 0);
    }

    #[test]
    fn freed_after_failure_is_zero_when_nothing_was_removed() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("kept");
        write(&dir.join("a.bin"), 5000);
        let item = item_with(&dir, "", disk_usage(&dir));

        assert_eq!(freed_after_failure(&item), 0);
    }

    #[test]
    fn freed_after_failure_is_zero_when_remaining_is_bigger() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("bigger");
        write(&dir.join("a.bin"), 5000);
        let item = item_with(&dir, "", 1);

        assert_eq!(freed_after_failure(&item), 0);
    }

    #[test]
    fn freed_after_failure_is_zero_for_a_file_that_still_exists() {
        let d = TempDir::new().unwrap();
        let file = d.path().join("file.bin");
        write(&file, 5000);
        let item = item_with(&file, "", disk_usage(&file));

        assert_eq!(freed_after_failure(&item), 0);
    }

    #[test]
    fn freed_after_failure_is_zero_for_a_command_item() {
        use super::native_commands::{DockerKind, NativeCommand};

        let d = TempDir::new().unwrap();
        let dir = d.path().join("cmd");
        write(&dir.join("a.bin"), 5000);
        let mut item = item_with(&dir, "", disk_usage(&dir));
        item.action = Action::Command(NativeCommand::DockerPrune {
            docker: PathBuf::from("/nonexistent"),
            kind: DockerKind::BuilderCache,
        });
        fs::remove_dir_all(&dir).unwrap();

        assert_eq!(freed_after_failure(&item), 0);
    }

    #[cfg(unix)]
    #[test]
    fn freed_after_failure_does_not_follow_a_symlink() {
        use std::os::unix::fs::symlink;

        let d = TempDir::new().unwrap();
        let target = d.path().join("target");
        write(&target.join("a.bin"), 5000);
        let link = d.path().join("link");
        symlink(&target, &link).unwrap();
        let before = disk_usage(&target);
        let item = item_with(&link, "", before);

        assert_eq!(freed_after_failure(&item), 0);
        assert!(target.join("a.bin").exists());
        assert_eq!(disk_usage(&target), before);
    }

    // ----- Scan progress and cancellation (PLAN 3.5) -------------------------

    fn scan_with_hooks(
        base: &Path,
        modules: &[&str],
        cancelled: &(dyn Fn() -> bool + Sync),
        progress: &(dyn Fn(ScanProgress) + Sync),
    ) -> Option<ScanResult> {
        let env = Env {
            home: base.join("__home"),
        };
        fs::create_dir_all(&env.home).unwrap();
        let modules: Vec<String> = modules.iter().map(|s| s.to_string()).collect();
        let hooks = ScanHooks {
            cancelled,
            progress,
        };
        scan_directory_with(base, &modules, &[], &env, &HashSet::new(), &hooks)
    }

    #[test]
    fn scan_with_hooks_matches_scan_directory() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join("build/out.bin"), 5000);
        write(&p.join(".dart_tool/x"), 5000);

        let expected = scan(d.path(), &["flutter"]);
        let cancelled = || false;
        let progress = |_: ScanProgress| {};
        let actual = scan_with_hooks(d.path(), &["flutter"], &cancelled, &progress).unwrap();

        assert_eq!(paths(&expected), paths(&actual));
    }

    #[test]
    fn scan_with_hooks_cancelled_from_start_is_none() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join("build/out.bin"), 5000);

        let cancelled = || true;
        let progress = |_: ScanProgress| {};
        assert!(scan_with_hooks(d.path(), &["flutter"], &cancelled, &progress).is_none());
    }

    #[test]
    fn scan_with_hooks_stops_after_a_path_event() {
        use std::sync::atomic::AtomicBool;

        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join("build/out.bin"), 5000);

        let flag = AtomicBool::new(false);
        let cancelled = || flag.load(Ordering::Relaxed);
        let progress = |event: ScanProgress| {
            if matches!(event, ScanProgress::Path { .. } | ScanProgress::Measuring { .. }) {
                flag.store(true, Ordering::Relaxed);
            }
        };
        assert!(scan_with_hooks(d.path(), &["flutter"], &cancelled, &progress).is_none());
    }

    #[test]
    fn scan_with_hooks_reports_phases_then_the_final_measuring() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("app");
        write(&p.join("pubspec.yaml"), 10);
        write(&p.join("build/out.bin"), 5000);

        let events = Mutex::new(Vec::new());
        let cancelled = || false;
        let progress = |event: ScanProgress| {
            events.lock().unwrap().push(event);
        };
        let result = scan_with_hooks(d.path(), &["flutter"], &cancelled, &progress);
        assert!(result.is_some());

        let events = events.into_inner().unwrap();
        let phases: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                ScanProgress::Phase { label } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            phases,
            vec![
                "Checking installed tools",
                "Searching your projects",
                "Measuring sizes"
            ]
        );
        assert!(matches!(events.first(), Some(ScanProgress::Phase { .. })));

        let first_measuring = events
            .iter()
            .position(|e| matches!(e, ScanProgress::Measuring { .. }));
        assert!(first_measuring.is_some());
        let first_measuring = first_measuring.unwrap();
        assert!(events
            .iter()
            .skip(first_measuring)
            .all(|e| matches!(e, ScanProgress::Measuring { .. })));

        match events.last() {
            Some(ScanProgress::Measuring { done, total, .. }) => assert_eq!(done, total),
            other => panic!("last event is not Measuring: {other:?}"),
        }
    }

    #[test]
    fn project_items_share_last_active_and_global_items_have_none() {
        let d = TempDir::new().unwrap();
        let project = d.path().join("app");
        write(&project.join("pubspec.yaml"), 10);
        write(&project.join("build/out.bin"), BIG);
        write(&project.join(".dart_tool/x"), BIG);
        write(
            &d.path().join("__home/Library/Caches/Trae/pending/blob"),
            BIG,
        );

        let r = scan(d.path(), &["flutter", "ai_assistants"]);
        let project_items: Vec<_> = r
            .items
            .iter()
            .filter(|i| i.item.group == "Projects")
            .collect();
        assert_eq!(project_items.len(), 2, "{}", paths(&r).join("\n"));
        let first = project_items[0].item.last_active;
        assert!(first.is_some(), "project activity was not detected");
        assert!(project_items
            .iter()
            .all(|i| i.item.last_active == first));

        let global = find(&r, "Trae/pending").expect("Trae pending item");
        assert_eq!(global.item.last_active, None);
    }

    #[test]
    fn disk_usage_cancellable_stops_early_and_matches_disk_usage() {
        let d = TempDir::new().unwrap();
        let dir = d.path().join("many");
        for i in 0..400 {
            write(&dir.join(format!("f{i}.bin")), 1);
        }

        let full = disk_usage(&dir);
        let partial = disk_usage_cancellable(&dir, &|| true);
        assert!(partial > 0);
        assert!(partial < full, "partial {partial} is not below full {full}");
        assert_eq!(disk_usage_cancellable(&dir, &|| false), full);
    }

    // ----- Parallel project walk (PLAN 3.4) ----------------------------------

    fn walk_project_candidates(
        base: &Path,
        modules: &[&str],
        max_depth: Option<usize>,
    ) -> Vec<String> {
        let enabled_modules: Vec<String> = modules.iter().map(|s| s.to_string()).collect();
        let enabled: HashSet<&str> = enabled_modules.iter().map(String::as_str).collect();
        let is_ignored = |_: &Path| false;
        let cancelled = || false;
        let progress = |_: ScanProgress| {};
        let hooks = ScanHooks {
            cancelled: &cancelled,
            progress: &progress,
        };
        let depth = max_depth
            .unwrap_or(MAX_DEPTH)
            .clamp(MIN_DEPTH, MAX_DEPTH_LIMIT);
        let mut out = Vec::new();
        collect_projects(base, &enabled, &is_ignored, &mut out, &hooks, depth);
        let mut paths: Vec<String> = out
            .iter()
            .map(|c| c.path.to_string_lossy().into_owned())
            .collect();
        paths.sort();
        paths
    }

    /// Small sequential version of the walk, used as the reference.
    fn reference_candidates(base: &Path, modules: &[&str], max_depth: usize) -> Vec<String> {
        let rules: Vec<&ProjectRule> = PROJECT_RULES
            .iter()
            .filter(|r| modules.contains(&r.module))
            .collect();
        let walker = WalkDir::new(base)
            .max_depth(max_depth)
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
                !SKIP_DIRS.contains(&name.as_ref())
            });
        let mut out = Vec::new();
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
                        if fs::symlink_metadata(&path).is_ok() {
                            out.push(path.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }

    fn build_mixed_tree(base: &Path) {
        for i in 0..30 {
            let root = base.join(format!("group{}/nested{}/project{i}", i % 3, i % 4));
            match i % 5 {
                0 => {
                    write(&root.join("pubspec.yaml"), 10);
                    write(&root.join("build/out.bin"), 5000);
                }
                1 => {
                    write(&root.join("package.json"), 10);
                    write(&root.join("node_modules/pkg/index.js"), 5000);
                }
                2 => {
                    write(&root.join("Cargo.toml"), 10);
                    write(&root.join("target/debug/app"), 5000);
                }
                3 => {
                    write(&root.join("requirements.txt"), 10);
                    write(&root.join("__pycache__/mod.pyc"), 5000);
                }
                _ => {
                    write(&root.join("composer.json"), 10);
                    write(&root.join("vendor/pkg/file.php"), 5000);
                }
            }
        }
    }

    const ALL_PROJECT_MODULES: &[&str] = &[
        "flutter",
        "node",
        "rust",
        "rust_incremental",
        "php",
        "python",
        "python_venv",
        "android",
        "dotnet",
        "ruby",
        "cpp",
        "unreal",
        "unity",
        "ai_assistants",
    ];

    #[test]
    fn parallel_walk_matches_the_sequential_reference() {
        let d = TempDir::new().unwrap();
        build_mixed_tree(d.path());

        let parallel = walk_project_candidates(d.path(), ALL_PROJECT_MODULES, None);
        let sequential = reference_candidates(d.path(), ALL_PROJECT_MODULES, MAX_DEPTH);

        assert_eq!(parallel.len(), sequential.len(), "candidate count differs");
        assert_eq!(parallel, sequential);
        assert!(parallel.len() >= 30, "only {} candidates", parallel.len());
    }

    #[test]
    fn parallel_walk_is_deterministic() {
        let d = TempDir::new().unwrap();
        build_mixed_tree(d.path());

        let first = walk_project_candidates(d.path(), ALL_PROJECT_MODULES, None);
        for _ in 0..4 {
            assert_eq!(walk_project_candidates(d.path(), ALL_PROJECT_MODULES, None), first);
        }
    }

    #[cfg(unix)]
    #[test]
    fn parallel_walk_does_not_follow_symlinked_directories() {
        use std::os::unix::fs::symlink;

        let d = TempDir::new().unwrap();
        let base = d.path().join("base");
        let outside = d.path().join("outside");
        write(&outside.join("package.json"), 10);
        write(&outside.join("node_modules/pkg/index.js"), 5000);
        fs::create_dir_all(&base).unwrap();
        symlink(&outside, base.join("linked")).unwrap();

        assert!(walk_project_candidates(&base, &["node"], None).is_empty());
    }

    #[test]
    fn parallel_walk_stops_when_cancelled_from_progress() {
        use std::sync::atomic::AtomicBool;

        let d = TempDir::new().unwrap();
        let base = d.path().join("base");
        write(&base.join("project/package.json"), 10);
        write(&base.join("project/node_modules/pkg/index.js"), 5000);

        let enabled_modules = ["node".to_string()];
        let enabled: HashSet<&str> = enabled_modules.iter().map(String::as_str).collect();
        let is_ignored = |_: &Path| false;
        let flag = AtomicBool::new(false);
        let cancelled = || flag.load(Ordering::Relaxed);
        let progress = |event: ScanProgress| {
            if matches!(event, ScanProgress::Path { .. }) {
                flag.store(true, Ordering::Relaxed);
            }
        };
        let hooks = ScanHooks {
            cancelled: &cancelled,
            progress: &progress,
        };
        let mut out = Vec::new();
        collect_projects(&base, &enabled, &is_ignored, &mut out, &hooks, MAX_DEPTH);

        assert!(out.is_empty(), "walk kept going after cancellation");
    }

    #[test]
    fn max_depth_limits_the_walk() {
        let d = TempDir::new().unwrap();
        let base = d.path().join("base");
        let project = base.join("a/b/project");
        write(&project.join("package.json"), 10);
        write(&project.join("node_modules/pkg/index.js"), 5000);

        let deep = walk_project_candidates(&base, &["node"], Some(4));
        assert!(deep.iter().any(|p| p.ends_with("b/project/node_modules")));
        assert!(walk_project_candidates(&base, &["node"], Some(2)).is_empty());
    }

    #[test]
    fn none_depth_behaves_like_the_default() {
        let d = TempDir::new().unwrap();
        let base = d.path().join("base");
        let project = base.join("l1/l2/l3/l4/l5/l6/l7/project");
        write(&project.join("package.json"), 10);
        write(&project.join("node_modules/pkg/index.js"), 5000);

        let found = walk_project_candidates(&base, &["node"], None);
        assert_eq!(found, walk_project_candidates(&base, &["node"], Some(8)));
        assert!(found.iter().any(|p| p.ends_with("project/node_modules")));
    }

    #[test]
    fn generated_folders_are_not_searched_for_nested_projects() {
        let d = TempDir::new().unwrap();
        let base = d.path().join("base");
        let app = base.join("app");
        write(&app.join("pubspec.yaml"), 10);
        write(&app.join("build/out.bin"), 5000);
        write(&app.join("ios/Pods/nested/package.json"), 10);
        write(&app.join("ios/Pods/nested/node_modules/pkg/index.js"), 5000);
        write(&app.join("vendor/nested/package.json"), 10);
        write(&app.join("vendor/nested/node_modules/pkg/index.js"), 5000);
        write(&app.join(".nuxt/nested/package.json"), 10);
        write(&app.join(".nuxt/nested/node_modules/pkg/index.js"), 5000);

        let found = walk_project_candidates(&base, &["flutter", "node"], None);
        assert!(found.iter().any(|p| p.ends_with("app/build")));
        assert!(found.iter().any(|p| p.ends_with("app/ios/Pods")));
        assert!(!found.iter().any(|p| p.contains("Pods/nested")));
        assert!(!found.iter().any(|p| p.contains("vendor/nested")));
        assert!(!found.iter().any(|p| p.contains(".nuxt/")));
    }

    /// Manual benchmark for the project walk (PLAN 3.4). It is ignored by
    /// default because it reads a real folder supplied through the
    /// `SWEEP_BENCH_DIR` environment variable.
    #[test]
    #[ignore]
    fn bench_project_scan() {
        let Ok(dir) = std::env::var("SWEEP_BENCH_DIR") else {
            println!("bench_project_scan skipped: set SWEEP_BENCH_DIR to a folder to scan");
            return;
        };
        let base = PathBuf::from(dir);
        let home = TempDir::new().unwrap();
        let env = Env {
            home: home.path().to_path_buf(),
        };
        let modules: Vec<String> = PROJECT_RULES
            .iter()
            .map(|r| r.module.to_string())
            .collect();
        // Phase events are timestamped relative to the start of the current run.
        let phases: Mutex<Vec<(String, Duration)>> = Mutex::new(Vec::new());
        let run_start = Mutex::new(Instant::now());
        let cancelled = || false;
        let progress = |event: ScanProgress| {
            let label = match event {
                ScanProgress::Phase { label } => label,
                ScanProgress::Measuring { .. } => "first Measuring event".to_string(),
                ScanProgress::Path { .. } => return,
            };
            let since_start = run_start.lock().map(|t| t.elapsed()).unwrap_or_default();
            if let Ok(mut v) = phases.lock() {
                // Only the first "Measuring" event marks the start of that stage.
                if v.iter().any(|(l, _)| l == &label) {
                    return;
                }
                v.push((label, since_start));
            }
        };
        let hooks = ScanHooks {
            cancelled: &cancelled,
            progress: &progress,
        };

        let mut durations = Vec::new();
        for run in 1..=3 {
            phases.lock().unwrap().clear();
            let started = Instant::now();
            *run_start.lock().unwrap() = started;
            let result = scan_directory_with(&base, &modules, &[], &env, &HashSet::new(), &hooks)
                .expect("benchmark scan was cancelled");
            let total = started.elapsed();
            println!("bench project scan run {run}: {total:?}");
            durations.push(total);

            // Timestamps of the phase boundaries, in the order they happened.
            let pts = phases.lock().unwrap().clone();
            let at = |label: &str| pts.iter().find(|(l, _)| l == label).map(|(_, d)| *d);
            let tools = at("Checking installed tools").unwrap_or_default();
            let search = at("Searching your projects").unwrap_or_default();
            let measure = at("Measuring sizes").unwrap_or_default();
            println!("  phases (run {run}):");
            println!("    Checking installed tools: {tools:?} (ends at {search:?})");
            println!("    Searching your projects:  {search:?} -> {measure:?} = {:?}", measure.saturating_sub(search));
            println!("    Measuring sizes:         {measure:?} -> end = {:?}", total.saturating_sub(measure));
            println!("    total: {total:?}");
            for (label, dt) in pts.iter() {
                println!("      mark {label} at {dt:?}");
            }

            if let Ok(out) = std::env::var("SWEEP_BENCH_OUT") {
                let mut items: Vec<String> =
                    result.items.iter().map(|i| i.item.path.clone()).collect();
                items.sort();
                fs::write(&out, items.join("\n")).unwrap();
            }
        }
        durations.sort();
        println!("bench project scan median: {:?}", durations[1]);
    }

    /// Benchmark the individual phases of the project scan (PLAN 3.4). Ignored;
    /// reads the folder from SWEEP_BENCH_DIR and uses a fresh temp home.
    #[test]
    #[ignore]
    fn bench_project_phases() {
        let Ok(dir) = std::env::var("SWEEP_BENCH_DIR") else {
            println!("bench_project_phases skipped: set SWEEP_BENCH_DIR to a folder to scan");
            return;
        };
        let base = PathBuf::from(dir);
        // Fresh temp home, so no global rule can ever match.
        let _home = TempDir::new().unwrap();
        let modules: Vec<String> = PROJECT_RULES
            .iter()
            .map(|r| r.module.to_string())
            .collect();
        let enabled_set: HashSet<&str> = modules.iter().map(String::as_str).collect();

        fn progress_dummy(_: ScanProgress) {}
        fn cancelled_dummy() -> bool {
            false
        }
        let cancelled = cancelled_dummy;
        let hooks = ScanHooks {
            cancelled: &cancelled,
            progress: &progress_dummy,
        };

        // One walk to learn the candidate set and the project roots.
        let mut raw_candidates = Vec::new();
        collect_projects(
            &base,
            &enabled_set,
            &|_: &Path| false,
            &mut raw_candidates,
            &hooks,
            MAX_DEPTH,
        );
        let n_candidates = raw_candidates.len();

        // The same filter pipeline the real scan uses after the walk.
        let candidates = drop_git_tracked(raw_candidates);
        let candidates = drop_shadowed_catch_alls(candidates);
        let candidates = drop_nested(candidates);
        let n_after_filter = candidates.len();

        // Distinct project roots of the surviving candidates.
        let roots: HashSet<&PathBuf> = candidates
            .iter()
            .filter_map(|c| c.project_root.as_ref())
            .collect();
        let n_roots = roots.len();

        // How many directories the walk has to visit, and how many of them are
        // skipped. This is the work the walk has to parallelise.
        let mut dirs_total = 0usize;
        let mut dirs_skipped = 0usize;
        for entry in WalkDir::new(&base)
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
                if SKIP_DIRS.contains(&name.as_ref()) {
                    dirs_skipped += 1;
                    return false;
                }
                true
            })
            .flatten()
        {
            if fs::read_dir(entry.path()).is_ok() {
                dirs_total += 1;
            }
        }

        // The floor of the walk: only enumerate the directories and build the
        // name sets, with no marker matching and no target expansion. This is
        // the part that cannot be avoided by any algorithm.
        let floor_start = Instant::now();
        let mut floor_entries = 0usize;
        fn enumerate(
            dir: &Path,
            depth: usize,
            max_depth: usize,
            entries: &mut usize,
        ) {
            let Ok(read_dir) = fs::read_dir(dir) else {
                return;
            };
            let mut names: HashSet<OsString> = HashSet::new();
            let mut children: Vec<PathBuf> = Vec::new();
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                *entries += 1;
                if depth < max_depth
                    && entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && !SKIP_DIRS.contains(&name.to_string_lossy().as_ref())
                {
                    children.push(entry.path());
                }
                names.insert(name);
            }
            for child in children {
                enumerate(&child, depth + 1, max_depth, entries);
            }
        }
        enumerate(&base, 0, MAX_DEPTH, &mut floor_entries);
        let floor_elapsed = floor_start.elapsed();

        // The raw syscall floor: read_dir and nothing else. No name sets, no
        // file_type checks, no rule matching.
        let raw_start = Instant::now();
        let mut raw_entries = 0usize;
        fn raw_enumerate(dir: &Path, depth: usize, max_depth: usize, entries: &mut usize) {
            let Ok(read_dir) = fs::read_dir(dir) else {
                return;
            };
            let mut children: Vec<PathBuf> = Vec::new();
            for entry in read_dir.flatten() {
                *entries += 1;
                if depth < max_depth
                    && entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && !SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
                {
                    children.push(entry.path());
                }
            }
            for child in children {
                raw_enumerate(&child, depth + 1, max_depth, entries);
            }
        }
        raw_enumerate(&base, 0, MAX_DEPTH, &mut raw_entries);
        let raw_elapsed = raw_start.elapsed();

        let mut collect_times: Vec<Duration> = Vec::new();
        let mut collect_seq_times: Vec<Duration> = Vec::new();
        let mut drop_times: Vec<Duration> = Vec::new();
        let mut age_times: Vec<Duration> = Vec::new();
        let mut age_seq_times: Vec<Duration> = Vec::new();
        let mut size_times: Vec<Duration> = Vec::new();
        let mut slowest: Vec<(u64, Duration, String)> = Vec::new();
        let mut last_sum: Duration = Duration::ZERO;
        let mut last_size_wall: Duration = Duration::ZERO;

        for run in 1..=3 {
            // (a) the walk on its own.
            let mut walk_candidates = Vec::new();
            let started = Instant::now();
            collect_projects(
                &base,
                &enabled_set,
                &|_: &Path| false,
                &mut walk_candidates,
                &hooks,
                MAX_DEPTH,
            );
            let collect_elapsed = started.elapsed();
            collect_times.push(collect_elapsed);

            // The same walk with a single worker, to show what the parallel
            // walk actually buys on this tree.
            let one_thread = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap();
            let mut seq_candidates = Vec::new();
            let seq_started = Instant::now();
            one_thread.install(|| {
                collect_projects(
                    &base,
                    &enabled_set,
                    &|_: &Path| false,
                    &mut seq_candidates,
                    &hooks,
                    MAX_DEPTH,
                );
            });
            collect_seq_times.push(seq_started.elapsed());

            // The activity pass with a single worker, to show what the
            // parallel activity pass buys.
            let mut age_seq = Vec::new();
            let age_seq_started = Instant::now();
            for root in roots.iter().copied() {
                age_seq.push(age::project_last_active(root, &cancelled_dummy));
            }
            age_seq_times.push(age_seq_started.elapsed());
            let _ = age_seq;

            // The drop pipeline (git-tracked, shadowed catch-alls, nested).
            let drop_started = Instant::now();
            let dropped = drop_git_tracked(walk_candidates);
            let dropped = drop_shadowed_catch_alls(dropped);
            let dropped = drop_nested(dropped);
            drop_times.push(drop_started.elapsed());
            let _ = dropped.len();

            // (b) the activity pass over the distinct roots.
            let started = Instant::now();
            let activity: Vec<Option<i64>> = roots
                .iter()
                .copied()
                .collect::<Vec<&PathBuf>>()
                .into_par_iter()
                .map(|root| age::project_last_active(root, &cancelled_dummy))
                .collect();
            let age_elapsed = started.elapsed();
            age_times.push(age_elapsed);

            // (c) the sizing stage.
            let started = Instant::now();
            let mut measured: Vec<(u64, Duration, String)> = candidates
                .par_iter()
                .map(|c| {
                    let item_started = Instant::now();
                    let bytes = disk_usage_cancellable(&c.path, &cancelled_dummy);
                    let elapsed = item_started.elapsed();
                    (bytes, elapsed, c.path.to_string_lossy().into_owned())
                })
                .collect();
            let size_elapsed = started.elapsed();
            size_times.push(size_elapsed);

            let sum: Duration = measured.iter().map(|(_, d, _)| *d).sum();
            let with_activity = activity.iter().filter(|a| a.is_some()).count();
            println!(
                "bench project phases run {run}: collect={collect_elapsed:?} age={age_elapsed:?} \
                 size={size_elapsed:?} (roots with activity {with_activity}/{n_roots})"
            );

            measured.sort_by_key(|a| std::cmp::Reverse(a.1));
            slowest = measured;
            last_sum = sum;
            last_size_wall = size_elapsed;
        }

        let mut collect_sorted = collect_times.clone();
        let mut collect_seq_sorted = collect_seq_times.clone();
        let mut drop_sorted = drop_times.clone();
        let mut age_sorted = age_times.clone();
        let mut age_seq_sorted = age_seq_times.clone();
        let mut size_sorted = size_times.clone();
        collect_sorted.sort();
        collect_seq_sorted.sort();
        drop_sorted.sort();
        age_sorted.sort();
        age_seq_sorted.sort();
        size_sorted.sort();
        println!(
            "bench project phases medians: collect={:?} collect_1_thread={:?} drop={:?} \
             age={:?} age_1_thread={:?} size={:?}",
            collect_sorted[1],
            collect_seq_sorted[1],
            drop_sorted[1],
            age_sorted[1],
            age_seq_sorted[1],
            size_sorted[1]
        );
        println!(
            "bench project phases medians: walk speedup (1 thread / N threads) = {:.2}x on {} threads; \
             activity speedup = {:.2}x",
            collect_seq_sorted[1].as_secs_f64() / collect_sorted[1].as_secs_f64().max(f64::EPSILON),
            rayon::current_num_threads(),
            age_seq_sorted[1].as_secs_f64() / age_sorted[1].as_secs_f64().max(f64::EPSILON),
        );

        let total_bytes: u64 = slowest.iter().map(|(b, _, _)| b).sum();
        println!("bench project phases counts:");
        println!("  directories the walk reads: {dirs_total} (skipped by SKIP_DIRS: {dirs_skipped})");
        println!(
            "  walk floor (sequential read_dir + name sets only): {floor_elapsed:?} over {floor_entries} entries"
        );
        println!(
            "  walk raw floor (sequential read_dir only): {raw_elapsed:?} over {raw_entries} entries"
        );
        println!("  candidates from the walk: {n_candidates}");
        println!("  candidates after the drop pipeline: {n_after_filter}");
        println!("  distinct project roots: {n_roots}");
        println!("  bytes measured in the last sizing run: {total_bytes}");
        println!("  10 slowest items (last run):");
        for (i, (bytes, elapsed, path)) in slowest.iter().take(10).enumerate() {
            println!("    #{i}: size={bytes} duration={elapsed:?} path={path}");
        }
        if let Some((bytes, elapsed, path)) = slowest.first() {
            let share = if last_sum.is_zero() {
                0.0
            } else {
                elapsed.as_secs_f64() * 100.0 / last_sum.as_secs_f64()
            };
            let wall_share = if last_size_wall.is_zero() {
                0.0
            } else {
                elapsed.as_secs_f64() * 100.0 / last_size_wall.as_secs_f64()
            };
            println!(
                "  slowest item: {path} size={bytes} duration={elapsed:?} \
                 share of summed item time {share:.1}%, share of wall-clock sizing {wall_share:.1}%"
            );
        }
    }
}
