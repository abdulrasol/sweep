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
    /// Run an official tool instead (PLAN 3.3). See `native_commands`.
    Command(NativeCommand),
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
        &["pip", "pip3", "uv", "poetry"],
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
        &["pip", "pip3", "uv", "poetry"],
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
        &["pip", "pip3", "uv", "poetry"],
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
    /// One folder of a catch-all listing (each folder in ~/Library/Caches). It gives
    /// way to any specific rule that lists the same folder or something inside it.
    catch_all: bool,
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
    editors::collect(env, &enabled, &mut candidates);
    simulators::collect(env, &enabled, &mut candidates);
    dart::collect(env, &enabled, &mut candidates);
    android::collect(env, &enabled, &mut candidates);
    node::collect(env, &enabled, &mut candidates);
    rust::collect(env, &enabled, &mut candidates);
    conda::collect(env, &enabled, &mut candidates);
    go::collect(env, &enabled, &mut candidates);
    docker::collect(env, &enabled, &mut candidates);
    brew::collect(env, &enabled, &mut candidates);
    ai_models::collect(env, &enabled, &mut candidates);
    collect_projects(base_path, &enabled, &is_ignored, &mut candidates);

    candidates.retain(|c| !is_ignored(&c.path));
    let candidates = drop_git_tracked(candidates);
    let candidates = drop_shadowed_catch_alls(candidates);
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
                        catch_all: false,
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
    v.extend(editors::PROTECTED.iter().map(|rel| home.join(rel)));
    v.extend(ai_models::PROTECTED.iter().map(|rel| home.join(rel)));
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
/// Returns the bytes freed: the scanned size, unless the command measured it.
pub fn execute(
    item: &ScannedItem,
    allowed_roots: &[PathBuf],
    protected: &[PathBuf],
) -> Result<u64, String> {
    match &item.action {
        Action::DeletePath => {
            delete_path(item, allowed_roots, protected).map(|()| item.item.size_bytes)
        }
        Action::Command(cmd) => {
            native_commands::run(cmd).map(|freed| freed.unwrap_or(item.item.size_bytes))
        }
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
        assert!(delete_path(&app_support_item, &[home.clone()], &protected).is_err());
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
            delete_path(&scanned(&home.join(token_rel)), &roots, &protected).is_err(),
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
}
