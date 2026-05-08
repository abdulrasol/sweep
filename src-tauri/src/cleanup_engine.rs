// Sweep Cleanup Engine - Optimized for your React Frontend
// Fully Multi-OS Compliant (Windows, Mac, Linux)
// Ultimate Expansion: Emulators, Virtualization, AI, and Media Caches
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CleanupItem {
    pub id: String,
    pub path: String,
    pub file_type: String, 
    #[serde(rename = "type")]
    pub category: String,
    pub size: String,      
    pub size_bytes: u64,
    pub status: String,    
    pub description: String,
}

pub struct Framework {
    pub id: &'static str,
    pub name: &'static str,
    pub marker: &'static str,
    pub cleanup_label: &'static str,
    pub folders_to_nuke: Vec<&'static str>,
    pub status: &'static str,
}

pub fn get_frameworks() -> Vec<Framework> {
    vec![
        Framework {
            id: "flutter",
            name: "Flutter",
            marker: "pubspec.yaml",
            cleanup_label: "Flutter Artifacts",
            folders_to_nuke: vec!["android/.gradle", ".dart_tool", "build", "ios/Pods", "ios/.symlinks"],
            status: "SAFE",
        },
        Framework {
            id: "node",
            name: "Node / PNPM",
            marker: "package.json",
            cleanup_label: "Modules & Cache",
            folders_to_nuke: vec!["node_modules", "dist", ".next", ".cache", ".pnpm-store"],
            status: "SAFE",
        },
        Framework {
            id: "rust",
            name: "Rust",
            marker: "Cargo.toml",
            cleanup_label: "Rust Build",
            folders_to_nuke: vec!["target"],
            status: "SAFE",
        },
        Framework {
            id: "php",
            name: "PHP / Laravel",
            marker: "composer.json",
            cleanup_label: "Vendor & Cache",
            folders_to_nuke: vec!["vendor", "storage/framework/cache/data", "storage/framework/views", "bootstrap/cache/*.php"],
            status: "SAFE",
        },
        Framework {
            id: "android",
            name: "Android / Java",
            marker: "build.gradle",
            cleanup_label: "Gradle Build",
            folders_to_nuke: vec!["build", ".gradle", "app/build"],
            status: "SAFE",
        },
        Framework {
            id: "dotnet",
            name: ".NET / C#",
            marker: "Program.cs",
            cleanup_label: "Build Binaries",
            folders_to_nuke: vec!["bin", "obj", "Debug", "Release"],
            status: "SAFE",
        },
        Framework {
            id: "ruby",
            name: "Ruby on Rails",
            marker: "Gemfile",
            cleanup_label: "Bundle & Tmp",
            folders_to_nuke: vec!["vendor/bundle", "tmp/cache", "log/*.log"],
            status: "SAFE",
        },
        Framework {
            id: "unreal",
            name: "Unreal Engine",
            marker: "*.uproject",
            cleanup_label: "Project Artifacts",
            folders_to_nuke: vec!["Intermediate", "Saved", "Binaries", "DerivedDataCache"],
            status: "DANGER",
        },
        Framework {
            id: "ai",
            name: "AI / ML Models",
            marker: ".gitattributes", 
            cleanup_label: "Model Checkpoints",
            folders_to_nuke: vec!["checkpoints", "runs", "outputs", ".cache/huggingface"],
            status: "REVIEW",
        },
        Framework {
            id: "python",
            name: "Python / Conda",
            marker: "environment.yml",
            cleanup_label: "Conda Env",
            folders_to_nuke: vec!["envs", "pkgs", ".conda"],
            status: "REVIEW",
        },
        Framework {
            id: "unity",
            name: "Unity",
            marker: "ProjectSettings/ProjectVersion.txt",
            cleanup_label: "Unity Library",
            folders_to_nuke: vec!["Library", "Temp", "Obj", "Logs"],
            status: "DANGER",
        },
        Framework {
            id: "go",
            name: "Go",
            marker: "go.mod",
            cleanup_label: "Go Binaries",
            folders_to_nuke: vec!["bin", "pkg"],
            status: "SAFE",
        },
    ]
}

pub fn scan_directory(base_path: &str, enabled_modules: Vec<String>, ignored_paths: Vec<String>) -> Vec<CleanupItem> {
    let mut items = Vec::new();
    let frameworks = get_frameworks();
    
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let cache_dir = dirs::cache_dir().unwrap_or_else(|| home_dir.join("Library/Caches"));
    let data_dir = dirs::data_local_dir().unwrap_or_else(|| home_dir.clone());

    let mut global_caches = Vec::new();

    // Emulators & Virtualization
    global_caches.push(("android", "Android AVD Images", home_dir.join(".android/avd"), "Emulator Data", "REVIEW"));
    global_caches.push(("os", "Vagrant Boxes", home_dir.join(".vagrant.d/boxes"), "Virtualization", "DANGER"));

    // AI / ML Global Caches
    global_caches.push(("ai", "Hugging Face Models", home_dir.join(".cache/huggingface"), "AI Model Cache", "REVIEW"));
    global_caches.push(("ai", "PyTorch Cache", home_dir.join(".cache/torch"), "AI Model Cache", "REVIEW"));

    // Multi-Platform Communication & Media
    global_caches.push(("social", "Telegram Desktop", data_dir.join("Telegram Desktop"), "Media Cache", "REVIEW"));
    global_caches.push(("social", "Discord Cache", data_dir.join("discord/Cache"), "App Cache", "SAFE"));
    global_caches.push(("social", "Spotify Cache", cache_dir.join("com.spotify.client"), "Media Cache", "SAFE"));

    // Global Package Managers
    global_caches.push(("node", "PNPM Store", data_dir.join("pnpm/store"), "Global Package Store", "SAFE"));
    global_caches.push(("python", "Conda Environments", home_dir.join("anaconda3/envs"), "Conda Envs", "REVIEW"));

    #[cfg(target_os = "macos")]
    {
        global_caches.push(("xcode", "Xcode DerivedData", home_dir.join("Library/Developer/Xcode/DerivedData"), "Build Cache", "SAFE"));
        global_caches.push(("homebrew", "Homebrew Cache", cache_dir.join("Homebrew"), "System Cache", "SAFE"));
        global_caches.push(("adobe", "Adobe Media Cache", cache_dir.join("Adobe/Common"), "Media Cache", "REVIEW"));
    }

    #[cfg(target_os = "windows")]
    {
        global_caches.push(("os", "Windows Temp", data_dir.join("Temp"), "System Cache", "REVIEW"));
        global_caches.push(("adobe", "Adobe Media Cache", data_dir.join("Adobe/Common"), "Media Cache", "REVIEW"));
        global_caches.push(("dotnet", "NuGet Cache", home_dir.join(".nuget/packages"), "Package Cache", "SAFE"));
    }

    global_caches.push(("docker", "Docker Data", home_dir.join(".docker"), "Container Cache", "REVIEW"));
    global_caches.push(("os", "Universal Cache", cache_dir, "System Cache", "REVIEW"));

    for (mod_id, label, path_buf, category, status) in global_caches {
        let path_str = path_buf.to_string_lossy().to_string();
        if enabled_modules.contains(&mod_id.to_string()) && !ignored_paths.contains(&path_str) {
            if path_buf.exists() {
                let size_bytes = compute_dir_size_bytes(&path_str);
                if size_bytes > 0 {
                    items.push(CleanupItem {
                        id: format!("global-{}", label.to_lowercase().replace(" ", "-")),
                        path: path_str,
                        file_type: "Global".to_string(),
                        category: category.to_string(),
                        size: format_bytes(size_bytes),
                        size_bytes,
                        status: status.to_string(),
                        description: match status {
                            "DANGER" => "Critical system/app component. Deleting may require app reconfiguration.".to_string(),
                            "REVIEW" => "Recommended for review. Contains potentially important user settings or cached media.".to_string(),
                            _ => format!("Standard {} cache. Safe to delete to reclaim space.", label),
                        },
                    });
                }
            }
        }
    }

    let active_frameworks: Vec<&Framework> = frameworks.iter()
        .filter(|fw| enabled_modules.contains(&fw.id.to_string()))
        .collect();

    if !active_frameworks.is_empty() {
        for entry in WalkDir::new(base_path)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            let path_str = path.to_string_lossy().to_string();
            
            if ignored_paths.iter().any(|ignored| path_str.starts_with(ignored)) {
                continue;
            }

            for fw in &active_frameworks {
                let marker_match = if fw.marker.contains('*') {
                    fs::read_dir(path).ok().map(|mut rd| rd.any(|e| e.ok().map(|ent| ent.file_name().to_string_lossy().ends_with(&fw.marker[1..])).unwrap_or(false))).unwrap_or(false)
                } else {
                    path.join(fw.marker).exists()
                };

                if marker_match {
                    let project_root = path;
                    let project_name = project_root.file_name().unwrap_or_default().to_string_lossy();

                    for sub in &fw.folders_to_nuke {
                        let sub_path = project_root.join(sub);
                        let sub_str = sub_path.to_string_lossy().to_string();
                        
                        if ignored_paths.contains(&sub_str) {
                            continue;
                        }

                        if sub_path.exists() {
                            let size_bytes = compute_dir_size_bytes(&sub_str);
                            if size_bytes > 0 {
                                items.push(CleanupItem {
                                    id: format!("{}-{}-{}", fw.id, sub.replace("/", "-"), project_name),
                                    path: sub_str,
                                    file_type: fw.name.to_string(),
                                    category: fw.cleanup_label.to_string(),
                                    size: format_bytes(size_bytes),
                                    size_bytes,
                                    status: fw.status.to_string(),
                                    description: format!("Removing these {} will clean up the {} folder in your {} project.", fw.cleanup_label, sub, fw.name),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    items.dedup_by(|a, b| a.id == b.id);
    items
}

pub fn delete_item(path_str: &str) -> Result<(), String> {
    let path = Path::new(path_str);
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("Failed to delete dir: {}", e))
    } else {
        fs::remove_file(path).map_err(|e| format!("Failed to delete file: {}", e))
    }
}

fn compute_dir_size_bytes(path: &str) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}
    
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    }
}
